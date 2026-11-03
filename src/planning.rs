// Planning mode: the work of laying out a large piece of work before any of it
// is done.
//
// A user writes `/plan` and the chat changes from a talk into a drawing board.
// The model is told, in a prompt of its own, to plan the work down to its
// smallest part; it has one tool for the plan file, and no other way of
// touching it; and when the user says the plan is good, the model calls the
// second tool, the talk of the planning is put away, and the plan itself becomes
// the first message of the work.
//
// The plan file stands in a folder of the data directory of the program, and not
// in the room of the chat. A plan is therefore never mixed with the work it
// plans, and a command of the terminal tool cannot write over it by accident.
// The tool the model has for the plan carries no place at all: it reaches the one
// file of the one chat, and nothing else. It cannot be switched off by the user,
// for a planning chat without it has nowhere to put its work.
//
// Both tools stand only while planning mode is on. When the plan is let go they
// go away with the planning prompt, so the model that does the work reads neither
// the one nor the other.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config;
use crate::tools::{self, with_rule};

// The name the model uses to change the plan, and the name it uses to say that
// the plan is done and the work may begin.
pub const EDIT_NAME: &str = "edit_plan";
pub const END_NAME: &str = "end_planning";

// The word a user writes for planning mode, and the words of the same command
// with its mark before it.
pub const COMMAND: &str = "plan";
pub const COMMAND_WORD: &str = "/plan";

// The words of the command, for the list of them the chat box puts up.
pub const COMMAND_WORDS: &str = "Plan the work before any of it is done. The model writes a plan of the whole piece of work, in the smallest detail, to a plan file of its own, and changes it as you answer. When the plan is good, say so, and the model begins the work from the plan.";

// The folder the plans stand in: a folder of the data directory of the program,
// beside the talks and the records of the attachments.
pub fn folder() -> PathBuf {
    config::paths().data.join("planning")
}

// The name of the plan file of one chat. The id of a chat is ten hexadecimal
// characters, so the name needs nothing else to keep it apart.
pub fn file_name(chat: &str) -> String {
    format!("{}.md", neat(chat))
}

// True when a name kept with a chat is one this module could have written. A
// strange name opens nothing at all.
fn looks_like_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    !stem.is_empty()
        && stem.len() <= 40
        && stem.chars().all(|one| one.is_ascii_hexdigit())
}

// The whole way to a plan, from the name kept with the chat. Nothing comes back
// for a name that is not one of these.
pub fn path_of(name: &str) -> Option<PathBuf> {
    if !looks_like_name(name) {
        return None;
    }
    Some(folder().join(name))
}

// Make the plan file of a chat, and give back the name kept with the chat. A
// plan that stands already is left as it stands, so a chat that goes into
// planning mode twice does not lose what it wrote.
pub fn create(chat: &str) -> String {
    let name = file_name(chat);
    if let Some(place) = path_of(&name) {
        if !place.is_file() {
            let _ = fs::create_dir_all(folder());
            let _ = fs::write(&place, opening());
        }
    }
    name
}

// The first words of a new plan. The file is never empty, for a tool that
// changes a file by matching words needs words to match.
fn opening() -> String {
    String::from(
        "# The plan\n\n\
         The plan stands here. Nothing of it is written yet.\n",
    )
}

// The whole of a plan, for the popup of the heading and for the first message of
// the work. Nothing comes back when the plan is gone.
pub fn words(name: &str) -> Option<String> {
    let place = path_of(name)?;
    fs::read_to_string(place).ok()
}

// The words of a plan that is not there: said plainly, so a model is never given
// a blank space for a file it was told to read.
pub const GONE_WORDS: &str = "(the plan file of this chat is not there)";

// The room of the plan file. A plan longer than this is a file no model can read
// whole, and a call that would grow it past this is refused.
const GREATEST_PLAN_BYTES: usize = 8 * 1024 * 1024;

// The most changes one call may make to the plan.
const MAX_EDITS: usize = 100;

