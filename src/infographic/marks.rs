// The marks an infographic draws for itself.
//
// A picture of a thing says more in one place than a line of words, and a model
// that wants a mark beside a bullet must not have to draw one. These are drawn
// here, in words of a 24 by 24 field, so a mark is one path of the frame and
// costs nothing to carry. Every mark is made of strokes of the current colour,
// with no ground of its own, so it reads on any panel of any palette.
//
// The set is kept to the marks an infographic asks for: the marks of yes and no,
// the marks of a way through a process, the marks of the common shapes of data,
// and the marks of the things a chart is about.

// One mark: the words of its drawing.
pub struct Mark {
    pub name: &'static str,
    // The words of a `path`. A mark of more than one piece carries more than one
    // string, and each is drawn.
    pub parts: &'static [&'static str],
    // Whether the piece is filled rather than drawn as a stroke.
    pub filled: &'static [bool],
    // What the mark is, in the words a model reads.
    pub about: &'static str,
}

// The marks of the program. The drawings stand in a field of 24 by 24.
pub const MARKS: &[Mark] = &[
    Mark {
        name: "check",
        parts: &["M4.5 12.6 L9.4 17.5 L19.5 6.9"],
        filled: &[false],
        about: "a thing is true, done, or good",
    },
    Mark {
        name: "cross",
        parts: &["M6.5 6.5 L17.5 17.5", "M17.5 6.5 L6.5 17.5"],
        filled: &[false, false],
        about: "a thing is false, missing, or wrong",
    },
    Mark {
        name: "dot",
        parts: &["M12 8.2 A3.8 3.8 0 1 1 11.99 8.2 Z"],
        filled: &[true],
        about: "a point of a list",
    },
    Mark {
        name: "arrow_right",
        parts: &["M4 12 H19", "M13.5 6.5 L19.5 12 L13.5 17.5"],
        filled: &[false, false],
        about: "the way on, to the next thing",
    },
    Mark {
        name: "arrow_left",
        parts: &["M20 12 H5", "M10.5 6.5 L4.5 12 L10.5 17.5"],
        filled: &[false, false],
        about: "back to the thing before",
    },
    Mark {
        name: "arrow_up",
        parts: &["M12 20 V5", "M6.5 10.5 L12 4.5 L17.5 10.5"],
        filled: &[false, false],
        about: "up, more, growing",
    },
    Mark {
        name: "arrow_down",
        parts: &["M12 4 V19", "M6.5 13.5 L12 19.5 L17.5 13.5"],
        filled: &[false, false],
        about: "down, less, falling",
    },
    Mark {
        name: "bulb",
        parts: &[
            "M12 3.5 A5.6 5.6 0 0 1 15.4 13.6 V16 H8.6 V13.6 A5.6 5.6 0 0 1 12 3.5 Z",
            "M10 19 H14",
            "M10.7 21.2 H13.3",
        ],
        filled: &[false, false, false],
        about: "an idea, a finding, a thing worth knowing",
    },
    Mark {
        name: "warn",
        parts: &[
            "M12 4.2 L21 19.6 H3 Z",
            "M12 10 V14.4",
            "M12 16.8 v0.1",
        ],
        filled: &[false, false, false],
        about: "a risk, a fault, a thing to be careful of",
    },
    Mark {
        name: "info",
        parts: &["M12 3.8 A8.2 8.2 0 1 1 11.99 3.8 Z", "M12 11 V16.6", "M12 7.6 v0.1"],
        filled: &[false, false, false],
        about: "a word of explanation",
    },
    Mark {
        name: "chart_bars",
        parts: &["M4.5 20 H19.5", "M7.5 20 V13", "M12 20 V7", "M16.5 20 V10.5"],
        filled: &[false, false, false, false],
        about: "numbers beside one another",
    },
    Mark {
        name: "chart_line",
        parts: &["M4 20 H20", "M5 16 L9.5 11 L13.5 14 L20 6"],
        filled: &[false, false],
        about: "numbers over time",
    },
    Mark {
        name: "chart_pie",
        parts: &[
            "M12 3.6 A8.4 8.4 0 1 1 11.9 3.6 Z",
            "M12 12 V3.6",
            "M12 12 L19.4 15.6",
        ],
        filled: &[false, false, false],
        about: "parts of a whole",
    },
    Mark {
        name: "clock",
        parts: &["M12 3.8 A8.2 8.2 0 1 1 11.99 3.8 Z", "M12 7.4 V12.4 L15.6 14.6"],
        filled: &[false, false],
        about: "time, a duration, a deadline",
    },
    Mark {
        name: "calendar",
        parts: &[
            "M4.5 6.5 H19.5 V20 H4.5 Z",
            "M4.5 10.5 H19.5",
            "M8.5 4 V7",
            "M15.5 4 V7",
        ],
        filled: &[false, false, false, false],
        about: "a day, a date, a schedule",
    },
    Mark {
        name: "globe",
        parts: &[
            "M12 3.8 A8.2 8.2 0 1 1 11.99 3.8 Z",
            "M3.8 12 H20.2",
            "M12 3.8 C8.4 7 8.4 17 12 20.2 C15.6 17 15.6 7 12 3.8 Z",
        ],
        filled: &[false, false, false],
        about: "the world, a place, everywhere",
    },
    Mark {
        name: "users",
        parts: &[
            "M9 11.4 A3.4 3.4 0 1 1 8.99 11.4 Z",
            "M3 20 C3 16.4 5.7 14.4 9 14.4 C12.3 14.4 15 16.4 15 20",
            "M16.4 8.4 A3 3 0 1 1 16.39 8.4 Z",
            "M16 14.6 C18.6 14.9 21 16.6 21 20",
        ],
        filled: &[false, false, false, false],
        about: "people, a team, an audience",
    },
    Mark {
        name: "gear",
        parts: &[
            "M12 8.8 A3.2 3.2 0 1 1 11.99 8.8 Z",
            "M12 3.4 V6 M12 18 V20.6 M3.4 12 H6 M18 12 H20.6 M6 6 L7.9 7.9 M16.1 16.1 L18 18 M18 6 L16.1 7.9 M7.9 16.1 L6 18",
        ],
        filled: &[false, false],
        about: "a setting, a way a thing is set up",
    },
    Mark {
        name: "lock",
        parts: &[
            "M6.4 11 H17.6 V20.2 H6.4 Z",
            "M8.8 11 V8.4 A3.2 3.2 0 0 1 15.2 8.4 V11",
        ],
        filled: &[false, false],
        about: "safety, a thing kept private, a wall",
    },
    Mark {
        name: "shield",
        parts: &["M12 3.6 L19 6.2 V12 C19 16.4 16 19.4 12 20.6 C8 19.4 5 16.4 5 12 V6.2 Z"],
        filled: &[false],
        about: "protection, defence, trust",
    },
    Mark {
        name: "bolt",
        parts: &["M13.4 3 L6.4 13.4 H11.4 L10.6 21 L17.6 10.6 H12.6 Z"],
        filled: &[false],
        about: "power, speed, energy",
    },
    Mark {
        name: "target",
        parts: &[
            "M12 3.8 A8.2 8.2 0 1 1 11.99 3.8 Z",
            "M12 8 A4 4 0 1 1 11.99 8 Z",
            "M12 11.2 A0.9 0.9 0 1 1 11.9 11.2 Z",
        ],
        filled: &[false, false, true],
        about: "a goal, an aim, the thing aimed at",
    },
    Mark {
        name: "star",
        parts: &["M12 3.6 L14.5 9.1 L20.4 9.8 L16 13.9 L17.1 19.8 L12 16.9 L6.9 19.8 L8 13.9 L3.6 9.8 L9.5 9.1 Z"],
        filled: &[false],
        about: "the best, a rating, a thing worth praise",
    },
    Mark {
        name: "doc",
        parts: &[
            "M6.4 3.6 H14.4 L18.4 7.6 V20.4 H6.4 Z",
            "M14.4 3.6 V7.6 H18.4",
            "M9 12 H15.8 M9 15.4 H15.8",
        ],
        filled: &[false, false, false],
        about: "a file, a page, a piece of writing",
    },
    Mark {
        name: "code",
        parts: &["M9 7.4 L4 12 L9 16.6", "M15 7.4 L20 12 L15 16.6", "M13.4 5.4 L10.6 18.6"],
        filled: &[false, false, false],
        about: "a program, words a machine reads",
    },
    Mark {
        name: "cpu",
        parts: &[
            "M7.4 7.4 H16.6 V16.6 H7.4 Z",
            "M10.6 10.6 H13.4 V13.4 H10.6 Z",
            "M10 4 V7.4 M14 4 V7.4 M10 16.6 V20 M14 16.6 V20 M4 10 H7.4 M4 14 H7.4 M16.6 10 H20 M16.6 14 H20",
        ],
        filled: &[false, false, false],
        about: "a chip, a processor, the work of a machine",
    },
    Mark {
        name: "database",
        parts: &[
            "M12 4 C7.6 4 5 5.4 5 7.2 C5 9 7.6 10.4 12 10.4 C16.4 10.4 19 9 19 7.2 C19 5.4 16.4 4 12 4 Z",
            "M5 7.2 V16.8 C5 18.6 7.6 20 12 20 C16.4 20 19 18.6 19 16.8 V7.2",
            "M5 12 C5 13.8 7.6 15.2 12 15.2 C16.4 15.2 19 13.8 19 12",
        ],
        filled: &[false, false, false],
        about: "a store of data, a base of records",
    },
    Mark {
        name: "layers",
        parts: &[
            "M12 3.6 L20 8 L12 12.4 L4 8 Z",
            "M4 12.4 L12 16.8 L20 12.4",
            "M4 16.4 L12 20.8 L20 16.4",
        ],
        filled: &[false, false, false],
        about: "parts on one another, a stack",
    },
    Mark {
        name: "search",
        parts: &["M10.8 4 A6.8 6.8 0 1 1 10.79 4 Z", "M15.6 15.6 L20 20"],
        filled: &[false, false],
        about: "a look for a thing, a search",
    },
    Mark {
        name: "money",
        parts: &[
            "M3.6 6.8 H20.4 V17.2 H3.6 Z",
            "M12 9.4 A2.6 2.6 0 1 1 11.99 9.4 Z",
            "M6.6 9.6 V14.4 M17.4 9.6 V14.4",
        ],
        filled: &[false, false, false],
        about: "money, cost, a price",
    },
    Mark {
        name: "flag",
        parts: &["M6.6 20.4 V4", "M6.6 4.8 H17.4 L14.6 8.6 L17.4 12.4 H6.6"],
        filled: &[false, false],
        about: "a mark of a place, a milestone, a warning",
    },
    Mark {
        name: "eye",
        parts: &[
            "M2.8 12 C5.6 7.2 8.8 4.8 12 4.8 C15.2 4.8 18.4 7.2 21.2 12 C18.4 16.8 15.2 19.2 12 19.2 C8.8 19.2 5.6 16.8 2.8 12 Z",
            "M12 9.2 A2.8 2.8 0 1 1 11.99 9.2 Z",
        ],
        filled: &[false, false],
        about: "a look, what is seen, attention",
    },
    Mark {
        name: "play",
        parts: &["M8 5.4 L18.4 12 L8 18.6 Z"],
        filled: &[false],
        about: "the start of a thing, a video",
    },
    Mark {
        name: "fire",
        parts: &["M12 3.4 C14.4 7 17.6 8.6 17.6 12.8 A5.6 5.6 0 0 1 6.4 12.8 C6.4 10.4 8 9.4 8.8 7.4 C9.6 9.4 11 9.8 12 3.4 Z"],
        filled: &[false],
        about: "heat, demand, a thing growing fast",
    },
    Mark {
        name: "link",
        parts: &[
            "M10 14 A3.6 3.6 0 0 1 10 8.8 L12.2 6.6 A3.6 3.6 0 0 1 17.2 11.8 L15.8 13.2",
            "M14 10 A3.6 3.6 0 0 1 14 15.2 L11.8 17.4 A3.6 3.6 0 0 1 6.8 12.2 L8.2 10.8",
        ],
        filled: &[false, false],
        about: "a join between two things",
    },
    Mark {
        name: "question",
        parts: &[
            "M12 3.8 A8.2 8.2 0 1 1 11.99 3.8 Z",
            "M9.6 9.4 A2.5 2.5 0 0 1 14.5 10.2 C14.5 12.2 12 12.2 12 14.4",
            "M12 17 v0.1",
        ],
        filled: &[false, false, false],
        about: "a question, an open thing",
    },
];

