// The terminal tool, and the bubblewrap sandbox around it.
//
// The file tools, the search and the image live in modules of their own, and every tool answers
// with one of the two forms here: `Outcome` for a command that ran as a program of the
// computer, and `Report` for a call the program did itself.
//
// The sandbox starts from an empty root, so a command sees nothing of this computer. Three
// items are put into it: the programs of the system, read only; the working directory of the
// chat, read and write; and the few files of /etc that a shell requests. The network is off,
// the process list is a new one, and a file outside the working directory is simply not there.
//
// The sandbox before this one bound the whole root of the host read only. That stopped a
// command from writing outside its folder, but every private file of the computer could still
// be read from inside, so the sandbox did not keep what its name promised. The empty root is
// the fix.
//
// A run that requests the sandbox and cannot get it does not run at all. The program never
// falls back to a plain run behind the back of the chat.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use crate::util;

// Name the model uses to request a terminal run.
pub const TOOL_NAME: &str = "run_terminal";

// Longest output kept for one run.
const MAX_OUTPUT: usize = 8000;

// The shell a command is handed to, both inside the sandbox and outside it.
const SHELL: &str = "/bin/sh";

// The path of the programs inside the sandbox. It holds the usual places and nothing of the
// user, for The environment of the host is removed inside the sandbox.
const JAIL_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

// The directory `/tmp` of the sandbox maps to. It is a directory of the working
// directory of the chat, so what a command writes there is kept with the working directory of the
// chat, is seen by the next command of that chat, and is seen by a command of the chat from
// outside the sandbox under the name `tmp`. A directory of its own for every command
// was the old way, and a model could not leave a file for the next command in it.
const TMP_ROOM: &str = "tmp";

// The name the sandbox gives its own computer. The name of the host is not wanted in there, and
// a command has no business reading it.
const JAIL_HOSTNAME: &str = "natter";

// The folders the programs of the system live in. They go into the sandbox read only, so a
// command can run what the computer has without changing it.
const PROGRAM_PATHS: [&str; 7] = ["/usr", "/bin", "/lib", "/lib64", "/lib32", "/libx32", "/sbin"];

// A system that keeps its programs outside those folders, such as NixOS or GNU System, gets
// those folders too. They hold programs and nothing else.
const EXTRA_PROGRAM_PATHS: [&str; 2] = ["/nix", "/gnu"];

// The files of /etc a shell and its tools request. They are put in one by one, and never the
// whole folder, so a private file of the computer stays out of reach: /etc/shadow, /etc/ssh,
// /etc/computer-id and the rest are not proposed.
const ETC_PATHS: [&str; 12] = [
    "/etc/passwd",
    "/etc/group",
    "/etc/shells",
    "/etc/nsswitch.conf",
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/localtime",
    "/etc/timezone",
    "/etc/alternatives",
    "/etc/terminfo",
    "/etc/ssl/certs",
    "/etc/pki/tls/certs",
];

// The markers of the trust a command requests when it fetches a page of the web: the bundle of
// certificates, the file that shows where the bundle remains and how it is to be read, and the
// store of the markers themselves. Only the two folders of the certificates went in before. A
// bundle that is a marker for a file kept elsewhere, which is how several systems write it,
// then came to nothing inside the sandbox: `curl` and `wget` answered with a broken CA bundle,
// and the only way out the command was left with was `--insecure`, which is no way out at all.
// A file goes in by its name, and where the name is a marker for an item kept elsewhere the
// item itself goes in under its own name, so the bundle is whole whichever way a system keeps
// it, and the sandbox starts whichever way the marker is written.
const CA_PATHS: [&str; 8] = [
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/ssl/certs/ca-bundle.crt",
    "/etc/ssl/cert.pem",
    "/etc/ssl/openssl.cnf",
    "/etc/ca-certificates.conf",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/pki/tls/openssl.cnf",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
];

// The folders a system writes its bundle into, when the file under /etc is a marker for one of
// them. They hold nothing but the markers that say who may be trusted, which is what the
// directory of the certificates holds as well.
const CA_ROOMS: [&str; 1] = ["/var/lib/ca-certificates"];

// What a command of the sandbox may reach beyond its own folder. Every part of it remains off,
// so a command sees nothing of the computer and reaches nothing beyond the working directory of
// the chat until the user opens it in the menu of the sandbox badge. The three remain off for a
// new chat, and for a chat written before they existed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Access {
    // A command may reach the network.
    pub network: bool,
    // `pip install` works. The chat gets a Python environment of its own to install into, and
    // the network to fetch from.
    pub pip: bool,
    // The commands that fetch a file of the web, `wget`, `curl` and `fetch`, may run.
    pub fetch: bool,
}

impl Access {
    // True when a command of this chat may reach the network at all. An install and a fetch
    // both need it, so either of them lets it through.
    pub fn net(&self) -> bool {
        self.network || self.pip || self.fetch
    }
}

// The tool description handed to the model. The call is requested one item: the command. No
// reason for the command is requested, for a model informed to say why it runs something spends
// its text on a line the user can work out from the command itself. A model that gives a reason
// of its own is still read, and the user still sees it, so a chat written before this change
// keeps what it had.
//
// The text follows the state of the sandbox, which is the only setting that can sandbox a
// command. A chat with the sandbox on is told the boundaries of the sandbox and what the chat
// lets a command reach beyond its own folder, so the model is not told a command has no
// network where the user opened the network, and is not left to find out by running something
// that fails. A chat with the sandbox off is told the command and the folder it starts in,
// and nothing else: the command of such a chat runs under the same eye as the file tools, and a
// line about a sandbox that is not there has turned a model away from a command it should have
// run.
pub fn definition(sandbox: bool, access: Access) -> Value {
    let mut reach = String::from(
        "Run one shell command and return its output. The command runs in the working directory of the chat. ",
    );
    if sandbox {
        reach.push_str(
            "The command runs in the sandbox, so it can see only that directory and the programs of the computer: no other file of the computer is visible, the home folder included. The path `/tmp/` resolves to a relative `./tmp/` directory of the working directory, so a file left there is kept, is seen by the next command of this chat, and is a file of the working directory of the chat. ",
        );
        reach.push_str(if access.net() {
            "The command may reach the network. "
        } else {
            "There is no network access. "
        });
        if access.pip {
            reach.push_str(
                "Python packages may be installed: pip is on the PATH and installs into a Python environment of this chat at .venv. ",
            );
        }
        if access.fetch {
            reach.push_str("wget, curl and fetch may be used to read a page of the web.");
        } else {
            reach.push_str("wget, curl and fetch are turned off, so do not request a file of the web with them.");
        }
    }
    json!({
        "type": "function",
        "function": {
            "name": TOOL_NAME,
            "description": with_rule(reach),
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to run, as one line of text."
                    }
                },
                "required": ["command"]
            }
        }
    })
}

// The rule every tool is proposed under. A model reads the list of tools at the header of every
// ask, and a model that has been asked to call a tool on every query spends its first turn of a
// chat trying to answer a message that requested nothing: it writes to itself that no real
// question is there, that the rule it was given cannot be met, and that it will answer briefly
// after all. That text come out as thinking before an answer that goes on as if none of it had
// been stated. The rule is the same one a person works under already, and saying it once takes
// the argument out of the model's way.
pub const CALL_RULE: &str = "Use a tool only when the user requested something a tool can do, or when it is plainly needed for the work. Never call a tool to fill a turn: answering in text is the right item when no tool is wanted.";

// Hang the rule of the tools on the description of one of them. A tool keeps its own text
// first, so what it does is read before the rule that shows when to use it.
pub fn with_rule(description: String) -> String {
    format!("{} {}", description.trim_end(), CALL_RULE)
}

// What one call of a tool the program does itself produced: the text the model is given back,
// and whether the call went well.
pub struct Report {
    pub ok: bool,
    pub output: String,
}

// What one run produced.
pub struct Outcome {
    pub ok: bool,
    pub output: String,
    // True when bubblewrap really wrapped the command and the command ran.
    pub sandboxed: bool,
    pub timed_out: bool,
}

impl Outcome {
    // Text for the tool block in the chat window.
    pub fn summary(&self) -> String {
        if self.timed_out {
            // Whatever the command printed before it was stopped is kept. A model that is
            // informed a call ran out of time still has to know what the call had to say up to
            // that moment, and throwing the text away loses the one item the run managed to do.
            if self.output.trim().is_empty() {
                return String::from(
                    "The call ran out of time and was stopped. It had printed nothing by then.",
                );
            }
            return format!(
                "The call ran out of time and was stopped. Everything it had printed by then remains below.\n{}",
                self.output
            );
        }
        self.output.clone()
    }
}

// Where bwrap is. None means the sandbox cannot be used. The answer is kept, so the computer is
// not searched again for every command.
pub fn bwrap_path() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            for candidate in [
                "/usr/bin/bwrap",
                "/usr/local/bin/bwrap",
                "/run/current-system/sw/bin/bwrap",
            ] {
                let path = PathBuf::from(candidate);
                if path.is_file() {
                    return Some(path);
                }
            }
            // Ask the PATH of the computer, so a folder outside the usual ones works.
            let found = Command::new("bwrap")
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            match found {
                Ok(_) => Some(PathBuf::from("bwrap")),
                Err(_) => None,
            }
        })
        .as_deref()
}

// True when bwrap is on this computer. A computer without it cannot sandbox anything, and the
// window records it.
pub fn installed() -> bool {
    bwrap_path().is_some()
}

// The folder `/tmp` of the sandbox means: `tmp` inside the working directory of the chat. It is
// created here, because the sandbox cannot start without it.
fn jail_tmp(workdir: &Path) -> String {
    let room = workdir.join(TMP_ROOM);
    let _ = std::fs::create_dir_all(&room);
    room.display().to_string()
}

// The path of the working directory as the sandbox sees it. Links are followed, so the place
// that is mounted and the place the command starts in are one place.
fn jail_workdir(workdir: &Path) -> String {
    workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf())
        .display()
        .to_string()
}

