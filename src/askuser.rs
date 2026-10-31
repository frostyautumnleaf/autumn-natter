// The tool that stays with a question the model has put to the user.
//
// A model that cannot go on without a word from the user — a token that was
// never given, a name that was never chosen, a yes or a no that the work turns
// on — asks instead of guessing. The program puts the question above the chat
// box, with the choices the model offered under it and a box of its own for an
// answer the model did not think of, and waits. What the user says goes back
// to the model as the result of the call, and the model goes on from there.
//
// A question of the ordinary kind is waited for a short while: thirty seconds by
// default, from the Chatting settings. When the time is gone the model is told
// that the user did not answer, and it is left to go on as best it can. A
// question the model has marked as one it cannot go on without is waited for as
// long as it takes, for a model that was told to ask only when it must is a
// model whose question had better be answered. Both waits are read from the
// settings, and a wait of nothing at all turns the timeout away.
//
// The call stands in the chat as the block of a run, as every other tool does:
// the user sees the question being written, then sees the block say what the
// question came to. The popup is only the front of it. A run the user stopped
// with their own hand leaves the popup as well, and the model is told that the
// answer never came.
//
// The tool falls under the badge of the tools, and under the line of its own in
// the drop-down of the badge, as the other tools of the program do.

use serde_json::{json, Value};

use crate::tools::with_rule;

// Name the model uses to put a question to the user.
pub const NAME: &str = "ask_user";

// The most choices one question may carry. A question with ten roads under it is
// not a question but a form, and a popup of a form is a thing to be read through
// before anything can be said.
pub const GREATEST_CHOICES: usize = 5;

// The longest question the popup shows. A model that wants to explain a great
// deal has the words of its own answer for that.
pub const GREATEST_QUESTION: usize = 600;

// The longest choice the popup shows. A choice that does not fit a line of the
// popup was not read as a choice.
pub const GREATEST_CHOICE: usize = 120;

// The wait of a question of the ordinary kind, when nothing stands in the way.
// It is the same thirty seconds the settings default to, so a question written
// with no wait of its own is waited for as long as the sheet of settings would
// have said.
pub const ORDINARY_WAIT: u32 = 30;

// What the model asked for.
#[derive(Clone, Debug, Default)]
pub struct Question {
    // The question, and the reason the model gives with it. The popup shows this
    // at the top, as the title of the ask.
    pub text: String,
    // The roads the model offered, in the order it offered them. A question with
    // no road under it is answered in the box of the popup.
    pub choices: Vec<String>,
    // True when the model says it cannot go on without an answer. Such a
    // question is waited for as long as it takes, unless the settings say else.
    pub critical: bool,
}

// The tool as the model is told of it.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": with_rule(String::from(
                "Put a question to the user and wait for the answer. Use it only when the work cannot go on without a word from the user: a secret or token that was never given, a choice the user has to make, a permission that has to be given. Ask one question, and offer up to five short answers the user may choose from; the user may always write an answer of their own instead. Say in the question why you are asking, for it is shown as the title of the ask. An ordinary question is waited for a short while and then given up, and you are told the user did not answer; use 'critical' only when you truly cannot go on, for such a question waits until the user answers.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The question, and the reason for asking it, in the words of the chat. It stands at the top of the ask."
                    },
                    "choices": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Up to five short answers the user may choose from, such as \"Yes, go on\" or \"Only the first file\". Leave it out when the answer cannot be guessed at; the user can write an answer whatever you offer."
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["normal", "critical"],
                        "description": "How long to wait. 'normal', which is the default, is given up after the short wait in the settings and you are told the user did not answer. 'critical' waits until the user answers, and is only for a question you cannot go on without."
                    }
                },
                "required": ["question"]
            }
        }
    })
}

// True when this name of a tool is the question of the user.
pub fn is_tool(name: &str) -> bool {
    name == NAME
}

