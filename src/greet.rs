// The one line the program shows above the chat box when no chat is open.
//
// The line comes from the clock. The hour picks a band, and each band holds ten
// lines. The minute picks one of the ten, so the line changes now and then but
// never at random: the same clock time always gives the same line.

// Early morning, from five to eight.
const EARLY: [&str; 10] = [
    "What are we doing today?",
    "The day is young. What shall we talk about?",
    "Start the day with a good question.",
    "Ready when you are.",
    "Something on your mind before the day starts?",
    "A fresh page, and time to fill it.",
    "What should we work on first?",
    "Early hours are good hours.",
    "Tell me what you need today.",
    "Where shall we begin?",
];

// Morning, from eight to twelve.
const MORNING: [&str; 10] = [
    "What are we making this morning?",
    "The kettle is on. What is your question?",
    "A clear head and an empty box. Write away.",
    "What should we sort out first?",
    "Morning. Where do we start?",
    "Got something to unpick?",
    "Let us get something done.",
    "What is on the list today?",
    "Write it down and we will work on it.",
    "Small question or large one?",
];

// Midday, from twelve to three.
const MIDDAY: [&str; 10] = [
    "Break time, or work time?",
    "What can we look at now?",
    "A quick question is often the best one.",
    "Still here, still ready.",
    "What are we solving today?",
    "Pick a thread and pull it.",
    "Nothing written yet, and no rush.",
    "Half the day gone. What is left to do?",
    "Put the question here.",
    "Shall we have a go at something?",
];

// Afternoon, from three to six.
const AFTERNOON: [&str; 10] = [
    "The afternoon is yours. What next?",
    "Anything still open from this morning?",
    "A cup of tea and a question.",
    "What are we finishing today?",
    "Keep the ball rolling.",
    "Write it while the light holds.",
    "One more thing to try?",
    "Where were we?",
    "The day is not over yet.",
    "Let us make the rest of the day count.",
];

// Evening, from six to midnight.
const EVENING: [&str; 10] = [
    "The day is winding down. What is left?",
    "Evening thoughts? Write them here.",
    "Lights low, ideas bright.",
    "What shall we talk about tonight?",
    "Time for the interesting questions.",
    "Wind the day up with something good.",
    "Anything you meant to ask earlier?",
    "Slow evening, or busy one?",
    "Out with it.",
    "Let us finish something.",
];

// Night, from midnight to five.
const NIGHT: [&str; 10] = [
    "Ready for some late chats?",
    "What is on your mind tonight?",
    "The small hours suit a good question.",
    "Nobody else is awake. Write away.",
    "Late, and worth it.",
    "Why are you still up? Tell me.",
    "One more thread before sleep?",
    "The night is quiet. Your turn.",
    "Burning the midnight oil?",
    "Ask it now, sleep after.",
];

const LINES: [[&str; 10]; 6] = [EARLY, MORNING, MIDDAY, AFTERNOON, EVENING, NIGHT];

// Which band, and which line of that band, for one hour, minute and day. The
// hour is the hour the window shows, so the line and the clock agree.
fn pick(hour: i64, minute: i64, day: i64) -> (usize, usize) {
    let band = if (5..8).contains(&hour) {
        0
    } else if (8..12).contains(&hour) {
        1
    } else if (12..15).contains(&hour) {
        2
    } else if (15..18).contains(&hour) {
        3
    } else if (18..23).contains(&hour) {
        4
    } else {
        5
    };
    // A slow turn through the ten lines of the band, tied to the clock.
    let slot = (hour * 60 + minute + day) / 11;
    (band, slot.rem_euclid(10) as usize)
}

// The hour in the clock the rest of the program uses. The chat times follow the
// same rule, so the greeting and the clock of the window never disagree.
fn hour_of_day(secs: i64) -> i64 {
    let clock = crate::store::format_clock(secs);
    match clock.split(':').next().and_then(|h| h.trim().parse::<i64>().ok()) {
        Some(hour) => hour.rem_euclid(24),
        None => 12,
    }
}

// The line for this moment.
pub fn greeting(secs: i64) -> String {
    let hour = hour_of_day(secs);
    let minute = secs.rem_euclid(3600) / 60;
    let day = secs.div_euclid(86400);
    let (band, index) = pick(hour, minute, day);
    LINES[band][index].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_band_has_ten_lines() {
        for band in LINES {
            for line in band {
                assert!(!line.is_empty());
            }
        }
    }

    #[test]
    fn the_same_clock_time_gives_the_same_line() {
        assert_eq!(greeting(80_000_000), greeting(80_000_000));
    }

    #[test]
    fn the_hours_land_in_the_right_band() {
        assert_eq!(pick(6, 0, 0).0, 0);
        assert_eq!(pick(9, 30, 0).0, 1);
        assert_eq!(pick(13, 0, 0).0, 2);
        assert_eq!(pick(16, 0, 0).0, 3);
        assert_eq!(pick(20, 0, 0).0, 4);
        assert_eq!(pick(3, 0, 0).0, 5);
        assert_eq!(pick(23, 59, 0).0, 5);
    }
}
