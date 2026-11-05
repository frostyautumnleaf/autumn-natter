// The tool that shows the contents of files to the user.
//
// The model names the files, and the program puts a block into the talk with one
// button in it. That button opens the window the chat already has for looking at
// one file whole: the same window, the same ways of closing it. Every file of the
// call stands in the window on a page of its own, and the user turns the pages
// with the two arrows, for a row of files is read one after the other.
//
// Only the kinds the window can really show are let through: words, a picture, a
// drawing of lines, a page of html, and a document of pages. An archive is
// refused, for a window of the chat is not a folder, and a kind nothing knows is
// refused too. The tool says which files it let through and which it would not,
// and a file the user may want to take away is what the offer tool is for.
//
// The window reads the file again when the user opens it, so what is shown stands
// on the machine at that moment and not at the moment the model made the call.
// The bytes are never kept in the words of the talk.
//
// The road of a file follows the state of the sandbox, as the file tools do:
// while the jail stands only the room of the chat is opened, and with the jail
// off a file of the machine may be shown. The tool is offered under the badge of
// the tools, and the drop-down of the badge holds its own line.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

use crate::tools::with_rule;

// Name the model uses to ask for files to be shown.
pub const NAME: &str = "display_files";

// The most files one call may put in the window. A model that named the whole of
// a repository was not asked to, and a page for every file of it is not a thing a
// user turns through.
pub const GREATEST_FILES: usize = 8;

// The greatest file the window reads. A file above this is named and refused: the
// window is for reading a page, and a file of that size is taken with the offer
// tool instead.
pub const GREATEST_FILE: u64 = 8 * 1024 * 1024;

// The kinds of file the window shows, as a picture of a drawing of lines.
const PICTURE_ENDINGS: &str = "png jpg jpeg gif webp bmp ico avif";
const VECTOR_ENDINGS: &str = "svg";
const PAGE_ENDINGS: &str = "html htm xhtml";
const DOCUMENT_ENDINGS: &str = "pdf";
const WORDS_ENDINGS: &str = "txt text md markdown mdx rst adoc asciidoc org tex bib csv tsv \
                             json jsonl yaml yml toml xml ini conf cfg env css scss less js mjs \
                             cjs ts tsx jsx rs py pyi c h cpp hpp cc hh go java kt kts scala \
                             swift rb php pl pm lua r dart vue svelte sql sh bash zsh fish diff \
                             patch po pot srt vtt log";

// How one file is shown: "words", "picture", "vector", "page", "document", or
// nothing at all when the window has no way of showing that kind. The word goes
// to the page with the file, so the page does not keep a list of its own.
pub fn shape(extension: &str) -> &'static str {
    let tail = extension.to_ascii_lowercase();
    let tail = tail.trim_start_matches('.');
    if tail.is_empty() {
        return "";
    }
    if has(PICTURE_ENDINGS, tail) {
        return "picture";
    }
    if has(VECTOR_ENDINGS, tail) {
        return "vector";
    }
    if has(PAGE_ENDINGS, tail) {
        return "page";
    }
    if has(DOCUMENT_ENDINGS, tail) {
        return "document";
    }
    if has(WORDS_ENDINGS, tail) {
        return "words";
    }
    ""
}

// True when the window has a way of showing this kind of file.
pub fn displayable(extension: &str) -> bool {
    !shape(extension).is_empty()
}

fn has(list: &str, tail: &str) -> bool {
    list.split(' ').any(|one| one == tail)
}

// The tool as the model is told of it. `sandbox` decides only the sentence of the
// room, as it does for the tools that work on the files.
pub fn definition(sandbox: bool) -> Value {
    let reach = if sandbox {
        "While the sandbox stands, only a file of that directory may be shown."
    } else {
        "The file may stand anywhere on this machine, and the tool runs in the program."
    };
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": with_rule(format!(
                "Show the contents of one or more files to the user in a window of the chat. The window is opened by the user: the call puts a block in the chat with one button in it, and the button opens the window over the files named here. Each file stands on a page of its own and the user turns the pages. Only these kinds are shown: words (txt, md, code, settings), a picture (png, jpg, gif, webp, bmp), a drawing of lines (svg), a page of html, and a document of pages (pdf). An archive such as a zip, a library, a program and a file of no known kind are refused, and answered with the word that they are not shown; offer such a file with offer_files instead. Call this tool ONLY when the user asked to have files displayed, or answered yes when you asked. Never call it on your own, and never call it to show your work that was not asked for. At the end of an answer that made or changed files you may ask in words whether the user wants any of them displayed, and call this tool only after the answer. {} The window reads the file when the user opens it, so a file changed afterwards is shown as it stands then. Up to eight files are taken by one call.",
                reach
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "The files to show to the user. Every name may be relative to the working directory of the chat, for example 'page.html' or 'notes/plan.md'. One name is enough; up to eight are taken."
                    },
                    "path": {
                        "type": "string",
                        "description": "One file to show, when there is only one and no array is given."
                    },
                    "note": {
                        "type": "string",
                        "description": "A short line for the block, in the words of the chat, such as 'The three files you asked to see'. Nothing is put there when it is left out."
                    }
                },
                "required": ["paths"]
            }
        }
    })
}

