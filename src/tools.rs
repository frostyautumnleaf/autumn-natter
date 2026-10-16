// The terminal tool, and the bubblewrap jail around it.
//
// One tool is defined today. More tools come later.
//
// The jail starts from an empty root, so a command sees nothing of this
// machine. Three things are put into it: the programs of the system, read
// only; the working directory of the chat, read and write; and the few files
// of /etc that a shell asks for. The network is off, the process list is a new
// one, and a file outside the working directory is simply not there.
//
// The jail before this one bound the whole root of the host read only. That
// stopped a command from writing outside its folder, but every private file of
// the machine could still be read from inside, so the sandbox did not keep
// what its name promised. The empty root is the mending.
//
// A run that asks for the jail and cannot get it does not run at all. The
// program never falls back to a plain run behind the back of the chat.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use crate::util;

// Name the model uses to ask for a terminal run.
pub const TOOL_NAME: &str = "run_terminal";

// Name the model uses to mark the start of a chapter of the chat. The call is
// of the program's own: the reader never sees it, and the model is asked not to
// speak of it.
pub const CHAPTER_TOOL_NAME: &str = "chapter";

// The longest name a chapter may have. The node beside the chat and the card
// over it both have to hold the name in a small space.
pub const MAX_CHAPTER_NAME: usize = 50;

// Longest output kept for one run.
const MAX_OUTPUT: usize = 8000;

// The shell a command is handed to, both inside the jail and outside it.
const SHELL: &str = "/bin/sh";

// The name the jail gives its own machine. The name of the host is not wanted
// in there, and a command has no business reading it.
const JAIL_HOSTNAME: &str = "natter";

// The folders the programs of the system live in. They go into the jail read
// only, so a command can run what the machine has without changing it.
const PROGRAM_PATHS: [&str; 7] = ["/usr", "/bin", "/lib", "/lib64", "/lib32", "/libx32", "/sbin"];

// A system that keeps its programs outside those folders, such as NixOS or GNU
// System, gets those folders too. They hold programs and nothing else.
const EXTRA_PROGRAM_PATHS: [&str; 2] = ["/nix", "/gnu"];

// The files of /etc a shell and its tools ask for. They are put in one by one,
// and never the whole folder, so a private file of the machine stays out of
// reach: /etc/shadow, /etc/ssh, /etc/machine-id and the rest are not offered.
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

// The tool description handed to the model.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": TOOL_NAME,
            "description": "Run one shell command and return what it printed. The command starts in the working directory of the chat. When the sandbox is on, the command sees only that directory and the programs of the machine: every other file of the machine, the home folder included, is not there, there is no network, and nothing is left of the command in the next one.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to run, in one line."
                    },
                    "purpose": {
                        "type": "string",
                        "description": "One short sentence that says why the command is needed."
                    }
                },
                "required": ["command"]
            }
        }
    })
}

// The tool description for a chapter of the chat.
pub fn chapter_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": CHAPTER_TOOL_NAME,
            "description": "Mark the start of a new chapter of this chat. Call it whenever a new part of the work begins: a new task, a new question, a new phase, or the next chapter of a story you are writing. You may call it as often as you like, and in the middle of your work: call it for a part just before you write that part, then go straight on writing. After the call you are told only that the mark was taken, and you go on where you left off. The name is a few words that say what the part is about. Do not call it twice for the same part, do not speak of this call, and do not mention chapters in your answer.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The name of the chapter, in a few words, no longer than 50 characters."
                    }
                },
                "required": ["name"]
            }
        }
    })
}

// The name of a chapter out of what the model sent. It is made one line, cut at
// MAX_CHAPTER_NAME, and empty when the model sent nothing usable.
pub fn chapter_name(raw: &str) -> String {
    let taken = match serde_json::from_str::<Value>(raw) {
        Ok(value) => value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        // A model that sent the name on its own, without the object.
        Err(_) => raw.to_string(),
    };
    let mut name = String::new();
    for word in taken.split_whitespace() {
        if !name.is_empty() {
            name.push(' ');
        }
        name.push_str(word);
    }
    name.chars().take(MAX_CHAPTER_NAME).collect::<String>().trim().to_string()
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
            return "the command ran out of time and was stopped".to_string();
        }
        self.output.clone()
    }
}

