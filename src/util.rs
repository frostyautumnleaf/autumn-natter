// Small helpers that do not belong to one system.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
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

// The clock of one call that is running. The call is given a time to come back
// within, and the program that started it reads the same clock: the user may
// lengthen the time or cut the call short while it runs, and a command that is
// stopped leaves behind everything it had printed by then.
#[derive(Clone, Debug)]
pub struct Watch {
    began: Instant,
    // Milliseconds the call may take, counted from the moment it began. The
    // greatest number stands for no limit at all, which is what a call is
    // given when the user turned the feature off.
    limit: Arc<AtomicU64>,
    // The user asked for the call to end now.
    stop: Arc<AtomicBool>,
}

// The number that stands for a call with no end of time.
const NO_LIMIT: u64 = u64::MAX / 2;

impl Watch {
    // A call that must come back within the time given.
    pub fn new(limit: Duration) -> Watch {
        Watch::held(limit.as_millis() as u64)
    }

    // The same, from milliseconds. The greatest number stands for no limit.
    fn held(millis: u64) -> Watch {
        Watch {
            began: Instant::now(),
            limit: Arc::new(AtomicU64::new(millis.min(NO_LIMIT))),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    // A call that is given no time at all: it runs until it comes back.
    pub fn unbounded() -> Watch {
        Watch::held(NO_LIMIT)
    }

    // The time the call was given, or nothing when it was given no limit.
    pub fn limit(&self) -> Option<Duration> {
        let held = self.limit.load(Ordering::Relaxed);
        if held >= NO_LIMIT {
            return None;
        }
        Some(Duration::from_millis(held))
    }

    // How long the call has been running.
    pub fn elapsed(&self) -> Duration {
        self.began.elapsed()
    }

    // The time the call has left, or nothing when it was given no limit. A call
    // that is over its time has nothing left, and never less.
    pub fn left(&self) -> Option<Duration> {
        let given = self.limit()?;
        let gone = self.began.elapsed();
        Some(given.checked_sub(gone).unwrap_or(Duration::ZERO))
    }

    // Give the call more time. Each press of the mark adds to the time that is
    // left, so a call that was near its end has the whole of the new time in
    // front of it.
    pub fn extend(&self, by: Duration) {
        // A call with no limit has nothing to add to.
        if self.limit().is_none() {
            return;
        }
        let left = self.left().unwrap_or(Duration::ZERO);
        self.limit.store(
            self.began.elapsed().as_millis() as u64 + left.as_millis() as u64 + by.as_millis() as u64,
            Ordering::Relaxed,
        );
    }

    // The user asked for the call to end. The clock is not moved: the call ends
    // because the flag stands, and the words that come back say who stopped it.
    pub fn stop_now(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    // True when the user asked for the call to end.
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    // True when there is nothing left to wait for: the time is gone, or the
    // user asked for the call to end.
    pub fn expired(&self) -> bool {
        self.stopped() || self.left().is_some_and(|left| left.is_zero())
    }
}

// Wait for a thing of the program to come back within the clock of the call. The
// thing is worked on a thread of its own, and the wait is bounded by the clock: so
// long as the time stands, or the user has not asked for the call to end, nothing
// is waited for further and nothing at all is given back.
//
// This stands for the tools that are not a command, and above all for a server of
// the user's own: such a thing reaches out over a pipe or over the network and can
// stand waiting for an answer that never comes. The clock of the call was read by
// nothing in that road, so a call that never came back held the chat with a block
// that said "running" for hours, and neither the mark that cuts the call short nor
// the mark that stops the answer could do anything, for nothing looked at the flag
// they set. A thing that never comes back is no reason for a chat to stand still
// for ever. The work may still be going on its thread when this gives up; the words
// the call gives back say that what the tool does after this is not known.
pub fn within<T: Send + 'static, W: FnOnce() -> T + Send + 'static>(watch: &Watch, work: W) -> Option<T> {
    let (told, hearing) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = told.send(work());
    });
    loop {
        let step = watch
            .left()
            .map(|left| left.min(RUN_STEP))
            .unwrap_or(RUN_STEP);
        match hearing.recv_timeout(step) {
            Ok(made) => return Some(made),
            // The thread that was to bring the thing is gone, so nothing of it is
            // to be had.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if watch.expired() {
            return None;
        }
    }
}

// Run a program and read what it prints. The call never waits longer than the
// timeout. A program that cannot be started gives a failed Capture.
pub fn capture<A: AsRef<std::ffi::OsStr>>(program: &str, args: &[A], timeout: Duration) -> Capture {
    capture_in(program, args, None, timeout)
}

// Same as capture, with a directory to start the program in.
pub fn capture_in<A: AsRef<std::ffi::OsStr>>(
    program: &str,
    args: &[A],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Capture {
    capture_watch_in(program, args, cwd, &Watch::new(timeout))
}

// The longest one step of the wait for a run that is going. The clock is read
// again at the end of every step, so a call that was given more time is left
// alone and one the user cut short is stopped soon after the mark is pressed.
// A run that comes back of its own accord is known at once, for its end is
// brought to the wait by a thread of its own.
const RUN_STEP: Duration = Duration::from_millis(100);

// The time the two readers of the output pipes are given to bring the last of
// the words after the run has stopped. Once the run is stopped the pipes close
// in a moment, so this is only a wall: a child that put itself outside the
// group of the run could hold a pipe open for ever, and the call never waits
// for it.
const WORDS_GRACE: Duration = Duration::from_millis(500);

// The same, with a clock the user may lengthen or cut short while the program
// runs. Everything the program printed before it was stopped stands in the
// answer, for a model that is told a call ran out of time still has to know
// what the call managed to say.
//
// Two rules keep the call from being held longer than the run it watches.
// The run is put in a group of processes of its own and the signal goes to the
// group, for a shell forks the command it was given, and the child it forks
// holds the output pipes open: a signal to the shell alone leaves the call
// waiting for that child to end by itself. And the two pipes are read on
// threads of their own into words the call shares with them, so a call that is
// stopped comes back at once with the words read up to that moment.
pub fn capture_watch_in<A: AsRef<std::ffi::OsStr>>(
    program: &str,
    args: &[A],
    cwd: Option<&Path>,
    watch: &Watch,
) -> Capture {
    let mut command = Command::new(program);
    command.args(args.iter().map(|arg| arg.as_ref()));
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    in_its_own_group(&mut command);
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

    // Read the two output pipes at the same time, each on a thread of its own.
    // One pipe must not wait for the other, or a program that prints a lot
    // stops writing, and neither of them may hold the call: the words are taken
    // from the shared place as they stand when the call is over.
    let out_words = Words::new();
    let err_words = Words::new();
    let out_reader = child
        .stdout
        .take()
        .map(|pipe| read_words(pipe, out_words.clone()));
    let err_reader = child
        .stderr
        .take()
        .map(|pipe| read_words(pipe, err_words.clone()));

    // A thread waits for the program itself, so that a run which comes back is
    // known at once and not at the next step of the wait here.
    let (told, hearing) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = told.send(child.wait());
    });
    let mut status: Option<std::process::ExitStatus> = None;
    let mut stopped = false;
    loop {
        let step = watch
            .left()
            .map(|left| left.min(RUN_STEP))
            .unwrap_or(RUN_STEP);
        match hearing.recv_timeout(step) {
            Ok(Ok(done)) => {
                status = Some(done);
                break;
            }
            // The thread that was to say how the run ended is gone, so nothing
            // is waited for further.
            Ok(Err(problem)) => {
                stop_group(pid);
                return Capture::failed(&problem.to_string());
            }
            Err(RecvTimeoutError::Disconnected) => {
                stop_group(pid);
                return Capture::failed("the end of the run could not be heard");
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if watch.expired() {
            stopped = true;
            break;
        }
    }
    // The call is over, and the whole of the run goes with it. What is still
    // going, and what still holds a pipe of the run open, is a child of the
    // program that was started: left alive it would print into a pipe nobody
    // reads for the rest of its life.
    stop_group(pid);
    // The readers are given a little time to bring what the run printed at the
    // end of it.
    wait_for_words(out_reader, WORDS_GRACE);
    wait_for_words(err_reader, WORDS_GRACE);

    let mut stdout = out_words.of();
    let mut stderr = err_words.of();
    trim_newlines(&mut stdout);
    trim_newlines(&mut stderr);
    match status {
        Some(done) => Capture {
            ok: done.success() && !stopped,
            code: done.code().unwrap_or(-1),
            out: stdout,
            err: stderr,
            timed_out: stopped,
        },
        None => Capture {
            ok: false,
            code: -1,
            out: stdout,
            err: stderr,
            timed_out: stopped,
        },
    }
}

// The words of one output pipe, held where the call can read them while the
// thread that reads the pipe is still adding to them.
#[derive(Clone)]
struct Words(Arc<Mutex<Vec<u8>>>);

impl Words {
    fn new() -> Words {
        Words(Arc::new(Mutex::new(Vec::new())))
    }

    // Add what was read from the pipe. Words are kept as bytes, so a run that
    // is stopped between the parts of a sign of more than one byte loses that
    // sign and nothing else.
    fn add(&self, made: &[u8]) {
        if let Ok(mut held) = self.0.lock() {
            held.extend_from_slice(made);
        }
    }

    // The words as they stand at this moment.
    fn of(&self) -> String {
        let held = self.0.lock().map(|held| held.clone()).unwrap_or_default();
        String::from_utf8_lossy(&held).to_string()
    }
}

// Read one pipe on a thread of its own, until the pipe closes. What was read
// goes into the shared words, so the call may take them whenever it is over,
// whether the pipe has closed or no.
fn read_words<R: Read + Send + 'static>(
    mut pipe: R,
    words: Words,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        let mut chunk = [0_u8; 8192];
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(made) => words.add(&chunk[..made]),
            Err(_) => break,
        }
    })
}

