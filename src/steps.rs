// The tool that shows the steps of the work as a block under the answer.
//
// A long answer often describes several steps in running text. The user then has
// to read the whole answer again to find the step they are on. This tool lets the
// model put those steps in a block of their own: a title, the text of one step at
// a time, and a row of buttons that selects a step.
//
// The call runs at once and waits for nothing. The block stands under the answer
// that made the call, and only once the model has stopped writing: a step list
// that came in the middle of an answer describes work that is not finished. The
// newest call of an answer wins, so a model that corrects its own list replaces it
// rather than adding a second block.
//
// The model may give as many steps as the work needs. The page lays out the row
// of step buttons for the width of the chat and adds arrows when the row does not
// fit, so a long list stays readable.
//
// The list rides on the message of the run, as an offered file does, so it is
// drawn again when the chat is opened another day.
//
// The tool falls under the Tools badge, and the drop-down of the badge holds a
// line of its own, as the other tools of the program do.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tools::with_rule;

// Name the model uses to show the steps of the work.
pub const NAME: &str = "show_steps";

// The longest words a step button shows. A label longer than this is a sentence,
// and the sentence belongs in the text of the step.
pub const GREATEST_LABEL: usize = 60;

// The longest words of the title of the block.
pub const GREATEST_TITLE: usize = 120;

// One step: the words on its button, and the text the block shows for it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Step {
    pub label: String,
    pub content: String,
}

// The whole block: the title over the text, and the steps under it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    pub steps: Vec<Step>,
}

// What became of one call. The block stands under the answer when `plan` holds
// anything, and the receipt goes back to the model either way.
pub struct Made {
    pub report: crate::tools::Report,
    pub plan: Option<Plan>,
}

// The tool as the model is told of it.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": with_rule(String::from(
                "Show the user a step by step list of the work in a block of its own under your answer. Use it when the work has several ordered steps the user will want to follow one at a time: a plan of what you are about to do, the stages of a procedure you just carried out, or the steps the user must take themselves. Give as many steps as the work needs. Each step has a short label for its button and the text of the step, and the whole block has a title. Call this once, at the END of the answer, after the work and the words are done: the block appears only when the answer has stopped, and a second call in the same answer replaces the first. Do not use it for a task of one or two steps, and do not repeat in the block words you already wrote in the answer.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "The head of the block, such as 'Steps to finish the install'. Keep it short. Leave it out and the block is headed 'Steps'."
                    },
                    "steps": {
                        "type": "array",
                        "description": "The steps, in the order the user reads them. As many as the work needs. A plain string is a step whose text is that string and whose button is named from it.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": {
                                    "type": "string",
                                    "description": "The short words on the button for this step, such as 'Install' or 'Run the tests'. Leave it out and the button is named from the start of the text."
                                },
                                "content": {
                                    "type": "string",
                                    "description": "The text of this step, shown in the block when the user selects it. One or two sentences, or the commands the user has to run."
                                }
                            },
                            "required": ["content"]
                        }
                    }
                },
                "required": ["steps"]
            }
        }
    })
}

// True when this name of a tool is the list of the steps of the work.
pub fn is_tool(name: &str) -> bool {
    name == NAME
}

