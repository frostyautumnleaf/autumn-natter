// The logs of the program: the output of llama-server, and the log the program writes about
// itself.
//
// One file means one start. It holds everything that start has to say, and it lives under
// `logs` in the data directory of the program. The Logs tab of the settings panel shows the ten
// newest of each type, so a start that went wrong can still be read after the next one has
// taken its place, and it is read again by itself while the current start-up writes into it.
//
// The names of the functions that hold a directory of their own record it with `_in`, as the
// file tools do. The rules of the logs are then checked without the data directory of the
// program, which is how the tests below try them.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::store;

// The name of a log of either type begins with the text of its type and ends with this ending.
// Only a file of that form is listed, and only such a file is deleted, so a file the user put
// in the folder himself is left where it is.
pub const FIRST: &str = "server-";
pub const PROGRAM_FIRST: &str = "program-";
pub const LAST: &str = ".log";

// How many logs of one type are kept. A run that is weeks old is of no use to anyone, and a
// server that was asked to say everything writes a great deal.
pub const MOST_LOGS: usize = 10;

// The most bytes of one log that go to the page. The user is informed when the log is longer
// than this, and the search of the page checks what was read.
pub const GREATEST_READ: usize = 512 * 1024;

// The log of the server this run of the program started, and the log of the program itself. A
// new start puts a new name of its type in its place, and an empty place means nothing of that
// type was started at all.
static RUNNING_SERVER: Mutex<Option<String>> = Mutex::new(None);
static RUNNING_PROGRAM: Mutex<Option<String>> = Mutex::new(None);

// Which log is meant: the output of llama-server, or the log the program writes of its own
// work. The two types are in the same folder, follow the same rules and are kept apart by the
// text their names begin with, so a name of one type is never listed, read or deleted as a name
// of the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    // What llama-server wrote about one start of the server.
    Server,
    // What the program wrote about its own work: the handoff summary, the size of the context
    // and the meter that shows it.
    Program,
}

impl Kind {
    // The text a name of this type begins with.
    pub fn first(self) -> &'static str {
        match self {
            Kind::Server => FIRST,
            Kind::Program => PROGRAM_FIRST,
        }
    }

    // The type in one word. The page requests a type with this word, and a line of the program
    // log reports it before it reports anything else.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Server => "server",
            Kind::Program => "program",
        }
    }

    // The type a page requests. A word that is neither type, and no word at all, is read as the
    // log of llama-server, which is the face that was alone before the log of the program was
    // added.
    pub fn of_word(word: &str) -> Kind {
        if word == "program" {
            Kind::Program
        } else {
            Kind::Server
        }
    }

    // Whether a file name is the name of a log of this type. The middle holds the second of the
    // start, and may hold the number of a second start in the same second. A name that leads
    // out of the folder is not a name at all.
    pub fn holds(self, name: &str) -> bool {
        let Some(middle) = name
            .strip_prefix(self.first())
            .and_then(|rest| rest.strip_suffix(LAST))
        else {
            return false;
        };
        !middle.is_empty()
            && middle.chars().all(|one| one.is_ascii_digit() || one == '-')
    }

    // The log of this type that this run of the program is writing, by its name. Empty when
    // nothing of this type was started, for the page then has nothing to read again by itself.
    pub fn held(self) -> String {
        self.slot()
            .lock()
            .map(|held| held.clone().unwrap_or_default())
            .unwrap_or_default()
    }

    // Say that this run of the program is writing this log of this type. An empty name reports
    // it is writing none, which is what a start that could not make its log leaves behind.
    pub fn took(self, name: &str) {
        if let Ok(mut held) = self.slot().lock() {
            *held = if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            };
        }
    }

    // The place of the type. The two types hold their names apart, so the log of the program is
    // never mistaken for the log of the server.
    fn slot(self) -> &'static Mutex<Option<String>> {
        match self {
            Kind::Server => &RUNNING_SERVER,
            Kind::Program => &RUNNING_PROGRAM,
        }
    }
}

// One log on disk.
pub struct Log {
    // The name of the file, which is also the name the page asks with.
    pub name: String,
    // The second the log was last written to.
    pub when: i64,
    // The same second in text, for the list the page shows.
    pub at: String,
    // The bytes of the log, and the same number in text for the page.
    pub bytes: u64,
    pub size: String,
    // True for the log this run of the program is writing, which is the one the page reads
    // again by itself.
    pub current: bool,
}