// The call that changes the plan. It carries no place: the plan file of this chat
// is the only file it can reach.
pub fn edit_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": EDIT_NAME,
            "description": with_rule(String::from(
                "Change the plan file of this chat. This tool has no path, because it reaches that one file and no other: the plan file stands in a folder of the planning directory of the program, and it is the only file this planning mode may change. Copy each old_string exactly from the plan, spacing included, and give the whole of a change to one call. The changes are made one after another, each against the plan as the change before it left it, and no two of them may cover the same text. Each old_string must match one place, unless replace_all is set for it; if one matches no place, or more than one, nothing at all is written. An old_string that is empty adds the words of new_string at the end of the plan, which is how a plan is written out in parts. A call that succeeded means the plan now holds what was sent, so read it again only when you must. Never write the plan with another tool, and never write the plan in your answer: this tool is the only way into the plan file.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "edits": {
                        "type": "array",
                        "description": "The changes to make in one call. Each change gives the words to replace and the words to put in their place.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_string": {
                                    "type": "string",
                                    "description": "The words to replace, copied from the plan. Empty means: add the words of new_string at the end of the plan."
                                },
                                "new_string": {
                                    "type": "string",
                                    "description": "The words to put in its place."
                                },
                                "replace_all": {
                                    "type": "boolean",
                                    "description": "Replace every place these words appear, instead of requiring that they appear once. Off when it is not given."
                                }
                            },
                            "required": ["old_string", "new_string"]
                        }
                    }
                },
                "required": ["edits"]
            }
        }
    })
}

// The call that says the plan is done: the user gave leave, and the work may
// begin. Nothing of the planning talk goes with the work.
pub fn end_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": END_NAME,
            "description": with_rule(String::from(
                "End planning mode and begin the work from the plan. Call this only after the user has said, in their own words, that the plan is agreed and the work may begin: words such as go ahead, start, begin, do it, or an answer to your question for leave. Never call it while the plan is still open, never call it to avoid more planning, and never call it twice in one turn. When it is called, the plan file is sent whole as the first message of a new talk, and nothing of this planning talk is kept, so the plan must stand on its own: it must name every file, every step and every check, and it must not refer to this conversation.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "why": {
                        "type": "string",
                        "description": "The words the user said that gave the leave to begin, or a short saying of what they agreed to."
                    }
                },
                "required": []
            }
        }
    })
}