// The words of one mark, or nothing at all when the name is not one of them.
pub fn of(name: &str) -> Option<&'static Mark> {
    let said = name.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    if said.is_empty() {
        return None;
    }
    MARKS.iter().find(|one| one.name == said)
}

// The names of the marks, for the guide and for a fault that names them.
pub fn names() -> String {
    MARKS
        .iter()
        .map(|one| one.name)
        .collect::<Vec<&str>>()
        .join(", ")
}

// The marks with what each of them is for, for the guide.
pub fn about() -> String {
    MARKS
        .iter()
        .map(|one| format!("{} - {}", one.name, one.about))
        .collect::<Vec<String>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mark_reads_as_a_drawing() {
        for one in MARKS {
            assert!(!one.parts.is_empty(), "{} has no drawing", one.name);
            assert_eq!(
                one.parts.len(),
                one.filled.len(),
                "{} tells a fill for every piece or for none",
                one.name
            );
            for part in one.parts {
                assert!(part.starts_with('M'), "{} does not start a path", one.name);
                // Every mark stands inside the field of 24 by 24.
                for word in part.split_whitespace() {
                    if let Ok(number) = word.parse::<f64>() {
                        assert!(
                            number >= -1.0 && number <= 25.0,
                            "{} names {number}, outside the field",
                            one.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_mark_is_found_by_the_words_a_model_writes() {
        assert!(of("check").is_some());
        assert!(of("Arrow-Right").is_some());
        assert!(of("arrow right").is_some());
        assert!(of("nothing_of_the_kind").is_none());
        assert!(of("").is_none());
    }
}