// One file of one call. The record of the message holds these, so the block is
// drawn again when the chat is opened another day, and in the page of the remote
// mode. `path` is the name the window asks the program for, and `shape` is how the
// window shows that file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Shown {
    pub name: String,
    pub extension: String,
    pub shape: String,
    pub path: String,
    pub size: u64,
}

// What became of one call. The block stands in the chat when `shown` holds
// anything, and the words of the receipt go back to the model either way.
pub struct Made {
    pub report: crate::tools::Report,
    // The files the window can show, in the order the model named them.
    pub shown: Vec<Shown>,
    // The words the model gave for the block, cut to one short line.
    pub note: String,
}

// Answer one call. Every file is looked for as the file tools look for a file, and
// a file the window cannot show, or that is not there, or that is bigger than the
// window reads, is reported and passed over. A call with nothing showable in it
// gives no block at all.
pub fn show(workdir: &Path, arguments: &str, sandbox: bool) -> Made {
    let value: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let note = trim_note(value.get("note").and_then(|v| v.as_str()).unwrap_or(""));
    let asked = asked_paths(&value);
    if asked.is_empty() {
        return Made {
            report: crate::tools::Report {
                ok: false,
                output: String::from("No file was named, so nothing is shown to the user"),
            },
            shown: Vec::new(),
            note,
        };
    }
    let mut shown: Vec<Shown> = Vec::new();
    let mut missed: Vec<String> = Vec::new();
    for one in asked.iter().take(GREATEST_FILES) {
        let Some(place) = place_of(workdir, one, sandbox) else {
            missed.push(format!("'{}' is not there, or is outside the chat", one));
            continue;
        };
        if !place.is_file() {
            missed.push(format!("'{}' is not a file", one));
            continue;
        }
        let name = place
            .file_name()
            .map(|one| one.to_string_lossy().to_string())
            .unwrap_or_default();
        let (_, tail) = crate::download::split_name(&name);
        if !displayable(&tail) {
            missed.push(format!(
                "'{}' is a {} file, which the window does not show",
                one,
                if tail.is_empty() {
                    String::from("file of no known kind")
                } else {
                    tail.clone()
                }
            ));
            continue;
        }
        let size = std::fs::metadata(&place).map(|m| m.len()).unwrap_or(0);
        if size > GREATEST_FILE {
            missed.push(format!(
                "'{}' is bigger than the window reads ({} MiB)",
                one,
                GREATEST_FILE / (1024 * 1024)
            ));
            continue;
        }
        shown.push(Shown {
            extension: tail.clone(),
            shape: shape(&tail).to_string(),
            path: clean_of(workdir, &place),
            name: cut_of(&name),
            size,
        });
    }
    let mut words = if shown.is_empty() {
        String::from("Nothing of what was named can be shown to the user")
    } else {
        let named: Vec<String> = shown.iter().map(full_name).collect();
        format!("Shown to the user: {}", named.join(", "))
    };
    if !missed.is_empty() {
        words.push_str(". ");
        words.push_str(&missed.join("; "));
    }
    Made {
        report: crate::tools::Report {
            ok: !shown.is_empty(),
            output: words,
        },
        shown,
        note,
    }
}

// The files the model named. The array is read first, and one name under `path`
// is taken when there is no array, as the offer tool does.
fn asked_paths(value: &Value) -> Vec<String> {
    let mut asked: Vec<String> = Vec::new();
    if let Some(list) = value.get("paths").and_then(|v| v.as_array()) {
        for one in list {
            if let Some(text) = one.as_str() {
                if !text.trim().is_empty() {
                    asked.push(text.trim().to_string());
                }
            }
        }
    }
    if asked.is_empty() {
        if let Some(text) = value.get("path").and_then(|v| v.as_str()) {
            if !text.trim().is_empty() {
                asked.push(text.trim().to_string());
            }
        }
    }
    asked
}

