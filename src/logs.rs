// The logs of llama-server, and the ten newest of them.
//
// One file stands for one start of the server. It holds everything the server
// has to say about that start, and it lives under `logs` in the data directory
// of the program. The Logs tab of the sheet of settings shows the ten newest,
// so a start that went wrong can still be read after the next one has taken
// its place, and it is read again by itself while the server of this run writes
// into it.
//
// The names of the functions that hold a folder of their own say so with
// `_in`, as the file tools do. The rules of the logs are then looked at without
// the data directory of the program, which is how the tests below try them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::store;

// The name of a log begins with these words and ends with this ending. Only a
// file of that shape is listed, and only such a file is thrown away, so a file
// the user put in the folder himself is left where it stands.
pub const FIRST: &str = "server-";
pub const LAST: &str = ".log";

// How many logs are kept. A run that is weeks old is of no use to anyone, and a
// server that was asked to say everything writes a great deal.
pub const MOST_LOGS: usize = 10;

// The most bytes of one log that go to the page. The user is told when the log
// is longer than this, and the search of the page looks at what was read.
pub const GREATEST_READ: usize = 512 * 1024;

// The log of the server this run of the program started. A new start puts a new
// name here, and an empty place means no server was started at all.
static RUNNING: Mutex<Option<String>> = Mutex::new(None);

// One log on disk.
pub struct Log {
    // The name of the file, which is also the name the page asks with.
    pub name: String,
    // The second the log was last written to.
    pub when: i64,
    // The same second in words, for the list the page shows.
    pub at: String,
    // The bytes of the log, and the same number in words for the page.
    pub bytes: u64,
    pub size: String,
    // True for the log this run of the program is writing, which is the one the
    // page reads again by itself.
    pub current: bool,
}

// What one log holds, and how much of it was left behind.
pub struct Reading {
    pub text: String,
    // The whole size of the log on disk.
    pub bytes: u64,
    // True when the front of the log is not here, because the log is longer
    // than the most that is read.
    pub cut: bool,
}

// The folder of the logs of this program.
pub fn dir() -> PathBuf {
    crate::config::paths().logs
}

// Whether a file name is the name of a log. The middle holds the second of the
// start, and may hold the number of a second start in the same second.
pub fn holds(name: &str) -> bool {
    let Some(middle) = name
        .strip_prefix(FIRST)
        .and_then(|rest| rest.strip_suffix(LAST))
    else {
        return false;
    };
    !middle.is_empty()
        && middle.chars().all(|one| one.is_ascii_digit() || one == '-')
}

// The log of the server this run started, by its name. Empty when no server was
// started, for the page then has nothing to read again by itself.
pub fn current() -> String {
    RUNNING
        .lock()
        .map(|held| held.clone().unwrap_or_default())
        .unwrap_or_default()
}

// Start the log of a new server and give back its place. The name holds the
// second the start stood in, so the logs of a folder stand in their own order by
// their names as well as by the time on them. The logs older than the ten
// newest are thrown away. None comes back when the folder cannot be made, and
// the server is then started with no log at all, as it was before.
pub fn start() -> Option<PathBuf> {
    start_in(&dir())
}

// The same, in a folder of your own.
pub fn start_in(where_: &Path) -> Option<PathBuf> {
    fs::create_dir_all(where_).ok()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    // A second start in the same second needs a name of its own.
    let mut name = format!("{}{}{}", FIRST, now, LAST);
    for part in 2..=99 {
        if !where_.join(&name).exists() {
            break;
        }
        name = format!("{}{}-{}{}", FIRST, now, part, LAST);
    }
    let place = where_.join(&name);
    // The file is made here, so a server that died before it wrote a word still
    // leaves a log to be seen in the list.
    let made = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&place);
    if made.is_err() {
        return None;
    }
    if let Ok(mut held) = RUNNING.lock() {
        *held = Some(name);
    }
    prune_in(where_);
    Some(place)
}

// The logs of the program, newest first, the ten newest of them.
pub fn list() -> Vec<Log> {
    list_in(&dir())
}

// The same, in a folder of your own.
pub fn list_in(where_: &Path) -> Vec<Log> {
    let mut found = all_in(where_);
    found.truncate(MOST_LOGS);
    found
}