// One of the program folders of the system, put into the sandbox. A folder that is a link, as
// /bin is on a system where everything lives under /usr, is created a link in the sandbox too,
// so the paths the programs know keep working.
fn push_program_path(args: &mut Vec<String>, path: &Path) {
    let text = path.display().to_string();
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_symlink() {
        // The target is kept as it is written, so a relative link stays relative and an
        // absolute one stays absolute.
        let target = std::fs::read_link(path)
            .map(|t| t.display().to_string())
            .unwrap_or_else(|_| String::from("/usr"));
        args.push("--symlink".into());
        args.push(target);
        args.push(text);
        return;
    }
    if meta.is_dir() {
        args.push("--ro-bind-try".into());
        args.push(text.clone());
        args.push(text);
    }
}

// Put one file or folder of the computer into the sandbox read only. A name that means nothing
// is passed over, which is how a list of files that a system may or may not have is worked
// through.
//
// A name that is a marker for a file kept elsewhere is mounted under its own name only while
// the folder it is in is out of the sandbox. The directory of the certificates is mounted whole,
// so the marker comes with it, and bubblewrap will not mount on a marker: the whole sandbox
// cannot start. That is how a certificate bundle that is a marker, which is how Arch and Fedora
// write it, kept every command of the sandbox from running on those systems. Where the marker is
// in the sandbox already, the target of the marker is mounted under its own name, so the marker
// still leads somewhere and a command that fetches a web page reads the bundle whole.
fn push_read_only(args: &mut Vec<String>, path: &str) {
    if !stands_as_a_mark(Path::new(path)) || !folder_in_the_jail(args, path) {
        args.push("--ro-bind-try".into());
        args.push(path.into());
        args.push(path.into());
        return;
    }
    // The marker leads nowhere that can be named from here, so it is passed over: a marker that
    // hangs in the environment inside the sandbox is better than a sandbox that never starts.
    let Ok(whole) = std::fs::canonicalize(path) else {
        return;
    };
    let stood = whole.display().to_string();
    // The target of the marker came into the sandbox with its own folder, so nothing is
    // added.
    if folder_in_the_jail(args, &stood) {
        return;
    }
    args.push("--ro-bind-try".into());
    args.push(stood.clone());
    args.push(stood);
}

// The markers that put a directory of the computer into the sandbox. After one of them come the
// place on the computer and the place in the sandbox.
const FOLDER_MARKS: [&str; 4] = ["--ro-bind", "--ro-bind-try", "--bind", "--bind-try"];

// True when the folder that holds the name is in the sandbox already, because a directory of
// the computer was put in over it. The text of such a folder come from the computer, markers
// and all, so the name is in the sandbox whatever it is.
fn folder_in_the_jail(args: &[String], path: &str) -> bool {
    let Some(folder) = Path::new(path).parent() else {
        return false;
    };
    let mut at = 0;
    while at < args.len() {
        if !FOLDER_MARKS.contains(&args[at].as_str()) {
            at += 1;
            continue;
        }
        // The marker, the place on the computer, and the place in the sandbox.
        let Some(inside) = args.get(at + 2) else { break };
        if folder.starts_with(Path::new(inside)) {
            return true;
        }
        at += 3;
    }
    false
}

// True when the name is a marker for a file kept elsewhere, and not the file itself.
fn stands_as_a_mark(place: &Path) -> bool {
    std::fs::symlink_metadata(place)
        .map(|made| made.is_symlink())
        .unwrap_or(false)
}

// True when the name means a directory of itself, and not for a marker that points somewhere
// else. A marker is passed over, because the place it leads to cannot be named from here.
fn stands_as_a_folder(place: &Path) -> bool {
    std::fs::symlink_metadata(place).map(|made| made.is_dir()).unwrap_or(false)
}

// True when the name means a file of the rules of a program: text that the program reads, or
// the settings of it. A marker that points somewhere else is not such a file, whatever its name
// reports.
fn stands_as_a_rule_file(place: &Path) -> bool {
    if !std::fs::symlink_metadata(place).map(|made| made.is_file()).unwrap_or(false) {
        return false;
    }
    let Some(end) = place.extension().and_then(|one| one.to_str()) else {
        return false;
    };
    end == "properties" || end == "cfg"
}

// The name of the folder that holds the way in to the console of a running Java program, and
// the text that let one in. It is kept out of the sandbox.
const JAVA_MANAGEMENT: &str = "management";

// The rules Java works under. A system that puts the programs of Java under /usr often puts
// their rules under /etc, and builds the programs to read them there. The empty root of the
// sandbox has none of them, so a run of `java` inside came to its rules and found nothing: the
// markers of the computer, the list of what may not be trusted, and the settings of the network
// were all removed, and the program went on with its own least rules. The `security` folder of
// every Java under /etc goes in read only, with the files of text and of settings that are
// beside it. A Java that is a marker for a place elsewhere is passed over, and so is
// `management/`.
fn java_paths() -> Vec<String> {
    java_rules(Path::new("/etc")).iter().map(|one| one.display().to_string()).collect()
}

// The rules of the Java programs that are under one root. The root is an item of the check as
// well, so the two of them can be checked without a computer of the type that keeps Java under
// /etc.
fn java_rules(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    let mut homes: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let place = entry.path();
        let stands = place.file_name().and_then(|one| one.to_str()).unwrap_or("");
        if !stands.starts_with("java") || !stands_as_a_folder(&place) {
            continue;
        }
        homes.push(place);
    }
    homes.sort();
    for home in homes {
        let security = home.join("security");
        if stands_as_a_folder(&security) {
            found.push(security);
        }
        let Ok(inner) = std::fs::read_dir(&home) else { continue };
        for one in inner.flatten() {
            let place = one.path();
            if place.file_name().and_then(|name| name.to_str()) == Some(JAVA_MANAGEMENT) {
                continue;
            }
            if stands_as_a_rule_file(&place) {
                found.push(place);
            }
        }
    }
    found.sort();
    found
}

// Build the argument list of the sandbox, up to the two markers that end it. The working
// directory is the only place a command may write.
fn jail_args(workdir: &Path, access: Access) -> Vec<String> {
    let wd = jail_workdir(workdir);
    let mut args: Vec<String> = vec![
        // No other processes, no other users, no other names, no other control groups, and no
        // network unless the chat opened it.
        if access.net() {
            // bwrap has no "all of them but the network", so the namespaces are named one by
            // one and the network namespace is the one left out.
            "--unshare-user".into()
        } else {
            "--unshare-all".into()
        },
    ];
    if access.net() {
        for part in ["--unshare-ipc", "--unshare-pid", "--unshare-uts", "--unshare-cgroup"] {
            args.push(part.into());
        }
    }
    args.extend_from_slice(&[
        // The user namespace is requested on its own as well, so the next option is honoured
        // and not quietly skipped.
        "--unshare-user".into(),
        // A command inside cannot build a sandbox of its own to climb out of.
        "--disable-userns".into(),
        // The sandbox is removed with the program, so no command is left remaining behind a
        // closed window.
        "--die-with-parent".into(),
        // A terminal session of its own, so a signal sent to the session of the program cannot
        // reach a command inside.
        "--new-session".into(),
        "--hostname".into(),
        JAIL_HOSTNAME.into(),
        // The empty root. Nothing of this computer is there until it is put in.
        "--tmpfs".into(),
        "/".into(),
    ]);
    // The programs of the computer, read only.
    for path in PROGRAM_PATHS.iter().chain(EXTRA_PROGRAM_PATHS.iter()) {
        push_program_path(&mut args, Path::new(path));
    }
    // The space `/tmp` of the sandbox, which is `tmp` of the working directory. The sandbox
    // cannot start without it, so it is created here, and not by the command.
    let tmp = jail_tmp(workdir);
    // The parts of a root a command still needs to run at all. /dev holds the few nodes a
    // program requests, and no node of the host. /proc is a new one, so the processes of the
    // computer are not to be seen. /tmp is the `tmp` folder of the working directory, so a file
    // written there is kept with the working directory of the chat and seen by the commands that
    // come after it.
    args.extend_from_slice(&[
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
        "--dir".into(),
        "/etc".into(),
    ]);
    // The few files of /etc, one by one. A missing one is passed over.
    for file in ETC_PATHS {
        args.push("--ro-bind-try".into());
        args.push(file.into());
        args.push(file.into());
    }
    // The bundle of the certificates and the file that shows how to read it, by their names, so
    // a bundle that is a marker for a file kept elsewhere still comes whole. The space the
    // bundle is written into goes in as well, for the markers of the certificates are searched
    // for there on such a system.
    for file in CA_PATHS {
        push_read_only(&mut args, file);
    }
    for room in CA_ROOMS {
        push_read_only(&mut args, room);
    }
    // The rules of Java, which the programs of the computer are built to read under /etc.
    for rule in java_paths() {
        push_read_only(&mut args, &rule);
    }
    // The directory `/tmp` maps to, which is a directory of the working directory. It is mounted before
    // the working directory itself, so a working directory that is under `/tmp` of the host
    // keeps its own place in the sandbox: the mount of the working directory is then put over
    // this one, where it belongs.
    args.push("--bind".into());
    args.push(tmp.clone());
    args.push("/tmp".into());
    // The working directory of the chat: the one place that may be written.
    args.push("--bind".into());
    args.push(wd.clone());
    args.push(wd.clone());
    // The environment of the host is removed inside the sandbox, so no variable of the user can
    // point a command at a place of the computer.
    args.push("--clearenv".into());
    // An install goes into the Python environment of the chat, so that directory is at the
    // header of the path of the programs and `pip` is the pip of it.
    let mut path = String::from(JAIL_PATH);
    let room = venv_room(workdir, access.pip);
    if let Some(bin) = room.paths {
        path = format!("{}:{}", bin, path);
    }
    args.extend_from_slice(&[
        "--setenv".into(),
        "PATH".into(),
        path,
        "--setenv".into(),
        "HOME".into(),
        wd.clone(),
        "--setenv".into(),
        "TMPDIR".into(),
        "/tmp".into(),
        "--setenv".into(),
        "SHELL".into(),
        SHELL.into(),
        "--setenv".into(),
        "LANG".into(),
        "C.UTF-8".into(),
        "--setenv".into(),
        "TERM".into(),
        "dumb".into(),
        "--setenv".into(),
        "PAGER".into(),
        "cat".into(),
        "--setenv".into(),
        "GIT_PAGER".into(),
        "cat".into(),
    ]);
    if let Some(env) = room.env {
        // `pip install` has to write somewhere, and every folder of the computer is read only
        // in here. The environment of the chat is that place. A system that keeps its own
        // Python packages out of the reach of a program is answered as well, or it refuses the
        // install of itself.
        args.extend_from_slice(&[
            "--setenv".into(),
            "VIRTUAL_ENV".into(),
            env,
            "--setenv".into(),
            "PIP_BREAK_SYSTEM_PACKAGES".into(),
            "1".into(),
        ]);
    }
    args.extend_from_slice(&["--chdir".into(), wd]);
    args
}

