// The tool that runs a call of another tool again with a changed parameter.
//
// `edit_file` fails when an `old_string` matches no place in the file. The model
// then has to write the whole call out again: the name of the file, and every
// change that was already right. That costs the room of the talk, and it gives
// the model another chance to make a second mistake in work that was not broken.
//
// This tool takes the call that failed and changes only the part that was wrong.
// Each change names the parameter it edits, so text that also stands in another
// parameter is never touched. The changed call then goes down the same road as
// any other call of that tool, and the model is given the tool's own answer.
//
// A parameter is named by a path, so a change reaches one place and not several:
// `path` for a parameter of the call, and `edits.0.old_string` for a value inside
// a list inside the call. A change that gives `old_string` must match exactly one
// place inside that parameter, or it is refused with the number of places it
// found.

use serde_json::{json, Value};

use crate::tools::with_rule;

// Name the model uses to have a failed call run again.
pub const NAME: &str = "rerun_tool";

// The line put at the end of the receipt of a call that ran and failed. Without
// it the model has to remember that this tool exists, and a model that has just
// been told its change matched nothing goes straight back to writing the whole
// call out again. A call that never ran gets no such line, for there is nothing
// there to mend in the parameters.
pub const HINT: &str = " To run this call again with one parameter changed, call rerun_tool: name the parameter and give the new text. You do not have to write the whole call out again.";

// The most changes one call may carry. A call that needs more than this is a call
// that should be written out again.
pub const MOST_CHANGES: usize = 12;

// True when this name of a tool is the running of a failed call again.
pub fn is_tool(name: &str) -> bool {
    name == NAME
}

// The tool as the model is told of it.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": with_rule(String::from(
                "Run a tool call that failed, or that did less than you meant, with one or more of its parameters changed. Use it as soon as a call of this program comes back with an error and the fix is a change to what you sent it rather than a change to your plan. The common case is edit_file answering that an old_string matched no place in the file, or matched several places: read the file again, then call this with parameter set to the parameter that holds the wrong text and the right text in new_string. It also fits a wrong file name, a wrong command, a wrong search string, or a list with one name in it that does not exist. You do not write the whole call out again: the tool takes the parameters of the call that failed, changes the places you name, and runs it. Every change names ONE parameter, so text that stands in another parameter is left alone. Name a parameter by a path: 'path' for a parameter of the call, or 'edits.0.old_string' for a value inside a list inside the call. If you leave out call and tool, the most recent failed call of this chat is used.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "call": {
                        "type": "string",
                        "description": "The id of the call to run again, as it stands in the tool_call_id of the call that failed. Leave it out to take the most recent failed call, or name the tool instead."
                    },
                    "tool": {
                        "type": "string",
                        "description": "The name of the tool whose most recent failed call is to be run again, such as 'edit_file'. Leave it out when you gave the id of the call."
                    },
                    "edits": {
                        "type": "array",
                        "description": "The changes to make before the call runs again. Each one names the parameter it changes, and no change reaches outside the parameter it names.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "parameter": {
                                    "type": "string",
                                    "description": "The parameter of the failed call to change, named by a path. Use the plain name for a parameter of the call, such as 'path', 'command' or 'text'. Use dots and the place in the list for a value inside a list or inside another object, such as 'edits.0.old_string', 'edits.1.new_string' or 'paths.2'."
                                },
                                "old_string": {
                                    "type": "string",
                                    "description": "The text to change, as it stands inside that parameter. Leave it out to write the whole parameter again with new_string. When you give it, it must match exactly one place inside that parameter, or the change is refused and the number of places it matched is told to you."
                                },
                                "new_string": {
                                    "type": "string",
                                    "description": "The text to put in its place. With old_string it is put where that text stood; without it, it becomes the whole parameter, and an empty string takes the parameter out. When the parameter holds something other than words, such as a number or a list, give the new value as JSON."
                                },
                                "replace_all": {
                                    "type": "boolean",
                                    "description": "Change every place the text stands inside that parameter. Without this a text that stands in more than one place is refused, so nothing is changed that you did not mean."
                                }
                            },
                            "required": ["parameter", "new_string"]
                        }
                    }
                },
                "required": ["edits"]
            }
        }
    })
}

// One change of one parameter.
#[derive(Clone, Debug, Default)]
pub struct Change {
    pub parameter: String,
    pub old: Option<String>,
    pub new: String,
    pub all: bool,
}