// What one log holds, and how much of it was left behind.
pub struct Reading {
    pub text: String,
    // The whole size of the log on disk.
    pub bytes: u64,
    // True when the start of the log is not here, because the log is longer than the most that
    // is read.
    pub cut: bool,
}

// The folder the logs of this program are in. Both types are in it, apart by the text their
// names begin with.
pub fn dir() -> PathBuf {
    crate::config::paths().logs
}

// Start the log of a new start of this type and give back its place. The name holds the second
// the start was in, so the logs of a folder are in their own order by their names as well as by
// the time on them. The logs of this type older than the ten newest are deleted. None returns
// when the folder cannot be made, and the start is then begun with no log at all, as it was
// before.
pub fn start(kind: Kind) -> Option<PathBuf> {
    start_in(kind, &dir())
}

// The same, in a directory of your own.
pub fn start_in(kind: Kind, where_: &Path) -> Option<PathBuf> {
    fs::create_dir_all(where_).ok()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    // A second start in the same second needs a name of its own.
    let mut name = format!("{}{}{}", kind.first(), now, LAST);
    for part in 2..=99 {
        if !where_.join(&name).exists() {
            break;
        }
        name = format!("{}{}-{}{}", kind.first(), now, part, LAST);
    }
    let place = where_.join(&name);
    // The file is created here, so a start that died before it wrote a word still leaves a log
    // to be seen in the list.
    let made = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&place);
    if made.is_err() {
        return None;
    }
    kind.took(&name);
    prune_in(kind, where_);
    Some(place)
}

// Add one line to the log of this type that this run of the program is writing. The text of the
// line belong to the one who wrote it, and are put down as they are now. Nothing returns and
// nothing is asked again: a log that cannot be written must not stop the work the user
// requested.
pub fn note_line(kind: Kind, line: &str) {
    note_line_in(kind, &dir(), line);
}

// The same, in a directory of your own. The log of this type that this run is writing is the
// file the line is added to, and a type that writes no log at all takes no line.
pub fn note_line_in(kind: Kind, where_: &Path, line: &str) {
    let held = kind.held();
    if held.is_empty() {
        return;
    }
    let place = where_.join(&held);
    if let Ok(mut file) = fs::OpenOptions::new().append(true).open(&place) {
        let _ = file.write_all(line.as_bytes());
        let _ = file.write_all(b"\n");
    }
}

// The log of this type that this run of the program is writing, by its name.
pub fn current(kind: Kind) -> String {
    kind.held()
}

// The logs of this type, newest first, the ten newest of them.
pub fn list(kind: Kind) -> Vec<Log> {
    list_in(kind, &dir())
}

// The same, in a directory of your own.
pub fn list_in(kind: Kind, where_: &Path) -> Vec<Log> {
    let mut found = all_in(kind, where_);
    found.truncate(MOST_LOGS);
    found
}

// Every log of this type of a folder, newest first, with no ceiling over their number. The list
// of the page is cut to the ten newest, and the clear away of the old ones needs to see all of
// them.
pub fn all_in(kind: Kind, where_: &Path) -> Vec<Log> {
    let held = kind.held();
    let mut found: Vec<Log> = Vec::new();
    if let Ok(entries) = fs::read_dir(where_) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !kind.holds(&name) {
                continue;
            }
            let Ok(info) = entry.metadata() else { continue };
            if !info.is_file() {
                continue;
            }
            let when = info
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|since| since.as_secs() as i64)
                .unwrap_or(0);
            let bytes = info.len();
            let current = !held.is_empty() && name == held;
            found.push(Log {
                at: store::format_day_clock(when),
                name,
                when,
                bytes,
                size: size_text(bytes),
                current,
            });
        }
    }
    // Newest first. Two logs of one second are distinguished by their names, for the name of a
    // log holds the second of its start.
    found.sort_by(|a, b| b.when.cmp(&a.when).then_with(|| b.name.cmp(&a.name)));
    found
}

// Delete the logs of this type beyond the newest ten. The log this run is writing is never
// deleted, whatever the time written on it reports.
pub fn prune(kind: Kind) {
    prune_in(kind, &dir());
}