// What the Python environment of a chat gave the sandbox: the directory of its programs, put at
// the header of the path, and the directory of the environment itself.
#[derive(Default)]
struct VenvRoom {
    paths: Option<String>,
    env: Option<String>,
}

// The folder the programs of the Python environment of a chat are in. It is in the working
// directory of the chat, so what is installed is kept with the working directory of the chat, and
// reaches nothing else.
fn venv_dir(workdir: &Path) -> PathBuf {
    workdir.join(".venv")
}

// Where the programs of the environment remain, of the two forms the two types of system use.
// Nothing when the environment is not there.
fn venv_bin(venv: &Path) -> Option<String> {
    for name in ["bin", "Scripts"] {
        let room = venv.join(name);
        if room.join("pip").is_file() || room.join("pip.exe").is_file() {
            return Some(room.display().to_string());
        }
    }
    None
}

// Make the Python environment of the chat when the chat may install packages, and say what it
// gives the sandbox. An environment that remains already is taken as it is now. One that cannot
// be made is requested once and not again, and the command is then served the pip of the
// computer, which shows its own trouble.
fn venv_room(workdir: &Path, wanted: bool) -> VenvRoom {
    if !wanted {
        return VenvRoom::default();
    }
    let venv = venv_dir(workdir);
    if venv_bin(&venv).is_none() {
        // A computer that cannot make an environment is asked once, and not again for every
        // command of the chat.
        let key = venv.display().to_string();
        let mut ask = false;
        if let Ok(mut set) = venv_tried().lock() {
            ask = set.insert(key);
        }
        if ask {
            make_venv(&venv);
        }
    }
    // Without an environment the sandbox keeps the path of the programs it always had, and
    // reports nothing of pip.
    let Some(bin) = venv_bin(&venv) else {
        return VenvRoom::default();
    };
    VenvRoom { paths: Some(bin), env: Some(venv.display().to_string()) }
}

// The environments this run of the program has been asked to make.
fn venv_tried() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static TRIED: OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    TRIED.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

// Ask the interpreter of the computer for an environment of the chat. The packages of the
// computer are left in sight, so the chat gains a place to install into and loses nothing it
// could read before.
fn make_venv(venv: &Path) {
    let room = venv.parent().unwrap_or_else(|| Path::new("/"));
    let place = venv.display().to_string();
    for name in ["python3", "python"] {
        let capture = util::capture_in(
            name,
            &["-m", "venv", "--system-site-packages", &place],
            Some(room),
            Duration::from_secs(120),
        );
        if capture.ok && venv_bin(venv).is_some() {
            return;
        }
    }
    // A half environment is worse than none at all, for it is on the path of the programs with
    // no pip in it.
    if venv_bin(venv).is_none() {
        let _ = std::fs::remove_dir_all(venv);
    }
}

// bubblewrap writes its own trouble at the start of the error output and leaves the command
// unrun. A command that ran and failed is another item, and it is distinguished by having
// printed something, or by ending on a signal of its own.
fn jail_failure(capture: &util::Capture) -> Option<String> {
    if capture.timed_out || !capture.out.trim().is_empty() || capture.code == 0 {
        return None;
    }
    let line = capture.err.lines().find(|line| !line.trim().is_empty())?;
    if !line.trim().starts_with("bwrap:") {
        return None;
    }
    Some(capture.err.trim().to_string())
}

// The commands that fetch a file of the web by name. They are kept out of the reach of a
// command until the user lets them, for fetching a page is the usual first step of a command
// that reaches further than the chat should.
const FETCH_NAMES: [&str; 3] = ["wget", "curl", "fetch"];

// The text of a command, cut at every marker that starts another one: `ls; wget x` is two
// parts, and either of them may be the one that fetches. The markers of
// a pair, as `&&` and `||`, cut at one of them and not at the other, which makes
// no difference to the parts that come out.
fn parts(command: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut one = String::new();
    for c in command.chars() {
        if c == ';' || c == '\n' || c == '|' || c == '&' || c == '(' || c == ')' {
            if !one.trim().is_empty() {
                out.push(one.trim().to_string());
            }
            one = String::new();
            continue;
        }
        one.push(c);
    }
    if !one.trim().is_empty() {
        out.push(one.trim().to_string());
    }
    out
}

// The name of the program one part of a command runs, with the wrappers and the variables of
// the part passed over. `sudo /usr/bin/wget`, `FOO=1 wget` and `time -p curl` all name the
// program that does the work.
fn head_word(part: &str) -> (String, Vec<String>) {
    // The text that may remains before the program of a part without being it.
    let skip = ["sudo", "command", "env", "time", "nohup", "nice", "stdbuf", "doas"];
    let mut words: Vec<String> = Vec::new();
    for word in part.split_whitespace() {
        if words.is_empty() {
            // A variable set for one command alone, as `PIP_NO_INPUT=1 pip install x`, reports
            // nothing of what the command runs.
            if let Some((name, rest)) = word.split_once('=') {
                if !name.is_empty() && !rest.is_empty() {
                    continue;
                }
            }
            let bare = word.rsplit('/').next().unwrap_or(word);
            if bare.is_empty() || skip.contains(&bare) {
                continue;
            }
        }
        words.push(word.to_string());
    }
    let head = words.first().cloned().unwrap_or_default();
    let bare = head.rsplit('/').next().unwrap_or(&head).to_string();
    (bare, words)
}

// True when the command requests one of the programs that fetch a page of the web.
pub fn uses_fetch(command: &str) -> bool {
    parts(command).iter().any(|part| {
        let (head, _) = head_word(part);
        FETCH_NAMES.contains(&head.as_str())
    })
}

// True when the part names pip: the program of that name, or `-m pip` of an interpreter.
fn names_pip(words: &[String], head: &str) -> bool {
    if head == "pip" || head == "pip3" || head.ends_with("pip3") || head.ends_with("pip") {
        return true;
    }
    words
        .windows(2)
        .any(|pair| pair[0] == "-m" && (pair[1] == "pip" || pair[1] == "pip3"))
}

// True when the command asks pip to write: to install, to remove, or to build a wheel. `pip
// list` and `pip freeze` read and are skipped.
pub fn uses_pip(command: &str) -> bool {
    parts(command).iter().any(|part| {
        let (head, words) = head_word(part);
        if !names_pip(&words, &head) {
            return false;
        }
        words.iter().any(|word| {
            word == "install" || word == "uninstall" || word == "wheel" || word == "download"
        })
    })
}

// Why a command is refused before it runs, or nothing when the chat allows it. The two are only
// asked of a sandboxed chat: a chat that runs its commands free has already been informed that
// they reach the whole computer.
pub fn refused(command: &str, access: Access) -> Option<String> {
    if !access.fetch && uses_fetch(command) {
        return Some(format!(
            "The command was not run: fetching with {} is off for this chat, and only the user can turn it on in the menu of the Sandbox badge. Do not try the command again, and do not search for another way to the web. State in your reply that this chat does not let a command reach the web, and go on without it.",
            FETCH_NAMES.join(", ")
        ));
    }
    if !access.pip && uses_pip(command) {
        return Some(
            "The command was not run: Python package installs are off for this chat, and only the user can turn them on. Do not try the command again, and do not search for another way to install. State in your reply that this chat does not let a command install packages, and go on without them.".to_string(),
        );
    }
    None
}

// Run one command. sandbox is true when the chat keeps the sandbox on. workdir is the directory
// the command starts in. access is what the chat lets a command reach beyond its own folder.
pub fn run(
    command: &str,
    workdir: &Path,
    sandbox: bool,
    access: Access,
    timeout: Duration,
) -> Outcome {
    run_watched(
        command,
        workdir,
        sandbox,
        access,
        &util::Watch::new(timeout),
    )
}

