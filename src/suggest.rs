// The tool that proposes the user the paths the conversation may go on by.
//
// An answer that ends at a choice leaves the user to type the choice out. The model may instead
// put that choice under the answer: up to three short buttons, each one carrying the text that
// go out when the user presses it. The call runs at once and waits for nothing, so the buttons
// are an propose and not a question. `ask_user` stays the way to request a word the work cannot
// go on without.
//
// The buttons are at the end of the answer that proposed them, and only once the model has
// stopped writing: a proposal that arrived in the middle of an answer is not finished work.
// Only the newest propose remains. A second call in one answer takes the first away, and the
// proposal of an older answer goes when the user speaks again, for the paths it named are no
// longer the paths the conversation is at. Both rules are read off the conversation by the
// page, so they hold in the window and in the browser page of the remote mode as well.
//
// The proposal rides on the message of the run, as an propose of a file does, so it is drawn
// again when the chat is opened another day.
//
// The tool falls under the badge of the tools, and the drop-down of the badge holds the line of
// its own, as the other tools of the program do.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tools::with_rule;

// Name the model uses to propose the next steps.
pub const NAME: &str = "offer_suggestions";

// The most buttons one call may put under the answer. Three are in one line over the width of
// the chat; a longer list is a menu, and a menu is not what the bottom of an answer is for.
pub const GREATEST_SUGGESTIONS: usize = 3;

// The longest text a button shows. A label that does not fit a button was written as a
// sentence, and the sentence belongs in the answer above.
pub const GREATEST_LABEL: usize = 70;

// The longest prompt a button sends. The prompt is sent as a message of the user, so it is cut
// as a message of the user would be, and not as a label.
pub const GREATEST_PROMPT: usize = 600;

// One button: the text it shows, and the text it sends. The two are one item most of the time,
// and are kept apart for a button that shows 'Run the tests' and sends a line of commands.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Suggestion {
    pub label: String,
    pub prompt: String,
}

// What became of one call. The buttons are under the answer when `asks` holds anything, and the
// text of the result go back to the model either way.
pub struct Made {
    pub report: crate::tools::Report,
    pub asks: Vec<Suggestion>,
}

// The tool as the model is informed of it.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": with_rule(String::from(
                "Offer up to three useful next-step buttons when the user has a real choice after your work. Each has a short label and an optional prompt written as the user's message. Call once at the end of your answer; a later call replaces it. Use `ask_user` for questions. Omit suggestions when the work is complete or there is no meaningful choice.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "suggestions": {
                        "type": "array",
                        "description": "Up to three next steps, ordered. A string serves as both label and prompt.",
                        "items": {
                            "oneOf": [
                                { "type": "string", "description": "Button label and prompt." },
                                {
                                    "type": "object",
                                    "properties": {
                                        "label": { "type": "string", "description": "Short button label." },
                                        "prompt": { "type": "string", "description": "Optional user-message prompt; defaults to the label." }
                                    },
                                    "required": ["label"]
                                }
                            ]
                        }
                    }
                },
                "required": ["suggestions"]
            }
        }
    })
}

// True when this name of a tool is the suggestions for the next steps.
pub fn is_tool(name: &str) -> bool {
    name == NAME
}

// Answer one call. Every part of the suggestion is read as the model gave it: an array of
// objects, or an array of plain text, or the older `options` name for the list. A button with
// no label is passed over, two buttons of one label come once, and the fourth and later buttons
// of a call are reported and dropped, for three is the most the bottom of an answer can hold.
pub fn offer(arguments: &str) -> Made {
    let value: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let list = value
        .get("suggestions")
        .and_then(|one| one.as_array())
        .or_else(|| value.get("options").and_then(|one| one.as_array()))
        .cloned()
        .unwrap_or_default();
    let mut asks: Vec<Suggestion> = Vec::new();
    let mut dropped = 0usize;
    for one in list.iter() {
        let (label, prompt) = match one {
            Value::String(text) => (text.clone(), String::new()),
            Value::Object(part) => (
                part.get("label")
                    .or_else(|| part.get("text"))
                    .or_else(|| part.get("title"))
                    .and_then(|one| one.as_str())
                    .unwrap_or("")
                    .to_string(),
                part.get("prompt")
                    .or_else(|| part.get("message"))
                    .or_else(|| part.get("send"))
                    .and_then(|one| one.as_str())
                    .unwrap_or("")
                    .to_string(),
            ),
            _ => continue,
        };
        let label = one_line(&label, GREATEST_LABEL);
        if label.is_empty() {
            continue;
        }
        let said = one_line(&prompt, GREATEST_PROMPT);
        let prompt = if said.is_empty() { label.clone() } else { said };
        if asks.iter().any(|held| held.label == label) {
            continue;
        }
        if asks.len() >= GREATEST_SUGGESTIONS {
            dropped += 1;
            continue;
        }
        asks.push(Suggestion { label, prompt });
    }
    if asks.is_empty() {
        return Made {
            report: crate::tools::Report {
                ok: false,
                output: String::from(
                    "No button was named, so nothing is proposed to the user at the end of the answer",
                ),
            },
            asks: Vec::new(),
        };
    }
    let named: Vec<String> = asks.iter().map(|one| one.label.clone()).collect();
    let mut output = format!(
        "{} {} offered for the end of your answer: {}. The buttons appear when the answer has stopped, and the user may press one or write their own.",
        asks.len(),
        if asks.len() == 1 { "next step is" } else { "next steps are" },
        named.join("; ")
    );
    if dropped > 0 {
        output.push_str(&format!(
            " {} further {} dropped, because three buttons are the most one answer may end with.",
            dropped,
            if dropped == 1 { "was" } else { "were" }
        ));
    }
    Made {
        report: crate::tools::Report { ok: true, output },
        asks,
    }
}