// The prompt that stands in the talk of a chat while planning mode is on. It goes
// out beside the words of the program, in a message of the role `system`, for
// every ask of a planning chat, and it goes away with planning mode.
pub fn system_prompt(plan: &str) -> String {
    let mut out = String::new();
    out.push_str(
        "PLANNING MODE IS ON FOR THIS CONVERSATION.\n\n\
         You work as the planning architect of one piece of work. Nothing of the \
         work is done now. Your one task is a plan of it that another model, or a \
         person, can carry out without asking a single question afterwards. Write \
         that plan into the plan file, and keep it there in the best shape you can \
         bring it to, turn after turn.\n\n",
    );
    out.push_str(&format!(
        "The plan file.\n\
         The plan of this chat stands in one file: `{}`. It lives in a folder of \
         the planning directory of this program, away from the work it plans. You \
         change it with the tool `{}` and with nothing else. Never write the plan \
         with any other tool: not with a file tool, not with a command of the \
         terminal, not by sending the words in an answer. The plan file is the \
         only file you may change in this mode, and `{}` is the only way into it. \
         Your answers to the user stay short: they say what changed in the plan, \
         what you still need, and nothing else. Never repeat the plan in the \
         talk.\n\n",
        plan, EDIT_NAME, EDIT_NAME
    ));
    out.push_str(
        "How deep the plan must go.\n\
         Plan everything in great detail, down to the smallest detail. A plan that \
         is broad, or that leaves a part of the work to be thought out later, is a \
         plan that has failed, whoever reads it. Work out the size of every part \
         before you write it: the name of every file that is made, read or \
         changed; the name of every function, method, table, field, setting and \
         message inside it; the words of every signature, and what each one takes \
         and gives back; the order the steps must go in, and why that order and no \
         other; the exact text of every new setting, prompt, message and error \
         word; the check that says when a step is really finished; and the way a \
         step is put right again when it came out wrong. Where a part cannot be \
         settled now, write down what is unknown, what the choices are, which one \
         you advise, and what the answer changes. Guess at nothing, and call \
         nothing obvious: both are how a plan goes wrong at the far end of the \
         work.\n\n",
    );
    out.push_str(
        "The shape of the plan.\n\
         Keep the plan in this order, and leave a part out only when it has truly \
         nothing in it.\n\
         1. What is asked for: the work in the words of the user, and what a \
         finished piece of work looks like.\n\
         2. The ground it stands on: what the work is now, which files and parts \
         take part in it, what the running program does today, and what may not be \
         touched. Read before you write this: never guess at the shape of a project.\n\
         3. The rules the work must keep: the shape of the code, the words of the \
         comments, what must not be added, and what must go on working as it does.\n\
         4. The whole of the work, in steps. Each step names the files it touches, \
         the parts inside them, what goes in and what comes out, why it comes \
         after the step before it, how long it takes, and the check that proves it \
         was done. A step is small enough to be checked on its own.\n\
         5. The tests: what is tested, by what command, with what input, and what \
         the answer must be. Include the cases that break easily: nothing in it, \
         very much in it, a name that is strange, a thing that is missing, a thing \
         that was stopped half way, a file of another shape, an older record read \
         by a newer program.\n\
         6. The risks: what may go wrong, what it would look like when it does, \
         and what is done about it before it happens.\n\
         7. The order of the work and the places it may be stopped: what must be \
         finished before what, and what may stand on its own.\n\
         8. The open questions: what only the user can answer, each with the \
         answer you advise and what turns on it.\n\
         9. What is not done: the parts of the work you are leaving out on \
         purpose, so nobody thinks they were kept.\n\n",
    );
    out.push_str(&format!(
        "How the work of planning goes.\n\
         Look before you write. You may read the project, list its folders, open \
         its files and run a command that only looks about, so that the plan is \
         built on what is there. Change nothing of the work: no file of the \
         project, no setting, no record, no commit. Where a tool would change \
         something, do not call it. Say in the plan what you looked at, so a \
         reader knows what the plan was built on.\n\
         Take the plan one step at a time: write the part that is settled, then \
         look again, then write the next part. Change the plan with `{}` as soon \
         as a part of it is settled, rather than holding a page of it back for the \
         next answer. A plan that is written in five calls is a better plan than \
         one written in one.\n\
         When the user asks for a change, change the plan file first and then \
         answer. When the user asks a question about the work, answer it from what \
         you read, and put what you learned into the plan.\n\
         Ask the user about what you cannot settle, a few questions at a time, \
         and write their answers into the plan as they come.\n\
         Keep the plan in the language the user writes in.\n\n",
        EDIT_NAME
    ));
    out.push_str(&format!(
        "Ending planning mode.\n\
         The work begins only when the user says it may. When they have said, in \
         their own words, that the plan is agreed and the work may begin, call `{}` \
         and say nothing else. When their words are not clear, ask once whether \
         they mean the plan is agreed. Never call `{}` on your own, never call it \
         because the plan feels long enough, and never call it twice in one turn.\n\
         When `{}` is called, this talk is put away: the plan file is sent whole as \
         the first message of a new talk, with the tools and the prompts it had \
         before planning mode, and none of these instructions. Nothing of this \
         conversation is kept, and the planning tools are gone. The plan must \
         therefore stand entirely on its own: it must say what the work is, what \
         the ground is, and what to do, without needing anything said here.\n",
        END_NAME, END_NAME, END_NAME
    ));
    out
}

// The command a message begins with, and the words left when it is taken away.
// Only the first word of a message is a command, and only when it begins with the
// mark of one. A message that is only the command comes back with nothing after
// it, which is the user asking for the mode without a request in it.
pub fn split(text: &str) -> (Option<Command>, String) {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix(COMMAND_WORD) else {
        return (None, text.to_string());
    };
    // `/planet` is no command: the word has to stand on its own.
    if let Some(next) = rest.chars().next() {
        if !next.is_whitespace() {
            return (None, text.to_string());
        }
    }
    (Some(Command::Plan), rest.trim_start().to_string())
}