// Give one reader of a pipe the time given to end. When the time is gone the
// reader is left to itself, and the call takes the words as they stand.
fn wait_for_words(reader: Option<std::thread::JoinHandle<()>>, grace: Duration) {
    let Some(reader) = reader else { return };
    let began = Instant::now();
    while !reader.is_finished() {
        if began.elapsed() >= grace {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = reader.join();
}

// Put the run in a group of processes of its own. The command a shell is given
// becomes a child of that shell, and the children of the run are of the group
// too, so one signal stops the whole of it. No group is set outside unix: the
// group of the run is then the group of the program that started it, and a
// signal to it would stop the program itself with the run. The two ways a run
// is stopped are the clock of the call and the mark the user presses; both go
// through stop_group here, and neither reaches a run that is no longer of the
// group of the program.
fn in_its_own_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

fn trim_newlines(text: &mut String) {
    while text.ends_with('\n') || text.ends_with('\r') || text.ends_with(' ') {
        text.pop();
    }
}

// Stop a whole run: the program that was started and every child of it. The id
// of the group is the id of the program, which was put in a group of its own
// by in_its_own_group. A signal to a negative id goes to the group of that id.
// The one road that stops a single process, and nothing of its children, is
// kill_pid.
pub fn stop_group(pid: u32) {
    #[cfg(unix)]
    {
        let _ = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

// Stop one program by its process id. A child it forked is not stopped with it
// and may hold an output pipe open, so a run that was started for its output is
// stopped by stop_group.
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

// The sixty four characters base64 writes with, in the order of the table.
const B64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

// Bytes turned into base64, as a data URL of a picture wants them. Three bytes
// go out as four characters, and the last two of those stand for nothing when
// the run of three was short. No crate is added for this: the program keeps the
// weight of its binary down, and twenty lines of it are cheaper than a part.
pub fn b64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for part in bytes.chunks(3) {
        let one = part[0];
        let two = *part.get(1).unwrap_or(&0);
        let three = *part.get(2).unwrap_or(&0);
        let joined = ((one as u32) << 16) | ((two as u32) << 8) | three as u32;
        let pick = |shift: u32| B64_TABLE[(joined as usize >> shift) & 63] as char;
        out.push(pick(18));
        out.push(pick(12));
        // A short run of two bytes says so with a mark, and one of one byte with
        // two of them.
        out.push(if part.len() > 1 { pick(6) } else { '=' });
        out.push(if part.len() > 2 { pick(0) } else { '=' });
    }
    out
}

// Bytes turned back out of base64, as a data URL of a picture keeps them. A run
// of marks that is no run of base64 is answered with nothing. The mark of the
// end, and the new lines a long run is broken with, are passed over.
pub fn b64_of(text: &str) -> Option<Vec<u8>> {
    let mut bits: u32 = 0;
    let mut held = 0u32;
    let mut out: Vec<u8> = Vec::new();
    for one in text.bytes() {
        let value = if one.is_ascii_uppercase() {
            one - b'A'
        } else if one.is_ascii_lowercase() {
            one - b'a' + 26
        } else if one.is_ascii_digit() {
            one - b'0' + 52
        } else if one == b'+' {
            62
        } else if one == b'/' {
            63
        } else if one == b'=' || one == b'\n' || one == b'\r' || one == b' ' {
            continue;
        } else {
            return None;
        };
        bits = (bits << 6) | u32::from(value);
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push(((bits >> held) & 0xff) as u8);
        }
    }
    Some(out)
}

// The room a length code of DEFLATE stands for, and the bits of the room that
// follow the code: the twenty nine codes of the lengths.
const LENGTH_ROOM: [(usize, u32); 29] = [
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 1),
    (13, 1),
    (15, 1),
    (17, 1),
    (19, 2),
    (23, 2),
    (27, 2),
    (31, 2),
    (35, 3),
    (43, 3),
    (51, 3),
    (59, 3),
    (67, 4),
    (83, 4),
    (99, 4),
    (115, 4),
    (131, 5),
    (163, 5),
    (195, 5),
    (227, 5),
    (258, 0),
];

// The way back a distance code stands for, and the bits of it that follow the
// code: the thirty codes of the distances.
const DISTANCE_ROOM: [(usize, u32); 30] = [
    (1, 0),
    (2, 0),
    (3, 0),
    (4, 0),
    (5, 1),
    (7, 1),
    (9, 2),
    (13, 2),
    (17, 3),
    (25, 3),
    (33, 4),
    (49, 4),
    (65, 5),
    (97, 5),
    (129, 6),
    (193, 6),
    (257, 7),
    (385, 7),
    (513, 8),
    (769, 8),
    (1025, 9),
    (1537, 9),
    (2049, 10),
    (3073, 10),
    (4097, 11),
    (6145, 11),
    (8193, 12),
    (12289, 12),
    (16385, 13),
    (24577, 13),
];

// The order the lengths of the codes of a block written with its own table stand
// in, and the greatest length of a code of that table.
const CODE_LENGTH_ORDER: [usize; 19] =
    [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

// The greatest length of a code of the shape, and the symbol that ends a block.
const GREATEST_CODE: usize = 15;
const END_OF_BLOCK: u16 = 256;

// A run of bytes, read a few bits at a time. The bits of a byte are read from
// its least end, which is how the shape writes them, and the words of a code are
// read from the front of the code.
struct Bits<'a> {
    data: &'a [u8],
    // The bit of the whole run that comes next.
    at: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Bits<'a> {
        Bits { data, at: 0 }
    }

    // One bit more, or nothing when the run is at an end.
    fn bit(&mut self) -> Option<u32> {
        let place = self.at / 8;
        if place >= self.data.len() {
            return None;
        }
        let shift = self.at % 8;
        self.at += 1;
        Some(((self.data[place] >> shift) & 1) as u32)
    }

    // A number of bits, the first of them the least of the number.
    fn few(&mut self, most: u32) -> Option<u32> {
        let mut value = 0u32;
        for step in 0..most {
            value |= self.bit()? << step;
        }
        Some(value)
    }

    // Go on to the bound of the next byte, which is how a run of bytes that was
    // put in whole begins.
    fn align(&mut self) {
        self.at = self.at.div_ceil(8) * 8;
    }

    // Two bytes, the first of them the least.
    fn pair(&mut self) -> Option<u32> {
        let one = self.few(8)?;
        let two = self.few(8)?;
        Some(one | (two << 8))
    }

    // The next whole bytes, for a run that was put in without being written over.
    fn whole(&mut self, most: usize) -> Option<&'a [u8]> {
        let began = self.at / 8;
        if began + most > self.data.len() {
            return None;
        }
        self.at = (began + most) * 8;
        Some(&self.data[began..began + most])
    }
}