// The same, in a directory of your own.
pub fn prune_in(kind: Kind, where_: &Path) {
    let held = kind.held();
    let mut found = all_in(kind, where_);
    if found.len() <= MOST_LOGS {
        return;
    }
    for one in found.drain(MOST_LOGS..) {
        if !held.is_empty() && one.name == held {
            continue;
        }
        let _ = fs::remove_file(where_.join(&one.name));
    }
}

// The text of one log of this type, from its end.
pub fn read(kind: Kind, id: &str, most: usize) -> Option<Reading> {
    read_in(kind, &dir(), id, most)
}

// The same, in a directory of your own. A name that is not the name of a log of this type gives
// nothing at all, so the page cannot request a file of the computer.
pub fn read_in(kind: Kind, where_: &Path, id: &str, most: usize) -> Option<Reading> {
    if !kind.holds(id) {
        return None;
    }
    let place = where_.join(id);
    let bytes = fs::read(&place).ok()?;
    let cap = most.clamp(1, GREATEST_READ);
    let total = bytes.len();
    if total <= cap {
        return Some(Reading {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            bytes: total as u64,
            cut: false,
        });
    }
    // The last bytes of the log, begun at a whole line, so the first line the page is given is
    // not half a line. The first byte is moved on while it is in the middle of a character of
    // more than one byte.
    let mut from = total - cap;
    while from < total && (bytes[from] & 0xc0) == 0x80 {
        from += 1;
    }
    match bytes[from..].iter().position(|one| *one == b'\n') {
        Some(after) => from += after + 1,
        None => from = total,
    }
    Some(Reading {
        text: String::from_utf8_lossy(&bytes[from..]).into_owned(),
        bytes: total as u64,
        cut: true,
    })
}