// The commands of the chat box, in the order they are listed. One command now,
// and the list is where the next one is put.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Plan,
}

impl Command {
    // The word the user writes for this command, mark included.
    pub fn word(&self) -> &'static str {
        COMMAND_WORD
    }

    // What the command is for, for the list the chat box puts up.
    pub fn words(&self) -> &'static str {
        COMMAND_WORDS
    }
}

// The words of the command list, for the state: the word of the command and what
// it is for.
pub fn list() -> Vec<Value> {
    vec![json!({
        "name": Command::Plan.word(),
        "words": Command::Plan.words(),
    })]
}

// Change the plan file of a chat. The plan is read whole, every change is matched
// against it as it stands, and the whole of the plan is written again or not at
// all. A call that names no change, or too many of them, is refused with a word
// about what the tool takes.
pub fn edit_plan(name: &str, raw: &str) -> tools::Report {
    let Some(place) = path_of(name) else {
        return tools::Report {
            ok: false,
            output: String::from("This chat has no plan file, so there is nothing to change."),
        };
    };
    let changes = match changes_of(raw) {
        Ok(made) => made,
        Err(why) => return tools::Report { ok: false, output: why },
    };
    let before: String = fs::read_to_string(&place).unwrap_or_default();
    // The changes go one after another, each against the plan as the change
    // before it left it. A plan is written in parts, and a model that gives a
    // part and a change in one call means the one to follow the other.
    let mut after = before;
    let mut made = 0usize;
    let mut added = 0usize;
    for change in &changes {
        if change.old.is_empty() {
            // The mark of an empty place: the words go at the end of the plan,
            // which is how a plan of many parts is written out.
            if !after.is_empty() && !after.ends_with('\n') {
                after.push('\n');
            }
            after.push_str(&change.new);
            added += 1;
            made += 1;
            continue;
        }
        let places = count_of(&after, &change.old);
        if places == 0 {
            return tools::Report {
                ok: false,
                output: format!(
                    "Nothing was written. These words are not in the plan: {}{}",
                    quote_head(&change.old),
                    if after.trim().is_empty() {
                        String::from(
                            " (the plan is empty, so put the whole part in new_string with an empty old_string)",
                        )
                    } else {
                        String::new()
                    }
                ),
            };
        }
        if places > 1 && !change.all {
            return tools::Report {
                ok: false,
                output: format!(
                    "Nothing was written. These words stand {} places in the plan, and one of them is not enough to say which was meant: {}",
                    places,
                    quote_head(&change.old)
                ),
            };
        }
        if change.all {
            after = after.replace(change.old.as_str(), change.new.as_str());
            made += places;
        } else {
            let found = after.find(change.old.as_str()).unwrap_or(0);
            after.replace_range(found..found + change.old.len(), &change.new);
            made += 1;
        }
    }
    if after.len() > GREATEST_PLAN_BYTES {
        return tools::Report {
            ok: false,
            output: String::from(
                "Nothing was written. The plan would grow past the length this program keeps a plan to.",
            ),
        };
    }
    if let Err(why) = write_plan(&place, &after) {
        return tools::Report {
            ok: false,
            output: format!("The plan could not be written: {}", why),
        };
    }
    let mut said = format!(
        "{} in the plan. The plan now holds {} lines.",
        if added > 0 && made == added {
            format!("{} part(s) added", added)
        } else {
            format!("{} change(s) made", made)
        },
        lines_of(&after)
    );
    if let Some(head) = head_of(&after) {
        said.push_str("\nThe plan begins:\n");
        said.push_str(&head);
    }
    tools::Report { ok: true, output: said }
}

// Write the plan again. The plan is a file of words, so the words are what is
// written; nothing of the shape of a file is kept.
fn write_plan(place: &Path, words: &str) -> Result<(), String> {
    if let Some(roof) = place.parent() {
        if !roof.is_dir() {
            fs::create_dir_all(roof).map_err(|why| why.to_string())?;
        }
    }
    fs::write(place, words).map_err(|why| why.to_string())
}