// A table of codes: how many codes there are of each length, and the symbols
// those codes stand for, the shortest first and in their order within a length.
// A canonical table gives its codes in that order, so the two together are the
// whole of the table.
struct Codes {
    counts: [u16; GREATEST_CODE + 1],
    symbols: Vec<u16>,
}

impl Codes {
    fn new(lengths: &[u8]) -> Codes {
        let mut counts = [0u16; GREATEST_CODE + 1];
        for one in lengths {
            if *one > 0 && *one <= GREATEST_CODE as u8 {
                counts[usize::from(*one)] += 1;
            }
        }
        let mut symbols: Vec<u16> = Vec::with_capacity(lengths.len());
        for length in 1..=GREATEST_CODE {
            for (place, one) in lengths.iter().enumerate() {
                if usize::from(*one) == length {
                    symbols.push(place as u16);
                }
            }
        }
        Codes { counts, symbols }
    }

    // A symbol out of the run, or nothing when the codes of the table come to an
    // end before the run does.
    fn read(&self, bits: &mut Bits) -> Option<u16> {
        let mut code = 0i64;
        let mut first = 0i64;
        let mut index = 0usize;
        for length in 1..=GREATEST_CODE {
            code |= i64::from(bits.bit()?);
            let count = i64::from(self.counts[length]);
            if code - count < first {
                let place = index + (code - first) as usize;
                return self.symbols.get(place).copied();
            }
            index += count as usize;
            // The first code of the next length is the code just after the last
            // of this length, with a place kept for the bit this length has no
            // more than the next one.
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

// The table of codes a block written with the table of the shape stands on: the
// one for the words and the lengths, and the one for the distances.
fn fixed_tables() -> (Codes, Codes) {
    let mut words = vec![0u8; 288];
    for (place, one) in words.iter_mut().enumerate() {
        *one = if place < 144 {
            8
        } else if place < 256 {
            9
        } else if place < 280 {
            7
        } else {
            8
        };
    }
    let distances = vec![5u8; 30];
    (Codes::new(&words), Codes::new(&distances))
}

// The table of codes of a block that carries its own table with it. The lengths
// of the codes of the two tables are themselves written with a table of a third
// kind.
fn read_tables(bits: &mut Bits) -> Option<(Codes, Codes)> {
    let words = 257 + usize::try_from(bits.few(5)?).unwrap_or(0);
    let distances = 1 + usize::try_from(bits.few(5)?).unwrap_or(0);
    let guides = 4 + usize::try_from(bits.few(4)?).unwrap_or(0);
    if words > 288 || distances > 30 || guides > CODE_LENGTH_ORDER.len() {
        return None;
    }
    let mut guide_lengths = vec![0u8; 19];
    for place in 0..guides {
        guide_lengths[CODE_LENGTH_ORDER[place]] = u8::try_from(bits.few(3)?).unwrap_or(0);
    }
    let guide = Codes::new(&guide_lengths);
    let mut lengths: Vec<u8> = Vec::with_capacity(words + distances);
    while lengths.len() < words + distances {
        let symbol = guide.read(bits)?;
        if symbol < 16 {
            lengths.push(u8::try_from(symbol).unwrap_or(0));
            continue;
        }
        let (nought, most, bias) = match symbol {
            16 => (false, 2u32, 3usize),
            17 => (true, 3, 3),
            18 => (true, 7, 11),
            _ => return None,
        };
        // The mark of a length held over says to write the length before it
        // again, so there must be one. A run of noughts needs nothing before it:
        // a table may begin with the whole of its front part nought.
        let last = *lengths.last().unwrap_or(&0);
        if !nought && lengths.is_empty() {
            return None;
        }
        let run = usize::try_from(bits.few(most)?).unwrap_or(0) + bias;
        let hold = if nought { 0 } else { last };
        if lengths.len() + run > words + distances {
            return None;
        }
        for _ in 0..run {
            lengths.push(hold);
        }
    }
    let (word_lengths, distance_lengths) = lengths.split_at(words);
    Some((Codes::new(word_lengths), Codes::new(distance_lengths)))
}

// Bytes made plain out of a run of DEFLATE, with the marks of a zlib stream in
// front of them when they are there. This is what a stream of a PDF that says
// `FlateDecode` holds: no crate is added for it, for the program keeps the
// weight of its binary down, and a hundred lines of it are cheaper than a part.
// A run that is no run of DEFLATE is answered with nothing, and so is one that
// goes on for more than the ceiling asked for.
pub fn inflate(bytes: &[u8], ceiling: usize) -> Option<Vec<u8>> {
    let mut bits = Bits::new(bytes);
    // The two marks of a zlib stream say what shape the bytes behind them are of.
    // A run without them is taken for raw DEFLATE, which is the same writing
    // without the marks about it.
    if bytes.len() >= 2 && bytes[0] & 0x0f == 8 {
        let check = (u16::from(bytes[0]) << 8) | u16::from(bytes[1]);
        if check % 31 == 0 {
            bits.at = 16;
            // The mark of a dictionary says that four bytes of the number of it
            // stand behind the two marks.
            if bytes[1] & 0x20 != 0 {
                bits.few(32)?;
            }
        }
    }
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.bit()?;
        let kind = bits.few(2)?;
        let (words, distances) = match kind {
            0 => {
                // A run of bytes put in whole, with its length twice over: once,
                // and once again with every bit turned about.
                bits.align();
                let length = usize::try_from(bits.pair()?).unwrap_or(0);
                let again = bits.pair()?;
                if again != (!u32::try_from(length).unwrap_or(0)) & 0xffff {
                    return None;
                }
                let held = bits.whole(length)?;
                out.extend_from_slice(held);
                if out.len() > ceiling {
                    return None;
                }
                if last == 1 {
                    return Some(out);
                }
                continue;
            }
            1 => fixed_tables(),
            2 => read_tables(&mut bits)?,
            _ => return None,
        };
        loop {
            let symbol = words.read(&mut bits)?;
            if symbol < END_OF_BLOCK {
                out.push(u8::try_from(symbol).unwrap_or(0));
                if out.len() > ceiling {
                    return None;
                }
                continue;
            }
            if symbol == END_OF_BLOCK {
                break;
            }
            let place = usize::from(symbol) - 257;
            let (room, more) = LENGTH_ROOM.get(place).copied()?;
            let length = room + usize::try_from(bits.few(more)?).unwrap_or(0);
            let back = usize::from(distances.read(&mut bits)?);
            let (distance, more) = DISTANCE_ROOM.get(back).copied()?;
            let way = distance + usize::try_from(bits.few(more)?).unwrap_or(0);
            if way == 0 || way > out.len() || out.len() + length > ceiling {
                return None;
            }
            // The words of the run copy out of the bytes before them, and may run
            // over the bytes they are making, which is how the shape pays so
            // little for a page of one letter over and over.
            let began = out.len() - way;
            for step in 0..length {
                let one = out[began + step];
                out.push(one);
            }
        }
        if last == 1 {
            return Some(out);
        }
    }
}

// Tokens with a comma every three digits, like 64,763. Both windows show the
// context size this way.
pub fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
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

#[cfg(test)]
mod tests {
    use super::*;

    // Bytes turned into base64, of the shapes a run of three bytes can come in.
    // The words of a picture that goes to the model are made this way, so the
    // table and the marks at the end must both be right.
    #[test]
    fn bytes_are_turned_into_base64() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64(&[0xff, 0xef, 0xbe]), "/+++");
        assert_eq!(b64(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), "AAECAwQFBgcICQ==");
        // A character of more than one byte is bytes, and not a character, to
        // the table.
        assert_eq!(b64("\u{e9}".as_bytes()), "w6k=");
    }