// The same, with the clock of the call itself. The time a command is given may be lengthened
// while it runs, and the user may cut it short; either way the command is stopped and all it
// had printed returns.
pub fn run_watched(
    command: &str,
    workdir: &Path,
    sandbox: bool,
    access: Access,
    watch: &util::Watch,
) -> Outcome {
    if command.trim().is_empty() {
        return Outcome {
            ok: false,
            output: "The model requested an empty command".to_string(),
            sandboxed: false,
            timed_out: false,
        };
    }
    let _ = std::fs::create_dir_all(workdir);
    if sandbox {
        if let Some(reason) = refused(command, access) {
            return Outcome {
                ok: false,
                output: reason,
                sandboxed: false,
                timed_out: false,
            };
        }
    }

    #[cfg(unix)]
    {
        if sandbox {
            return run_jailed(command, workdir, access, watch);
        }
        let capture = util::capture_watch_in(SHELL, &["-c", command, "natter"], Some(workdir), watch);
        finish(capture, false)
    }
    #[cfg(not(unix))]
    {
        // There is no bubblewrap outside Linux and macOS. A chat that requested the sandbox is
        // not served a plain run behind its back, even when a chat file from another computer
        // carries the marker. The window keeps the badge off in that case, and reports why.
        if sandbox {
            let reason = jail_check()
                .err()
                .unwrap_or_else(|| String::from("bubblewrap is not available on this system"));
            return Outcome {
                ok: false,
                output: format!("The sandbox did not run this command: {}", reason),
                sandboxed: false,
                timed_out: false,
            };
        }
        #[cfg(windows)]
        {
            // Windows has no sandbox. The command runs in the working directory with cmd.exe,
            // and the window shows that the sandbox is unavailable.
            let capture = util::capture_watch_in("cmd.exe", &["/C", command], Some(workdir), watch);
            finish(capture, false)
        }
        #[cfg(not(windows))]
        {
            let _ = watch;
            Outcome {
                ok: false,
                output: "No shell is available on this system".to_string(),
                sandboxed: false,
                timed_out: false,
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        Outcome {
            ok: false,
            output: "No shell is available on this system".to_string(),
            sandboxed: false,
            timed_out: false,
        }
    }
}

// Run one command inside the sandbox. When the sandbox cannot be had, the command does not run
// at all: a chat that requested the sandbox is never given a plain run behind its back.
#[cfg(unix)]
fn run_jailed(command: &str, workdir: &Path, access: Access, watch: &util::Watch) -> Outcome {
    let failed = |reason: String| Outcome {
        ok: false,
        output: format!("The sandbox did not run this command: {}", reason),
        sandboxed: false,
        timed_out: false,
    };
    // A sandbox with the network allowed is a different sandbox, so it is asked the same question
    // for itself.
    if let Err(problem) = jail_check_of(access) {
        return failed(problem);
    }
    let Some(bwrap) = bwrap_path() else {
        return failed(String::from("bubblewrap (bwrap) was not found"));
    };
    let mut args = jail_args(workdir, access);
    args.push("--".into());
    args.push(SHELL.into());
    args.push("-c".into());
    args.push(command.to_string());
    let program = bwrap.display().to_string();
    let capture = util::capture_watch_in(&program, &args, Some(workdir), watch);
    if let Some(problem) = jail_failure(&capture) {
        return failed(problem);
    }
    finish(capture, true)
}

fn finish(capture: util::Capture, sandboxed: bool) -> Outcome {
    let mut output = capture.all();
    if capture.timed_out {
        return Outcome {
            ok: false,
            output: trim_output(&output),
            sandboxed,
            timed_out: true,
        };
    }
    if output.trim().is_empty() {
        output = if capture.ok {
            "(no output)".to_string()
        } else {
            format!("The command failed with exit code {}", capture.code)
        };
    }
    Outcome {
        ok: capture.ok,
        output: trim_output(&output),
        sandboxed,
        timed_out: false,
    }
}

// Keep the output small enough for the chat window and for the model.
fn trim_output(text: &str) -> String {
    if text.chars().count() <= MAX_OUTPUT {
        return text.to_string();
    }
    let head: String = text.chars().take(3000).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(5000)
        .collect::<Vec<char>>()
        .iter()
        .rev()
        .collect();
    format!("{}\n... output truncated in the middle ...\n{}", head, tail)
}

// A place put into a line of the shell. A marker of quoting inside a folder name is created
// safe, so a folder named with one cannot break the line of the check.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

// Whether the sandbox of this computer keeps what it promises. Everything is checked by the
// sandbox itself, in a directory of the test alone: that bubblewrap starts, that a file outside
// the working directory cannot be read, that the working directory can be written, that two
// parts of the root of the computer are not there, and that the network is cut off. The answer
// is kept, so the computer is asked once.
pub fn jail_check() -> Result<(), String> {
    jail_check_of(Access::default())
}

// The same question of the form of the sandbox the chat requests: a sandbox with the network
// allowed keeps its other restrictions, but they are different restrictions and are tested on their own. The
// answer is kept, so the computer is asked once of each of the two forms.
pub fn jail_check_of(access: Access) -> Result<(), String> {
    static PLAIN: OnceLock<Result<(), String>> = OnceLock::new();
    static OPEN: OnceLock<Result<(), String>> = OnceLock::new();
    if access.net() {
        return OPEN
            .get_or_init(|| probe_the_jail(Access { network: true, pip: false, fetch: false }))
            .clone();
    }
    PLAIN.get_or_init(|| probe_the_jail(Access::default())).clone()
}

// The five lines the sandbox is asked to write. The directory belongs to the check alone, so a chat is
// never touched.
#[cfg(unix)]
fn probe_the_jail(access: Access) -> Result<(), String> {
    let Some(bwrap) = bwrap_path() else {
        return Err(String::from(
            "bubblewrap (bwrap) was not found. Install it to run terminal commands in the sandbox.",
        ));
    };
    static TURN: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "autumn-natter-jail-check-{}-{}",
        std::process::id(),
        TURN.fetch_add(1, Ordering::SeqCst)
    ));
    let workdir = root.join("work");
    if std::fs::create_dir_all(&workdir).is_err() {
        return Err(String::from(
            "the sandbox check could not create a working directory",
        ));
    }
    // A file beside the working directory, which no command inside may read.
    let secret = root.join("secret");
    let _ = std::fs::write(&secret, "AUTUMN-NATTER-JAIL-CHECK");
    let secret_text = secret.display().to_string();
    let work_text = jail_workdir(&workdir);
    // Two parts of the root of the computer that are never put into the sandbox: the file that
    // holds the secret text of the accounts, and the folder the computer writes its logs into.
    // Both are on every system of the type this program runs on, so either of them seen inside
    // means the root of the computer got in. The home folder is not checked, for the working
    // directory of a chat may well are inside it.
    let accounts_file = Path::new("/etc").join("shadow");
    let logs_folder = Path::new("/var").join("log");
    // The lines of /proc/net/dev are counted instead of read with a tool, so the check needs
    // nothing but the shell. A new network namespace holds the loop device alone, which is two
    // lines of header and one line of device.
    let mut script = format!(
        "if [ -r {secret} ]; then echo hidden=no; else echo hidden=yes; fi; \
         if : > {work}/probe 2>/dev/null; then echo writable=yes; else echo writable=no; fi; \
         if [ -e {accounts} ]; then echo accounts=seen; else echo accounts=hidden; fi; \
         if [ -e {logs} ]; then echo logs=seen; else echo logs=hidden; fi;",
        secret = shell_quote(&secret_text),
        work = shell_quote(&work_text),
        accounts = shell_quote(&accounts_file.display().to_string()),
        logs = shell_quote(&logs_folder.display().to_string())
    );
    // The network is only asked of a sandbox that promised to cut it off.
    if !access.net() {
        script.push_str(
            " lines=0; while IFS= read -r row; do lines=$((lines+1)); done < /proc/net/dev 2>/dev/null; \
             if [ \"$lines\" = '3' ]; then echo net=off; else echo \"net=on:$lines\"; fi",
        );
    }
    let mut args = jail_args(&workdir, access);
    args.push("--".into());
    args.push(SHELL.into());
    args.push("-c".into());
    args.push(script);
    let program = bwrap.display().to_string();
    let capture = util::capture_in(&program, &args, Some(&workdir), Duration::from_secs(20));
    let _ = std::fs::remove_dir_all(&root);
    if capture.timed_out {
        return Err(String::from("bubblewrap did not answer the sandbox check"));
    }
    if let Some(problem) = jail_failure(&capture) {
        return Err(format!("bubblewrap could not start the sandbox: {}", problem));
    }
    let problems = check_lines(&capture.all(), access.net());
    if problems.is_empty() {
        return Ok(());
    }
    Err(problems.join(". "))
}

// Nothing to check where bubblewrap cannot run at all.
#[cfg(not(unix))]
fn probe_the_jail(_access: Access) -> Result<(), String> {
    Err(String::from(
        "bubblewrap is not available on this system, so commands cannot be sandboxed",
    ))
}

// What the answer of the sandbox reports is wrong. An empty answer means the sandbox keeps
// everything it promises.
fn check_lines(text: &str, wants_net: bool) -> Vec<String> {
    let holds = |line: &str| text.lines().any(|seen| seen.trim() == line);
    let mut problems = Vec::new();
    // A file beside the working directory that can still be read is the worst of them: keeping
    // it out is what the sandbox is there for.
    if !holds("hidden=yes") {
        problems.push(String::from(
            "a file outside the working directory was readable inside the sandbox",
        ));
    }
    if !holds("writable=yes") {
        problems.push(String::from(
            "the working directory was not writable inside the sandbox",
        ));
    }
    // Either of the two parts of the host root remaining inside reports the same: the root of
    // the computer was let in. The text is kept as one so the window reports it once.
    if !holds("accounts=hidden") || !holds("logs=hidden") {
        problems.push(String::from(
            "the root of the computer is visible inside the sandbox",
        ));
    }
    if !wants_net && !holds("net=off") {
        problems.push(String::from("the network is not blocked inside the sandbox"));
    }
    problems
}

// Read the arguments of a tool call. The model sometimes writes no JSON at all, so a plain line
// is taken as the command. The second half of the answer is the reason the model gave for the
// command. That reason is no longer requested, but it is still read here, so a model that gives
// one of its own, and a chat written before the change, are shown as they were.
pub fn parse_arguments(raw: &str) -> (String, String) {
    let text = raw.trim();
    if text.is_empty() {
        return (String::new(), String::new());
    }
    // Cut a JSON object out of a fenced block, if the model added one.
    let cleaned = extract_object(text).unwrap_or(text);
    if let Ok(value) = serde_json::from_str::<Value>(cleaned) {
        let command = value
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        let purpose = value
            .get("purpose")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        if !command.trim().is_empty() {
            return (command, purpose);
        }
        // Some models use "cmd" or "shell" instead.
        for key in ["cmd", "shell", "script"] {
            if let Some(alt) = value.get(key).and_then(|c| c.as_str()) {
                if !alt.trim().is_empty() {
                    return (alt.to_string(), purpose);
                }
            }
        }
    }
    // Not JSON. Take the text itself, without a code fence.
    let mut plain = text.to_string();
    for fence in ["```json", "```", "~~~"] {
        if let Some(rest) = plain.strip_prefix(fence) {
            plain = rest.trim_start().to_string();
        }
        if let Some(rest) = plain.strip_suffix(fence) {
            plain = rest.trim_end().to_string();
        }
    }
    let line = plain.lines().next().unwrap_or("").trim().to_string();
    (line, String::new())
}