// What the model asked for, read out of the words of the call. A call that gave
// no question is answered with a question that says so, so the popup is never an
// empty box and the model is never left without a receipt.
pub fn parse(arguments: &str) -> Question {
    let value: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let text = one_line(
        value
            .get("question")
            .and_then(|v| v.as_str())
            .or_else(|| value.get("title").and_then(|v| v.as_str()))
            .unwrap_or(""),
        GREATEST_QUESTION,
    );
    let mut choices: Vec<String> = Vec::new();
    if let Some(list) = value.get("choices").and_then(|v| v.as_array()) {
        for one in list.iter() {
            let Some(text) = one.as_str() else { continue };
            let cut = one_line(text, GREATEST_CHOICE);
            if !cut.is_empty() && choices.len() < GREATEST_CHOICES {
                choices.push(cut);
            }
        }
    }
    // A model that writes the halves of a call in the order it likes often gives
    // `options` for the list of answers, and `type` for the kind of question.
    if choices.is_empty() {
        if let Some(list) = value.get("options").and_then(|v| v.as_array()) {
            for one in list.iter() {
                let Some(text) = one.as_str() else { continue };
                let cut = one_line(text, GREATEST_CHOICE);
                if !cut.is_empty() && choices.len() < GREATEST_CHOICES {
                    choices.push(cut);
                }
            }
        }
    }
    let kind = value
        .get("kind")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("type").and_then(|v| v.as_str()))
        .unwrap_or("");
    Question {
        text: if text.is_empty() {
            String::from("The model asked a question but wrote no words with it. Answer in your own words.")
        } else {
            text
        },
        choices,
        critical: kind.eq_ignore_ascii_case("critical"),
    }
}

// The words of the head of the block while the call is being written: the
// question as far as it has come. The run of words the model is quoting is read
// whether it has been closed or not, so the head says something from the first
// letter of the question.
pub fn partial_words(arguments: &str) -> String {
    let Some(at) = arguments.find("\"question\"") else {
        return String::new();
    };
    let letters: Vec<char> = arguments[at + "\"question\"".len()..].chars().collect();
    let mut read = 0usize;
    while read < letters.len() && (letters[read] == ':' || letters[read].is_whitespace()) {
        read += 1;
    }
    if read >= letters.len() || letters[read] != '"' {
        return String::new();
    }
    let mut word = String::new();
    read += 1;
    while read < letters.len() {
        let ch = letters[read];
        if ch == '\\' {
            if read + 1 < letters.len() {
                word.push(letters[read + 1]);
                read += 2;
                continue;
            }
            break;
        }
        if ch == '"' {
            break;
        }
        word.push(ch);
        read += 1;
    }
    one_line(&word, GREATEST_QUESTION)
}

// The wait of a question, read from the settings. A question of the ordinary
// kind that was given no wait at all keeps the short wait of the tool, so a
// setting of nothing does not turn an ordinary question into one that waits for
// ever: that is what 'critical' is for.
pub fn wait_of(critical: bool, ordinary: u32, weighty: u32) -> u32 {
    if critical {
        weighty
    } else if ordinary == 0 {
        ORDINARY_WAIT
    } else {
        ordinary
    }
}

// The result of a call that was answered. The words go back to the model, so
// they say what the user said and nothing else.
pub fn answered_words(answer: &str) -> String {
    let said = answer.trim();
    if said.is_empty() {
        return String::from("The user answered, and their answer was nothing at all");
    }
    format!("The user answered: {}", one_line(said, 2000))
}

// The result of a question that was given up. The model is told the wait it set
// itself, so it can weigh whether to ask again in another way.
pub fn given_up_words(seconds: u32) -> String {
    format!(
        "The user did not answer within {} {}. You may go on without their answer, or ask again for what you truly need",
        seconds,
        if seconds == 1 { "second" } else { "seconds" }
    )
}

// The result of a run the user stopped with their own hand. The question was
// never answered, and the popup went with the run.
pub fn stopped_words() -> String {
    String::from("The user stopped the answer, so the question was never answered")
}