    // The clock of a call: the time it has left, the time added to it, and the
    // hand of the user. A call that was given no time at all is left alone by
    // all three.
    #[test]
    fn the_clock_of_a_call_counts_and_can_be_moved() {
        let watch = Watch::new(Duration::from_millis(40));
        assert!(!watch.expired());
        assert!(watch.left().is_some());
        std::thread::sleep(Duration::from_millis(90));
        assert!(watch.expired());
        assert_eq!(watch.left(), Some(Duration::ZERO));
        // More time puts the end of the call back, so the call is no longer over.
        watch.extend(Duration::from_millis(400));
        assert!(!watch.expired());
        // The hand of the user ends the call at once, and says who ended it.
        watch.stop_now();
        assert!(watch.expired());
        assert!(watch.stopped());
        let free = Watch::unbounded();
        assert!(free.limit().is_none());
        assert!(free.left().is_none());
        assert!(!free.expired());
        free.extend(Duration::from_secs(60));
        assert!(free.limit().is_none());
    }

    // A thing of the program that comes back within the clock is given back as it
    // came.
    #[test]
    fn a_thing_that_comes_back_within_its_clock_is_given_back() {
        let watch = Watch::new(Duration::from_secs(30));
        let made = within(&watch, || String::from("the words of the tool"));
        assert_eq!(made.as_deref(), Some("the words of the tool"));
    }