// One change of the plan.
struct Change {
    old: String,
    new: String,
    all: bool,
}

// The changes of one call. The call may be given whole, or in pieces that are
// still coming in, and a call whose words are cut in half is answered with the
// words that say so rather than with a half change to the plan.
fn changes_of(raw: &str) -> Result<Vec<Change>, String> {
    let value: Value = serde_json::from_str(raw).map_err(|_| {
        String::from("The call was not whole words of JSON, so nothing was written.")
    })?;
    let Some(items) = value.get("edits").and_then(|made| made.as_array()) else {
        return Err(String::from(
            "The call named no changes. Give the changes of the plan in `edits`.",
        ));
    };
    if items.is_empty() {
        return Err(String::from("The call named no changes."));
    }
    if items.len() > MAX_EDITS {
        return Err(format!(
            "The call asks for {} changes, and this tool takes at most {}.",
            items.len(),
            MAX_EDITS
        ));
    }
    let mut out: Vec<Change> = Vec::with_capacity(items.len());
    for item in items {
        let old = item
            .get("old_string")
            .and_then(|one| one.as_str())
            .ok_or_else(|| String::from("A change of the call has no old_string."))?
            .to_string();
        let new = item
            .get("new_string")
            .and_then(|one| one.as_str())
            .ok_or_else(|| String::from("A change of the call has no new_string."))?
            .to_string();
        let all = item
            .get("replace_all")
            .and_then(|one| one.as_bool())
            .unwrap_or(false);
        out.push(Change { old, new, all });
    }
    Ok(out)
}

// How many places one run of words stands in another.
fn count_of(text: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut places = 0usize;
    let mut from = 0usize;
    while let Some(found) = text[from..].find(needle) {
        places += 1;
        from += found + needle.len();
    }
    places
}

