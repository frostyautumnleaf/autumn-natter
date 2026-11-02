// The log of the program itself.
//
// The log of llama-server says what the server did. This log says what the
// program did: when it weighed the room of a talk against the limit set for the
// summarisation, when it asked the model for a handoff summary and with which
// numbers, why it asked again, where it stopped the writing, and every moment the
// meter of the context moved. The words of a fault often stand in one log and the
// reason for them in the other, so the two stand in one folder, apart by the words
// their names begin with, and the Logs tab of the sheet of settings shows either.
//
// One line stands for one thing the program did:
//
//     2026-11-02 03:01:44.123 room the talk weighs 213733 of 262144
//
// The time to the millisecond stands first, so that two lines of one second keep
// their order. One word then says which part of the program the line came from, so
// a search of the log gives back one part by itself. The rest of the line is the
// thing that happened, in the words a user reads on the page.
//
// One file stands for one run of the program, as one file stands for one start of
// the server, and the ten newest of each are kept. Writing a line never fails the
// work that asked for it: a program that could not make its log at all works on as
// it did before, and says nothing of it.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::logs;
use crate::store;

// The part of the program a line came from. One word each, so that a search of the
// log gives back one part alone.

// The log of this run was begun, and what ran when it was.
pub const START: &str = "start";
// The meter of the context: the number of the talk that the user is shown.
pub const METER: &str = "meter";
// The room of the talk, weighed against the limit set for the summarisation.
pub const ROOM: &str = "room";
// The handoff summary: every ask, the numbers it carried, how it ended, and every
// ask made again and why.
pub const SUMMARY: &str = "summary";
// The writing stopped at the limit, the room that was made, and what the answer
// was able to do with it.
pub const CUT: &str = "cut";

// Begin the log of this run of the program and give back its place. None comes
// back when the folder cannot be made, and the program then works as it did with no
// log of its own. The line that says the log was begun is the first word of the
// file, so a run that wrote nothing else still says when it stood.
pub fn start() -> Option<PathBuf> {
    let place = logs::start(logs::Kind::Program)?;
    note(
        START,
        &format!(
            "the log of this run is begun. Autumn Natter {}",
            env!("CARGO_PKG_VERSION")
        ),
    );
    Some(place)
}

// Add one line to the log of this run. `part` is one of the words above, and
// `words` says what happened. A mark of an end inside the words is turned into a
// space, for one line stands for one thing the program did and two lines would
// read as two things. Nothing is asked again and nothing fails: a log that cannot
// be written is a loss of words and not a loss of the work.
pub fn note(part: &str, words: &str) {
    logs::note_line(logs::Kind::Program, &line_of(part, words));
}

// One line of the log: the moment, the part it came from, and the words of the
// thing that happened. Kept apart from the writing of it, so that the shape of a
// line is looked at without a folder to write into.
fn line_of(part: &str, words: &str) -> String {
    format!("{} {} {}", stamp(), part, words.replace('\n', " "))
}

// The moment a line stands at: the day and the clock time of this machine to the
// millisecond, as in `2026-11-02 03:01:44.123`. The millisecond is what keeps the
// lines of one second in their order, for the meter of the context moves twice a
// second at most and an answer writes a great many of them.
fn stamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let least = now.subsec_millis();
    #[cfg(unix)]
    {
        let when = secs as libc::time_t;
        let mut broken: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: localtime_r fills the struct it is given.
        let filled = unsafe { libc::localtime_r(&when, &mut broken) };
        if !filled.is_null() {
            return format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
                broken.tm_year + 1900,
                broken.tm_mon + 1,
                broken.tm_mday,
                broken.tm_hour,
                broken.tm_min,
                broken.tm_sec,
                least
            );
        }
    }
    // No local time to be had: the day and the clock of the seconds the machine
    // counted from their beginning, which keeps the lines in their order.
    let (year, month, day) = store::civil_from_days(secs.div_euclid(86400));
    let of_day = secs.rem_euclid(86400);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        year,
        month,
        day,
        of_day / 3600,
        (of_day / 60) % 60,
        of_day % 60,
        least
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // The time of a line holds the day, the clock and the millisecond of it, and
    // always in the same shape, whatever machine it is read on.
    #[test]
    fn the_time_of_a_line_holds_the_millisecond() {
        let made = stamp();
        // 2026-11-02 03:01:44.123
        assert_eq!(made.len(), 23, "the shape of the time: {}", made);
        assert_eq!(made.matches('-').count(), 2, "the day: {}", made);
        assert_eq!(made.matches(':').count(), 2, "the clock: {}", made);
        assert_eq!(made.matches('.').count(), 1, "the millisecond: {}", made);
        assert!(made.as_bytes()[4] == b'-', "the year stands first: {}", made);
        assert!(made.chars().next().unwrap().is_ascii_digit(), "the time begins with the year");
    }

    // A line is written as one line: the moment, then the part, then the words of
    // it, and a mark of an end in the words is turned into a space, for one line
    // stands for one thing the program did.
    #[test]
    fn a_line_holds_the_part_before_its_words() {
        let made = line_of(METER, "the talk weighs 412\nand was 900");
        assert_eq!(made.matches('\n').count(), 0, "one line for one thing: {}", made);
        assert!(made.starts_with("20"), "the moment stands first: {}", made);
        // The day, then the clock, then the part, then the words of it.
        let after = made.split(' ').nth(2).expect("the part of the line");
        assert_eq!(after, METER, "the part stands before the words: {}", made);
        assert!(
            made.ends_with("the talk weighs 412 and was 900"),
            "the mark of an end became a space: {}",
            made
        );
    }

    // The log of this run is written through the rule of the logs, which is tried
    // in `logs`, where the file of a run is begun in a folder of the test. What is
    // looked at here is the shape of a line, which is all this part of the program
    // adds to a line of the log.
}