// One line of words, cut at a length. Marks of a new line are turned into spaces,
// for a title of a popup and the head of a block are each one line.
fn one_line(words: &str, bound: usize) -> String {
    let flat = words.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= bound {
        return flat;
    }
    let mut out = String::new();
    for ch in flat.chars().take(bound) {
        out.push(ch);
    }
    out.push('\u{2026}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shape of the tool: one question, up to five answers to choose from, and
    // the two kinds of wait.
    #[test]
    fn the_shape_of_the_tool_asks_one_question() {
        let made = definition();
        let tool = &made["function"];
        assert_eq!(tool["name"], NAME);
        let said = tool["description"].as_str().expect("words");
        assert!(said.contains("cannot go on without"), "{said}");
        let part = &tool["parameters"]["properties"];
        assert_eq!(part["question"]["type"], "string");
        assert_eq!(part["choices"]["type"], "array");
        assert_eq!(part["kind"]["enum"][0], "normal");
        assert_eq!(part["kind"]["enum"][1], "critical");
        assert_eq!(tool["parameters"]["required"][0], "question");
        assert!(is_tool(NAME));
        assert!(!is_tool("offer_files"));
    }

    // The words of the call are read the long way and the short way, and the
    // halves that came in the order the model liked are read as well.
    #[test]
    fn the_call_is_read_whichever_way_it_was_written() {
        let plain = parse(
            r#"{"question":"Which file should I take?","choices":["The first","The last"],"kind":"critical"}"#,
        );
        assert_eq!(plain.text, "Which file should I take?");
        assert_eq!(plain.choices.len(), 2);
        assert_eq!(plain.choices[1], "The last");
        assert!(plain.critical, "the model said it cannot go on");
        // `options` and `type` stand for the same two things, for a model writes
        // the word it knows.
        let other = parse(r#"{"question":"A token?","options":["Here it is"],"type":"normal"}"#);
        assert_eq!(other.choices, vec![String::from("Here it is")]);
        assert!(!other.critical);
        // Nothing of the kind is a question of the ordinary kind, and a question
        // with no words says so rather than standing empty.
        let bare = parse("{}");
        assert!(bare.text.starts_with("The model asked a question"));
        assert!(bare.choices.is_empty());
        assert!(!bare.critical);
        // More than five answers, and a choice of many lines: the popup holds
        // five lines of one line each.
        let many = parse(&format!(
            r#"{{"question":"Which?","choices":[{}]}}"#,
            (1..9)
                .map(|n| format!("\"one\\ntwo {n}\""))
                .collect::<Vec<_>>()
                .join(",")
        ));
        assert_eq!(many.choices.len(), GREATEST_CHOICES);
        assert_eq!(many.choices[0], "one two 1");
        // A question of many lines is one line, and a long one is cut.
        let long = parse(&format!(r#"{{"question":"{}"}}"#, "word ".repeat(300)));
        assert_eq!(long.text.chars().count(), GREATEST_QUESTION + 1);
        assert!(long.text.ends_with('\u{2026}'));
    }

    // The two waits: a question the model cannot go on without waits for ever by
    // default, and an ordinary one waits the short while even when the setting
    // was left at nothing.
    #[test]
    fn the_wait_of_a_question_comes_from_the_settings() {
        assert_eq!(wait_of(false, 10, 0), 10);
        assert_eq!(wait_of(false, 30, 0), 30);
        assert_eq!(wait_of(false, 0, 0), ORDINARY_WAIT);
        assert_eq!(wait_of(true, 10, 0), 0, "a weighty question waits for ever");
        assert_eq!(wait_of(true, 10, 120), 120);
    }

    // The three receipts the model is given.
    #[test]
    fn the_receipt_says_what_became_of_the_question() {
        assert_eq!(answered_words("  the first one  "), "The user answered: the first one");
        assert!(answered_words("   ").contains("nothing at all"));
        assert!(given_up_words(10).contains("10 seconds"));
        assert!(given_up_words(1).contains("1 second"));
        assert!(stopped_words().contains("never answered"));
    }

    // The head of the block has something to say while the question is being
    // written, and after it.
    #[test]
    fn the_head_of_a_block_being_written_says_the_question() {
        assert_eq!(
            partial_words(r#"{"question":"Which file?"}"#),
            "Which file?"
        );
        assert_eq!(partial_words(r#"{"question":"Which fi"#), "Which fi");
        assert_eq!(partial_words(""), "");
    }
}