// The first lines of the plan, for the receipt. The model is given a look at
// where it is, not a copy of the file.
fn head_of(words: &str) -> Option<String> {
    let mut out = String::new();
    let mut lines = 0usize;
    for line in words.lines() {
        if line.trim().is_empty() {
            continue;
        }
        out.push_str(line.trim_end());
        out.push('\n');
        lines += 1;
        if lines >= 12 || out.len() > 1200 {
            break;
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

// How many lines a run of words holds.
fn lines_of(words: &str) -> usize {
    if words.is_empty() {
        return 0;
    }
    words.lines().count()
}

// A run of words for a fault word, cut short and on one line.
fn quote_head(words: &str) -> String {
    let one_line: String = words
        .chars()
        .map(|one| if one == '\n' || one == '\r' { ' ' } else { one })
        .collect();
    let cut: String = one_line.chars().take(80).collect();
    format!("`{}`", cut.trim())
}

// The name of a chat id turned into what a file name may hold. The id of a chat
// is already hexadecimal; this is the guard for a name that came from elsewhere.
fn neat(name: &str) -> String {
    name.chars()
        .filter(|one| one.is_ascii_alphanumeric())
        .take(40)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The plan file stands in a folder of the data directory, and only a name
    // this module wrote opens a way to it.
    #[test]
    fn a_plan_of_a_chat_is_one_file_of_the_planning_folder() {
        let _guard = config::test_turn();
        let name = create("ab12cd34ef");
        assert_eq!(name, "ab12cd34ef.md");
        let place = path_of(&name).expect("the way to the plan");
        assert!(place.starts_with(folder()));
        assert!(place.is_file(), "the plan file is made with planning mode");
        let held = words(&name).expect("the plan reads back");
        assert!(held.contains("# The plan"), "{}", held);
        // A second look leaves the plan as it stands.
        let _ = fs::write(&place, "the plan as the model wrote it");
        create("ab12cd34ef");
        assert_eq!(words(&name).unwrap(), "the plan as the model wrote it");
        let _ = fs::remove_file(&place);
        assert!(path_of("").is_none());
        assert!(path_of("../escape.md").is_none());
        assert!(path_of("notes.md").is_none());
    }

    // The tool changes the plan by matching words, and an empty place adds a part
    // at the end of it.
    #[test]
    fn the_plan_is_changed_by_matching_words() {
        let _guard = config::test_turn();
        let name = create("1234567890");
        let made = edit_plan(
            &name,
            r#"{"edits":[{"old_string":"Nothing of it is written yet.","new_string":"The work is laid out below."}]}"#,
        );
        assert!(made.ok, "{}", made.output);
        assert!(made.output.contains("1 change"), "{}", made.output);
        assert!(words(&name).unwrap().contains("The work is laid out below"));
        let added = edit_plan(
            &name,
            r#"{"edits":[{"old_string":"","new_string":"\n## Step one\n"}]}"#,
        );
        assert!(added.ok, "{}", added.output);
        let held = words(&name).unwrap();
        assert!(held.ends_with("## Step one\n"), "{}", held);
        let place = path_of(&name).unwrap();
        let _ = fs::remove_file(&place);
    }

    // A change that matches no place, or more than one, writes nothing at all.
    #[test]
    fn a_change_that_could_be_two_places_writes_nothing() {
        let _guard = config::test_turn();
        let name = create("aabbccddeeff");
        let place = path_of(&name).unwrap();
        fs::write(&place, "step and step again").unwrap();
        let none = edit_plan(&name, r#"{"edits":[{"old_string":"nowhere","new_string":"x"}]}"#);
        assert!(!none.ok);
        assert!(none.output.contains("not in the plan"), "{}", none.output);
        let twice = edit_plan(&name, r#"{"edits":[{"old_string":"step","new_string":"part"}]}"#);
        assert!(!twice.ok);
        assert!(twice.output.contains("2 places"), "{}", twice.output);
        assert_eq!(fs::read_to_string(&place).unwrap(), "step and step again");
        let all = edit_plan(
            &name,
            r#"{"edits":[{"old_string":"step","new_string":"part","replace_all":true}]}"#,
        );
        assert!(all.ok, "{}", all.output);
        assert_eq!(
            fs::read_to_string(&place).unwrap(),
            "part and part again"
        );
        let _ = fs::remove_file(&place);
    }

    // The word of the user, and the words left when the command is taken away.
    #[test]
    fn the_command_stands_at_the_head_of_the_message() {
        let (command, rest) = split("/plan build the side bar");
        assert_eq!(command, Some(Command::Plan));
        assert_eq!(rest, "build the side bar");
        let (alone, nothing) = split("/plan");
        assert_eq!(alone, Some(Command::Plan));
        assert!(nothing.is_empty());
        let (none, kept) = split("please /plan later");
        assert_eq!(none, None);
        assert_eq!(kept, "please /plan later");
        let (planet, words) = split("/planets are far");
        assert_eq!(planet, None);
        assert_eq!(words, "/planets are far");
    }

    // The two tools name themselves, and neither takes a place.
    #[test]
    fn the_tools_of_planning_carry_no_place() {
        let edit = edit_definition();
        assert_eq!(edit["function"]["name"], json!(EDIT_NAME));
        let wanted = edit["function"]["parameters"]["required"]
            .as_array()
            .expect("a list of what the call wants");
        assert_eq!(wanted.len(), 1, "the call wants `edits` and nothing else");
        assert_eq!(wanted[0], json!("edits"));
        let said = edit["function"]["description"].as_str().unwrap();
        assert!(said.contains("no path"), "{}", said);
        let ended = end_definition();
        assert_eq!(ended["function"]["name"], json!(END_NAME));
        assert!(ended["function"]["description"]
            .as_str()
            .unwrap()
            .contains("the user has said"));
    }

    // The prompt says what the model must do with the plan file, and how deep it
    // must go.
    #[test]
    fn the_prompt_names_the_one_way_into_the_plan() {
        let _guard = config::test_turn();
        let said = system_prompt("ab12cd34ef.md");
        assert!(said.contains("PLANNING MODE IS ON"));
        assert!(said.contains("ab12cd34ef.md"));
        assert!(said.contains(EDIT_NAME));
        assert!(said.contains("down to the smallest detail"));
        assert!(said.contains(END_NAME));
        // The plan file is named, and no other file is offered to it.
        assert!(!said.contains("edit_file"), "{}", said);
    }
}