// Read the arguments of a call that is still being written. The model sends them piece by
// piece, so the object is often not finished, a quote is left open, and the last marker of an
// escape has no word behind it yet. What is there of a text is given back, and nothing is
// created up of what is not there. The answer is the command and the purpose, as
// `parse_arguments` gives them.
pub fn partial_arguments(raw: &str) -> (String, String) {
    let text = raw.trim_start();
    let command = partial_text(text, "command");
    let purpose = partial_text(text, "purpose");
    if !command.is_empty() || !purpose.is_empty() {
        return (command, purpose);
    }
    // Nothing of a named part has come. A model that writes the command with no markers of JSON
    // at all is answered with the line so far. Text that open an object is no command, and say
    // nothing of one yet.
    if text.is_empty() || text.starts_with('{') {
        return (String::new(), String::new());
    }
    let mut plain = text.to_string();
    for fence in ["```json", "```", "~~~"] {
        if let Some(rest) = plain.strip_prefix(fence) {
            plain = rest.trim_start().to_string();
        }
    }
    let line = plain.lines().next().unwrap_or("").trim().to_string();
    (line, String::new())
}

// The text of one named part of an object that may not be finished. The name is searched for,
// and the text behind it is read to the closing marker, or to the end of what has arrive so
// far. A part this page does not know, or a number where a text was searched for, gives
// nothing. Every tool that lets the user watch a call being written reads its own part out with
// this.
pub fn partial_text(text: &str, wanted: &str) -> String {
    let key = format!("\"{}\"", wanted);
    let Some(at) = text.find(&key) else {
        return String::new();
    };
    let head = text[at + key.len()..].trim_start();
    // The name of a part is worth nothing without the marker of a value behind it, and a value
    // of no quotes is no text.
    let Some(after) = head.strip_prefix(':') else {
        return String::new();
    };
    let Some(value) = after.trim_start().strip_prefix('"') else {
        return String::new();
    };
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            // The part is finished.
            break;
        }
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        // A marker of something the model meant to finish. Without the character behind it
        // there is nothing to give back, and what comes after it cannot be read either, so the
        // rest is left for the next look.
        let Some(next) = chars.next() else { break };
        match next {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' | 'f' => {}
            'u' => {
                let mut hex = String::new();
                for _ in 0..4 {
                    match chars.next() {
                        Some(digit) if digit.is_ascii_hexdigit() => hex.push(digit),
                        _ => return out,
                    }
                }
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(letter) => out.push(letter),
                    None => return out,
                }
            }
            // The markers of a quote, a backslash and the rest mean the character itself.
            other => out.push(other),
        }
    }
    out
}

// All of a call that is being written, shown part by part. Every part of the object that has
// arrive is on a line of its own under the name of it, and a part that is still open gives what
// has come so far. A part that holds a list or another object is given as the JSON of it so
// far, so the user watches the call grow instead of watching an empty block.
//
// This is the back of the live writing of the tool blocks. A tool whose text is worth showing
// in another form, such as the command of a terminal run or the text of a new file, records it
// itself; every other tool is shown with this.
pub fn live_arguments(raw: &str) -> String {
    let letters: Vec<char> = raw.trim_start().chars().collect();
    let mut at = match letters.iter().position(|one| *one == '{') {
        Some(place) => place + 1,
        None => return String::new(),
    };
    let mut out: Vec<String> = Vec::new();
    loop {
        at = rest_of_space(&letters, at);
        if at >= letters.len() {
            break;
        }
        match letters[at] {
            '}' => break,
            ',' => {
                at += 1;
                continue;
            }
            _ => {}
        }
        if letters[at] != '"' {
            break;
        }
        let (name, after) = match read_quoted(&letters, at) {
            Some(made) => made,
            None => break,
        };
        let colon = rest_of_space(&letters, after);
        if colon >= letters.len() {
            out.push(format!("{}:", name));
            break;
        }
        if letters[colon] != ':' {
            at = after;
            continue;
        }
        let place = rest_of_space(&letters, colon + 1);
        if place >= letters.len() {
            out.push(format!("{}:", name));
            break;
        }
        let (value, next) = read_value(&letters, place);
        out.push(format!("{}: {}", name, value));
        if next <= place {
            break;
        }
        at = next;
    }
    out.join("\n")
}

// The place the text go on from, past the spaces of the JSON.
fn rest_of_space(letters: &[char], mut at: usize) -> usize {
    while at < letters.len() && letters[at].is_whitespace() {
        at += 1;
    }
    at
}

// The text of a quoted part, and the place after its closing marker. A part the model has not
// shut yet gives what has come.
fn read_quoted(letters: &[char], at: usize) -> Option<(String, usize)> {
    let mut read = at + 1;
    let mut out = String::new();
    while read < letters.len() {
        let ch = letters[read];
        if ch == '"' {
            return Some((out, read + 1));
        }
        if ch != '\\' {
            out.push(ch);
            read += 1;
            continue;
        }
        let Some(next) = letters.get(read + 1) else {
            return Some((out, read + 1));
        };
        match next {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' | 'f' => {}
            other => out.push(*other),
        }
        read += 2;
    }
    Some((out, read))
}

// The value that is at a place, and the place after it. Text give their own text; a list or an
// object gives the JSON of it as far as it has come, and the rest gives what is there.
fn read_value(letters: &[char], at: usize) -> (String, usize) {
    match letters[at] {
        '"' => match read_quoted(letters, at) {
            Some((words, next)) => (words, next),
            None => (String::new(), at + 1),
        },
        open @ ('{' | '[') => {
            let close = match open {
                '{' => '}',
                _ => ']',
            };
            let mut depth = 0usize;
            let mut read = at;
            let mut inside = false;
            while read < letters.len() {
                let ch = letters[read];
                if inside {
                    if ch == '\\' {
                        read += 1;
                    } else if ch == '"' {
                        inside = false;
                    }
                    read += 1;
                    continue;
                }
                match ch {
                    '"' => inside = true,
                    one if one == open => depth += 1,
                    one if one == close => {
                        depth -= 1;
                        if depth == 0 {
                            return (slice(&letters, at, read + 1), read + 1);
                        }
                    }
                    _ => {}
                }
                read += 1;
            }
            // The model has not shut it yet. Everything from here is the part.
            (slice(&letters, at, letters.len()), letters.len())
        }
        _ => {
            let mut read = at;
            while read < letters.len() && !matches!(letters[read], ',' | '}' | ']') {
                read += 1;
            }
            (slice(&letters, at, read).trim().to_string(), read)
        }
    }
}

// The text of a place in the characters of the call.
fn slice(letters: &[char], from: usize, to: usize) -> String {
    letters[from..to.max(from)].iter().collect()
}