// The name of a file as the block shows it, with its tail behind it.
fn full_name(one: &Shown) -> String {
    if one.extension.is_empty() {
        one.name.clone()
    } else {
        format!("{}.{}", one.name, one.extension)
    }
}

// The name of a file as the block shows it: the part before the tail, cut in the
// middle when it is too long for a line.
fn cut_of(name: &str) -> String {
    let (stem, _) = crate::download::split_name(name);
    let mut held = String::new();
    for (number, ch) in stem.chars().enumerate() {
        if number >= 90 {
            held.push('\u{2026}');
            break;
        }
        held.push(ch);
    }
    held
}

// The road of a file, read as the file tools read a name. While the jail stands
// the room of the chat is the only place opened; with the jail off a name of the
// machine is taken as it is given.
fn place_of(workdir: &Path, asked: &str, sandbox: bool) -> Option<std::path::PathBuf> {
    crate::editfile::place_kept(workdir, asked, sandbox)
        .ok()
        .filter(|place| place.is_file())
}

// The name the window asks for. A file of the room of the chat is named from the
// room, as the offer tool names it; a file outside the room keeps the whole road.
fn clean_of(workdir: &Path, place: &Path) -> String {
    let room = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf());
    match place.strip_prefix(&room) {
        Ok(tail) => {
            let words = tail.to_string_lossy().replace('\\', "/");
            if words.is_empty() {
                place.to_string_lossy().replace('\\', "/")
            } else {
                words
            }
        }
        Err(_) => place.to_string_lossy().replace('\\', "/"),
    }
}

// The first line of what the model wrote for the block.
fn trim_note(words: &str) -> String {
    let line = words.trim().lines().next().unwrap_or("").trim();
    let mut held = String::new();
    for (number, ch) in line.chars().enumerate() {
        if number >= 120 {
            held.push('\u{2026}');
            break;
        }
        held.push(ch);
    }
    held
}

// True of the name of this tool, so the engine knows the call is one of the tools
// the program does itself.
pub fn is_tool(name: &str) -> bool {
    name == NAME
}

// The head of the block of a call that is still being written: the first file the
// model named, and the count of the rest when it named more than one.
pub fn partial_words(arguments: &str) -> String {
    let value: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let asked = asked_paths(&value);
    match asked.len() {
        0 => String::new(),
        1 => last_of(&asked[0]),
        held => format!("{} +{}", last_of(&asked[0]), held - 1),
    }
}

// The last part of a name the model gave, for the head of a block.
fn last_of(asked: &str) -> String {
    asked.replace('\\', "/").rsplit('/').next().unwrap_or("").to_string()
}