// What the model asked for: which call to run again, and what to change in it.
#[derive(Clone, Debug, Default)]
pub struct Asking {
    pub call: String,
    pub tool: String,
    pub edits: Vec<Change>,
}

// The one thing the head of a block being written names: the tool whose call is
// to be run again, or the id of the call when no tool was named.
pub fn partial_words(raw: &str) -> String {
    let tool = crate::tools::partial_text(raw, "tool");
    if !tool.is_empty() {
        return tool;
    }
    let call = crate::tools::partial_text(raw, "call");
    if !call.is_empty() {
        return call;
    }
    String::from("the last failed call")
}

// Read the words of a call. The list of changes is required, and every change
// names a parameter and says what goes in its place.
pub fn asking(arguments: &str) -> Result<Asking, String> {
    let value: Value = match serde_json::from_str(arguments) {
        Ok(one) => one,
        Err(_) => return Err(String::from("the parameters of this call are not JSON")),
    };
    let list = match value.get("edits").and_then(|one| one.as_array()) {
        Some(list) => list.clone(),
        None => {
            // One change given on its own, rather than in a list of them.
            match value.get("edits") {
                Some(Value::Object(_)) => vec![value["edits"].clone()],
                _ => {
                    return Err(String::from(
                        "no change was given. Name the parameter to change in edits.",
                    ))
                }
            }
        }
    };
    if list.is_empty() {
        return Err(String::from(
            "no change was given. Name the parameter to change in edits.",
        ));
    }
    if list.len() > MOST_CHANGES {
        return Err(format!(
            "{} changes were given, and {} is the most one call may carry. Write the call out again instead.",
            list.len(),
            MOST_CHANGES
        ));
    }
    let mut edits: Vec<Change> = Vec::new();
    for one in list.iter() {
        let parameter = one
            .get("parameter")
            .or_else(|| one.get("key"))
            .or_else(|| one.get("name"))
            .and_then(|held| held.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if parameter.is_empty() {
            return Err(String::from(
                "a change named no parameter. Every change must name the one parameter it edits.",
            ));
        }
        let new = match one.get("new_string").or_else(|| one.get("value")) {
            Some(Value::String(text)) => text.clone(),
            Some(held) => held.to_string(),
            None => {
                return Err(format!(
                    "the change of '{}' gave no new_string.",
                    parameter
                ))
            }
        };
        let old = one
            .get("old_string")
            .or_else(|| one.get("old"))
            .and_then(|held| held.as_str())
            .map(|text| text.to_string())
            .filter(|text| !text.is_empty());
        edits.push(Change {
            parameter,
            old,
            new,
            all: one
                .get("replace_all")
                .and_then(|held| held.as_bool())
                .unwrap_or(false),
        });
    }
    Ok(Asking {
        call: value
            .get("call")
            .or_else(|| value.get("call_id"))
            .and_then(|held| held.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
        tool: value
            .get("tool")
            .or_else(|| value.get("tool_name"))
            .and_then(|held| held.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
        edits,
    })
}

// The parameters of the failed call, with the changes made. The words that go
// back in the error say what was wrong and what the model can do next.
pub fn apply(raw: &str, edits: &[Change]) -> Result<String, String> {
    let mut root: Value = serde_json::from_str(raw).map_err(|_| {
        String::from("the parameters of that call are not an object of JSON, so nothing can be changed in them")
    })?;
    if !root.is_object() {
        return Err(String::from(
            "the parameters of that call are not an object of JSON, so nothing can be changed in them",
        ));
    }
    for (number, one) in edits.iter().enumerate() {
        change(&mut root, one)
            .map_err(|why| format!("change {} of '{}': {}", number + 1, one.parameter, why))?;
    }
    Ok(root.to_string())
}

// The words of the parameters, with the names of them, for the receipt the model
// is given. The value of a parameter of words is shown as the words themselves,
// and everything else as JSON, cut at a length so a long file never goes back.
pub fn shown(raw: &str, edits: &[Change]) -> String {
    let value: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    let mut out: Vec<String> = Vec::new();
    for one in edits {
        let place = match find(&value, &one.parameter) {
            Ok(place) => place,
            Err(_) => continue,
        };
        out.push(format!(
            "{}: {}",
            one.parameter,
            one_line(&cut(&text_of(place)), 160)
        ));
    }
    out.join("; ")
}

// One change, made in the one parameter it names.
fn change(root: &mut Value, one: &Change) -> Result<(), String> {
    let place = find_mut(root, &one.parameter)?;
    let was_words = place.is_string();
    let before = text_of(place);
    let made = match one.old.as_ref() {
        None => {
            if was_words {
                one.new.clone()
            } else {
                // The parameter holds a number, a list or an object. The new value
                // is read as JSON, so the kind of it does not change by accident.
                let fresh: Value = serde_json::from_str(&one.new).map_err(|_| {
                    format!(
                        "'{}' holds {} and not words, so new_string must be a new value of that kind written as JSON",
                        one.parameter, kind_of(place)
                    )
                })?;
                *place = fresh;
                return Ok(());
            }
        }
        Some(old) => {
            let found = before.matches(old.as_str()).count();
            if found == 0 {
                return Err(format!(
                    "the text to change does not stand inside '{}' at all. It holds: {}",
                    one.parameter,
                    one_line(&cut(&before), 240)
                ));
            }
            if found > 1 && !one.all {
                return Err(format!(
                    "the text to change stands {} times inside '{}'. Give more of the text so that it names one place, or set replace_all to change every place of it",
                    found, one.parameter
                ));
            }
            if one.all {
                before.replace(old.as_str(), one.new.as_str())
            } else {
                let at = before.find(old.as_str()).unwrap_or(0);
                format!(
                    "{}{}{}",
                    &before[..at],
                    one.new,
                    &before[at + old.len()..]
                )
            }
        }
    };
    if was_words {
        *place = Value::String(made);
        return Ok(());
    }
    let fresh: Value = serde_json::from_str(&made).map_err(|_| {
        format!(
            "the change left '{}' holding words that are not {} any more",
            one.parameter,
            kind_of(place)
        )
    })?;
    *place = fresh;
    Ok(())
}

// The value a path names, for a change to be made in it. A part of the path names
// a key of an object, or the place in a list.
fn find_mut<'a>(root: &'a mut Value, path: &str) -> Result<&'a mut Value, String> {
    let parts: Vec<&str> = path.split('.').filter(|one| !one.is_empty()).collect();
    if parts.is_empty() {
        return Err(String::from("no parameter was named"));
    }
    let mut at = root;
    for (number, part) in parts.iter().enumerate() {
        let next = step(at, part).map_err(|why| {
            format!(
                "{} (the path stands to '{}' here)",
                why,
                parts[..=number].join(".")
            )
        })?;
        at = next;
    }
    Ok(at)
}

// The value a path names, read without changing anything.
fn find<'a>(root: &'a Value, path: &str) -> Result<&'a Value, String> {
    let parts: Vec<&str> = path.split('.').filter(|one| !one.is_empty()).collect();
    if parts.is_empty() {
        return Err(String::from("no parameter was named"));
    }
    let mut at = root;
    for part in parts.iter() {
        at = match at {
            Value::Object(map) => map
                .get(*part)
                .ok_or_else(|| format!("there is no parameter named '{}'", part))?,
            Value::Array(list) => {
                let place = part
                    .parse::<usize>()
                    .map_err(|_| format!("'{}' is not a place in a list", part))?;
                list.get(place).ok_or_else(|| {
                    format!("the list holds {} places, so there is no place {}", list.len(), place)
                })?
            }
            _ => return Err(format!("'{}' holds no parameter named '{}'", part, part)),
        };
    }
    Ok(at)
}