    // The fault these tests stand for: a thing of the program that never comes
    // back. The clock of the call is now read in that road, so the call is given up
    // when its time is gone instead of holding the chat with a block that says
    // "running" for as long as the program stands.
    #[test]
    fn a_thing_that_never_comes_back_is_given_up_with_its_clock() {
        let watch = Watch::new(Duration::from_millis(300));
        let began = Instant::now();
        let made = within(&watch, || {
            std::thread::sleep(Duration::from_secs(30));
            String::from("words that come too late")
        });
        let gone = began.elapsed();
        assert!(made.is_none(), "a thing that never came back was given back");
        assert!(
            gone < Duration::from_secs(3),
            "the call was held for {} ms after its clock was gone",
            gone.as_millis()
        );
    }

    // The same fault with the mark of the hand: a call that was given no time at
    // all, and a thing that never comes back. The mark that cuts the call short has
    // to be answered for at once, for that mark is the one thing the user has.
    #[test]
    fn a_thing_that_never_comes_back_is_given_up_with_the_hand() {
        let watch = Watch::unbounded();
        let clock = watch.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            clock.stop_now();
        });
        let began = Instant::now();
        let made = within(&watch, || {
            std::thread::sleep(Duration::from_secs(30));
            String::from("words that come too late")
        });
        let gone = began.elapsed();
        assert!(made.is_none(), "a call the user stopped was given back");
        assert!(
            watch.stopped(),
            "the clock does not say the user stopped the call"
        );
        assert!(
            gone < Duration::from_secs(3),
            "the call was held for {} ms after the mark was pressed",
            gone.as_millis()
        );
    }

    // A run of words written with the table of a block, and a run put in whole:
    // both with the two marks of a zlib stream about them, as the machine that
    // wrote them wrote them. These are the two shapes a stream of a PDF page
    // comes in, and the words must come back as they went.
    #[test]
    fn the_shape_of_the_machine_is_made_plain() {
        let words = "The plan of the work stands here, down to the smallest detail. ".repeat(8);
        let written = figures(
            "78da0bc9485528c849cc53c84f532801b2cbf38bb2158a4b12f3528a1532528b52751452f2\
             cbf3144af2c1b2c5b9893939a9c5250a29a9258999397a0a21a3da87a67600bd90b321",
        );
        assert_eq!(
            inflate(&written, 1 << 20).as_deref(),
            Some(words.as_bytes()),
            "the words of a block written with its own table came back other than they went"
        );
        let whole = figures("7801010500faff68656c6c6f062c0215");
        assert_eq!(
            inflate(&whole, 1 << 20).as_deref(),
            Some(b"hello".as_slice()),
            "a run put in whole was not read as such"
        );
    }

    // A run that is no run of DEFLATE, and one that would grow past the ceiling
    // it was given: both answered with nothing.
    #[test]
    fn a_run_that_is_no_stream_is_answered_with_nothing() {
        assert!(inflate(b"these are no bytes of DEFLATE", 1 << 20).is_none());
        let words = "The plan of the work stands here, down to the smallest detail. ".repeat(8);
        let written = figures(
            "78da0bc9485528c849cc53c84f532801b2cbf38bb2158a4b12f3528a1532528b52751452f2\
             cbf3144af2c1b2c5b9893939a9c5250a29a9258999397a0a21a3da87a67600bd90b321",
        );
        assert!(inflate(&written, 8).is_none());
        assert_eq!(inflate(&written, 1 << 20).map(|made| made.len()), Some(words.len()));
    }

    // True when a program with these words in its command line is still going.
    #[cfg(unix)]
    fn still_going(needle: &str) -> bool {
        Command::new("pgrep")
            .args(["-f", needle])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|done| done.success())
            .unwrap_or(false)
    }

    // A run that is given less time than it takes is stopped, and the call
    // comes back at once instead of waiting for the run to end by itself. The
    // shell forks the command it was given, and that child holds the output
    // pipe open, so the signal has to go to the group of the run.
    #[cfg(unix)]
    #[test]
    fn a_run_out_of_time_is_stopped() {
        let began = Instant::now();
        let made = capture_watch_in(
            "sh",
            &["-c", "sleep 30", "natter"],
            None,
            &Watch::new(Duration::from_millis(200)),
        );
        let gone = began.elapsed();
        assert!(
            made.timed_out,
            "a run of 30 seconds was not stopped by a watch of 200 ms"
        );
        assert!(
            gone < Duration::from_secs(5),
            "the call was held for {} ms, which is the length of the run and not of the watch",
            gone.as_millis()
        );
    }

    // A command that forked is stopped with it: the children of the run go when
    // its time is gone, and nothing of it is left behind.
    #[cfg(unix)]
    #[test]
    fn a_run_that_forked_is_stopped_with_it() {
        let began = Instant::now();
        let made = capture_watch_in(
            "sh",
            &["-c", "sleep 32 & sleep 32", "natter"],
            None,
            &Watch::new(Duration::from_millis(200)),
        );
        let gone = began.elapsed();
        assert!(made.timed_out, "the run was not stopped when its time was gone");
        assert!(
            gone < Duration::from_secs(5),
            "the call was held for {} ms",
            gone.as_millis()
        );
        assert!(
            !still_going("sleep 32"),
            "a child of the run was left going after the call came back"
        );
    }

    // A run that ends but leaves a child holding the output pipe open must not
    // hold the call: the words it printed come back at once, and the child that
    // was left is stopped with the call.
    #[cfg(unix)]
    #[test]
    fn a_run_that_leaves_a_pipe_open_comes_back() {
        let began = Instant::now();
        let made = capture_watch_in(
            "sh",
            &["-c", "sleep 34 & echo here", "natter"],
            None,
            &Watch::unbounded(),
        );
        let gone = began.elapsed();
        assert!(
            made.out.contains("here"),
            "the words the run printed did not come back: {:?}",
            made.out
        );
        assert!(
            gone < Duration::from_secs(5),
            "the call was held for {} ms by a pipe one child left open",
            gone.as_millis()
        );
        assert!(
            !still_going("sleep 34"),
            "a child of the run was left going after the call came back"
        );
    }

    // A call that is stopped in the middle of a run still carries everything
    // the run had printed by then, for a model has to know what the call
    // managed to do before it was stopped.
    #[cfg(unix)]
    #[test]
    fn the_words_before_the_end_come_back() {
        let made = capture_watch_in(
            "sh",
            &["-c", "echo one; sleep 36; echo two", "natter"],
            None,
            &Watch::new(Duration::from_millis(600)),
        );
        assert!(made.timed_out, "the run was not stopped when its time was gone");
        assert!(
            made.out.contains("one"),
            "the words printed before the end were lost: {:?}",
            made.out
        );
        assert!(
            !made.out.contains("two"),
            "words printed after the end came back: {:?}",
            made.out
        );
    }

    // A call the user cuts short with the mark is stopped the same way as a
    // call whose time ran out, and the words it had printed come back too.
    #[cfg(unix)]
    #[test]
    fn a_call_the_user_stops_is_stopped_with_it() {
        let watch = Watch::new(Duration::from_secs(600));
        let clock = watch.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            clock.stop_now();
        });
        let began = Instant::now();
        let made = capture_watch_in(
            "sh",
            &["-c", "echo one; sleep 38; echo two", "natter"],
            None,
            &watch,
        );
        let gone = began.elapsed();
        assert!(made.timed_out, "a call the user stopped was not stopped");
        assert!(watch.stopped(), "the clock does not say the user stopped the call");
        assert!(
            made.out.contains("one"),
            "the words printed before the stop were lost: {:?}",
            made.out
        );
        assert!(
            gone < Duration::from_secs(5),
            "the call was held for {} ms after the mark was pressed",
            gone.as_millis()
        );
    }

    // A short run is not held by the wait either: it comes back with its words
    // and its code as soon as it is done.
    #[cfg(unix)]
    #[test]
    fn a_short_run_comes_back_at_once() {
        let began = Instant::now();
        let made = capture_watch_in(
            "sh",
            &["-c", "echo quick", "natter"],
            None,
            &Watch::new(Duration::from_secs(30)),
        );
        let gone = began.elapsed();
        assert!(made.ok, "a run that ended well was not told to be well");
        assert_eq!(made.code, 0);
        assert_eq!(made.out, "quick");
        assert!(!made.timed_out);
        assert!(
            gone < Duration::from_secs(1),
            "a run of a moment was held for {} ms",
            gone.as_millis()
        );
    }

    // The bytes of a run written as pairs of hexadecimal figures.
    fn figures(text: &str) -> Vec<u8> {
        let joined: String = text.chars().filter(|one| one.is_ascii_hexdigit()).collect();
        (0..joined.len() / 2)
            .map(|pair| u8::from_str_radix(&joined[pair * 2..pair * 2 + 2], 16).unwrap_or(0))
            .collect()
    }
}