// Every log of a folder, newest first, with no ceiling over their number. The
// list of the page is cut to the ten newest, and the clear away of the old ones
// needs to see all of them.
pub fn all_in(where_: &Path) -> Vec<Log> {
    let held = current();
    let mut found: Vec<Log> = Vec::new();
    if let Ok(entries) = fs::read_dir(where_) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !holds(&name) {
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
    // Newest first. Two logs of one second are told apart by their names, for
    // the name of a log holds the second of its start.
    found.sort_by(|a, b| b.when.cmp(&a.when).then_with(|| b.name.cmp(&a.name)));
    found
}

// Throw away the logs beyond the newest ten. The log this run is writing is
// never thrown away, whatever the time written on it says.
pub fn prune() {
    prune_in(&dir());
}

// The same, in a folder of your own.
pub fn prune_in(where_: &Path) {
    let held = current();
    let mut found = all_in(where_);
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

// The words of one log of the program, from its end.
pub fn read(id: &str, most: usize) -> Option<Reading> {
    read_in(&dir(), id, most)
}

// The same, in a folder of your own. A name that is not the name of a log gives
// nothing at all, so the page cannot ask for a file of the machine.
pub fn read_in(where_: &Path, id: &str, most: usize) -> Option<Reading> {
    if !holds(id) {
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
    // The last bytes of the log, begun at a whole line, so the first line the
    // page is given is not half a line. The first byte is moved on while it
    // stands in the middle of a character of more than one byte.
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

// The bytes of a log in words, as the page shows them.
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

    // A folder of its own for one test, for the tests run together.
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

    // One log of the test, with the time it is meant to stand at.
    fn made(where_: &Path, second: i64, bytes: usize) -> PathBuf {
        let place = where_.join(format!("{}{}{}", FIRST, second, LAST));
        fs::write(&place, vec![b'x'; bytes]).expect("a log of the test can be written");
        let _ = fs::OpenOptions::new()
            .write(true)
            .open(&place)
            .and_then(|file| {
                file.set_modified(SystemTime::from(
                    UNIX_EPOCH + std::time::Duration::from_secs(second as u64),
                ))
            });
        place
    }

    // Only a name of the shape of a log is a log. A name that leads out of the
    // folder is not one either, so the page cannot ask for a file of the machine.
    #[test]
    fn a_log_is_known_by_the_shape_of_its_name() {
        assert!(holds("server-1762103944.log"));
        assert!(holds("server-1762103944-2.log"));
        assert!(!holds("server-.log"), "no second of a start");
        assert!(!holds(".log"));
        assert!(!holds("server-chat.log"), "no digits at all");
        assert!(!holds("server-1762103944.txt"));
        assert!(!holds("notes.txt"));
        assert!(!holds("server-1762103944.log/"));
        assert!(!holds("../server-1762103944.log"));
        assert!(!holds("server-1762103944.LOG"));
    }

    // The ten newest logs are kept and the rest are thrown away. The list stands
    // newest first, whatever order the folder gives them back in.
    #[test]
    fn the_ten_newest_logs_are_kept() {
        let place = room("keep");
        // Thirteen logs, one a second apart, of a size that tells them apart.
        let first: i64 = 1_600_000_000;
        for n in 0..13i64 {
            made(&place, first + n, 10 + n as usize);
        }
        assert_eq!(all_in(&place).len(), 13, "every log of the folder is seen");
        let kept = list_in(&place);
        assert_eq!(kept.len(), MOST_LOGS, "only the ten newest are listed");
        assert_eq!(kept[0].when, first + 12, "the newest stands first");
        assert_eq!(kept[1].when, first + 11);
        assert_eq!(kept.last().expect("a list with a foot in it").when, first + 3);
        prune_in(&place);
        let after = all_in(&place);
        assert_eq!(after.len(), MOST_LOGS, "the old logs are gone");
        assert_eq!(after[0].when, first + 12, "the newest is left where it stood");
        assert!(!place.join(format!("{}{}{}", FIRST, first, LAST)).exists());
        assert!(place.join(format!("{}{}{}", FIRST, first + 12, LAST)).exists());
        let _ = fs::remove_dir_all(&place);
    }

    // A log that is longer than the most that is read gives back its end, begun
    // at a whole line, and says that the front is not there.
    #[test]
    fn the_end_of_a_long_log_is_read_from_a_whole_line() {
        let place = room("tail");
        let name = format!("{}1600000001{}", FIRST, LAST);
        let mut words = String::new();
        for n in 0..600 {
            words.push_str(&format!("line {} of the log of llama-server\n", n));
        }
        fs::write(place.join(&name), &words).expect("a long log can be written");
        let whole = read_in(&place, &name, GREATEST_READ).expect("the log can be read");
        assert!(!whole.cut, "the whole of a short log is here");
        assert!(whole.text.ends_with("line 599 of the log of llama-server\n"));
        assert!(whole.text.starts_with("line 0 of the log"));

        let small = read_in(&place, &name, 900).expect("the end of the log can be read");
        assert!(small.cut, "the front of the log is left behind");
        assert!(small.bytes as usize > 900, "the whole size is told");
        assert!(small.text.ends_with("line 599 of the log of llama-server\n"));
        assert!(
            small.text.starts_with("line "),
            "the first line of what is read is a whole line: {:?}",
            &small.text[..24.min(small.text.len())]
        );
        assert!(!small.text.contains("of the log of llama-server of the log"));
        let _ = fs::remove_dir_all(&place);
    }

    // A name that is not the name of a log reads nothing at all.
    #[test]
    fn a_name_that_is_no_log_reads_nothing() {
        let place = room("none");
        fs::write(place.join("settings.json"), b"a file of the machine").expect("a file can be written");
        assert!(read_in(&place, "settings.json", 100).is_none());
        assert!(read_in(&place, "", 100).is_none());
        assert!(read_in(&place, "server-1600000000.log", 100).is_none(), "no such log");
        let _ = fs::remove_dir_all(&place);
    }

    // A new log is made under the folder of the logs, and a second start in the
    // same second does not stand on top of the first one.
    #[test]
    fn a_new_log_is_made_and_named_for_its_second() {
        let place = room("start");
        let first = start_in(&place).expect("a log can be started");
        let second = start_in(&place).expect("a second log can be started");
        assert_ne!(first, second, "two starts, two logs");
        assert!(first.exists(), "the log stands before the server writes a word");
        let names = all_in(&place);
        assert_eq!(names.len(), 2, "both starts are listed");
        for one in names.iter() {
            assert!(holds(&one.name), "a log of the program: {}", one.name);
            assert!(place.join(&one.name).exists(), "a listed log stands");
        }
        let _ = fs::remove_dir_all(&place);
    }

    // The words of a size, as the page shows them.
    #[test]
    fn the_size_of_a_log_is_said_in_words() {
        assert_eq!(size_text(512), "512 B");
        assert_eq!(size_text(2048), "2 KiB");
        assert_eq!(size_text(5 * 1024 * 1024 + 8), "5.0 MiB");
    }
}
