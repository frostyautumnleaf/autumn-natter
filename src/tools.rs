// The terminal tool, and the bubblewrap sandbox around it.
//
// One tool is defined today. More tools will come later. Every run goes
// through the sandbox when the sandbox is on, and the working directory of a
// repository chat is that repository.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::util;

// Name the model uses to ask for a terminal run.
pub const TOOL_NAME: &str = "run_terminal";

// Longest output kept for one run.
const MAX_OUTPUT: usize = 8000;

// The tool description handed to the model.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": TOOL_NAME,
            "description": "Run one shell command and return what it printed. Use it to look at files or to change them inside the working directory. The command runs in a sandbox when the sandbox is on, and it has no network access.",
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

// What one run produced.
pub struct Outcome {
    pub ok: bool,
    pub output: String,
    // True when bubblewrap really wrapped the command.
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

// Where bwrap is. None means the sandbox cannot be used.
pub fn bwrap_path() -> Option<PathBuf> {
    for candidate in ["/usr/bin/bwrap", "/usr/local/bin/bwrap", "/run/current-system/sw/bin/bwrap"] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    // Ask the PATH of the machine, so a folder outside the usual ones works.
    let found = Command::new("bwrap")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if found.is_ok() {
        return Some(PathBuf::from("bwrap"));
    }
    None
}

// True when bwrap exists. A machine without it runs plain shell commands, and
// the window says so.
pub fn available() -> bool {
    bwrap_path().is_some()
}

// Ask bwrap whether it can start at all. User namespaces are switched off on
// some systems, and then the sandbox is not usable.
pub fn probe() -> Result<(), String> {
    let Some(path) = bwrap_path() else {
        return Err("bubblewrap (bwrap) was not found".to_string());
    };
    let mut command = Command::new(&path);
    command
        .args([
            "--unshare-all",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--",
            "/bin/sh",
            "-c",
            "true",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output = command.output();
    match output {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(format!(
            "bwrap could not start a test jail: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(e) => Err(format!("cannot run {}: {}", path.display(), e)),
    }
}

// Build the argument list for a sandboxed run. The working directory is bind
// mounted read and write, so the chat can change its own files.
fn bwrap_args(workdir: &Path) -> Vec<String> {
    let wd = workdir.display().to_string();
    let mut args: Vec<String> = vec![
        // No network, no other processes, no other users.
        "--unshare-all".into(),
        "--die-with-parent".into(),
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
        // The working directory is the only writable place.
        "--bind".into(),
        wd.clone(),
        wd.clone(),
        "--clearenv".into(),
        "--setenv".into(),
        "PATH".into(),
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        "--setenv".into(),
        "HOME".into(),
        wd.clone(),
        "--setenv".into(),
        "TMPDIR".into(),
        wd.clone(),
        "--setenv".into(),
        "LANG".into(),
        "C.UTF-8".into(),
        "--setenv".into(),
        "TERM".into(),
        "dumb".into(),
        "--chdir".into(),
        wd.clone(),
    ];
    // A host file the command may need, when it is there.
    for file in ["/etc/resolv.conf", "/etc/ssl", "/etc/hosts", "/etc/nsswitch.conf"] {
        if Path::new(file).exists() {
            args.push("--ro-bind".into());
            args.push(file.into());
            args.push(file.into());
        }
    }
    args.push("--".into());
    args
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
            if let Some(path) = bwrap_path() {
                let mut args = bwrap_args(workdir);
                args.push("/bin/sh".into());
                args.push("-c".into());
                args.push(command.to_string());
                let program = path.display().to_string();
                let capture = util::capture_in(&program, &args, Some(workdir), timeout);
                return finish(capture, true);
            }
        }
        let capture = util::capture_in("/bin/sh", &["-c", command, "_"], Some(workdir), timeout);
        finish(capture, false)
    }
    #[cfg(windows)]
    {
        // Windows has no bubblewrap. The command runs in the working directory
        // with cmd.exe, and the window shows that the sandbox is unavailable.
        let _ = sandbox;
        let capture = util::capture_in("cmd.exe", &["/C", command], Some(workdir), timeout);
        finish(capture, false)
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
    format!(
        "{}\n... output cut in the middle ...\n{}",
        head, tail
    )
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