// The text of the header of the block while the call is being written: the first label, as far
// as it has come. A model that writes the objects of the list in the order it likes is read
// whether the label came first or the prompt did, and a list of plain text gives its first
// word.
pub fn partial_words(arguments: &str) -> String {
    if let Some(label) = quoted_after(arguments, "label") {
        if !label.is_empty() {
            return one_line(&label, GREATEST_LABEL);
        }
    }
    let list_at = arguments
        .find("\"suggestions\"")
        .or_else(|| arguments.find("\"options\""));
    let Some(at) = list_at else {
        return String::new();
    };
    let tail = &arguments[at..];
    let Some(open) = tail.find('[') else {
        return String::new();
    };
    // A list of objects names its buttons by their labels. The label of the first object has
    // not come yet, and the header waits for it, as the header of a file tool waits for the
    // name of the file. A list of plain text names itself, and its first word is read.
    let rest = tail[open + 1..].trim_start();
    if rest.starts_with('{') {
        return String::new();
    }
    match quoted_after(rest, "") {
        Some(word) => one_line(&word, GREATEST_LABEL),
        None => String::new(),
    }
}

// The first quoted string that is after `key`, read whether the quote was closed or not. An
// empty key reads the first quoted string of the text.
fn quoted_after(words: &str, key: &str) -> Option<String> {
    let from = if key.is_empty() {
        0
    } else {
        let mark = format!("\"{key}\"");
        words.find(&mark)? + mark.len()
    };
    let letters: Vec<char> = words[from..].chars().collect();
    let mut read = 0usize;
    if key.is_empty() {
        while read < letters.len() && letters[read] != '"' {
            read += 1;
        }
    } else {
        while read < letters.len() && (letters[read] == ':' || letters[read].is_whitespace()) {
            read += 1;
        }
    }
    if read >= letters.len() || letters[read] != '"' {
        return None;
    }
    read += 1;
    let mut word = String::new();
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
    Some(word)
}

// One line of text, cut at a length. Markers of a new line are turned into spaces, for a button
// shows one line and the header of a block shows one.
fn one_line(words: &str, bound: usize) -> String {
    let flat = words.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= bound {
        return flat;
    }
    let cut: String = flat.chars().take(bound).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Three buttons of labels and prompts, in the order the model gave them.
    #[test]
    fn three_buttons_of_a_call_stand_in_the_order_they_were_given() {
        let made = offer(
            "{\"suggestions\":[{\"label\":\"Run the tests\",\"prompt\":\"Run the tests of the core and tell me what failed.\"},{\"label\":\"Show me the diff\"}]}",
        );
        assert!(made.report.ok, "{}", made.report.output);
        assert_eq!(made.asks.len(), 2);
        assert_eq!(made.asks[0].label, "Run the tests");
        assert_eq!(
            made.asks[0].prompt,
            "Run the tests of the core and tell me what failed."
        );
        assert_eq!(made.asks[1].label, "Show me the diff");
        assert_eq!(made.asks[1].prompt, "Show me the diff", "a label with no prompt sends itself");
    }

    // A call of plain text is read as three buttons of one word each, and a fourth is dropped
    // and stated.
    #[test]
    fn the_fourth_button_of_a_call_is_dropped_and_said() {
        let made = offer("{\"suggestions\":[\"one\",\"two\",\"three\",\"four\"]}");
        assert!(made.report.ok);
        assert_eq!(made.asks.len(), 3, "three buttons are the most");
        assert!(made.report.output.contains("1 further was dropped"), "{}", made.report.output);
    }

    // A call that named nothing usable is answered, and returns nothing.
    #[test]
    fn a_call_with_nothing_usable_offers_nothing() {
        let made = offer("{\"suggestions\":[\"  \",\"  \"]}");
        assert!(!made.report.ok);
        assert!(made.asks.is_empty());
        let empty = offer("{}");
        assert!(!empty.report.ok);
        let broken = offer("{\"suggestions\":[");
        assert!(!broken.report.ok);
    }

    // Two buttons of one label come once, for two buttons that say the same are one button read
    // twice.
    #[test]
    fn two_buttons_of_one_label_come_once() {
        let made = offer("{\"suggestions\":[\"Run it\",\"Run it\",\"Read it\"]}");
        assert_eq!(made.asks.len(), 2);
        assert_eq!(made.asks[1].label, "Read it");
    }

    // The header of a call being written names the first label, and a list of plain text names
    // its first word.
    #[test]
    fn the_head_of_a_call_being_written_names_the_first_button() {
        assert_eq!(
            partial_words("{\"suggestions\":[{\"label\":\"Run the tests\",\"prompt\":\"go\"}]}"),
            "Run the tests"
        );
        assert_eq!(
            partial_words("{\"suggestions\":[{\"prompt\":\"go\",\"label\":\"Read\"}"),
            "Read"
        );
        assert_eq!(
            partial_words("{\"suggestions\":[{\"prompt\":\"go\"}"),
            "",
            "the header of an object whose label has not come waits for it"
        );
        assert_eq!(partial_words("{\"suggestions\":[\"Run the"), "Run the");
        assert_eq!(partial_words("{}"), "");
    }
}