// One part of a path, taken from a value that may be an object or a list.
fn step<'a>(at: &'a mut Value, part: &str) -> Result<&'a mut Value, String> {
    match at {
        Value::Object(map) => {
            if !map.contains_key(part) {
                return Err(format!(
                    "there is no parameter named '{}'. The parameters are: {}",
                    part,
                    keys_of(map)
                ));
            }
            map.get_mut(part)
                .ok_or_else(|| format!("there is no parameter named '{}'", part))
        }
        Value::Array(list) => {
            let place = part
                .parse::<usize>()
                .map_err(|_| format!("'{}' is not a place in a list", part))?;
            if place >= list.len() {
                return Err(format!(
                    "the list holds {} places, so there is no place {}",
                    list.len(),
                    place
                ));
            }
            Ok(&mut list[place])
        }
        other => Err(format!(
            "'{}' stands inside {}, which holds nothing",
            part,
            kind_of(other)
        )),
    }
}

// The names of the parameters of an object, in the order they stand.
fn keys_of(map: &serde_json::Map<String, Value>) -> String {
    if map.is_empty() {
        return String::from("none");
    }
    map.keys().cloned().collect::<Vec<_>>().join(", ")
}

// What a value holds, in the words a model is told.
fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "nothing",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "words",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

// The words of a value: a value of words gives its own words, and anything else
// gives the JSON of it.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