// The bytes of a log in text, as the page shows them.
pub fn size_text(bytes: u64) -> String {
    const THOUSAND: u64 = 1024;
    if bytes < THOUSAND {
        return format!("{} B", bytes);
    }
    if bytes < THOUSAND * THOUSAND {
        return format!("{} KiB", bytes / THOUSAND);
    }
    format!("{:.1} MiB", bytes as f64 / (THOUSAND * THOUSAND) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A directory of its own for one test, for the tests run together.
    fn room(what: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let place = std::env::temp_dir().join(format!(
            "atnatter-logs-{}-{}-{}",
            std::process::id(),
            what,
            n
        ));
        let _ = fs::remove_dir_all(&place);
        fs::create_dir_all(&place).expect("a test folder for the logs can be made");
        place
    }

    // One log of the test, of one type, with the time it is meant for.
    fn made(kind: Kind, where_: &Path, second: i64, bytes: usize) -> PathBuf {
        let place = where_.join(format!("{}{}{}", kind.first(), second, LAST));
        fs::write(&place, vec![b'x'; bytes]).expect("a log of the test can be written");
        let _ = fs::OpenOptions::new()
            .write(true)
            .open(&place)
            .and_then(|file| {
                file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(second as u64))
            });
        place
    }

    // Only a name of the form of a log is a log. A name that leads out of the folder is not one
    // either, so the page cannot request a file of the computer.
    #[test]
    fn a_log_is_known_by_the_shape_of_its_name() {
        assert!(Kind::Server.holds("server-1762103944.log"));
        assert!(Kind::Server.holds("server-1762103944-2.log"));
        assert!(!Kind::Server.holds("server-.log"), "no second of a start");
        assert!(!Kind::Server.holds(".log"));
        assert!(!Kind::Server.holds("server-chat.log"), "no digits at all");
        assert!(!Kind::Server.holds("server-1762103944.txt"));
        assert!(!Kind::Server.holds("notes.txt"));
        assert!(!Kind::Server.holds("server-1762103944.log/"));
        assert!(!Kind::Server.holds("../server-1762103944.log"));
        assert!(!Kind::Server.holds("server-1762103944.LOG"));
    }

    // The two types are distinguished by the text their names begin with. A name of one type is
    // never read, listed or deleted as a name of the other, and no word of a page brings a type
    // that is not one of the two.
    #[test]
    fn the_two_kinds_tell_their_names_apart() {
        assert!(Kind::Program.holds("program-1762103944.log"));
        assert!(Kind::Program.holds("program-1762103944-2.log"));
        assert!(!Kind::Program.holds("server-1762103944.log"), "a name of the other type");
        assert!(!Kind::Server.holds("program-1762103944.log"), "a name of the other type");
        assert!(!Kind::Program.holds("program-.log"), "no second of a start");
        assert_eq!(Kind::Server.first(), "server-");
        assert_eq!(Kind::Program.first(), PROGRAM_FIRST);
        assert_eq!(Kind::Server.word(), "server");
        assert_eq!(Kind::Program.word(), "program");
        // The page asks with the word, and a word that is neither type means the log of
        // llama-server, which is the face that was alone before.
        assert_eq!(Kind::of_word("program"), Kind::Program);
        assert_eq!(Kind::of_word("server"), Kind::Server);
        assert_eq!(Kind::of_word(""), Kind::Server);
        assert_eq!(Kind::of_word("what this then"), Kind::Server);
    }

    // The ten newest logs are kept and the rest are deleted. The list remains newest first,
    // whatever order the folder gives them back in.
    #[test]
    fn the_ten_newest_logs_are_kept() {
        let place = room("keep");
        // Thirteen logs, one a second apart, of a size that reports them apart.
        let first: i64 = 1_600_000_000;
        for n in 0..13i64 {
            made(Kind::Server, &place, first + n, 10 + n as usize);
        }
        assert_eq!(all_in(Kind::Server, &place).len(), 13, "every log of the folder is seen");
        let kept = list_in(Kind::Server, &place);
        assert_eq!(kept.len(), MOST_LOGS, "only the ten newest are listed");
        assert_eq!(kept[0].when, first + 12, "the newest stands first");
        assert_eq!(kept[1].when, first + 11);
        assert_eq!(kept.last().expect("a list with a foot in it").when, first + 3);
        prune_in(Kind::Server, &place);
        let after = all_in(Kind::Server, &place);
        assert_eq!(after.len(), MOST_LOGS, "the old logs are gone");
        assert_eq!(after[0].when, first + 12, "the newest is left where it was");
        assert!(!place.join(format!("{}{}{}", FIRST, first, LAST)).exists());
        assert!(place.join(format!("{}{}{}", FIRST, first + 12, LAST)).exists());
        let _ = fs::remove_dir_all(&place);
    }

    // A log that is longer than the most that is read returns its end, begun at a whole line,
    // and reports that the front is not there.
    #[test]
    fn the_end_of_a_long_log_is_read_from_a_whole_line() {
        let place = room("tail");
        let name = format!("{}1600000001{}", FIRST, LAST);
        let mut words = String::new();
        for n in 0..600 {
            words.push_str(&format!("line {} of the log of llama-server\n", n));
        }
        fs::write(place.join(&name), &words).expect("a long log can be written");
        let whole = read_in(Kind::Server, &place, &name, GREATEST_READ).expect("the log can be read");
        assert!(!whole.cut, "all of a short log is here");
        assert!(whole.text.ends_with("line 599 of the log of llama-server\n"));
        assert!(whole.text.starts_with("line 0 of the log"));

        let small = read_in(Kind::Server, &place, &name, 900).expect("the end of the log can be read");
        assert!(small.cut, "the start of the log is left behind");
        assert!(small.bytes as usize > 900, "the whole size is informed");
        assert!(small.text.ends_with("line 599 of the log of llama-server\n"));
        assert!(
            small.text.starts_with("line "),
            "the first line of what is read is a whole line: {:?}",
            &small.text[..24.min(small.text.len())]
        );
        assert!(!small.text.contains("of the log of llama-server of the log"));
        let _ = fs::remove_dir_all(&place);
    }

    // A name that is not the name of a log reads nothing at all, and so does the name of the
    // other type, whatever is under it.
    #[test]
    fn a_name_that_is_no_log_reads_nothing() {
        let place = room("none");
        fs::write(place.join("settings.json"), b"a file of the machine").expect("a file can be written");
        assert!(read_in(Kind::Server, &place, "settings.json", 100).is_none());
        assert!(read_in(Kind::Server, &place, "", 100).is_none());
        assert!(read_in(Kind::Server, &place, "server-1600000000.log", 100).is_none(), "no such log");
        // A log of the program remains, but it is not a log of the server.
        let other = made(Kind::Program, &place, 1_600_000_005, 20);
        assert!(other.exists(), "the log of the program stands");
        assert!(
            read_in(Kind::Server, &place, &other.file_name().unwrap().to_string_lossy(), 100).is_none(),
            "a log of the other type is not read"
        );
        let _ = fs::remove_dir_all(&place);
    }

    // The two types are in one folder and are kept apart there: each list holds its own logs,
    // and the ten ceiling is weighed for each type on its own, so the log of the program is
    // never deleted to make space for a start of the server.
    #[test]
    fn the_two_kinds_are_kept_apart_in_one_folder() {
        let place = room("kinds");
        let first: i64 = 1_700_000_000;
        for n in 0..12i64 {
            made(Kind::Server, &place, first + n, 10 + n as usize);
            made(Kind::Program, &place, first + n, 20 + n as usize);
        }
        let servers = list_in(Kind::Server, &place);
        let programs = list_in(Kind::Program, &place);
        assert_eq!(servers.len(), MOST_LOGS, "ten logs of the server");
        assert_eq!(programs.len(), MOST_LOGS, "ten logs of the program");
        for one in servers.iter() {
            assert!(Kind::Server.holds(&one.name), "a server log: {}", one.name);
        }
        for one in programs.iter() {
            assert!(Kind::Program.holds(&one.name), "a program log: {}", one.name);
        }
        prune_in(Kind::Server, &place);
        assert_eq!(all_in(Kind::Server, &place).len(), MOST_LOGS, "the old server logs are removed");
        assert_eq!(
            all_in(Kind::Program, &place).len(),
            12,
            "nothing of the program was deleted"
        );
        let _ = fs::remove_dir_all(&place);
    }

    // A line is added to the log this run is writing, and to no other. A type that wrote no log
    // at all leaves the call of a line doing nothing, so a log that cannot be written stops no
    // work.
    #[test]
    fn a_line_is_added_to_the_log_being_written() {
        let place = room("line");
        let held = start_in(Kind::Program, &place).expect("a log of the program can be started");
        assert_eq!(current(Kind::Program), held.file_name().unwrap().to_string_lossy());
        note_line_in(Kind::Program, &place, "2026-11-02 03:01:44.123 meter the conversation weighs 412");
        note_line_in(Kind::Program, &place, "2026-11-02 03:01:45.004 summary the summary is kept");
        let read = read_in(Kind::Program, &place, &current(Kind::Program), GREATEST_READ)
            .expect("the log of the program can be read");
        assert_eq!(
            read.text,
            "2026-11-02 03:01:44.123 meter the conversation weighs 412\n2026-11-02 03:01:45.004 summary the summary is kept\n"
        );
        // The log of the server of the same folder is not written to, and a type that began
        // nothing takes no line.
        assert!(all_in(Kind::Server, &place).is_empty(), "no log of the server stands");
        Kind::Server.took("");
        note_line_in(Kind::Server, &place, "nothing of this is written");
        assert!(all_in(Kind::Server, &place).is_empty(), "no log was made for a line");
        Kind::Program.took("");
        assert!(current(Kind::Program).is_empty(), "a name is given back as nothing");
        let _ = fs::remove_dir_all(&place);
    }

    // A new log is created under the directory of the logs, and a second start in the same
    // second is not on top of the first one.
    #[test]
    fn a_new_log_is_made_and_named_for_its_second() {
        let place = room("start");
        let first = start_in(Kind::Server, &place).expect("a log can be started");
        let second = start_in(Kind::Server, &place).expect("a second log can be started");
        assert_ne!(first, second, "two starts, two logs");
        assert!(first.exists(), "the log is before the server writes a word");
        let names = all_in(Kind::Server, &place);
        assert_eq!(names.len(), 2, "both starts are listed");
        for one in names.iter() {
            assert!(Kind::Server.holds(&one.name), "a log of the server: {}", one.name);
            assert!(place.join(&one.name).exists(), "a listed log stands");
        }
        let _ = fs::remove_dir_all(&place);
    }

    // The text of a size, as the page shows it.
    #[test]
    fn the_size_of_a_log_is_said_in_words() {
        assert_eq!(size_text(512), "512 B");
        assert_eq!(size_text(2048), "2 KiB");
        assert_eq!(size_text(5 * 1024 * 1024 + 8), "5.0 MiB");
    }
}