// The bytes of one file of a block, for the window. The name is read the same way
// the tool read it, and the same rule of the room lies over it: while the jail
// stands only the room of the chat is opened, and with the jail off a name of the
// machine is taken as it is given. A kind the window does not show is refused
// here too, so a name cannot be used to read a file the tool would not show.
pub fn bytes_of(
    workdir: &Path,
    asked: &str,
    sandbox: bool,
) -> Option<(String, String, Vec<u8>)> {
    let place = place_of(workdir, asked, sandbox)?;
    let name = place.file_name()?.to_string_lossy().to_string();
    let (_, tail) = crate::download::split_name(&name);
    if !displayable(&tail) {
        return None;
    }
    let size = std::fs::metadata(&place).map(|m| m.len()).unwrap_or(0);
    if size > GREATEST_FILE {
        return None;
    }
    let bytes = std::fs::read(&place).ok()?;
    Some((name, crate::download::mime(&tail).to_string(), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room_of(name: &str) -> std::path::PathBuf {
        let place = std::env::temp_dir().join(format!(
            "autumn-natter-display-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&place);
        std::fs::create_dir_all(&place).expect("the room of the test could not be made");
        place
    }

    // The window shows five kinds and nothing else. An archive and a kind nothing
    // knows are turned away, which is what the tool tells the model.
    #[test]
    fn the_kinds_of_the_window_are_known() {
        assert_eq!(shape("md"), "words");
        assert_eq!(shape("rs"), "words");
        assert_eq!(shape("png"), "picture");
        assert_eq!(shape("svg"), "vector");
        assert_eq!(shape("html"), "page");
        assert_eq!(shape("pdf"), "document");
        assert_eq!(shape("zip"), "");
        assert_eq!(shape("tar"), "");
        assert_eq!(shape(""), "");
        assert!(displayable("MD"), "a tail is asked without its case");
        assert!(!displayable("zip"));
    }

    // The files of the call are put in the window in the order they were named,
    // and the file the window cannot show is told to the model.
    #[test]
    fn the_files_of_a_call_are_shown_and_the_rest_are_told() {
        let room = room_of("call");
        std::fs::write(room.join("page.html"), "<p>here</p>").expect("the page was written");
        std::fs::create_dir_all(room.join("notes")).expect("the folder was made");
        std::fs::write(room.join("notes/plan.md"), "# Plan").expect("the plan was written");
        std::fs::write(room.join("work.zip"), "not a file to show")
            .expect("the archive was written");
        let made = show(
            &room,
            r#"{"paths":["page.html","notes/plan.md","work.zip","gone.txt"]}"#,
            true,
        );
        assert!(made.report.ok, "{}", made.report.output);
        assert_eq!(made.shown.len(), 2, "{}", made.report.output);
        assert_eq!(made.shown[0].path, "page.html");
        assert_eq!(made.shown[0].shape, "page");
        assert_eq!(made.shown[1].path, "notes/plan.md");
        assert_eq!(made.shown[1].extension, "md");
        assert!(made.report.output.contains("zip"), "{}", made.report.output);
        assert!(made.report.output.contains("gone.txt"), "{}", made.report.output);
        let _ = std::fs::remove_dir_all(&room);
    }

    // The room of the chat is the only place opened while the jail stands. With
    // the jail off a file of the machine is shown, which is the rule the file
    // tools work to as well.
    #[test]
    fn the_room_of_the_files_follows_the_state_of_the_sandbox() {
        let room = room_of("jail");
        let beside = std::env::temp_dir().join(format!(
            "autumn-natter-display-beside-{}.md",
            std::process::id()
        ));
        std::fs::write(&beside, "a file of the machine").expect("the file was written");
        let kept = show(&room, r#"{"paths":["../outside.md"]}"#, true);
        assert!(!kept.report.ok, "a name out of the room was shown with the jail on");
        let whole = format!(r#"{{"paths":["{}"]}}"#, beside.display());
        let let_in = show(&room, &whole, false);
        assert!(let_in.report.ok, "{}", let_in.report.output);
        assert_eq!(let_in.shown.len(), 1);
        assert!(
            let_in.shown[0].path.ends_with(".md"),
            "the whole road of the file was lost: {}",
            let_in.shown[0].path
        );
        let _ = std::fs::remove_dir_all(&room);
        let _ = std::fs::remove_file(&beside);
    }

    // The window asks for the bytes by the name the block carries, and the same
    // rule of the room lies over the asking. A kind the window does not show is
    // refused there too.
    #[test]
    fn the_bytes_of_a_block_come_by_the_name_of_the_block() {
        let room = room_of("bytes");
        std::fs::write(room.join("read.txt"), "the words").expect("the file was written");
        std::fs::write(room.join("book.zip"), "no").expect("the archive was written");
        let made = bytes_of(&room, "read.txt", true).expect("the words of the file");
        assert_eq!(made.0, "read.txt");
        assert_eq!(made.2, b"the words".to_vec());
        assert!(bytes_of(&room, "book.zip", true).is_none(), "an archive was read");
        assert!(bytes_of(&room, "../gone.me", true).is_none());
        let _ = std::fs::remove_dir_all(&room);
    }

    // The tool is told of the two rules the model has to keep: it shows nothing
    // the user did not ask for, and it asks in words before it shows.
    #[test]
    fn the_tool_says_when_it_may_be_called() {
        let made = definition(true);
        let call = made.get("function").expect("the tool is a function");
        assert_eq!(call.get("name").and_then(|v| v.as_str()), Some(NAME));
        let words = call
            .get("description")
            .and_then(|v| v.as_str())
            .expect("the tool is described");
        assert!(words.contains("ONLY when the user asked"), "the tool does not say it waits for the user to ask");
        assert!(words.contains("ask in words"), "the tool does not say to ask at the end");
        assert!(words.contains("sandbox"), "the room of the files is not said");
        let down = definition(false);
        let said = down
            .get("function")
            .and_then(|f| f.get("description"))
            .and_then(|v| v.as_str())
            .expect("the tool is described");
        assert!(said.contains("anywhere on this machine"), "the reach of the tool does not follow the sandbox");
        assert!(!said.contains("While the sandbox stands"), "the jail is named with the jail off");
    }
}