// Return the part between the first brace and the matching one.
pub fn extract_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // The arguments are checked without running anything, so the form of the sandbox is watched
    // even on a computer without bubblewrap.
    #[test]
    fn the_jail_starts_from_an_empty_root() {
        let args = jail_args(Path::new("/work/somewhere"), Access::default());
        let text = args.join("\n");
        // The root of the sandbox is its own empty space, and the root of the host is never
        // put in.
        assert!(text.contains("--tmpfs\n/\n"));
        assert!(!text.contains("--ro-bind\n/\n/\n"));
        assert!(!text.contains("--bind\n/\n/\n"));
        // The sandbox boundaries of the sandbox.
        for mark in [
            "--unshare-all",
            "--unshare-user",
            "--disable-userns",
            "--die-with-parent",
            "--new-session",
        ] {
            assert!(text.contains(mark), "the sandbox lost {}", mark);
        }
        // The working directory is the one place that may be written, and it is the place the
        // command starts in.
        assert!(text.contains("--bind\n/work/somewhere\n/work/somewhere\n"));
        assert!(text.contains("--chdir\n/work/somewhere"));
        // `/tmp` is a directory of the working directory and not a space of the sandbox, so
        // no `tmpfs` is above it.
        assert!(text.contains("--bind\n/work/somewhere/tmp\n/tmp\n"));
        assert!(!text.contains("--tmpfs\n/tmp\n"));
        // No private file of the computer is proposed.
        for forbidden in ["/etc/shadow", "/etc/ssh", "/etc/machine-id", "/home"] {
            assert!(!text.contains(forbidden), "the sandbox proposes {}", forbidden);
        }
    }

    // A command that fetches a page has to read the bundle of the certificates, and the file
    // that shows how to read it, or it answers with a broken bundle and nothing is left to it
    // but to trust nothing. A bundle that is a marker for a file kept elsewhere, which is how
    // Arch and Fedora write it, is proposed under the name of the file it leads to, so the
    // marker in the sandbox still leads somewhere.
    #[test]
    fn the_jail_offers_the_bundle_of_the_certificates() {
        let args = jail_args(Path::new("/work/somewhere"), Access::default());
        let text = args.join("\n");
        for part in [
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/ssl/cert.pem",
            "/etc/ssl/openssl.cnf",
            "/etc/pki/tls/certs/ca-bundle.crt",
            "/var/lib/ca-certificates",
        ] {
            let place = Path::new(part);
            if !stands_as_a_mark(place) {
                assert!(
                    text.contains(&format!("--ro-bind-try\n{}\n{}\n", part, part)),
                    "the sandbox keeps {} out",
                    part
                );
                continue;
            }
            let Ok(whole) = std::fs::canonicalize(place) else { continue };
            let stood = whole.display().to_string();
            assert!(
                text.contains(&format!("--ro-bind-try\n{}\n{}\n", stood, stood)),
                "the sandbox keeps the target of {} out: {}",
                part,
                stood
            );
        }
    }

    // No mount of the sandbox may be made on a marker that is inside a directory of the
    // sandbox, for bubblewrap refuses it and the whole sandbox cannot start. That is how the
    // sandbox of this branch came to nothing on the systems that write their bundle of the
    // certificates as a marker.
    #[test]
    fn no_mount_of_the_jail_is_made_on_a_mark() {
        let args = jail_args(Path::new("/work/somewhere"), Access::default());
        let mut at = 0;
        while at < args.len() {
            if !FOLDER_MARKS.contains(&args[at].as_str()) {
                at += 1;
                continue;
            }
            let dest = args[at + 2].clone();
            assert!(
                !(stands_as_a_mark(Path::new(&dest)) && folder_in_the_jail(&args[..at], &dest)),
                "the sandbox was set to mount on the marker {}",
                dest
            );
            at += 3;
        }
    }

    // The directories of the computer that are in the sandbox are extracted from the arguments
    // built so far, so a name is known to be there already. A folder that is a marker in the
    // sandbox, as `/bin` is, and the empty `/etc` made manually are not folders of the
    // computer, and hold nothing of it.
    #[test]
    fn the_folders_of_the_jail_are_read_out_of_the_arguments() {
        let args: Vec<String> = [
            "--unshare-all",
            "--tmpfs",
            "/",
            "--ro-bind",
            "/usr",
            "/usr",
            "--symlink",
            "usr/bin",
            "/bin",
            "--dir",
            "/etc",
            "--ro-bind-try",
            "/etc/ssl/certs",
            "/etc/ssl/certs",
            "--ro-bind-try",
            "/etc/passwd",
            "/etc/passwd",
        ]
        .iter()
        .map(|one| String::from(*one))
        .collect();
        assert!(folder_in_the_jail(&args, "/etc/ssl/certs/ca-certificates.crt"));
        assert!(folder_in_the_jail(&args, "/usr/share/ca-certificates/ca.p12"));
        assert!(!folder_in_the_jail(&args, "/etc/ssl/openssl.cnf"));
        assert!(!folder_in_the_jail(&args, "/etc/passwd"));
        assert!(!folder_in_the_jail(&args, "/bin/sh"));
    }

    // A bundle that is a marker for a file kept elsewhere is not put into the sandbox under the
    // name of the marker when the directory of the marker is in the sandbox, for the marker
    // came into the sandbox with its folder and bubblewrap will not mount on it. The target of
    // the marker is mounted under its own name instead.
    #[cfg(unix)]
    #[test]
    fn the_jail_never_mounts_on_a_mark() {
        let room = std::fs::canonicalize(room_of("mark")).expect("the room of the test");
        let certs = room.join("certs");
        let kept = room.join("kept");
        std::fs::create_dir_all(&certs).unwrap();
        std::fs::create_dir_all(&kept).unwrap();
        let bundle = kept.join("ca-bundle.trust.crt");
        std::fs::write(&bundle, "TRUSTED").unwrap();
        let mark = certs.join("ca-certificates.crt");
        std::os::unix::fs::symlink(&bundle, &mark).unwrap();
        let mark = mark.display().to_string();
        let bundle = bundle.display().to_string();
        let certs = certs.display().to_string();
        let kept = kept.display().to_string();

        // The directory of the marker is out of the sandbox, so nothing is in the way of the
        // name and bubblewrap follows the marker to the bytes.
        let mut out: Vec<String> = Vec::new();
        push_read_only(&mut out, &mark);
        assert_eq!(
            out,
            [String::from("--ro-bind-try"), mark.clone(), mark.clone()],
            "a marker outside a directory of the sandbox goes in under its own name"
        );

        // The directory of the marker is in the sandbox, and the marker is in it: no argument
        // names the marker, and the target of the marker is mounted as a whole.
        let mut out: Vec<String> =
            vec![String::from("--ro-bind-try"), certs.clone(), certs.clone()];
        push_read_only(&mut out, &mark);
        assert!(
            !out.iter().any(|one| one == &mark),
            "the sandbox was set to mount on the marker: {:?}",
            out
        );
        assert_eq!(
            out[3..],
            [String::from("--ro-bind-try"), bundle.clone(), bundle.clone()],
            "the target of the marker was not mounted under its own name"
        );

        // The item the marker leads to came into the sandbox with its own folder, so nothing at
        // all is added for it.
        let mut out: Vec<String> = vec![
            String::from("--ro-bind-try"),
            certs.clone(),
            certs.clone(),
            String::from("--ro-bind-try"),
            kept.clone(),
            kept,
        ];
        let before = out.len();
        push_read_only(&mut out, &mark);
        assert_eq!(out.len(), before, "an item of the sandbox was mounted again");
        let _ = std::fs::remove_dir_all(&room);
    }

    // The sandbox of a chat whose bundle of the certificates is a marker for a file kept
    // elsewhere, which is how Arch and Fedora write it. The sandbox starts, and the bytes of
    // the bundle come through the marker. On those systems the sandbox could not start at all,
    // and no command of the sandbox ran.
    #[cfg(unix)]
    #[test]
    fn the_jail_starts_when_the_bundle_is_a_mark() {
        if let Err(problem) = jail_check() {
            eprintln!("the sandbox of this box is not usable: {}", problem);
            return;
        }
        let room = std::fs::canonicalize(room_of("bundle")).expect("the room of the test");
        let certs = room.join("etc/ssl/certs");
        let kept = room.join("etc/ca-certificates/extracted");
        std::fs::create_dir_all(&certs).unwrap();
        std::fs::create_dir_all(&kept).unwrap();
        let bundle = kept.join("ca-bundle.trust.crt");
        std::fs::write(&bundle, "TRUSTED-BYTES\n").unwrap();
        let mark = certs.join("ca-certificates.crt");
        std::os::unix::fs::symlink(&bundle, &mark).unwrap();
        // The arguments of the sandbox of a chat, with the directories of the test put in as
        // the sandbox puts in the directories of a computer: the directory of the certificates
        // first, and the bundle by its name after it.
        let mut args: Vec<String> = [
            "--unshare-all",
            "--unshare-user",
            "--disable-userns",
            "--die-with-parent",
            "--new-session",
            "--hostname",
            JAIL_HOSTNAME,
            "--tmpfs",
            "/",
        ]
        .iter()
        .map(|one| String::from(*one))
        .collect();
        for path in PROGRAM_PATHS.iter().chain(EXTRA_PROGRAM_PATHS.iter()) {
            push_program_path(&mut args, Path::new(path));
        }
        args.extend_from_slice(&[
            String::from("--dev"),
            String::from("/dev"),
            String::from("--dir"),
            String::from("/etc"),
        ]);
        push_read_only(&mut args, &certs.display().to_string());
        push_read_only(&mut args, &mark.display().to_string());
        args.extend_from_slice(&[
            String::from("--"),
            String::from(SHELL),
            String::from("-c"),
            format!("cat '{}'", mark.display()),
        ]);
        let bwrap = bwrap_path().expect("bubblewrap stands on this box");
        let made = util::capture(&bwrap.display().to_string(), &args, Duration::from_secs(20));
        let _ = std::fs::remove_dir_all(&room);
        assert!(made.ok, "the sandbox did not start: {}", made.err);
        assert!(
            made.out.contains("TRUSTED-BYTES"),
            "the bundle did not come through the marker: {} / {}",
            made.out,
            made.err
        );
    }

    // The rules of Java are under /etc on the systems that put their programs under /usr. They
    // are extracted from that root, and three items are left out: a program that is a marker
    // for a place elsewhere, a `security` folder that is such a marker, and `management/`,
    // which holds the text of the console.
    #[cfg(unix)]
    #[test]
    fn the_rules_of_java_are_read_out_of_their_root() {
        static TURN: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "autumn-natter-java-rules-{}-{}",
            std::process::id(),
            TURN.fetch_add(1, Ordering::SeqCst)
        ));
        let home = root.join("java-21-openjdk");
        std::fs::create_dir_all(home.join("security/policy")).unwrap();
        std::fs::write(home.join("security/java.security"), "rules").unwrap();
        std::fs::write(home.join("net.properties"), "network").unwrap();
        std::fs::write(home.join("jvm-amd64.cfg"), "flags").unwrap();
        std::fs::write(home.join("psfont.properties.ja"), "fonts").unwrap();
        std::fs::write(home.join("notes.txt"), "words").unwrap();
        std::fs::create_dir_all(home.join("management")).unwrap();
        std::fs::write(home.join("management/jmxremote.password"), "SECRET").unwrap();
        // A Java that is a marker for a place elsewhere, and a `security` folder that is such a
        // marker.
        std::fs::create_dir_all(root.join("elsewhere/thing")).unwrap();
        std::fs::create_dir_all(root.join("java-mock")).unwrap();
        std::os::unix::fs::symlink("../elsewhere/thing", root.join("java-mark")).unwrap();
        std::os::unix::fs::symlink("../elsewhere", root.join("java-mock/security")).unwrap();
        // A program of another type, with a directory of rules of its own.
        std::fs::create_dir_all(root.join("python-3/security")).unwrap();
        let found: Vec<String> = java_rules(&root)
            .iter()
            .map(|one| one.strip_prefix(&root).unwrap().display().to_string())
            .collect();
        assert_eq!(
            found,
            vec![
                "java-21-openjdk/jvm-amd64.cfg",
                "java-21-openjdk/net.properties",
                "java-21-openjdk/security",
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // bubblewrap leaves the command unrun when the sandbox cannot be made. That is
    // distinguished from a command that ran and failed.
    #[test]
    fn the_failure_of_a_jail_is_told_from_a_failing_command() {
        let made = |out: &str, err: &str, code: i32| util::Capture {
            ok: code == 0,
            code,
            out: out.to_string(),
            err: err.to_string(),
            timed_out: false,
        };
        // A sandbox that could not be made.
        let unmade = made("", "bwrap: Creating new namespace failed: Operation not permitted", 1);
        assert!(jail_failure(&unmade).is_some());
        // A command that ran and failed. Its own text is in the output.
        let failed = made("nothing here", "", 2);
        assert!(jail_failure(&failed).is_none());
        // A command that printed a line to the error output and nothing else.
        let noisy = made("", "cat: no such file", 1);
        assert!(jail_failure(&noisy).is_none());
        // A command that ran out of time is never a sandbox that failed to start.
        let late = util::Capture {
            ok: false,
            code: -1,
            out: String::new(),
            err: String::from("bwrap: something"),
            timed_out: true,
        };
        assert!(jail_failure(&late).is_none());
    }

    // The places of the check are named in a line of the shell, and a directory of the computer
    // may carry a marker of quoting in its name.
    #[test]
    fn a_place_of_the_check_is_named_safely() {
        assert_eq!(shell_quote("/tmp/work"), "'/tmp/work'");
        assert_eq!(shell_quote("/home/o'brien/work"), r#"'/home/o'\''brien/work'"#);
        // The text of the check keeps their form when a name holds a space.
        let line = format!("if [ -r {} ]; then echo yes; fi", shell_quote("/tmp/a b/c"));
        assert_eq!(line, "if [ -r '/tmp/a b/c' ]; then echo yes; fi");
    }

    // The text the sandbox writes back are read for what is wrong with it.
    #[test]
    fn the_answer_of_the_check_is_read() {
        let good = "hidden=yes\nwritable=yes\naccounts=hidden\nlogs=hidden\nnet=off\n";
        assert!(check_lines(good, false).is_empty());
        // A file outside the working directory that can be read is the worst of them.
        let bad = good.replace("hidden=yes", "hidden=no");
        let problems = check_lines(&bad, false);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("outside the working directory"));
        // The root of the computer got in, and the network was left on. Two of the five lines
        // remain wrong, and they are informed as two errors.
        let worse = good
            .replace("logs=hidden", "logs=seen")
            .replace("net=off", "net=on:7");
        assert_eq!(check_lines(&worse, false).len(), 2);
        // The root seen in both halves is still one error of one type.
        let both = good.replace("logs=hidden", "logs=seen").replace(
            "accounts=hidden",
            "accounts=seen",
        );
        assert_eq!(check_lines(&both, false).len(), 1);
        // Nothing at all is not a good sandbox.
        assert_eq!(check_lines("", false).len(), 4);
        // A sandbox that was requested the network is not faulted for having it.
        let open = "hidden=yes\nwritable=yes\naccounts=hidden\nlogs=hidden\n";
        assert!(check_lines(open, true).is_empty());
        assert_eq!(check_lines(open, false).len(), 1);
    }

    // The three settings of the sandbox remain off, and the network is opened by an install or
    // a fetch as much as by its own switch.
    #[test]
    fn every_part_of_the_access_stands_off() {
        let off = Access::default();
        assert!(!off.network && !off.pip && !off.fetch);
        assert!(!off.net());
        // Either of the two that need the network lets it through.
        assert!(Access { pip: true, ..Access::default() }.net());
        assert!(Access { fetch: true, ..Access::default() }.net());
        assert!(Access { network: true, ..Access::default() }.net());
    }

    // The sandbox of a chat that may reach the web keeps every other sandbox boundary, and the
    // network namespace is the one it does not unshare.
    #[test]
    fn a_jail_with_the_network_names_the_walls_one_by_one() {
        let open = jail_args(
            Path::new("/work/somewhere"),
            Access { network: true, pip: true, fetch: false },
        );
        let text = open.join("\n");
        assert!(!text.contains("--unshare-all"), "the network must be skipped");
        for mark in [
            "--unshare-user",
            "--unshare-ipc",
            "--unshare-pid",
            "--unshare-uts",
            "--unshare-cgroup",
            "--disable-userns",
        ] {
            assert!(text.contains(mark), "the sandbox lost {}", mark);
        }
        // Without the install the path is the path it always was, and pip is given nothing.
        let plain = jail_args(Path::new("/work/somewhere"), Access::default());
        let words = plain.join("\n");
        assert!(!words.contains(".venv"));
        assert!(!words.contains("PIP_BREAK_SYSTEM_PACKAGES"));
        assert!(words.contains("--unshare-all"));
    }

    // An install goes into the Python environment of the chat, so the path of the programs
    // starts in that environment and pip is informed where it is. The program makes the
    // environment itself, so this test plants one and asks that the sandbox is informed of it.
    #[test]
    fn an_install_goes_into_the_environment_of_the_chat() {
        let room = std::env::temp_dir().join(format!(
            "autumn-natter-venv-{}-{}",
            std::process::id(),
            TURN_OF_TEST.fetch_add(1, Ordering::SeqCst)
        ));
        let workdir = room.join("work");
        let programs = venv_dir(&workdir).join("bin");
        std::fs::create_dir_all(&programs).unwrap();
        std::fs::write(programs.join("pip"), "").unwrap();
        let pip = Access { pip: true, ..Access::default() };
        let text = jail_args(&workdir, pip).join("\n");
        let bin = venv_dir(&workdir).display().to_string();
        let stood = format!("{}/bin:{}", bin, JAIL_PATH);
        assert!(text.contains(&stood), "MISSING {}", stood);
        assert!(text.contains("PIP_BREAK_SYSTEM_PACKAGES\n1"));
        assert!(text.contains(&format!("VIRTUAL_ENV\n{}\n", bin)));
        let _ = std::fs::remove_dir_all(&room);
    }

    // The commands that fetch, and the commands that install, are known by what they name, and
    // not by all of the line.
    #[test]
    fn a_fetch_and_an_install_are_known_by_their_names() {
        assert!(uses_fetch("wget https://example.com/a.tar"));
        assert!(uses_fetch("curl -sS https://example.com"));
        assert!(uses_fetch("fetch -o out https://example.com"));
        assert!(uses_fetch("ls -la; wget https://example.com"));
        assert!(uses_fetch("cd notes && curl https://example.com > page.html"));
        assert!(uses_fetch("sudo /usr/bin/wget https://example.com"));
        assert!(uses_fetch("echo hello | curl -T - https://example.com"));
        // A note about another command is not the program of one.
        assert!(!uses_fetch("echo wget"));
        assert!(!uses_fetch("grep curl notes.md"));
        assert!(!uses_fetch("cat curling.txt"));
        assert!(!uses_fetch("ls"));

        assert!(uses_pip("pip install requests"));
        assert!(uses_pip("pip3 install --user rich"));
        assert!(uses_pip("python3 -m pip install rich"));
        assert!(uses_pip("PIP_NO_INPUT=1 pip install rich"));
        assert!(uses_pip(".venv/bin/pip install rich"));
        // A read of pip does not write anything.
        assert!(!uses_pip("pip list"));
        assert!(!uses_pip("pip show requests"));
        assert!(!uses_pip("python3 -m pip freeze"));
        assert!(!uses_pip("ls .venv/bin/pip"));
    }

    // A command the chat does not allow is answered before it runs, and the answer reports what
    // to open.
    #[test]
    fn a_command_the_chat_does_not_allow_is_answered_without_running() {
        let off = Access::default();
        let fetch = refused("wget https://example.com", off).expect("the fetch is refused");
        assert!(fetch.contains("wget"));
        // The user is informed what to open, and the model is informed plainly not to try again
        // or to search for another way round it: a refusal that only names the error is read by
        // a model as an item to work round.
        assert!(fetch.contains("menu of the Sandbox badge"));
        assert!(fetch.contains("Do not try the command again"));
        assert!(fetch.contains("only the user can turn it on"));
        let pip = refused("pip install requests", off).expect("the install is refused");
        assert!(pip.contains("Python package installs"));
        assert!(pip.contains("Do not try the command again"));
        // The same commands go through when the chat opened them.
        let open = Access { network: true, pip: true, fetch: true };
        assert!(refused("wget https://example.com", open).is_none());
        assert!(refused("pip install requests", open).is_none());
        // An install does not open the fetch, and a fetch does not open an install.
        let only_fetch = Access { fetch: true, ..Access::default() };
        assert!(refused("pip install requests", only_fetch).is_some());
        let only_pip = Access { pip: true, ..Access::default() };
        assert!(refused("wget https://example.com", only_pip).is_some());
        // A command that needs neither goes through whatever remains off.
        assert!(refused("ls -la", off).is_none());
    }

    // The text handed to the model follow what the chat allows, so the model is not informed
    // there is no network where the user opened one.
    #[test]
    fn the_tool_says_what_the_chat_allows() {
        let off = definition(true, Access::default());
        let said = off["function"]["description"].as_str().unwrap_or("").to_string();
        assert!(said.contains("There is no network access."));
        assert!(said.contains("turned off"));
        assert!(!said.contains("pip is on the PATH"));
        let open = definition(true, Access { network: true, pip: true, fetch: true });
        let words = open["function"]["description"].as_str().unwrap_or("").to_string();
        assert!(words.contains("The command may reach the network."));
        assert!(words.contains("pip is on the PATH"));
        assert!(words.contains("wget, curl and fetch may be used"));
        assert!(!words.contains("There is no network access."));
    }

    // A chat with the sandbox off runs its commands under the same eye as the tools of the
    // files, with nothing restricted about the folder it starts in. The text handed to the model
    // say the command and the folder, and nothing of a sandbox, of a network or of a program
    // that remains off: such a chat has none of those items to tell, and a model once informed
    // that fetching was turned off left a command it should have run alone.
    #[test]
    fn a_chat_without_the_sandbox_is_told_nothing_of_it() {
        for access in [
            Access::default(),
            Access { network: true, pip: true, fetch: true },
        ] {
            let said = definition(false, access)["function"]["description"]
                .as_str()
                .unwrap_or("")
                .to_string();
            assert!(said.contains("Run one shell command"), "{}", said);
            assert!(said.contains("the working directory of the chat"), "{}", said);
            for forbidden in ["sandbox", "sandbox", "network", "wget", "curl", "pip"] {
                assert!(
                    !said.to_lowercase().contains(forbidden),
                    "a chat with no sandbox is informed of {}: {}",
                    forbidden,
                    said
                );
            }
        }
    }

    #[cfg(unix)]
    static TURN_OF_TEST: AtomicUsize = AtomicUsize::new(0);

    // The directory of a test, made under the temp folder of the computer and removed again.
    // The tests take turns from a counter, so two of them never work in one directory.
    #[cfg(unix)]
    fn room_of(name: &str) -> PathBuf {
        let place = std::env::temp_dir().join(format!(
            "autumn-natter-ca-{}-{}-{}",
            std::process::id(),
            TURN_OF_TEST.fetch_add(1, Ordering::SeqCst),
            name
        ));
        let _ = std::fs::remove_dir_all(&place);
        std::fs::create_dir_all(&place).expect("the room of the test could not be made");
        place
    }

    // The sandbox of this computer, run for real. A box without bubblewrap, or with user
    // namespaces switched off, reports what it found and passes at once.
    #[cfg(unix)]
    #[test]
    fn the_jail_hides_the_machine() {
        if let Err(problem) = jail_check() {
            eprintln!("the sandbox of this box is not usable: {}", problem);
            return;
        }
        let room = std::env::temp_dir().join(format!(
            "autumn-natter-jail-test-{}-{}",
            std::process::id(),
            TURN_OF_TEST.fetch_add(1, Ordering::SeqCst)
        ));
        let workdir = room.join("work");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::write(room.join("secret"), "NOT-FOR-A-SANDBOX").unwrap();
        // A file beside the working directory of the chat, and the directory above it, and the
        // logs of the computer: none of them may be read from inside. The path to the working
        // directory of the chat is empty in the sandbox, so only a directory of the computer that no
        // chat is ever filed in is checked here.
        let out = run(
            &format!(
                "cat '{}'; cat ../secret 2>&1; if [ -e /var/log ]; then echo LOGS-THERE; fi",
                room.join("secret").display()
            ),
            &workdir,
            true,

            Access::default(),

            Duration::from_secs(20),
        );
        assert!(out.sandboxed, "the run said it was not wrapped");
        for hidden in ["NOT-FOR-A-SANDBOX", "LOGS-THERE"] {
            assert!(!out.output.contains(hidden), "{}", out.output);
        }
        // The working directory of the chat may be written, and what is written stays.
        let wrote = run("echo kept > note.txt", &workdir, true, Access::default(), Duration::from_secs(20));
        assert!(wrote.ok, "{}", wrote.output);
        assert_eq!(
            std::fs::read_to_string(workdir.join("note.txt")).unwrap().trim(),
            "kept"
        );
        // The programs of the computer remain read only, so a command cannot put a file among
        // them.
        let system = run(
            "test -w /usr && echo WRITABLE || echo READONLY",
            &workdir,
            true,

            Access::default(),

            Duration::from_secs(20),
        );
        assert!(system.output.contains("READONLY"), "{}", system.output);
        // A command starts in the working directory of the chat. What it writes beside that
        // directory is written in the sandbox, and never on the computer.
        let place = run("pwd", &workdir, true, Access::default(), Duration::from_secs(20));
        assert_eq!(place.output.trim(), jail_workdir(&workdir));
        let beside = run(
            "echo nope > ../secret 2>/dev/null; echo tried",
            &workdir,
            true,

            Access::default(),

            Duration::from_secs(20),
        );
        assert!(beside.ok, "{}", beside.output);
        assert_eq!(
            std::fs::read_to_string(room.join("secret")).unwrap(),
            "NOT-FOR-A-SANDBOX"
        );
        // `/tmp` is a directory of the working directory of the chat. The path inside the sandbox is
        // `/tmp`, the file is in `tmp` of the working directory of the chat, and what one command
        // leaves there is seen by the next.
        let left = run(
            "echo gone > \"$TMPDIR/left\"; echo \"$TMPDIR\"",
            &workdir,
            true,

            Access::default(),

            Duration::from_secs(20),
        );
        assert_eq!(left.output.trim(), "/tmp");
        let kept = run(
            "test -e \"$TMPDIR/left\" && echo KEPT || echo GONE",
            &workdir,
            true,

            Access::default(),

            Duration::from_secs(20),
        );
        assert!(kept.output.contains("KEPT"), "{}", kept.output);
        assert!(
            workdir.join("tmp").join("left").is_file(),
            "the file of /tmp was not in the working directory of the chat"
        );
        // A command that is not wrapped records it plainly, and is flagged as not sandboxed, so
        // the window can tell the user.
        let plain = run("cat ../secret 2>&1", &workdir, false, Access::default(), Duration::from_secs(20));
        assert!(!plain.sandboxed);
        assert!(plain.output.contains("NOT-FOR-A-SANDBOX"), "{}", plain.output);
        let _ = std::fs::remove_dir_all(&room);
    }

    // A command that is given less time than it takes is stopped, wrapped and plain. The shell
    // forks the command it was given, and that child holds the output pipes open, so a signal
    // to the shell alone leaves the call waiting for the run to end by itself. The text the
    // command printed before it was stopped return with the answer, and nothing of the run is
    // left going.
    #[cfg(unix)]
    #[test]
    fn a_command_out_of_time_is_stopped_wrapped_and_plain() {
        let workdir = room_of("watch");
        for jailed in [false, true] {
            let kind = if jailed { "wrapped" } else { "plain" };
            if jailed {
                if let Err(problem) = jail_check() {
                    eprintln!("the sandbox of this box is not usable: {}", problem);
                    continue;
                }
            }
            let began = std::time::Instant::now();
            let made = run_watched(
                "echo one; sleep 35; echo two",
                &workdir,
                jailed,
                Access::default(),
                &util::Watch::new(Duration::from_millis(600)),
            );
            let gone = began.elapsed();
            assert!(
                made.timed_out,
                "a {kind} command was not stopped when its time was gone"
            );
            assert!(
                made.output.contains("one"),
                "the words before the end were lost, {kind}: {}",
                made.output
            );
            assert!(
                !made.output.contains("two"),
                "words printed after the end came back, {kind}: {}",
                made.output
            );
            assert!(
                gone < Duration::from_secs(5),
                "a {kind} call was held for {} ms",
                gone.as_millis()
            );
        }
        let _ = std::fs::remove_dir_all(&workdir);
    }

    // The call is requested one item: the command. No reason for the command is requested, so a
    // model may run a command without first writing a line about it, and the text of the
    // definition lead it nowhere near such a line.
    #[test]
    fn the_tool_asks_for_no_reason_of_the_command() {
        let made = definition(true, Access::default());
        let call = made.get("function").expect("the tool is a function");
        assert_eq!(
            call.get("name").and_then(|v| v.as_str()),
            Some(TOOL_NAME),
            "the name of the tool is the one the engine runs"
        );
        let mut keys: Vec<String> = call
            .get("parameters")
            .and_then(|p| p.get("properties"))
            .and_then(|p| p.as_object())
            .expect("the parts of a call are listed")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        assert_eq!(keys, vec![String::from("command")]);
        let want = call
            .get("parameters")
            .and_then(|p| p.get("required"))
            .and_then(|r| r.as_array())
            .expect("the parts that must come are listed");
        assert_eq!(want.len(), 1);
        assert_eq!(want[0].as_str(), Some("command"));
        // Nothing of a reason is in the text handed to the model.
        let words = made.to_string().to_lowercase();
        assert!(
            !words.contains("purpose"),
            "the definition still requests a purpose: {}",
            made
        );
    }

    // The text of a command is extracted from an object that is not finished, for the model
    // writes it one piece at a time. Nothing is created up of what has not come.
    #[test]
    fn the_words_of_a_command_are_read_while_the_call_is_written() {
        // Nothing has come, and the marker of an object alone is no command.
        assert_eq!(partial_arguments(""), (String::new(), String::new()));
        assert_eq!(partial_arguments("{"), (String::new(), String::new()));
        assert_eq!(
            partial_arguments("{\"comm"),
            (String::new(), String::new())
        );
        // The quote of the command is left open, and the text of it is reads as far as they
        // have come.
        assert_eq!(partial_arguments("{\"command\":\"ls"), ("ls".into(), String::new()));
        assert_eq!(
            partial_arguments("{\"command\":\"ls -la\",\"purpose\":\"see what is"),
            (String::from("ls -la"), String::from("see what is"))
        );
        // All of the call, which is what parse_arguments is given when the answer is over. Both
        // readings answer the same.
        let whole = "{\"command\":\"ls -la\",\"purpose\":\"see what is here\"}";
        assert_eq!(
            partial_arguments(whole),
            parse_arguments(whole)
        );
        assert_eq!(
            partial_arguments(whole),
            (String::from("ls -la"), String::from("see what is here"))
        );
    }

    // The markers a model puts inside the text of a command is reads the same way as
    // `parse_arguments` reads them, and a marker with nothing behind it is left for the next
    // look.
    #[test]
    fn the_marks_inside_a_command_are_read_while_it_is_written() {
        // A quote of the shell, held by a marker. The quote that closes the text of the command
        // has not come yet, and the two markers inside it mean the quotes of the command.
        assert_eq!(
            partial_arguments(r#"{"command":"echo \"hi\""#),
            (String::from("echo \"hi\""), String::new())
        );
        // The same command with the quote of it closed, which is what the model sends a moment
        // later.
        assert_eq!(
            partial_arguments(r#"{"command":"echo \"hi\""}"#),
            (String::from("echo \"hi\""), String::new())
        );
        // A new line inside the command.
        assert_eq!(
            partial_arguments("{\"command\":\"echo a\\nb"),
            (String::from("echo a\nb"), String::new())
        );
        // The marker of a new line came last of all, so the word it brings has not come yet.
        assert_eq!(
            partial_arguments("{\"command\":\"echo a\\"),
            (String::from("echo a"), String::new())
        );
        // Four hexadecimal characters make one letter. Without all four the letter is not
        // known, and what is behind it cannot be read either.
        assert_eq!(
            partial_arguments("{\"command\":\"arrow \\u2192 done"),
            (String::from("arrow \u{2192} done"), String::new())
        );
        assert_eq!(
            partial_arguments("{\"command\":\"arrow \\u21"),
            (String::from("arrow "), String::new())
        );
        // All of the call reads the same as the part of it.
        let whole = r#"{"command":"echo \"hi\""}"#;
        assert_eq!(partial_arguments(whole), parse_arguments(whole));
        assert_eq!(partial_arguments(whole).0, "echo \"hi\"");
    }

    // A model that writes the command with no markers of JSON at all is answered with the line
    // so far, and not with nothing.
    #[test]
    fn a_command_with_no_marks_of_json_is_read_as_a_line() {
        assert_eq!(
            partial_arguments("ls -la"),
            (String::from("ls -la"), String::new())
        );
        assert_eq!(
            partial_arguments("ls -l"),
            (String::from("ls -l"), String::new())
        );
        assert_eq!(
            partial_arguments("```json\ncargo test\n"),
            (String::from("cargo test"), String::new())
        );
        // A number where a text was searched for is no command.
        assert_eq!(
            partial_arguments("{\"command\":5}"),
            (String::new(), String::new())
        );
    }
}