// Words cut at a length, with the mark of the ellipsis at the end.
fn cut(words: &str) -> String {
    const MOST: usize = 600;
    if words.chars().count() <= MOST {
        return words.to_string();
    }
    let kept: String = words.chars().take(MOST).collect();
    format!("{}...", kept.trim_end())
}

// One line of words, for a receipt.
fn one_line(words: &str, bound: usize) -> String {
    let flat = words.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= bound {
        return flat;
    }
    let kept: String = flat.chars().take(bound).collect();
    format!("{}...", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(text: &str) -> Vec<Change> {
        asking(text).expect("the call was read").edits
    }

    // A change reaches the one parameter it names. The same text standing in
    // another parameter is left where it stands.
    #[test]
    fn a_change_reaches_only_the_parameter_it_names() {
        let held = r#"{"path":"notes/plan.md","edits":[{"old_string":"notes/plan.md","new_string":"x"}]}"#;
        let made = apply(held, &changes(
            r#"{"edits":[{"parameter":"path","old_string":"plan","new_string":"todo"}]}"#,
        ))
        .expect("the change was made");
        let back: Value = serde_json::from_str(&made).unwrap();
        assert_eq!(back["path"], "notes/todo.md");
        assert_eq!(
            back["edits"][0]["old_string"], "notes/plan.md",
            "the same words in another parameter were not touched"
        );
    }

    // A value inside a list is reached by its place in the list.
    #[test]
    fn a_value_inside_a_list_is_reached_by_its_place() {
        let held = r#"{"path":"a.txt","edits":[{"old_string":"one","new_string":"two"},{"old_string":"three","new_string":"four"}]}"#;
        let made = apply(held, &changes(
            r#"{"edits":[{"parameter":"edits.0.old_string","new_string":"ONE"}]}"#,
        ))
        .expect("the change was made");
        let back: Value = serde_json::from_str(&made).unwrap();
        assert_eq!(back["edits"][0]["old_string"], "ONE");
        assert_eq!(back["edits"][0]["new_string"], "two");
        assert_eq!(back["edits"][1]["old_string"], "three");
    }

    // Text that stands in more than one place inside the parameter is refused,
    // and the number of places it stands in is said.
    #[test]
    fn text_that_stands_in_two_places_is_refused() {
        let held = r#"{"command":"echo cat; cat file"}"#;
        let why = apply(held, &changes(
            r#"{"edits":[{"parameter":"command","old_string":"cat","new_string":"dog"}]}"#,
        ))
        .expect_err("two places must be refused");
        assert!(why.contains("2 times"), "{}", why);
        assert!(why.contains("replace_all"), "{}", why);
        // With replace_all every place of it goes.
        let made = apply(held, &changes(
            r#"{"edits":[{"parameter":"command","old_string":"cat","new_string":"dog","replace_all":true}]}"#,
        ))
        .expect("every place was changed");
        assert!(made.contains("echo dog; dog file"), "{}", made);
    }

    // Text that does not stand in the parameter is refused, and the parameter is
    // said with what it holds.
    #[test]
    fn text_that_is_not_in_the_parameter_is_refused() {
        let held = r#"{"path":"a.txt"}"#;
        let why = apply(held, &changes(
            r#"{"edits":[{"parameter":"path","old_string":"gone","new_string":"x"}]}"#,
        ))
        .expect_err("no place must be refused");
        assert!(why.contains("does not stand inside 'path'"), "{}", why);
        assert!(why.contains("a.txt"), "{}", why);
    }

    // A parameter that is not there is refused with the names that are.
    #[test]
    fn a_parameter_that_is_not_there_is_refused_with_the_names_that_are() {
        let held = r#"{"path":"a.txt","command":"ls"}"#;
        let why = apply(held, &changes(
            r#"{"edits":[{"parameter":"file","new_string":"b.txt"}]}"#,
        ))
        .expect_err("a name that is not there must be refused");
        assert!(why.contains("no parameter named 'file'"), "{}", why);
        assert!(why.contains("path, command"), "{}", why);
    }

    // A place in a list that is past its end is refused with the length of it.
    #[test]
    fn a_place_past_the_end_of_a_list_is_refused() {
        let held = r#"{"paths":["a","b"]}"#;
        let why = apply(held, &changes(r#"{"edits":[{"parameter":"paths.5","new_string":"c"}]}"#))
            .expect_err("no such place must be refused");
        assert!(why.contains("holds 2 places"), "{}", why);
    }

    // A parameter that holds a number is given a new number, and words are
    // refused for it.
    #[test]
    fn a_number_is_changed_as_a_number() {
        let held = r#"{"timeout":30}"#;
        let made = apply(held, &changes(r#"{"edits":[{"parameter":"timeout","new_string":"120"}]}"#))
            .expect("the number was changed");
        let back: Value = serde_json::from_str(&made).unwrap();
        assert_eq!(back["timeout"], 120, "the new number went in as a number");
        let why = apply(held, &changes(
            r#"{"edits":[{"parameter":"timeout","new_string":"a while"}]}"#,
        ))
        .expect_err("words for a number must be refused");
        assert!(why.contains("as JSON"), "{}", why);
    }

    // A whole list is replaced by the JSON of the new one.
    #[test]
    fn a_list_is_replaced_by_the_json_of_the_new_one() {
        let held = r#"{"paths":["a.png","gone.png","b.png"]}"#;
        let made = apply(held, &changes(
            r#"{"edits":[{"parameter":"paths","new_string":"[\"a.png\", \"b.png\"]"}]}"#,
        ))
        .expect("the list was changed");
        let back: Value = serde_json::from_str(&made).unwrap();
        assert_eq!(back["paths"].as_array().unwrap().len(), 2);
    }

    // Several changes go in the order they were given, each in its own place.
    #[test]
    fn several_changes_go_in_the_order_they_were_given() {
        let held = r#"{"path":"a.txt","edits":[{"old_string":"x","new_string":"y"}]}"#;
        let made = apply(held, &changes(
            r#"{"edits":[{"parameter":"path","old_string":"a.txt","new_string":"b.txt"},{"parameter":"edits.0.old_string","new_string":"X"}]}"#,
        ))
        .expect("both changes were made");
        let back: Value = serde_json::from_str(&made).unwrap();
        assert_eq!(back["path"], "b.txt");
        assert_eq!(back["edits"][0]["old_string"], "X");
        assert_eq!(back["edits"][0]["new_string"], "y");
    }

    // A call with no change, or with a change that names nothing, is refused.
    #[test]
    fn a_call_with_no_change_is_refused() {
        assert!(asking("{}").is_err());
        assert!(asking(r#"{"edits":[]}"#).is_err());
        assert!(asking(r#"{"edits":[{"new_string":"x"}]}"#).is_err());
        assert!(asking(r#"{"edits":[{"parameter":"path"}]}"#).is_err());
        let many: Vec<String> = (0..13)
            .map(|n| format!(r#"{{"parameter":"p{}","new_string":"x"}}"#, n))
            .collect();
        assert!(asking(&format!(r#"{{"edits":[{}]}}"#, many.join(","))).is_err());
    }

    // The id of the call and the name of the tool are read when they are given.
    #[test]
    fn the_call_and_the_tool_are_read_when_they_are_given() {
        let one = asking(
            r#"{"call":"call-7","tool":"edit_file","edits":[{"parameter":"path","new_string":"b.txt"}]}"#,
        )
        .expect("the call was read");
        assert_eq!(one.call, "call-7");
        assert_eq!(one.tool, "edit_file");
        assert_eq!(one.edits.len(), 1);
        assert_eq!(one.edits[0].parameter, "path");
        assert!(one.edits[0].old.is_none(), "no old_string means set the whole");
    }

    // The words the receipt speaks of name each parameter and what it held.
    #[test]
    fn the_receipt_names_the_parameters_that_changed() {
        let held = r#"{"path":"notes/plan.md","edits":[{"old_string":"one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen"}]}"#;
        let edits = changes(r#"{"edits":[{"parameter":"path","new_string":"b.txt"}]}"#);
        let said = shown(held, &edits);
        assert!(said.contains("path:"), "{}", said);
        assert!(said.contains("notes/plan.md"), "{}", said);
        assert!(said.len() < 400, "the receipt of a long value is cut");
    }

    // Words outside English are cut on their bytes, so a change does not fall
    // between the parts of a letter and break the rest of the parameter.
    #[test]
    fn words_outside_english_are_changed_whole() {
        let held = r#"{"text":"Ünïcode naïve — change this, naïve"}"#;
        let made = apply(held, &changes(
            r#"{"edits":[{"parameter":"text","old_string":"change this","new_string":"keep this"}]}"#,
        ))
        .expect("the change was made");
        let back: Value = serde_json::from_str(&made).unwrap();
        assert_eq!(back["text"], "Ünïcode naïve — keep this, naïve");
    }
}