// Answer one call. Every part is read as the model gave it: a list of objects, or
// a list of plain words, and the usual aliases for the two parts of a step. A
// step with no text is passed over. The number of steps is not limited.
pub fn show(arguments: &str) -> Made {
    let value: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let list = value
        .get("steps")
        .and_then(|one| one.as_array())
        .or_else(|| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut steps: Vec<Step> = Vec::new();
    for one in list.iter() {
        let (label, content) = match one {
            Value::String(text) => (String::new(), text.clone()),
            Value::Object(part) => (
                part.get("label")
                    .or_else(|| part.get("name"))
                    .or_else(|| part.get("title"))
                    .and_then(|one| one.as_str())
                    .unwrap_or("")
                    .to_string(),
                part.get("content")
                    .or_else(|| part.get("text"))
                    .or_else(|| part.get("description"))
                    .or_else(|| part.get("body"))
                    .and_then(|one| one.as_str())
                    .unwrap_or("")
                    .to_string(),
            ),
            _ => continue,
        };
        let content = content.trim().to_string();
        if content.is_empty() {
            continue;
        }
        let given = one_line(&label, GREATEST_LABEL);
        let label = if given.is_empty() {
            one_line(&content, 34)
        } else {
            given
        };
        steps.push(Step { label, content });
    }
    if steps.is_empty() {
        return Made {
            report: crate::tools::Report {
                ok: false,
                output: String::from(
                    "No step had any text, so no list of steps is shown to the user.",
                ),
            },
            plan: None,
        };
    }
    let title = one_line(
        value.get("title").and_then(|one| one.as_str()).unwrap_or(""),
        GREATEST_TITLE,
    );
    let made = Plan { title, steps };
    let named: Vec<String> = made
        .steps
        .iter()
        .enumerate()
        .map(|(n, one)| format!("{}. {}", n + 1, one.label))
        .collect();
    Made {
        report: crate::tools::Report {
            ok: true,
            output: format!(
                "{} {} shown at the end of your answer: {}. The block appears when the answer has stopped, and the user moves between the steps with its buttons.",
                made.steps.len(),
                if made.steps.len() == 1 { "step is" } else { "steps are" },
                named.join("; ")
            ),
        },
        plan: Some(made),
    }
}

// The words of the head of the block while the call is being written: the title
// of the block when the model wrote one, and the label of the first step when it
// did not. A step whose label has not come yet is named from its text, as the
// finished block names it.
pub fn partial_words(arguments: &str) -> String {
    if let Some(title) = quoted_after(arguments, "title") {
        let said = one_line(&title, GREATEST_TITLE);
        if !said.is_empty() {
            return said;
        }
    }
    if let Some(label) = quoted_after(arguments, "label") {
        let said = one_line(&label, GREATEST_LABEL);
        if !said.is_empty() {
            return said;
        }
    }
    let tail = arguments
        .find("\"steps\"")
        .map(|at| &arguments[at..])
        .unwrap_or(arguments);
    match plain_list(tail).into_iter().next() {
        Some(word) => one_line(&word, GREATEST_LABEL),
        None => String::new(),
    }
}

// The words of the call that are shown in the block while it is written: one line
// per step, with the label of the step and the text of it so far. A model that
// writes the text before the label is read the same way, and a list of plain words
// is read as a list of steps of one text each.
pub fn live_body(arguments: &str) -> String {
    let labels = all_values(arguments, "label").unwrap_or_default();
    let texts = all_values(arguments, "content")
        .or_else(|| all_values(arguments, "text"))
        .or_else(|| all_values(arguments, "description"));
    let mut out = String::new();
    if let Some(title) = quoted_after(arguments, "title") {
        if !title.trim().is_empty() {
            out.push_str(&format!("{}\n", title.trim()));
        }
    }
    match texts {
        Some(texts) => {
            let count = texts.len().max(labels.len());
            for n in 0..count {
                let label = labels
                    .get(n)
                    .filter(|one| !one.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("Step {}", n + 1));
                let text = texts.get(n).cloned().unwrap_or_default();
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&format!("{}. {}\n{}\n", n + 1, label.trim(), text));
            }
            out
        }
        // The text of the first step has not come yet. A list of plain words gives
        // its words under their own numbers, and a list of objects waits for the
        // text of its first step.
        None => {
            let tail = arguments
                .find("\"steps\"")
                .map(|at| &arguments[at..])
                .unwrap_or(arguments);
            for (n, word) in plain_list(tail).into_iter().enumerate() {
                out.push_str(&format!("{}. {}\n", n + 1, word));
            }
            out
        }
    }
}

// The words of a list of plain strings, read from the first `[` of the words to
// the `]` of that list. The last string may still be open, and gives what has
// come. A list of objects gives nothing, for its words stand under keys.
fn plain_list(words: &str) -> Vec<String> {
    let Some(open) = words.find('[') else {
        return Vec::new();
    };
    // A list of objects is not a list of plain words: its strings stand under
    // their keys, and reading them here would name a step for a key.
    if words[open + 1..].trim_start().starts_with('{') {
        return Vec::new();
    }
    let letters: Vec<char> = words[open + 1..].chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut read = 0usize;
    while read < letters.len() {
        let ch = letters[read];
        if ch == ']' {
            break;
        }
        if ch != '"' {
            read += 1;
            continue;
        }
        read += 1;
        let mut word = String::new();
        while read < letters.len() {
            let one = letters[read];
            if one == '\\' {
                if read + 1 < letters.len() {
                    word.push(letters[read + 1]);
                    read += 2;
                    continue;
                }
                break;
            }
            if one == '"' {
                read += 1;
                break;
            }
            word.push(one);
            read += 1;
        }
        if !word.trim().is_empty() {
            out.push(word);
        }
    }
    out
}

// Every value of one named part of an object that may not be finished, in the
// order they stand. The words of a value that is still being quoted give what has
// come, and the look stops there, for what comes after it cannot be read.
fn all_values(text: &str, key: &str) -> Option<Vec<String>> {
    let needle = format!("\"{}\"", key);
    if !text.contains(&needle) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(&needle) {
        let at = from + rel + needle.len();
        let rest = &text[at..];
        let Some(colon) = rest.find(':') else { break };
        let after = &rest[colon + 1..];
        let Some(open) = after.find('"') else { break };
        let mut read = open + 1;
        let letters: Vec<char> = after.chars().collect();
        let mut word = String::new();
        let mut closed = false;
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
                closed = true;
                break;
            }
            word.push(ch);
            read += 1;
        }
        out.push(word);
        if !closed {
            break;
        }
        from = at + colon + 1 + read + 1;
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