// Where bwrap is. None means the jail cannot be used. The answer is kept, so
// the machine is not searched again for every command.
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
            // Ask the PATH of the machine, so a folder outside the usual ones
            // works.
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

// True when bwrap stands on this machine. A machine without it cannot sandbox
// anything, and the window says so.
pub fn installed() -> bool {
    bwrap_path().is_some()
}

// The path of the working directory as the jail sees it. Links are followed, so
// the place that is mounted and the place the command starts in are one place.
fn jail_workdir(workdir: &Path) -> String {
    workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf())
        .display()
        .to_string()
}

// One of the program folders of the system, put into the jail. A folder that
// is a link, as /bin is on a system where everything lives under /usr, is made
// a link in the jail too, so the paths the programs know keep working.
fn push_program_path(args: &mut Vec<String>, path: &Path) {
    let text = path.display().to_string();
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_symlink() {
        // The target is kept as it is written, so a relative link stays
        // relative and an absolute one stays absolute.
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

// Build the argument list of the jail, up to the two marks that end it. The
// working directory is the only place a command may write.
fn jail_args(workdir: &Path) -> Vec<String> {
    let wd = jail_workdir(workdir);
    let mut args: Vec<String> = vec![
        // No network, no other processes, no other users, no other names, no
        // other control groups.
        "--unshare-all".into(),
        // The user namespace is asked for on its own as well, so the next
        // option is honoured and not quietly skipped.
        "--unshare-user".into(),
        // A command inside cannot build a jail of its own to climb out of.
        "--disable-userns".into(),
        // The jail goes away with the program, so no command is left standing
        // behind a closed window.
        "--die-with-parent".into(),
        // A terminal session of its own, so a signal sent to the session of
        // the program cannot reach a command inside.
        "--new-session".into(),
        "--hostname".into(),
        JAIL_HOSTNAME.into(),
        // The empty root. Nothing of this machine is there until it is put in.
        "--tmpfs".into(),
        "/".into(),
    ];
    // The programs of the machine, read only.
    for path in PROGRAM_PATHS.iter().chain(EXTRA_PROGRAM_PATHS.iter()) {
        push_program_path(&mut args, Path::new(path));
    }
    // The parts of a root a command still needs to run at all. /dev holds the
    // few nodes a program asks for, and no node of the host. /proc is a new
    // one, so the processes of the machine are not to be seen. /tmp is a room
    // of the jail, empty at the start of every command and gone with it.
    args.extend_from_slice(&[
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        "--dir".into(),
        "/etc".into(),
    ]);
    // The few files of /etc, one by one. A missing one is passed over.
    for file in ETC_PATHS {
        args.push("--ro-bind-try".into());
        args.push(file.into());
        args.push(file.into());
    }
    // The working directory of the chat: the one place that may be written.
    args.push("--bind".into());
    args.push(wd.clone());
    args.push(wd.clone());
    // The air of the host is let out of the room, so no variable of the user
    // can point a command at a place of the machine.
    args.push("--clearenv".into());
    args.extend_from_slice(&[
        "--setenv".into(),
        "PATH".into(),
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
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
        "--chdir".into(),
        wd,
    ]);
    args
}

// bubblewrap writes its own trouble at the front of the error output and leaves
// the command unrun. A command that ran and failed is another thing, and it is
// told apart by having printed something, or by ending on a signal of its own.
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

// Run one command. sandbox is true when the chat keeps the sandbox on.
// workdir is the directory the command starts in.
pub fn run(command: &str, workdir: &Path, sandbox: bool, timeout: Duration) -> Outcome {
    if command.trim().is_empty() {
        return Outcome {
            ok: false,
            output: "the model asked for an empty command".to_string(),
            sandboxed: false,
            timed_out: false,
        };
    }
    let _ = std::fs::create_dir_all(workdir);

    #[cfg(unix)]
    {
        if sandbox {
            return run_jailed(command, workdir, timeout);
        }
        let capture = util::capture_in(SHELL, &["-c", command, "natter"], Some(workdir), timeout);
        finish(capture, false)
    }
    #[cfg(not(unix))]
    {
        // There is no bubblewrap outside Linux and macOS. A chat that asked for
        // the sandbox is not served a plain run behind its back, even when a
        // chat file from another machine carries the mark. The window keeps the
        // badge off in that case, and says why.
        if sandbox {
            let reason = jail_check()
                .err()
                .unwrap_or_else(|| String::from("there is no bubblewrap on this system"));
            return Outcome {
                ok: false,
                output: format!("the sandbox did not run this command: {}", reason),
                sandboxed: false,
                timed_out: false,
            };
        }
        #[cfg(windows)]
        {
            // Windows has no jail. The command runs in the working directory
            // with cmd.exe, and the window shows that the sandbox is
            // unavailable.
            let capture = util::capture_in("cmd.exe", &["/C", command], Some(workdir), timeout);
            finish(capture, false)
        }
        #[cfg(not(windows))]
        {
            let _ = timeout;
            Outcome {
                ok: false,
                output: "no shell is available on this system".to_string(),
                sandboxed: false,
                timed_out: false,
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        Outcome {
            ok: false,
            output: "no shell is available on this system".to_string(),
            sandboxed: false,
            timed_out: false,
        }
    }
}

// Run one command inside the jail. When the jail cannot be had, the command
// does not run at all: a chat that asked for the sandbox is never given a plain
// run behind its back.
#[cfg(unix)]
fn run_jailed(command: &str, workdir: &Path, timeout: Duration) -> Outcome {
    let failed = |reason: String| Outcome {
        ok: false,
        output: format!("the sandbox did not run this command: {}", reason),
        sandboxed: false,
        timed_out: false,
    };
    if let Err(problem) = jail_check() {
        return failed(problem);
    }
    let Some(bwrap) = bwrap_path() else {
        return failed(String::from("bubblewrap (bwrap) was not found"));
    };
    let mut args = jail_args(workdir);
    args.push("--".into());
    args.push(SHELL.into());
    args.push("-c".into());
    args.push(command.to_string());
    let program = bwrap.display().to_string();
    let capture = util::capture_in(&program, &args, Some(workdir), timeout);
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
            format!("the command failed with code {}", capture.code)
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
    format!("{}\n... output cut in the middle ...\n{}", head, tail)
}

// A place put into a line of the shell. A mark of quoting inside a folder name
// is made safe, so a folder named with one cannot break the line of the check.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

// Whether the jail of this machine keeps what it promises. Everything is looked
// at by the jail itself, in a room of the check alone: that bubblewrap starts,
// that a file outside the working directory cannot be read, that the working
// directory can be written, that two parts of the root of the machine are not
// there, and that the network is cut off. The answer is kept, so the machine is
// asked once.
pub fn jail_check() -> Result<(), String> {
    static CHECK: OnceLock<Result<(), String>> = OnceLock::new();
    CHECK.get_or_init(probe_the_jail).clone()
}

// The five lines the jail is asked to write. The room is of the check alone, so
// a chat is never touched.
#[cfg(unix)]
fn probe_the_jail() -> Result<(), String> {
    let Some(bwrap) = bwrap_path() else {
        return Err(String::from(
            "bubblewrap (bwrap) was not found. Install it to sandbox the terminal tool.",
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
            "the check of the sandbox could not make a folder to work in",
        ));
    }
    // A file beside the working directory, which no command inside may read.
    let secret = root.join("secret");
    let _ = std::fs::write(&secret, "AUTUMN-NATTER-JAIL-CHECK");
    let secret_text = secret.display().to_string();
    let work_text = jail_workdir(&workdir);
    // Two parts of the root of the machine that are never put into the jail:
    // the file that holds the secret words of the accounts, and the folder the
    // machine writes its logs into. Both stand on every system of the kind this
    // program runs on, so either of them seen inside means the root of the
    // machine got in. The home folder is not looked at, for the working
    // directory of a chat may well stand inside it.
    let accounts_file = Path::new("/etc").join("shadow");
    let logs_folder = Path::new("/var").join("log");
    // The lines of /proc/net/dev are counted instead of read with a tool, so
    // the check needs nothing but the shell. A new network room holds the loop
    // device alone, which is two lines of head and one line of device.
    let script = format!(
        "if [ -r {secret} ]; then echo hidden=no; else echo hidden=yes; fi; \
         if : > {work}/probe 2>/dev/null; then echo writable=yes; else echo writable=no; fi; \
         if [ -e {accounts} ]; then echo accounts=seen; else echo accounts=hidden; fi; \
         if [ -e {logs} ]; then echo logs=seen; else echo logs=hidden; fi; \
         lines=0; while IFS= read -r row; do lines=$((lines+1)); done < /proc/net/dev 2>/dev/null; \
         if [ \"$lines\" = '3' ]; then echo net=off; else echo \"net=on:$lines\"; fi",
        secret = shell_quote(&secret_text),
        work = shell_quote(&work_text),
        accounts = shell_quote(&accounts_file.display().to_string()),
        logs = shell_quote(&logs_folder.display().to_string())
    );
    let mut args = jail_args(&workdir);
    args.push("--".into());
    args.push(SHELL.into());
    args.push("-c".into());
    args.push(script);
    let program = bwrap.display().to_string();
    let capture = util::capture_in(&program, &args, Some(&workdir), Duration::from_secs(20));
    let _ = std::fs::remove_dir_all(&root);
    if capture.timed_out {
        return Err(String::from("bubblewrap did not answer the check of the sandbox"));
    }
    if let Some(problem) = jail_failure(&capture) {
        return Err(format!("bubblewrap could not start a jail: {}", problem));
    }
    let problems = check_lines(&capture.all());
    if problems.is_empty() {
        return Ok(());
    }
    Err(problems.join(". "))
}

// Nothing to check where bubblewrap cannot run at all.
#[cfg(not(unix))]
fn probe_the_jail() -> Result<(), String> {
    Err(String::from(
        "bubblewrap is not available on this system, so commands cannot be sandboxed",
    ))
}

// What the answer of the jail says is wrong. An empty answer means the jail
// keeps everything it promises.
fn check_lines(text: &str) -> Vec<String> {
    let holds = |line: &str| text.lines().any(|seen| seen.trim() == line);
    let mut problems = Vec::new();
    // A file beside the working directory that can still be read is the worst
    // of them: keeping it out is what the sandbox is there for.
    if !holds("hidden=yes") {
        problems.push(String::from(
            "a file outside the working directory could still be read from inside",
        ));
    }
    if !holds("writable=yes") {
        problems.push(String::from(
            "the working directory could not be written inside the jail",
        ));
    }
    // Either of the two parts of the host root standing inside says the same
    // thing: the root of the machine was let in. The words are kept as one so
    // the window says it once.
    if !holds("accounts=hidden") || !holds("logs=hidden") {
        problems.push(String::from(
            "the root of the machine is visible inside the jail",
        ));
    }
    if !holds("net=off") {
        problems.push(String::from("the network is not cut off inside the jail"));
    }
    problems
}

// Read the arguments of a tool call. The model sometimes writes no JSON at
// all, so a plain line is taken as the command.
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

// Read the arguments of a call that is still being written. The model sends
// them piece by piece, so the object is often not finished, a quote stands
// open, and the last mark of an escape has no word behind it yet. What is there
// of a text is given back, and nothing is made up of what is not there. The
// answer is the command and the purpose, as `parse_arguments` gives them.
pub fn partial_arguments(raw: &str) -> (String, String) {
    let text = raw.trim_start();
    let command = partial_text(text, "command");
    let purpose = partial_text(text, "purpose");
    if !command.is_empty() || !purpose.is_empty() {
        return (command, purpose);
    }
    // Nothing of a named part has come. A model that writes the command with no
    // marks of JSON at all is answered with the line so far. Words that open an
    // object are no command, and say nothing of one yet.
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

// The words of one named part of an object that may not be finished. The name
// is looked for, and the text behind it is read to the closing mark, or to the
// end of what has come in so far. A part this page does not know, or a number
// where a text was looked for, gives nothing.
fn partial_text(text: &str, wanted: &str) -> String {
    let key = format!("\"{}\"", wanted);
    let Some(at) = text.find(&key) else {
        return String::new();
    };
    let head = text[at + key.len()..].trim_start();
    // The name of a part is worth nothing without the mark of a value behind
    // it, and a value of no quotes is no text.
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
        // A mark of something the model meant to finish. Without the character
        // behind it there is nothing to give back, and what comes after it
        // cannot be read either, so the rest is left for the next look.
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
            // The marks of a quote, a backslash and the rest stand for the
            // character itself.
            other => out.push(other),
        }
    }
    out
}

// Return the part between the first brace and the matching one.
fn extract_object(text: &str) -> Option<&str> {
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

    // The name of a chapter is what the reader sees beside the chat, so it is
    // made one line and kept short.
    #[test]
    fn a_chapter_name_is_one_line_and_short() {
        assert_eq!(chapter_name(r#"{"name":"Fixing the parser"}"#), "Fixing the parser");
        // A name the length of the limit, and one longer than it.
        let long = "n".repeat(MAX_CHAPTER_NAME + 12);
        assert_eq!(chapter_name(&format!(r#"{{"name":"{long}"}}"#)).len(), MAX_CHAPTER_NAME);
        // Marks of a list, newlines and runs of spaces are made into one line.
        let messy = r#"  Ways\tof\n  the  work  "#;
        assert_eq!(chapter_name(&format!(r#"{{"name":"{messy}"}}"#)), "Ways of the work");
        // A model that sent the name on its own, without the object.
        assert_eq!(chapter_name("The work"), "The work");
        // Nothing usable gives nothing, and the call is answered as such.
        assert_eq!(chapter_name(r#"{"name":"   "}"#), "");
        assert_eq!(chapter_name("{}"), "");
    }

    // The arguments are looked at without running anything, so the shape of the
    // jail is watched even on a machine without bubblewrap.
    #[test]
    fn the_jail_starts_from_an_empty_root() {
        let args = jail_args(Path::new("/work/somewhere"));
        let text = args.join("\n");
        // The root of the jail is its own empty room, and the root of the host
        // is never put in.
        assert!(text.contains("--tmpfs\n/\n"));
        assert!(!text.contains("--ro-bind\n/\n/\n"));
        assert!(!text.contains("--bind\n/\n/\n"));
        // The walls of the jail.
        for mark in [
            "--unshare-all",
            "--unshare-user",
            "--disable-userns",
            "--die-with-parent",
            "--new-session",
        ] {
            assert!(text.contains(mark), "the jail lost {}", mark);
        }
        // The working directory is the one place that may be written, and it is
        // the place the command starts in.
        assert!(text.contains("--bind\n/work/somewhere\n/work/somewhere\n"));
        assert!(text.contains("--chdir\n/work/somewhere"));
        // No private file of the machine is offered.
        for forbidden in ["/etc/shadow", "/etc/ssh", "/etc/machine-id", "/home"] {
            assert!(!text.contains(forbidden), "the jail offers {}", forbidden);
        }
    }

    // bubblewrap leaves the command unrun when the jail cannot be made. That is
    // told apart from a command that ran and failed.
    #[test]
    fn the_failure_of_a_jail_is_told_from_a_failing_command() {
        let made = |out: &str, err: &str, code: i32| util::Capture {
            ok: code == 0,
            code,
            out: out.to_string(),
            err: err.to_string(),
            timed_out: false,
        };
        // A jail that could not be made.
        let unmade = made("", "bwrap: Creating new namespace failed: Operation not permitted", 1);
        assert!(jail_failure(&unmade).is_some());
        // A command that ran and failed. Its own words stand in the output.
        let failed = made("nothing here", "", 2);
        assert!(jail_failure(&failed).is_none());
        // A command that printed a line to the error output and nothing else.
        let noisy = made("", "cat: no such file", 1);
        assert!(jail_failure(&noisy).is_none());
        // A command that ran out of time is never a jail that failed to start.
        let late = util::Capture {
            ok: false,
            code: -1,
            out: String::new(),
            err: String::from("bwrap: something"),
            timed_out: true,
        };
        assert!(jail_failure(&late).is_none());
    }

    // The places of the check are named in a line of the shell, and a folder of
    // the machine may carry a mark of quoting in its name.
    #[test]
    fn a_place_of_the_check_is_named_safely() {
        assert_eq!(shell_quote("/tmp/work"), "'/tmp/work'");
        assert_eq!(shell_quote("/home/o'brien/work"), r#"'/home/o'\''brien/work'"#);
        // The words of the check keep their shape when a name holds a space.
        let line = format!("if [ -r {} ]; then echo yes; fi", shell_quote("/tmp/a b/c"));
        assert_eq!(line, "if [ -r '/tmp/a b/c' ]; then echo yes; fi");
    }

    // The words the jail writes back are read for what is wrong with it.
    #[test]
    fn the_answer_of_the_check_is_read() {
        let good = "hidden=yes\nwritable=yes\naccounts=hidden\nlogs=hidden\nnet=off\n";
        assert!(check_lines(good).is_empty());
        // A file outside the room that can be read is the worst of them.
        let bad = good.replace("hidden=yes", "hidden=no");
        let problems = check_lines(&bad);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("outside the working directory"));
        // The root of the machine got in, and the network was left on. Two of
        // the five lines stand wrong, and they are told as two faults.
        let worse = good
            .replace("logs=hidden", "logs=seen")
            .replace("net=off", "net=on:7");
        assert_eq!(check_lines(&worse).len(), 2);
        // The root seen in both halves is still one fault of one kind.
        let both = good.replace("logs=hidden", "logs=seen").replace(
            "accounts=hidden",
            "accounts=seen",
        );
        assert_eq!(check_lines(&both).len(), 1);
        // Nothing at all is not a good jail.
        assert_eq!(check_lines("").len(), 4);
    }

    #[cfg(unix)]
    static TURN_OF_TEST: AtomicUsize = AtomicUsize::new(0);

    // The jail of this machine, run for real. A box without bubblewrap, or with
    // user namespaces switched off, says what it found and passes at once.
    #[cfg(unix)]
    #[test]
    fn the_jail_hides_the_machine() {
        if let Err(problem) = jail_check() {
            eprintln!("the jail of this box is not usable: {}", problem);
            return;
        }
        let room = std::env::temp_dir().join(format!(
            "autumn-natter-jail-test-{}-{}",
            std::process::id(),
            TURN_OF_TEST.fetch_add(1, Ordering::SeqCst)
        ));
        let workdir = room.join("work");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::write(room.join("secret"), "NOT-FOR-A-JAIL").unwrap();
        // A file beside the room of the chat, and the room above it, and the
        // logs of the machine: none of them is to be read from inside. The road
        // to the room of the chat stands empty in the jail, so only a folder of
        // the machine that no chat is ever filed in is looked at here.
        let out = run(
            &format!(
                "cat '{}'; cat ../secret 2>&1; if [ -e /var/log ]; then echo LOGS-THERE; fi",
                room.join("secret").display()
            ),
            &workdir,
            true,
            Duration::from_secs(20),
        );
        assert!(out.sandboxed, "the run said it was not wrapped");
        for hidden in ["NOT-FOR-A-JAIL", "LOGS-THERE"] {
            assert!(!out.output.contains(hidden), "{}", out.output);
        }
        // The room of the chat may be written, and what is written stays.
        let wrote = run("echo kept > note.txt", &workdir, true, Duration::from_secs(20));
        assert!(wrote.ok, "{}", wrote.output);
        assert_eq!(
            std::fs::read_to_string(workdir.join("note.txt")).unwrap().trim(),
            "kept"
        );
        // The programs of the machine stand read only, so a command cannot put
        // a file among them.
        let system = run(
            "test -w /usr && echo WRITABLE || echo READONLY",
            &workdir,
            true,
            Duration::from_secs(20),
        );
        assert!(system.output.contains("READONLY"), "{}", system.output);
        // A command starts in the room of the chat. What it writes beside that
        // room is written in the jail, and never on the machine.
        let place = run("pwd", &workdir, true, Duration::from_secs(20));
        assert_eq!(place.output.trim(), jail_workdir(&workdir));
        let beside = run(
            "echo nope > ../secret 2>/dev/null; echo tried",
            &workdir,
            true,
            Duration::from_secs(20),
        );
        assert!(beside.ok, "{}", beside.output);
        assert_eq!(
            std::fs::read_to_string(room.join("secret")).unwrap(),
            "NOT-FOR-A-JAIL"
        );
        // The room of one command is empty when the next one starts, and it is
        // not the room of the machine.
        let left = run(
            "echo gone > \"$TMPDIR/left\"; echo \"$TMPDIR\"",
            &workdir,
            true,
            Duration::from_secs(20),
        );
        assert_eq!(left.output.trim(), "/tmp");
        let clean = run(
            "test -e \"$TMPDIR/left\" && echo LEFT || echo CLEAN",
            &workdir,
            true,
            Duration::from_secs(20),
        );
        assert!(clean.output.contains("CLEAN"), "{}", clean.output);
        // A command that is not wrapped says so plainly, and is marked as not
        // sandboxed, so the window can tell the reader.
        let plain = run("cat ../secret 2>&1", &workdir, false, Duration::from_secs(20));
        assert!(!plain.sandboxed);
        assert!(plain.output.contains("NOT-FOR-A-JAIL"), "{}", plain.output);
        let _ = std::fs::remove_dir_all(&room);
    }

    // The words of a command are read out of an object that is not finished, for
    // the model writes it one piece at a time. Nothing is made up of what has
    // not come.
    #[test]
    fn the_words_of_a_command_are_read_while_the_call_is_written() {
        // Nothing has come, and the mark of an object alone is no command.
        assert_eq!(partial_arguments(""), (String::new(), String::new()));
        assert_eq!(partial_arguments("{"), (String::new(), String::new()));
        assert_eq!(
            partial_arguments("{\"comm"),
            (String::new(), String::new())
        );
        // The quote of the command stands open, and the words of it are read as
        // far as they have come.
        assert_eq!(partial_arguments("{\"command\":\"ls"), ("ls".into(), String::new()));
        assert_eq!(
            partial_arguments("{\"command\":\"ls -la\",\"purpose\":\"see what is"),
            (String::from("ls -la"), String::from("see what is"))
        );
        // The whole of the call, which is what parse_arguments is given when the
        // answer is over. Both readings answer the same.
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

    // The marks a model puts inside the words of a command are read the same way
    // as `parse_arguments` reads them, and a mark with nothing behind it is left
    // for the next look.
    #[test]
    fn the_marks_inside_a_command_are_read_while_it_is_written() {
        // A quote of the shell, held by a mark. The quote that closes the text
        // of the command has not come yet, and the two marks inside it stand for
        // the quotes of the command.
        assert_eq!(
            partial_arguments(r#"{"command":"echo \"hi\""#),
            (String::from("echo \"hi\""), String::new())
        );
        // The same command with the quote of it closed, which is what the model
        // sends a moment later.
        assert_eq!(
            partial_arguments(r#"{"command":"echo \"hi\""}"#),
            (String::from("echo \"hi\""), String::new())
        );
        // A new line inside the command.
        assert_eq!(
            partial_arguments("{\"command\":\"echo a\\nb"),
            (String::from("echo a\nb"), String::new())
        );
        // The mark of a new line came last of all, so the word it brings has not
        // come yet.
        assert_eq!(
            partial_arguments("{\"command\":\"echo a\\"),
            (String::from("echo a"), String::new())
        );
        // Four hexadecimal characters make one letter. Without all four the
        // letter is not known, and what stands behind it cannot be read either.
        assert_eq!(
            partial_arguments("{\"command\":\"arrow \\u2192 done"),
            (String::from("arrow \u{2192} done"), String::new())
        );
        assert_eq!(
            partial_arguments("{\"command\":\"arrow \\u21"),
            (String::from("arrow "), String::new())
        );
        // The whole of the call reads the same as the part of it.
        let whole = r#"{"command":"echo \"hi\""}"#;
        assert_eq!(partial_arguments(whole), parse_arguments(whole));
        assert_eq!(partial_arguments(whole).0, "echo \"hi\"");
    }

    // A model that writes the command with no marks of JSON at all is answered
    // with the line so far, and not with nothing.
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
        // A number where a text was looked for is no command.
        assert_eq!(
            partial_arguments("{\"command\":5}"),
            (String::new(), String::new())
        );
    }
}
