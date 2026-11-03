// Small helpers that do not belong to one system.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

    // The wait steps back every so often, so a long time costs nothing.
    pub fn pause(&self) {
        std::thread::sleep(Duration::from_millis(100));
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

// The same, with a clock the user may lengthen or cut short while the program
// runs. Everything the program printed before it was stopped stands in the
// answer, for a model that is told a call ran out of time still has to know
// what the call managed to say.
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
    // The watchdog stops a program that does not end by itself, and it stops
    // one the user asked to be cut short. The clock is read again on every
    // step, so a call that was given more time is left alone.
    let clock = watch.clone();
    std::thread::spawn(move || loop {
        if flag.load(Ordering::Relaxed) {
            return;
        }
        if clock.expired() {
            killed_flag.store(true, Ordering::Relaxed);
            kill_pid(pid);
            return;
        }
        clock.pause();
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

    // The bytes of a run written as pairs of hexadecimal figures.
    fn figures(text: &str) -> Vec<u8> {
        let joined: String = text.chars().filter(|one| one.is_ascii_hexdigit()).collect();
        (0..joined.len() / 2)
            .map(|pair| u8::from_str_radix(&joined[pair * 2..pair * 2 + 2], 16).unwrap_or(0))
            .collect()
    }
}