// The first quoted string that stands after `key`, read whether the quote was
// closed or not. An empty key reads the first quoted string of the words.
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

// One line of words, cut at a length. Marks of a new line are turned into spaces,
// for a button shows one line and the head of a block shows one.
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

    // Steps of labels and text, in the order the model gave them.
    #[test]
    fn the_steps_of_a_call_stand_in_the_order_they_were_given() {
        let made = show(
            "{\"title\":\"Ship it\",\"steps\":[{\"label\":\"Build\",\"content\":\"Run build.sh.\"},{\"label\":\"Test\",\"content\":\"Run the tests.\"}]}",
        );
        assert!(made.report.ok, "{}", made.report.output);
        let plan = made.plan.expect("the block stands");
        assert_eq!(plan.title, "Ship it");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].label, "Build");
        assert_eq!(plan.steps[0].content, "Run build.sh.");
        assert_eq!(plan.steps[1].label, "Test");
    }

    // The model may give as many steps as it wants: twenty come through as
    // twenty, and the receipt counts them.
    #[test]
    fn a_long_list_of_steps_comes_through_whole() {
        let list: Vec<String> = (0..20)
            .map(|n| format!("{{\"label\":\"Step {n}\",\"content\":\"Text {n}\"}}"))
            .collect();
        let made = show(&format!("{{\"steps\":[{}]}}", list.join(",")));
        assert!(made.report.ok);
        let plan = made.plan.expect("the block stands");
        assert_eq!(plan.steps.len(), 20, "no ceiling over the number of steps");
        assert!(made.report.output.starts_with("20 steps are shown"), "{}", made.report.output);
        assert!(made.report.output.contains("20. Step 19"), "{}", made.report.output);
    }

    // A step of plain words is read as a step whose text is those words, and its
    // button is named from the start of the text.
    #[test]
    fn a_step_of_plain_words_names_its_own_button() {
        let made = show("{\"steps\":[\"Check out the branch and open it in the editor.\"]}");
        let plan = made.plan.expect("the block stands");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].label, "Check out the branch and open it i…");
        assert_eq!(plan.title, "", "a call with no title gives no title");
    }

    // A step with no text is passed over, and a call of nothing but such steps
    // says that nothing stands.
    #[test]
    fn a_step_with_no_text_is_passed_over() {
        let made = show("{\"steps\":[{\"label\":\"One\"},{\"content\":\"  \"},{\"content\":\"Real\"}]}");
        let plan = made.plan.expect("the block stands");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].content, "Real");
        let nothing = show("{\"steps\":[{\"label\":\"One\"}]}");
        assert!(!nothing.report.ok);
        assert!(nothing.plan.is_none());
        let broken = show("{\"steps\":[");
        assert!(!broken.report.ok);
    }

    // The head of a call being written names the title, and the label of the
    // first step when no title came.
    #[test]
    fn the_head_of_a_call_being_written_names_the_block() {
        assert_eq!(partial_words("{\"title\":\"Ship it\",\"steps\":["), "Ship it");
        assert_eq!(
            partial_words("{\"steps\":[{\"label\":\"Build\",\"content\":\"go\"}]}"),
            "Build"
        );
        assert_eq!(partial_words("{\"steps\":[\"Check out the"), "Check out the");
        assert_eq!(partial_words("{}"), "");
    }

    // The body of a block being written holds every step so far, numbered, with
    // the text of each as far as it has come.
    #[test]
    fn the_body_of_a_block_being_written_holds_the_steps_so_far() {
        let whole = "{\"title\":\"Ship it\",\"steps\":[{\"label\":\"Build\",\"content\":\"Run build.sh.\"},{\"label\":\"Test\",\"content\":\"Run the tests\"}";
        let shown = live_body(whole);
        assert!(shown.contains("Ship it"), "{}", shown);
        assert!(shown.contains("1. Build"), "{}", shown);
        assert!(shown.contains("Run build.sh."), "{}", shown);
        assert!(shown.contains("2. Test"), "{}", shown);
        assert!(shown.contains("Run the tests"), "{}", shown);
        // The text came before the label: the step is still numbered and shown.
        let backwards = live_body("{\"steps\":[{\"content\":\"Run it\"}");
        assert!(backwards.contains("1. Step 1"), "{}", backwards);
        assert!(backwards.contains("Run it"), "{}", backwards);
        assert_eq!(live_body("{}"), "");
    }
}
