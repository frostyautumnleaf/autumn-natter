// Small helpers that do not belong to one system.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// Result of a short running program.
pub struct Capture {
    pub ok: bool,
    pub code: i32,
    pub out: String,
    pub err: String,
    pub timed_out: bool,
}

impl Capture {
    fn failed(message: &str) -> Capture {
        Capture {
            ok: false,
            code: -1,
            out: String::new(),
            err: message.to_string(),
            timed_out: false,
        }
    }

    // Standard output and standard output of error, joined.
    pub fn all(&self) -> String {
        let mut text = self.out.clone();
        if !self.err.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&self.err);
        }
        text
    }
}

// Run a program and read what it prints. The call never waits longer than the
// timeout. A program that cannot be started gives a failed Capture.
pub fn capture<A: AsRef<std::ffi::OsStr>>(
    program: &str,
    args: &[A],
    timeout: Duration,
) -> Capture {
    capture_in(program, args, None, timeout)
}

// Same as capture, with a directory to start the program in.
pub fn capture_in<A: AsRef<std::ffi::OsStr>>(
    program: &str,
    args: &[A],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Capture {
    let mut command = Command::new(program);
    command.args(args.iter().map(|arg| arg.as_ref()));
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Capture::failed(&format!("{}: {}", program, e)),
    };

    let pid = child.id();
    let finished = Arc::new(AtomicBool::new(false));
    let killed = Arc::new(AtomicBool::new(false));
    let flag = finished.clone();
    let killed_flag = killed.clone();
    // The watchdog stops a program that does not end by itself.
    std::thread::spawn(move || {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if flag.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !flag.load(Ordering::Relaxed) {
            killed_flag.store(true, Ordering::Relaxed);
            kill_pid(pid);
        }
    });

    // Read the two output pipes at the same time. One pipe must not wait for
    // the other, or a program that prints a lot stops writing.
    let err_pipe = child.stderr.take();
    let err_thread = std::thread::spawn(move || match err_pipe {
        Some(mut pipe) => read_to_end(&mut pipe),
        None => String::new(),
    });
    let mut stdout = match child.stdout.as_mut() {
        Some(pipe) => read_to_end(pipe),
        None => String::new(),
    };
    drop(child.stdout.take());
    let status = child.wait();
    let mut stderr = err_thread.join().unwrap_or_default();
    finished.store(true, Ordering::Relaxed);

    match status {
        Ok(status) => {
            trim_newlines(&mut stdout);
            trim_newlines(&mut stderr);
            Capture {
                ok: status.success(),
                code: status.code().unwrap_or(-1),
                out: stdout,
                err: stderr,
                timed_out: killed.load(Ordering::Relaxed),
            }
        }
        Err(e) => Capture::failed(&e.to_string()),
    }
}

fn read_to_end<R: Read>(reader: &mut R) -> String {
    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn trim_newlines(text: &mut String) {
    while text.ends_with('\n') || text.ends_with('\r') || text.ends_with(' ') {
        text.pop();
    }
}

// Stop a program by its process id.
pub fn kill_pid(pid: u32) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

// Split one argument string into parts, like a shell does. Quotes keep parts
// together. This is used for the extra arguments of llama.cpp.
pub fn split_args(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for ch in text.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None => match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    started = true;
                }
                c if c.is_whitespace() => {
                    if started {
                        out.push(std::mem::take(&mut current));
                    }
                    started = false;
                }
                c => {
                    current.push(c);
                    started = true;
                }
            },
        }
    }
    if started || !current.is_empty() {
        out.push(current);
    }
    out
}

// Tokens with a comma every three digits, like 64,763. Both windows show the
// context size this way.
pub fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// A free TCP port on the local machine. The port is taken from the operating
// system, so two runs do not clash.
pub fn free_port() -> Option<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);
    Some(port)
}

// The address of this machine on the local network. The UDP connect call does
// not send a packet, it only selects the route the system would use.
pub fn lan_address() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // A private address is used so no packet can leave the network.
    socket.connect("10.255.255.255:1").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip())
}
