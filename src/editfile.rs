// The two file tools. One changes a file of the chat, and the other makes a
// new one there.
//
// Both tools work in the program itself. They do not start a shell, so they do
// not need bubblewrap, and a chat that cannot get the jail is not left without
// file tools. They are kept to the room of the chat, which is the one room the
// jail of the terminal tool may be written in: the name of the file is joined
// onto that room, a road that goes up out of a folder is refused, links are
// followed before the place is looked at, and the records of the attachments of
// the chat are kept out of both tools. A chat with the sandbox on and a chat
// with it off therefore reach the same files, and no more.
//
// The changes of one call are all matched against the file as it stood before
// the call, and are written together or not at all. A batch that is part
// written is a worse thing than a batch refused, for the model cannot work out
// which half landed.
//
// Nothing of this is a place holder. No part of a call asks for a mark that
// stands for other words, and no words are put behind the answer to tell the
// window what was made.

use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::attach;
use crate::tools::{self, with_rule};

// The name the model uses to change a file, and the name it uses to make one.
pub const EDIT_NAME: &str = "edit_file";
pub const CREATE_NAME: &str = "create_file";

// The longest file either tool reads or writes. The whole file is read to find
// the places to change, so the cap stands on the file and not on a change.
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

// The most changes one call may ask for. Every one of them costs a look through
// the whole file, so a batch of a few thousand would hold the program for
// minutes over one call.
const MAX_EDITS: usize = 100;

// The most places one change marked for every place may open. The limit on the
// changes of a call says how many words are looked for, not how many places any
// of them stands in, so one change of a short mark could fill the room alone.
const MAX_MATCHES: usize = 10_000;

// The bounds of the difference the receipt shows. The receipt is a word to the
// model about what changed, and not a copy of the file: the difference is cut
// to a window about the first change, and the window is cut again at a few
// bounds, so a file of a million lines costs the same as one of ten.
const DIFF_WINDOW_LINES: usize = 120;
const DIFF_CONTEXT: usize = 2;
const DIFF_LINES: usize = 40;
const DIFF_LINE_CHARS: usize = 200;
const DIFF_CHARS: usize = 4000;
// The most lines the window may hold. The difference is worked out by a table
// the size of the window times itself, so a window of a million lines would
// hold the program for minutes. The change stands in the front of the window,
// so a cap this high keeps it whole for every change the file tools may make.
const MAX_DIFF_WINDOW_LINES: usize = 2000;

// The front parts of a name a model brings over from another program, where its
// files stood under a folder of that name.
const HABIT_PREFIXES: [&str; 3] = ["/mnt/data", "/mnt/outputs", "/workspace"];

// The call that changes a file. Every change is asked for as the words that
// stand in the file now, so nothing here needs a place holder of any kind.
pub fn edit_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": EDIT_NAME,
            "description": with_rule(String::from(
                "Change a file in the chat by replacing exact text with other exact text. Prefer this over writing a whole file again with the terminal tool, because only the changed part is sent. Copy each old_string exactly from the file, spacing included. Put every change to one file in the same call, not in several calls. Every old_string is matched against the file as it was before this call, and no two of them may cover the same text. Each must match one place, unless replace_all is set for it. If one matches no place, or more than one, nothing at all is written. An old_string is never empty: use create_file for a new file. The file is in the working directory of the chat, which is the only place this tool works, and the tool runs in the program, so it works the same whether the sandbox is on or off. A call that succeeded means the file now holds what was sent, so read it again only when you must.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file to change. The name may be relative to the working directory of the chat, for example 'notes.txt' or 'src/parse.rs', or a whole name from the root of the machine. While the sandbox stands, only the working directory of the chat may be worked on; with the sandbox off any file of this machine may be."
                    },
                    "edits": {
                        "type": "array",
                        "description": "The changes to apply in one call. Each change gives the text to replace and the text to put in its place.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_string": {
                                    "type": "string",
                                    "description": "The text to replace, copied from the file. Never empty."
                                },
                                "new_string": {
                                    "type": "string",
                                    "description": "The text to put in its place."
                                },
                                "replace_all": {
                                    "type": "boolean",
                                    "description": "Replace every place this text appears, instead of requiring that it appears once. Off when it is not given."
                                }
                            },
                            "required": ["old_string", "new_string"]
                        }
                    }
                },
                "required": ["path", "edits"]
            }
        }
    })
}

// The call that makes a new file. The whole file is given at once, so there is
// no part of it that stands for something else.
pub fn create_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": CREATE_NAME,
            "description": with_rule(String::from(
                "Create a new file in the working directory of the chat, with the text given for it. This tool never overwrites a file and never empties one: if a file of that name already stands with words in it, the call fails, the file is left exactly as it was, and edit_file is the tool that changes it. So look first whether the file is there, and when a file that exists is wanted changed rather than made, call edit_file instead. The folders on the way to the file are created. The whole file is written at once, so text that grows in steps is better added with edit_file. The tool runs in the program, so it works the same whether the sandbox is on or off.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file to create. The name may be relative to the working directory of the chat, for example 'notes.txt' or 'src/parse.rs', or a whole name from the root of the machine. While the sandbox stands, only the working directory of the chat may be worked on; with the sandbox off any file of this machine may be. A file that already exists is never overwritten and never emptied: such a call fails and the file stands unchanged."
                    },
                    "text": {
                        "type": "string",
                        "description": "The whole content of the new file. An empty string makes an empty file."
                    }
                },
                "required": ["path", "text"]
            }
        }
    })
}

// The part of a call that holds the name of the file, under the few names a
// model may have chosen for it, in the order they are worth looking at. The
// same few names are read wherever the name of the file is wanted, so a head
// does not lose a file because the model spelled its part another way.
const PATH_KEYS: [&str; 3] = ["path", "file_path", "file"];

// The name of the file a call asks for, as far as it has come in. Nothing is
// made up of words that have not come in yet.
pub fn partial_words(raw: &str) -> String {
    let text = raw.trim_start();
    for key in PATH_KEYS {
        let words = tools::partial_text(text, key);
        if !words.is_empty() {
            return words;
        }
    }
    String::new()
}

// The name of the file of a call that is finished. The user sees this in the
// head of the block, so the whole name is worth having.
pub fn file_of(raw: &str) -> String {
    match object(raw) {
        Some(call) => first_of(&call, &PATH_KEYS).unwrap_or_default(),
        None => partial_words(raw),
    }
}

// The words of a call that are still being written, as the user is given
// them. The user sees the words of the call come in, as the user watches a
// command: the head names the file the call works on, and the body holds the
// words the call is making. Nothing is made up of words that have not come in
// yet.
pub fn live_params(name: &str, raw: &str) -> (String, String) {
    match name {
        CREATE_NAME => (tools::partial_text(raw.trim_start(), "text"), file_of(raw)),
        EDIT_NAME => (edit_params(raw), file_of(raw)),
        _ => (String::new(), String::new()),
    }
}

// The words of the changes of a call that is still being written, in the order
// they stand. Each change is shown as the words that go out and the words that
// come in, one line of a mark between the two, so the user sees the change as
// it is made.
//
// The two halves of a change are read side by side and not one inside the other,
// for a model writes them in the order it likes. One that puts the words to come
// in first is read as it comes: the mark of the change stands over those words
// until the words to take out arrive, and the user is not held looking at an
// empty block while the call is written. A change whose two halves are both
// still nothing is left out, for a mark over nothing says nothing. The words to
// take out are only worth a line when they are really words, since an empty
// mark of a change is a call that will be refused anyway.
fn edit_params(raw: &str) -> String {
    let text = raw.trim_start();
    let olds = all_values(text, "old_string");
    let news = all_values(text, "new_string");
    let mut out = String::new();
    // The longer of the two lists says how many changes have been begun, for a
    // model may have written either half of the last one first.
    for number in 0..olds.len().max(news.len()) {
        let old = olds.get(number).filter(|one| !one.trim().is_empty());
        // The words to put in count as come in when the model has written the
        // mark that opens them, so the mark of a change shows the user that
        // the words behind it are on their way.
        let new = news.get(number);
        if old.is_none() && new.is_none() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        if let Some(words) = old {
            out.push_str(words);
        }
        if let Some(words) = new {
            if old.is_some() {
                out.push('\n');
            }
            out.push_str("> ");
            out.push_str(words);
        }
    }
    out
}

// Every value of one named part of an object that may not be finished, in the
// order they stand. The values are the words between the marks of a quote, and
// the marks of an escape are read as the character they stand for. A value that
// is not closed yet gives what has come, and the look stops there, for what
// comes after it cannot be read.
fn all_values(text: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{}\"", key);
    let mut out: Vec<String> = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find(&needle) {
        let abs_at = search_from + rel;
        // The value stands after the mark of a colon, and the mark that opens
        // it is the first mark of a quote after it.
        let rest = &text[abs_at + needle.len()..];
        let Some(colon) = rest.find(':') else {
            break;
        };
        let after_colon = &rest[colon + 1..];
        let Some(open) = after_colon.find('"') else {
            break;
        };
        let open_pos = abs_at + needle.len() + colon + 1 + open;
        let (value, next, closed) = value_at(text, open_pos);
        out.push(value);
        if !closed {
            break;
        }
        search_from = next;
    }
    out
}

// The words of a value that stands at `open`, the position of the mark that
// opens it, read to the mark that closes it. The answer is the words, the
// position after the closing mark, and whether the value is closed. A value of
// nothing at all, and a mark of an escape with no word behind it, give what has
// come and say the value is not closed.
fn value_at(text: &str, open: usize) -> (String, usize, bool) {
    let mut out = String::new();
    let mut byte_pos = open;
    let mut closed = false;
    let mut chars = text[open..].chars();
    let _ = chars.next();
    byte_pos += 1;
    while let Some(ch) = chars.next() {
        if ch == '"' {
            closed = true;
            byte_pos += 1;
            break;
        }
        if ch != '\\' {
            out.push(ch);
            byte_pos += ch.len_utf8();
            continue;
        }
        let Some(next) = chars.next() else {
            break;
        };
        byte_pos += 1 + next.len_utf8();
        match next {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' | 'f' => {}
            'u' => {
                let mut hex = String::new();
                for _ in 0..4 {
                    match chars.next() {
                        Some(digit) if digit.is_ascii_hexdigit() => {
                            hex.push(digit);
                            byte_pos += 1;
                        }
                        _ => return (out, byte_pos, false),
                    }
                }
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(letter) => out.push(letter),
                    None => return (out, byte_pos, false),
                }
            }
            other => out.push(other),
        }
    }
    (out, byte_pos, closed)
}

// One change of a call.
struct Change {
    before: String,
    after: String,
    everywhere: bool,
}

// What one call of the edit tool did.
#[derive(Debug)]
struct Made {
    // The whole file as it is now.
    after: String,
    // How many places were changed.
    count: usize,
    // Where in the file the first change stands, counted in bytes.
    first: usize,
    // A few words of what went out, and of what came in.
    taken: String,
    put: String,
    // How many lines of the file were written, and how many were taken away,
    // over the whole of the call. The two stand in the head of the block of the
    // run, for a change of a file is judged by what it did to the lines of it.
    added: usize,
    removed: usize,
}

// The words of a file, and the two marks that say how it stood: the marks of
// its lines, and the mark of a byte order at its front.
struct Standing {
    words: String,
    newline: String,
    bom: String,
    // The file as it was read. The words are put again only when the file has
    // not changed while they were being worked on.
    bytes: Vec<u8>,
}

// The words of a call, made into an object. A model that put the object inside a
// fenced block is read through the fence.
fn object(raw: &str) -> Option<Value> {
    let text = raw.trim();
    let cut = tools::extract_object(text).unwrap_or(text);
    match serde_json::from_str::<Value>(cut) {
        Ok(value) if value.is_object() => Some(value),
        _ => None,
    }
}

// The first of a few names the model may have used for one part of a call, in
// the order they are worth looking at. A part with no words in it is not an
// answer, so it is passed over.
fn first_of(call: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = call.get(*key).and_then(|v| v.as_str()) {
            if !text.trim().is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

// The room of the chat, followed through its links.
fn room(workdir: &Path) -> PathBuf {
    workdir.canonicalize().unwrap_or_else(|_| workdir.to_path_buf())
}

// True when a place stands inside a room. Both of them are places that have
// already been followed through their links.
fn under(the_room: &Path, place: &Path) -> bool {
    place.starts_with(the_room)
}

// The front parts of a name taken off, when the name comes from another program
// and its files stood under a folder of one of those names. The rest of the name
// is the name of the file here.
fn habit(name: &str) -> &str {
    for prefix in HABIT_PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix) {
            if rest.is_empty() || rest.starts_with('/') {
                return rest.trim_start_matches('/');
            }
        }
    }
    name
}

// True when a name leads into the folder the records of the attachments of the
// chat stand in. Those files belong to the chat and to the user, and a tool
// has no business with them.
fn in_shelf(path: &Path) -> bool {
    let wanted = Component::Normal(std::ffi::OsStr::new(attach::FOLDER));
    path.components().any(|part| part == wanted)
}

// The longest front part of a place that stands on the disk, followed through
// its links. The parts of the name that are not there yet are put back after
// it, so a file that is to be made is judged by the room it will stand in.
fn as_far_as_stands(path: &Path) -> PathBuf {
    let mut behind: Vec<std::ffi::OsString> = Vec::new();
    let mut walked = path.to_path_buf();
    while !walked.as_os_str().is_empty() {
        if let Ok(real) = walked.canonicalize() {
            let mut out = real;
            for part in behind.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match walked.file_name() {
            Some(part) => behind.push(part.to_os_string()),
            None => break,
        }
        if !walked.pop() {
            break;
        }
    }
    path.to_path_buf()
}

// The name of a file of the chat. A name with a road that goes up out of a
// folder is refused, and so is a name that leads into the records of the
// attachments. The place that comes back has been followed through its links,
// so it cannot be led out of the room by a link planted inside it.
pub fn place_in(workdir: &Path, raw: &str) -> Result<PathBuf, String> {
    place_kept(workdir, raw, true)
}

// The same, with the wall made known. A chat that stands in the jail works in
// the room of the chat and nowhere else, for that room is the one room the jail
// is written in. A chat with the jail off reaches the machine as a command of
// the terminal tool does, so a name that stands outside the room of the chat is
// taken as it is given: absolute, or relative to the room, with the road
// through '..' among the parts of it. The store of the files the user hung on
// the chat is kept out of reach either way, for those are nobody's to touch but
// the user's.
pub fn place_kept(workdir: &Path, raw: &str, in_room: bool) -> Result<PathBuf, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(String::from("The name of the file is required"));
    }
    let the_room = room(workdir);
    let cut = habit(name);
    let joined = if Path::new(cut).is_absolute() {
        PathBuf::from(cut)
    } else {
        the_room.join(cut)
    };
    if in_room {
        for part in joined.components() {
            if part == Component::ParentDir {
                return Err(format!(
                    "'{}' points outside the working directory of the chat. While the sandbox stands, that directory is the only place the file tools work, so give a name relative to it, as '{}'.",
                    name,
                    last(&joined)
                ));
            }
        }
    }
    if in_shelf(&joined) {
        return Err(format!(
            "'{}' points into the attachment store of the chat. Tool calls cannot read or write those files.",
            name
        ));
    }
    let checked = if in_room {
        as_far_as_stands(&joined)
    } else {
        neat(&joined)
    };
    if in_room && !under(&the_room, &checked) {
        return Err(format!(
            "'{}' is outside the working directory of the chat, which while the sandbox stands is the only place the file tools work. Give a name relative to it, as '{}', or turn the sandbox off to work on the machine as a command does.",
            name,
            last(&joined)
        ));
    }
    Ok(checked)
}

// A name of a file made neat -- without the parts that stand for the folder
// itself, and without the parts that go back a step -- but nothing asked of the
// machine: a file that is not there yet is a name all the same.
fn neat(path: &Path) -> PathBuf {
    let mut made = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                made.pop();
            }
            other => made.push(other.as_os_str()),
        }
    }
    made
}

// The room the file tools stand in. With the wall up it is the room of the chat;
// without it, the whole machine, which is what a name that is not kept to a
// room leaves.
fn room_for(workdir: &Path, in_room: bool) -> PathBuf {
    if in_room {
        return room(workdir);
    }
    PathBuf::from(std::path::MAIN_SEPARATOR_STR)
}

// The name of a file, for a word to the model.
fn last(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

// The words of a file, made to plain new lines. The marks of how its lines ended
// and the mark of a byte order at its front are kept, so the file is written
// back as it stood.
fn read_words(place: &Path) -> Result<Standing, String> {
    let name = last(place);
    let bytes = match std::fs::read(place) {
        Ok(made) => made,
        Err(_) => {
            return Err(format!(
                "'{}' does not exist. Create it with create_file, or list the working directory of the chat to see what is there.",
                name
            ))
        }
    };
    if bytes.len() > MAX_FILE_BYTES {
        return Err(format!(
            "'{}' is larger than the {} bytes a file tool may read.",
            name, MAX_FILE_BYTES
        ));
    }
    let mut text = match String::from_utf8(bytes.clone()) {
        Ok(words) => words,
        Err(_) => return Err(format!("'{}' is not text, so a file tool cannot change it.", name)),
    };
    let mut bom = String::new();
    if let Some(rest) = text.strip_prefix('\u{feff}') {
        bom = String::from('\u{feff}');
        text = rest.to_string();
    }
    if text.contains('\0') {
        return Err(format!("'{}' is a binary file, not text, so a file tool cannot change it.", name));
    }
    let lines = text.matches('\n').count();
    let ends = text.matches("\r\n").count();
    let newline = if ends > 0 && ends * 2 >= lines { "\r\n" } else { "\n" };
    Ok(Standing {
        words: text.replace("\r\n", "\n"),
        newline: newline.to_string(),
        bom,
        bytes,
    })
}

// A place beside a file, for a new version of it. The name holds the id of this
// running program and a count, so two writes never take one place.
fn side_file(place: &Path) -> PathBuf {
    static TURN: AtomicUsize = AtomicUsize::new(0);
    let name = last(place);
    let mark = format!(
        ".{}.natter-{:x}-{}",
        name,
        std::process::id(),
        TURN.fetch_add(1, Ordering::SeqCst)
    );
    place.with_file_name(mark)
}

// The marks of a file are put back on its words, so an edit of a file that ends
// its lines the Windows way leaves them that way.
fn payload(standing: &Standing) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    if !standing.bom.is_empty() {
        out.extend_from_slice(standing.bom.as_bytes());
    }
    if standing.newline == "\r\n" {
        for (number, part) in standing.words.split('\n').enumerate() {
            if number > 0 {
                out.extend_from_slice(b"\r\n");
            }
            out.extend_from_slice(part.as_bytes());
        }
    } else {
        out.extend_from_slice(standing.words.as_bytes());
    }
    out
}

// The marks a file stood with, kept for the new version of it, so an edit does
// not take the executable mark away.
#[cfg(unix)]
fn keep_mode(from: &Path, to: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(from) {
        let mode = meta.permissions().mode();
        let _ = std::fs::set_permissions(to, std::fs::Permissions::from_mode(mode));
    }
}

#[cfg(not(unix))]
fn keep_mode(_from: &Path, _to: &Path) {}

// The first bytes of a file, and no more than one more than were asked for. A
// file that grew is told apart by that one byte.
fn head_bytes(place: &Path, want: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(place).ok()?;
    let mut out: Vec<u8> = Vec::new();
    file.take((want + 1) as u64)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

// Write the words of a file again. The new version is made beside the old one
// and then put in its place, so a write that does not finish never leaves a file
// half changed. Three things are looked at just before the place is taken: that
// the file is still in the room of the chat, that it is still the file the
// changes were worked out of, and that nothing of the road was changed for a
// link while the words were being made ready.
fn write_words(the_room: &Path, place: &Path, standing: &Standing, as_it_was: &[u8]) -> Result<(), String> {
    let name = last(place);
    let made = payload(standing);
    if made.len() > MAX_FILE_BYTES {
        return Err(format!(
            "the new '{}' is larger than the {} bytes a file tool may write.",
            name, MAX_FILE_BYTES
        ));
    }
    if let Some(parent) = place.parent() {
        if let Err(problem) = std::fs::create_dir_all(parent) {
            return Err(format!("the folder for '{}' could not be created: {}", name, problem));
        }
    }
    let side = side_file(place);
    if let Err(problem) = std::fs::write(&side, &made) {
        return Err(format!("'{}' could not be written: {}", name, problem));
    }
    keep_mode(place, &side);
    let gone = |problem: String| -> String {
        let _ = std::fs::remove_file(&side);
        problem
    };
    if !under(the_room, &as_far_as_stands(place)) {
        return Err(gone(format!(
            "'{}' moved outside the working directory of the chat while this call was prepared, so nothing was written.",
            name
        )));
    }
    if let Some(now) = head_bytes(place, as_it_was.len()) {
        if now != as_it_was {
            return Err(gone(format!(
                "'{}' changed while this call was prepared, so nothing was written. Read the file again and apply the changes to the text it holds now.",
                name
            )));
        }
    }
    if let Err(problem) = std::fs::rename(&side, place) {
        return Err(gone(format!("'{}' could not be replaced: {}", name, problem)));
    }
    Ok(())
}

// `replace_all`, read strictly. A model writes the word false between quotes as
// often as not, and reading that word as it were true would change every place
// in the file when the model asked for one place.
fn strict_bool(value: Option<&Value>) -> Option<bool> {
    match value {
        None => Some(false),
        Some(Value::Null) => Some(false),
        Some(Value::Bool(on)) => Some(*on),
        Some(Value::String(word)) => {
            let word = word.trim().to_ascii_lowercase();
            match word.as_str() {
                "true" | "1" | "yes" => Some(true),
                "false" | "0" | "no" | "" => Some(false),
                _ => None,
            }
        }
        Some(Value::Number(n)) => n.as_i64().map(|number| number != 0),
        Some(_) => None,
    }
}

// The changes of a call, read and made ready. Every one of them is looked at
// before anything is written.
fn parse_changes(call: &Value) -> Result<Vec<Change>, String> {
    let items = call
        .get("edits")
        .and_then(|v| v.as_array())
        .ok_or_else(|| String::from(
            "'edits' is required, as a list of {old_string, new_string}. Put every change to this file in that list.",
        ))?;
    if items.is_empty() {
        return Err(String::from(
            "'edits' is empty. Say which text of the file is to be changed.",
        ));
    }
    if items.len() > MAX_EDITS {
        return Err(format!(
            "{} changes in one call is over the limit of {}. Nothing was written. Send them in groups of {} at a time, and write one group before the next.",
            items.len(),
            MAX_EDITS,
            MAX_EDITS
        ));
    }
    let mut out: Vec<Change> = Vec::new();
    for (number, item) in items.iter().enumerate() {
        let index = number + 1;
        let entry = item.as_object().ok_or_else(|| {
            format!("change {} is not an object with old_string and new_string in it", index)
        })?;
        let before = entry.get("old_string").and_then(|v| v.as_str()).ok_or_else(|| {
            format!("change {} needs old_string and new_string to both be text", index)
        })?;
        let after = entry.get("new_string").and_then(|v| v.as_str()).ok_or_else(|| {
            format!("change {} needs old_string and new_string to both be text", index)
        })?;
        let everywhere = strict_bool(entry.get("replace_all")).ok_or_else(|| {
            format!("change {} needs replace_all to be true or false", index)
        })?;
        if before.is_empty() {
            return Err(format!(
                "change {} has no text to replace. An empty old_string does not create a file: use create_file for that.",
                index
            ));
        }
        out.push(Change {
            before: before.replace("\r\n", "\n"),
            after: after.replace("\r\n", "\n"),
            everywhere,
        });
    }
    Ok(out)
}

// How many places words stand in a text. Words that stand in one another are
// not counted twice.
fn count_of(text: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut made = 0usize;
    let mut from = 0usize;
    while let Some(at) = text[from..].find(needle) {
        made += 1;
        from += at + needle.len();
    }
    made
}

// Every change of the call, worked out against the words as they stood before
// the call. The places are found first, and two of them that cover the same
// words are a fault of the call. The changes are then made from the back of the
// file forward, so the places before them keep their numbers.
fn apply_all(before: &str, changes: &[Change], name: &str) -> Result<Made, String> {
    // Where a change stands, what it puts there, and which change it was.
    let mut places: Vec<(usize, usize, &str, usize)> = Vec::new();
    for (number, change) in changes.iter().enumerate() {
        let index = number + 1;
        let found = count_of(before, &change.before);
        if found == 0 {
            return Err(format!(
                "change {} found no text to replace in '{}'. The text must match the file exactly, spacing included. Read the file and copy the text from it.",
                index, name
            ));
        }
        if found > 1 && !change.everywhere {
            return Err(format!(
                "change {} matches {} places in '{}'. Include more of the surrounding lines, so that it matches one place, or set replace_all on that change to change all {} of them.",
                index, found, name, found
            ));
        }
        // One change over the whole file needs no places worked out at all:
        // nothing else is there to cover the same words with it.
        if change.everywhere && changes.len() == 1 {
            let first = before.find(&change.before).unwrap_or(0);
            let out = lines_in(&change.before);
            let written = lines_in(&change.after);
            return Ok(Made {
                after: before.replace(&change.before, &change.after),
                count: found,
                first,
                taken: change.before.clone(),
                put: change.after.clone(),
                // Every place of the file took the same change, so the lines of
                // one place are the lines of all of them.
                added: written * found,
                removed: out * found,
            });
        }
        if change.everywhere && found > MAX_MATCHES {
            return Err(format!(
                "change {} matches {} places of '{}', over the limit of {} for one change in a batch. Nothing was written. Send it in a call of its own, or use a longer match.",
                index, found, name, MAX_MATCHES
            ));
        }
        let mut at = before.find(&change.before);
        while let Some(place) = at {
            places.push((place, place + change.before.len(), change.after.as_str(), index));
            if !change.everywhere {
                break;
            }
            let rest = place + change.before.len();
            at = before[rest..].find(&change.before).map(|later| rest + later);
        }
    }
    places.sort_by_key(|place| place.0);
    for (one, next) in places.iter().zip(places[1..].iter()) {
        if next.0 < one.1 {
            return Err(format!(
                "change {} and change {} cover the same text of '{}'. Every old_string is matched against the file as it was before this call, so two changes cannot cover one place. Put the two of them in one change.",
                one.3, next.3, name
            ));
        }
    }
    let mut out = String::with_capacity(before.len());
    let mut cursor = 0usize;
    let mut added = 0usize;
    let mut removed = 0usize;
    for (start, end, words, _) in &places {
        out.push_str(&before[cursor..*start]);
        out.push_str(words);
        added += lines_in(words);
        removed += lines_in(&before[*start..*end]);
        cursor = *end;
    }
    out.push_str(&before[cursor..]);
    let (start, end, _, index) = places[0];
    let _ = index;
    Ok(Made {
        after: out,
        count: places.len(),
        first: start,
        taken: before[start..end].to_string(),
        put: places[0].2.to_string(),
        added,
        removed,
    })
}

// How many lines a stretch of a file holds, counted the way the lines of a file
// are counted: a mark of a new line closes the line it stands on and does not
// open another. Nothing at all is no lines.
fn lines_in(words: &str) -> usize {
    if words.is_empty() {
        return 0;
    }
    words.matches('\n').count() + if words.ends_with('\n') { 0 } else { 1 }
}

// How many lines words make, said the way a line of the answer says it. A file
// of nothing at all has no line in it.
fn line_words(lines: usize) -> String {
    if lines == 1 {
        return String::from("one line");
    }
    format!("{} lines", lines)
}

// The offsets of a window of `lines` lines either side of `index`, in a text.
// A window that runs past the front of the text starts at the front, and one
// that runs past the back of it ends at the back.
fn diff_window(text: &str, index: usize, lines: usize) -> (usize, usize) {
    let mut start = index;
    for _ in 0..lines {
        match text[..start].rfind('\n') {
            Some(newline) => start = newline,
            None => {
                start = 0;
                break;
            }
        }
    }
    if start > 0 && text.as_bytes()[start] == b'\n' {
        start += 1;
    }
    let mut end = index;
    for _ in 0..lines {
        match text[end..].find('\n') {
            Some(newline) => end += newline + 1,
            None => {
                end = text.len();
                break;
            }
        }
    }
    (start, end.max(index))
}

// The lines of a unified difference between two texts, with a few lines of
// context about each change. The two texts are cut to a window already, so the
// table of the difference stays small. The line numbers are given against the
// whole file, through `offset`, which is the line of the front of the window
// minus one, so the difference points at the file and not at the window. A run
// of one line is numbered without a count, as a difference of the usual kind
// is.
fn difference(before: &[&str], after: &[&str], context: usize, offset: usize) -> Vec<String> {
    let n = before.len();
    let m = after.len();
    // The table of the longest common run, read from the back of both texts.
    let width = m + 1;
    let mut table = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        let row = i * width;
        let next_row = (i + 1) * width;
        for j in (0..m).rev() {
            let value = if before[i] == after[j] {
                table[next_row + j + 1] + 1
            } else {
                table[next_row + j].max(table[row + j + 1])
            };
            table[row + j] = value;
        }
    }
    // Each line as it stands in the two texts: a line that stands in both is
    // context, a line that stands in the first alone is taken out, and a line
    // that stands in the second alone is put in.
    let mut marks: Vec<(char, &str)> = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if before[i] == after[j] {
            marks.push((' ', before[i]));
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            marks.push(('-', before[i]));
            i += 1;
        } else {
            marks.push(('+', after[j]));
            j += 1;
        }
    }
    while i < n {
        marks.push(('-', before[i]));
        i += 1;
    }
    while j < m {
        marks.push(('+', after[j]));
        j += 1;
    }
    // The line of the first text each mark stands on, and of the second, given
    // against the whole file. A mark that stands in one text alone has no line
    // of its own in the other, and stands after the line before it there.
    let mut old_line = vec![0usize; marks.len()];
    let mut new_line = vec![0usize; marks.len()];
    let (mut o, mut nw) = (1usize, 1usize);
    for k in 0..marks.len() {
        old_line[k] = o + offset;
        new_line[k] = nw + offset;
        if marks[k].0 != '+' {
            o += 1;
        }
        if marks[k].0 != '-' {
            nw += 1;
        }
    }
    // The lines a user needs to see: each change, and the context about it.
    let mut want = vec![false; marks.len()];
    for (k, (mark, _)) in marks.iter().enumerate() {
        if *mark == ' ' {
            continue;
        }
        let lo = k.saturating_sub(context);
        let hi = (k + context + 1).min(marks.len());
        for wanted in &mut want[lo..hi] {
            *wanted = true;
        }
    }
    // The runs of wanted lines, each of which is a hunk of the difference.
    let mut out: Vec<String> = Vec::new();
    let mut from: Option<usize> = None;
    let close = |out: &mut Vec<String>, from: usize, to: usize| {
        let mut old_count = 0usize;
        let mut new_count = 0usize;
        for (mark, _) in &marks[from..to] {
            if *mark != '+' {
                old_count += 1;
            }
            if *mark != '-' {
                new_count += 1;
            }
        }
        let old_part = if old_count > 1 {
            format!("{},{}", old_line[from], old_count)
        } else {
            old_line[from].to_string()
        };
        let new_part = if new_count > 1 {
            format!("{},{}", new_line[from], new_count)
        } else {
            new_line[from].to_string()
        };
        out.push(format!("@@ -{} +{} @@", old_part, new_part));
        for (mark, line) in &marks[from..to] {
            out.push(format!("{}{}", mark, line));
        }
    };
    for (k, wanted) in want.iter().enumerate() {
        if *wanted && from.is_none() {
            from = Some(k);
        }
        if !wanted && from.is_some() {
            let f = from.unwrap();
            from = None;
            close(&mut out, f, k);
        }
    }
    if let Some(f) = from {
        close(&mut out, f, marks.len());
    }
    out
}

// What a call that went well tells the model: how many places were changed, and
// the difference, cut to a window about the first change and numbered against
// the whole file. The file itself is where the whole of it is read again.
fn receipt(name: &str, before: &str, made: &Made) -> String {
    let plural = if made.count == 1 { "" } else { "s" };
    // The two counts of the change stand with the words of it: what was written,
    // and what was taken away. The page reads them into the head of the block.
    let head = format!(
        "Edited {} ({} replacement{}, +{} -{})",
        name, made.count, plural, made.added, made.removed
    );
    // The window about the first change, kept wide enough to hold the change
    // whole, and cut again at a bound of lines so the table of the difference
    // stays small for a change that runs long.
    let (start, end) = diff_window(before, made.first, DIFF_WINDOW_LINES);
    let end = end.max(made.first + made.taken.len());
    let mut lines = 0usize;
    let mut probe = start;
    while probe < end && lines < MAX_DIFF_WINDOW_LINES {
        match before[probe..].find('\n') {
            Some(newline) => {
                probe += newline + 1;
                lines += 1;
            }
            None => {
                probe = end;
                break;
            }
        }
    }
    let end = probe.min(end);
    let before_window = &before[start..end];
    // The front of the window was kept wide enough to hold the change whole, so
    // the words of it stand in the window, and the replacement is replayed on
    // what stands there.
    let after_window = before_window.replace(&made.taken, &made.put);
    // The line of the file the front of the window stands on, so the difference
    // is numbered against the file and not against the window.
    let first_line = before[..start].matches('\n').count() + 1;
    let before_lines: Vec<&str> = before_window.split('\n').collect();
    let after_lines: Vec<&str> = after_window.split('\n').collect();
    let mut diff = difference(&before_lines, &after_lines, DIFF_CONTEXT, first_line - 1);
    if diff.is_empty() {
        return head;
    }
    if diff.len() > DIFF_LINES {
        let mut cut: Vec<String> = diff[..DIFF_LINES].to_vec();
        cut.push(String::from("... (more diff lines)"));
        diff = cut;
    }
    for line in diff.iter_mut() {
        let count = line.chars().count();
        if count > DIFF_LINE_CHARS {
            let mut kept: String = line.chars().take(DIFF_LINE_CHARS).collect();
            kept.push_str(&format!("... (+{} chars)", count - DIFF_LINE_CHARS));
            *line = kept;
        }
    }
    let mut body = diff.join("\n");
    if body.len() > DIFF_CHARS {
        body.truncate(DIFF_CHARS);
        body.push_str("\n... (result truncated)");
    }
    format!("{}\n{}", head, body)
}

// The words a call of no shape is answered with. The model is told the shape,
// for a call that is refused for its shape is often meant well.
fn wrong_shape(edit_tool: bool) -> String {
    if edit_tool {
        return String::from(
            "The call needs the name of a file and a list of changes, as {\"path\": \"notes.txt\", \"edits\": [{\"old_string\": \"the words now\", \"new_string\": \"the words after\"}]}",
        );
    }
    String::from(
        "The call needs the name of a file and the text for it, as {\"path\": \"notes.txt\", \"text\": \"the whole of the file\"}",
    )
}

// Change a file of the chat. Every change of the call is made, or none of them
// is, and the answer says which of the two happened.
pub fn edit(workdir: &Path, raw: &str) -> tools::Report {
    edit_room(workdir, raw, true)
}

// The same, with the wall made known: see `place_kept`.
pub fn edit_room(workdir: &Path, raw: &str, in_room: bool) -> tools::Report {
    let failed = |output: String| tools::Report {
        ok: false,
        output,
    };
    let Some(call) = object(raw) else {
        return failed(wrong_shape(true));
    };
    let Some(name) = first_of(&call, &PATH_KEYS) else {
        return failed(String::from("The name of the file is required"));
    };
    let changes = match parse_changes(&call) {
        Ok(made) => made,
        Err(problem) => return failed(problem),
    };
    let place = match place_kept(workdir, &name, in_room) {
        Ok(place) => place,
        Err(problem) => return failed(problem),
    };
    let standing = match read_words(&place) {
        Ok(words) => words,
        Err(problem) => return failed(problem),
    };
    let short = last(&place);
    let made = match apply_all(&standing.words, &changes, &short) {
        Ok(made) => made,
        Err(problem) => return failed(problem),
    };
    let after = Standing {
        words: made.after.clone(),
        newline: standing.newline.clone(),
        bom: standing.bom.clone(),
        bytes: Vec::new(),
    };
    let the_room = room_for(workdir, in_room);
    match write_words(&the_room, &place, &after, &standing.bytes) {
        Ok(()) => tools::Report {
            ok: true,
            output: receipt(&short, &standing.words, &made),
        },
        Err(problem) => failed(problem),
    }
}

// Make a new file in the room of the chat. A file that stands there with words
// in it is left as it is. The one exception is a file of nothing at all, for no
// change can match nothing, and such a file would else never be filled.
pub fn create(workdir: &Path, raw: &str) -> tools::Report {
    create_room(workdir, raw, true)
}

// The same, with the wall made known: see `place_kept`.
pub fn create_room(workdir: &Path, raw: &str, in_room: bool) -> tools::Report {
    let failed = |output: String| tools::Report {
        ok: false,
        output,
    };
    let Some(call) = object(raw) else {
        return failed(wrong_shape(false));
    };
    let Some(name) = first_of(&call, &PATH_KEYS) else {
        return failed(String::from("The name of the file is required"));
    };
    let mut words = String::new();
    let mut given = false;
    for key in ["text", "content", "body", "new_string"] {
        if let Some(text) = call.get(key).and_then(|v| v.as_str()) {
            words = text.replace("\r\n", "\n");
            given = true;
            break;
        }
    }
    if !given {
        return failed(format!(
            "the text for '{}' is required. An empty string makes an empty file.",
            name
        ));
    }
    if words.len() > MAX_FILE_BYTES {
        return failed(format!(
            "the text for '{}' is larger than the {} bytes a file tool may write.",
            name, MAX_FILE_BYTES
        ));
    }
    let place = match place_kept(workdir, &name, in_room) {
        Ok(place) => place,
        Err(problem) => return failed(problem),
    };
    let the_room = room_for(workdir, in_room);
    if let Some(parent) = place.parent() {
        if let Err(problem) = std::fs::create_dir_all(parent) {
            return failed(format!(
                "the folder for '{}' could not be created: {}",
                name, problem
            ));
        }
    }
    // The room is looked at again, for a folder of the road may have been
    // changed for a link while the folders on the way were being made.
    if !under(&the_room, &as_far_as_stands(&place)) {
        return failed(format!(
            "'{}' moved outside the working directory of the chat while it was prepared, so nothing was written.",
            name
        ));
    }
    // The lines of the file, counted the way a line of words ends: a mark of a
    // new line at the end closes the last line and does not open another.
    let lines = if words.is_empty() {
        0usize
    } else {
        words.matches('\n').count() + if words.ends_with('\n') { 0 } else { 1 }
    };
    let stood = std::fs::symlink_metadata(&place).ok();
    if let Some(meta) = stood {
        if meta.len() > 0 {
            return failed(format!(
                "'{}' already exists with {} bytes in it. create_file makes a new file and never overwrites one; to change this file, use edit_file.",
                name,
                meta.len()
            ));
        }
        // The file is empty, so it is filled instead of made again.
        if let Err(problem) = std::fs::write(&place, words.as_bytes()) {
            return failed(format!("'{}' could not be written: {}", name, problem));
        }
        return tools::Report {
            ok: true,
            output: format!(
                "Created '{}'. {}, {} bytes.",
                name,
                line_words(lines),
                words.len()
            ),
        };
    }
    use std::io::Write;
    let opened = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&place);
    match opened {
        Ok(mut file) => {
            if let Err(problem) = file.write_all(words.as_bytes()) {
                drop(file);
                let _ = std::fs::remove_file(&place);
                return failed(format!(
                    "'{}' could not be written, so it was removed again: {}",
                    name, problem
                ));
            }
            tools::Report {
                ok: true,
                output: format!(
                    "Created '{}'. {}, {} bytes.",
                    name,
                    line_words(lines),
                    words.len()
                ),
            }
        }
        Err(problem) => failed(format!("'{}' could not be created: {}", name, problem)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The room of a chat, made for one test and taken away again. The tests take
    // turns through a count, so two of them never work in one room.
    static TURN_OF_TEST: AtomicUsize = AtomicUsize::new(0);

    fn room_of(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "autumn-natter-file-tools-{}-{}",
            std::process::id(),
            TURN_OF_TEST.fetch_add(1, Ordering::SeqCst)
        ));
        let place = root.join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&place).expect("the working directory of the chat could not be created");
        place
    }

    // The shape of both calls. No part of either of them stands for other words:
    // there is no place holder, no mark of a file put behind the answer, and
    // nothing to fill in with a name that is not a name.
    #[test]
    fn no_part_of_a_file_call_stands_for_another_part() {
        let made = edit_definition();
        let call = made.get("function").expect("the tool is a function");
        assert_eq!(call.get("name").and_then(|v| v.as_str()), Some(EDIT_NAME));
        let mut keys: Vec<String> = call
            .get("parameters")
            .and_then(|p| p.get("properties"))
            .and_then(|p| p.as_object())
            .expect("the parts of a call are listed")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        assert_eq!(keys, vec![String::from("edits"), String::from("path")]);
        let want = call
            .get("parameters")
            .and_then(|p| p.get("required"))
            .and_then(|r| r.as_array())
            .expect("the parts that must come are listed");
        assert_eq!(want, &vec![String::from("path"), String::from("edits")][..]);

        let made = create_definition();
        let call = made.get("function").expect("the tool is a function");
        assert_eq!(call.get("name").and_then(|v| v.as_str()), Some(CREATE_NAME));
        let mut keys: Vec<String> = call
            .get("parameters")
            .and_then(|p| p.get("properties"))
            .and_then(|p| p.as_object())
            .expect("the parts of a call are listed")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        assert_eq!(keys, vec![String::from("path"), String::from("text")]);

        // Nothing of a place holder stands in the words handed to the model.
        for made in [edit_definition(), create_definition()] {
            let words = made.to_string().to_lowercase();
            for forbidden in ["placeholder", "place holder", "sentinel", "__", "<file>", "todo: fill"] {
                assert!(!words.contains(forbidden), "the call offers {}: {}", forbidden, made);
            }
        }
    }

    // Every change is matched against the file as it stood before the call. A
    // change that could match what an earlier change of the same call brought
    // in would land in words the model never read.
    #[test]
    fn every_change_is_matched_against_the_file_as_it_stood() {
        let before = "one\ntwo\nthree\n";
        let changes = vec![
            Change { before: "one".into(), after: "uno".into(), everywhere: false },
            // The mark of the first change is not in the file as it stood.
            Change { before: "un".into(), after: "un".into(), everywhere: false },
        ];
        let made = apply_all(before, &changes, "notes.txt");
        assert!(made.is_err());
        assert!(made.unwrap_err().contains("found no text to replace"));
    }

    // Two changes of one call cannot cover one place.
    #[test]
    fn two_changes_that_cover_one_place_are_refused() {
        let before = "alpha beta\n";
        let changes = vec![
            Change { before: "alpha".into(), after: "aleph".into(), everywhere: false },
            Change { before: "alpha beta".into(), after: "bet".into(), everywhere: false },
        ];
        let made = apply_all(before, &changes, "notes.txt");
        assert!(made.is_err());
        assert!(made.unwrap_err().contains("cover the same text"));
    }

    // Words that stand in more than one place need the change marked for every
    // place, or the call is refused with the number of the places in it.
    #[test]
    fn words_in_several_places_need_replace_all() {
        let before = "xa xa xa\n";
        // The one change, asked for one place.
        let changes = vec![Change {
            before: "xa".into(),
            after: "xb".into(),
            everywhere: false,
        }];
        let made = apply_all(before, &changes, "many.txt");
        let problem = made.unwrap_err();
        assert!(problem.contains("3 places"), "{}", problem);
        // The same change, marked for every place.
        let changes = vec![Change {
            before: "xa".into(),
            after: "xb".into(),
            everywhere: true,
        }];
        let made = apply_all(before, &changes, "many.txt").expect("every place is changed");
        assert_eq!(made.after, "xb xb xb\n");
        assert_eq!(made.count, 3);
        assert_eq!(made.first, 0);
        // A batch of two: the places of both are worked out, and the count holds
        // every place that was changed.
        let changes = vec![
            Change { before: "xa".into(), after: "xb".into(), everywhere: true },
            Change { before: "\n".into(), after: "!\n".into(), everywhere: false },
        ];
        let made = apply_all(before, &changes, "many.txt").expect("the batch is written");
        assert_eq!(made.after, "xb xb xb!\n");
        assert_eq!(made.count, 4);
    }

    // A name of a file is a place in the room of the chat, and nowhere else. A
    // road that goes up out of a folder, a place of the machine named whole, a
    // link that leads out of the room, and the records of the attachments are
    // all refused.
    #[test]
    fn a_name_is_kept_inside_the_room_of_the_chat() {
        let root = room_of("chat");
        assert!(place_in(&root, "notes.txt").is_ok());
        assert!(place_in(&root, "src/parse.rs").is_ok());
        for outside in ["../elsewhere", "notes/../../x", "/etc/passwd", ".attachments/ab12cd.json"]
        {
            let refused = place_in(&root, outside);
            assert!(refused.is_err(), "{} was let through", outside);
        }
        // A name a model brought over from another program keeps its last part,
        // and that part stands in the room of the chat.
        let habit = place_in(&root, "/mnt/data/notes/plan.txt").expect("the habit name is a file name");
        assert_eq!(last(&habit), "plan.txt");
        assert!(under(&room(&root), &habit));
        // A link standing in the room and pointing out of it.
        #[cfg(unix)]
        {
            let secret = root.parent().expect("the room has a folder above it");
            let outside = secret.join("secret.txt");
            std::fs::write(&outside, "NOT-FOR-A-TOOL").unwrap();
            std::os::unix::fs::symlink(&outside, root.join("link.txt")).unwrap();
            let refused = place_in(&root, "link.txt");
            assert!(refused.is_err(), "a link out of the room was let through");
            // The room itself may be named whole, which is how a model names a
            // file it read out of a listing.
            let whole = room(&root).join("notes.txt").display().to_string();
            assert!(place_in(&root, &whole).is_ok());
        }
    }

    // With the sandbox down the file tools reach the machine, as a command of
    // the terminal tool does: a name outside the room of the chat is then a name
    // like any other, and a road that goes back a step is a part of it. The
    // store of the files the user hung on the chat is kept out of reach either
    // way, and the wall is the sandbox alone.
    #[test]
    fn a_name_outside_the_room_is_worked_on_while_the_jail_is_down() {
        let root = room_of("wide");
        let elsewhere = room_of("yonder");
        let far = elsewhere.join("plan.txt");
        let said = format!(
            "{{\"path\":\"{}\",\"text\":\"written outside the room\\n\"}}",
            far.display()
        );
        // The wall up: the same name is refused, and nothing stands there.
        let kept = create_room(&root, &said, true);
        assert!(!kept.ok, "a name outside the room was let through");
        assert!(!far.exists());
        // The wall down: the file is made where it was named.
        let made = create_room(&root, &said, false);
        assert!(made.ok, "{}", made.output);
        assert!(far.is_file(), "the file was not written where it was named");
        // A name that goes back a step is a road of the machine, and not a way
        // out of anything, when there is no wall about the room.
        let back = format!(
            "{}/sub/../up.txt",
            elsewhere.display()
        );
        let stepped = create_room(
            &root,
            &format!("{{\"path\":\"{}\",\"text\":\"up\\n\"}}", back),
            false,
        );
        assert!(stepped.ok, "{}", stepped.output);
        assert!(elsewhere.join("up.txt").is_file());
        // The store of the files of the chat is out of reach with the wall down
        // as well as with the wall up.
        let shelf = format!(
            "{}/.attachments/ab12cd.json",
            room(&root).display()
        );
        assert!(place_kept(&root, &shelf, false).is_err());
    }

    // A batch is written all at once, or not at all, and the answer says what
    // was changed, as a difference numbered against the file.
    #[test]
    fn a_batch_of_changes_is_written_once_or_not_at_all() {
        let root = room_of("chat");
        let notes = root.join("notes.txt");
        std::fs::write(&notes, "one\ntwo\nthree\n").unwrap();
        // The second change names words that are not in the file.
        let half = r#"{"path":"notes.txt","edits":[{"old_string":"one","new_string":"uno"},{"old_string":"four","new_string":"done"}]}"#;
        let out = edit(&root, half);
        assert!(!out.ok, "{}", out.output);
        assert_eq!(std::fs::read_to_string(&notes).unwrap(), "one\ntwo\nthree\n");
        // The same call with the words mended writes both changes, and the
        // answer counts them and shows the difference of the first.
        let whole = r#"{"path":"notes.txt","edits":[{"old_string":"one","new_string":"uno"},{"old_string":"three","new_string":"tri"}]}"#;
        let out = edit(&root, whole);
        assert!(out.ok, "{}", out.output);
        assert_eq!(std::fs::read_to_string(&notes).unwrap(), "uno\ntwo\ntri\n");
        assert!(out.output.starts_with("Edited notes.txt (2 replacements,"), "{}", out.output);
        assert!(out.output.contains("@@ -1,3 +1,3 @@"), "{}", out.output);
        assert!(out.output.contains("-one"), "{}", out.output);
        assert!(out.output.contains("+uno"), "{}", out.output);
        // A change that is not on the first line is numbered against the file,
        // and the words that went out and came in stand in the difference.
        let next = r#"{"path":"notes.txt","edits":[{"old_string":"two","new_string":"deux"}]}"#;
        let out = edit(&root, next);
        assert!(out.output.starts_with("Edited notes.txt (1 replacement,"), "{}", out.output);
        assert!(out.output.contains("-two"), "{}", out.output);
        assert!(out.output.contains("+deux"), "{}", out.output);
    }

    // The difference is numbered against the whole file, and not against the
    // window it was cut from, so a change in the middle of a long file points
    // at the line it stands on.
    #[test]
    fn the_difference_is_numbered_against_the_file() {
        let root = room_of("chat");
        let place = root.join("mid.txt");
        let words: Vec<String> = (1..=9001).map(|n| format!("line{}", n)).collect();
        std::fs::write(&place, words.join("\n") + "\n").unwrap();
        let out = edit(
            &root,
            r#"{"path":"mid.txt","edits":[{"old_string":"line8000\n","new_string":"CHANGED\n"}]}"#,
        );
        assert!(out.ok, "{}", out.output);
        assert!(out.output.contains("@@ -7998"), "{}", out.output);
        assert!(out.output.contains("+CHANGED"), "{}", out.output);
        assert!(out.output.contains("-line8000"), "{}", out.output);
    }

    // A change that adds a line is numbered for the line it grows, and a change
    // in the first lines still numbers from one.
    #[test]
    fn the_difference_counts_the_lines_it_grows() {
        let root = room_of("chat");
        let place = root.join("grow.txt");
        let words: Vec<String> = (1..=401).map(|n| format!("line{}", n)).collect();
        std::fs::write(&place, words.join("\n") + "\n").unwrap();
        let out = edit(
            &root,
            r#"{"path":"grow.txt","edits":[{"old_string":"line200\n","new_string":"A\nB\n"}]}"#,
        );
        assert!(out.ok, "{}", out.output);
        assert!(out.output.contains("@@ -198,5 +198,6 @@"), "{}", out.output);
        let top = root.join("top.txt");
        let words: Vec<String> = (1..=200).map(|n| format!("line{}", n)).collect();
        std::fs::write(&top, words.join("\n") + "\n").unwrap();
        let out = edit(
            &root,
            r#"{"path":"top.txt","edits":[{"old_string":"line2\n","new_string":"TOP\n"}]}"#,
        );
        assert!(out.ok, "{}", out.output);
        assert!(out.output.contains("@@ -1"), "{}", out.output);
    }

    // A file that ends its lines the Windows way is read through that and
    // written back the same way, so an edit does not change every line of it.
    #[test]
    fn the_lines_of_a_file_keep_the_marks_they_had() {
        let root = room_of("chat");
        let place = root.join("lines.txt");
        std::fs::write(&place, b"one\r\ntwo\r\n").unwrap();
        let out = edit(
            &root,
            r#"{"path":"lines.txt","edits":[{"old_string":"two","new_string":"deux"}]}"#,
        );
        assert!(out.ok, "{}", out.output);
        assert_eq!(std::fs::read(&place).unwrap(), b"one\r\ndeux\r\n");
    }

    // A new file is made, folders on the way are made with it, and a file that
    // stands there with words in it is left as it is.
    #[test]
    fn a_new_file_is_made_and_one_that_stands_is_left() {
        let root = room_of("chat");
        let made = create(&root, r#"{"path":"notes/hello.txt","text":"hello\n"}"#);
        assert!(made.ok, "{}", made.output);
        assert_eq!(std::fs::read_to_string(root.join("notes/hello.txt")).unwrap(), "hello\n");
        assert!(made.output.contains("one line"), "{}", made.output);
        let again = create(&root, r#"{"path":"notes/hello.txt","text":"other"}"#);
        assert!(!again.ok, "{}", again.output);
        assert!(again.output.contains("edit_file"), "{}", again.output);
        assert_eq!(std::fs::read_to_string(root.join("notes/hello.txt")).unwrap(), "hello\n");
        // A file of nothing is filled, for no change can match nothing.
        std::fs::write(root.join("empty.txt"), "").unwrap();
        assert!(create(&root, r#"{"path":"empty.txt","text":"filled\n"}"#).ok);
        assert_eq!(std::fs::read_to_string(root.join("empty.txt")).unwrap(), "filled\n");
        // A name out of the room makes nothing, neither the file nor the folders
        // on the way to it.
        assert!(!create(&root, r#"{"path":"../escape.txt","text":"x"}"#).ok);
        assert!(!root.parent().unwrap().join("escape.txt").exists());
        assert!(!create(&root, r#"{"path":".attachments/ab12cd.json","text":"x"}"#).ok);
    }

    // The words of the file of a call that is not finished yet. The user
    // watches the name of the file come in, as the user watches a command.
    #[test]
    fn the_name_of_the_file_is_read_while_the_call_is_written() {
        assert_eq!(partial_words(""), String::new());
        assert_eq!(partial_words("{"), String::new());
        assert_eq!(partial_words(r#"{"path":"no"#), "no");
        assert_eq!(
            partial_words(r#"{"path":"notes/todo.txt","edits":[{"old_string":"x"#),
            "notes/todo.txt"
        );
        assert_eq!(file_of(r#"{"path":"notes/todo.txt","edits":[]}"#), "notes/todo.txt");
        // A call with no name of a file in it yet gives nothing.
        assert_eq!(file_of(r#"{"edits":[]}"#), String::new());
        // The name of the file is read wherever the model put it, so a call that
        // writes the changes first and the name last is named as soon as the name
        // begins.
        assert_eq!(file_of(r#"{"edits":[],"path":"notes/to"}"#), "notes/to");
        assert_eq!(
            file_of(r#"{"edits":[{"old_string":"one"}],"path":"notes/to"#),
            "notes/to"
        );
        // A model that spelled its part another way is read the same way while
        // the call is written as it is read when the call is over.
        assert_eq!(partial_words(r#"{"file_path":"notes/to"#), "notes/to");
        assert_eq!(partial_words(r#"{"file":"notes/to"#), "notes/to");
        assert_eq!(file_of(r#"{"file":"notes/todo.txt"}"#), "notes/todo.txt");
    }

    // The two halves of a change are read in the order they come in, and not only
    // in the order the shape of the call asks for them. A model that writes the
    // words to put in before the words to take out is read while it writes, so
    // the user of the block is not left looking at an empty body until the two
    // halves happen to meet.
    #[test]
    fn the_two_halves_of_a_change_are_read_in_the_order_they_come() {
        // The order the shape of the call asks for, which is the order most
        // models keep to: the words to take out come first, and the words to put
        // in join them when their mark is written.
        assert_eq!(
            edit_params(r#"{"path":"notes/todo.txt","edits":[{"old_string":"one","new_string":"u"#),
            "one\n> u"
        );
        // The other way about. The mark of the change stands over the words that
        // have come, and the words to take out walk in front of them when they
        // arrive.
        assert_eq!(edit_params(r#"{"edits":[{"new_string":"un"#), "> un");
        assert_eq!(
            edit_params(r#"{"edits":[{"new_string":"uno","old_string":"o"#),
            "o\n> uno"
        );
        // The mark of a change is shown as soon as it has a half of its own, for
        // a user watching a block wants to see it move.
        assert_eq!(edit_params(r#"{"edits":[{"new_string":""#), "> ");
        assert_eq!(
            edit_params(r#"{"edits":[{"old_string":"one","new_string":""#),
            "one\n> "
        );
        // Two changes written the other way about are read side by side, so each
        // half of each change stands where it belongs.
        assert_eq!(
            edit_params(
                r#"{"edits":[{"new_string":"uno","old_string":"one"},{"new_string":"dos","old_string":"tw"#),
            "one\n> uno\ntw\n> dos"
        );
        // A change with no words at all in it is left out, and so is a change of
        // nothing but a space between the words to take out, for a mark over
        // nothing says nothing.
        assert_eq!(edit_params(""), String::new());
        assert_eq!(edit_params(r#"{"path":"notes/todo.txt","edits":[{#), String::new());
        assert_eq!(edit_params(r#"{"edits":[{"old_string":"  "#), String::new());
        // The whole of a call is read the same way as half of one.
        assert_eq!(
            edit_params(
                r#"{"edits":[{"new_string":"uno","old_string":"one"}],"path":"notes/todo.txt"}"#
            ),
            "one\n> uno"
        );
        // Both halves of a call that is still being written: the name of the file
        // in the head, and the words of the change in the body, each as far as it
        // has come.
        let (body, name) = live_params(EDIT_NAME, r#"{"edits":[{"new_string":"un"#);
        assert_eq!(body, "> un");
        assert_eq!(name, String::new());
        let (body, name) = live_params(EDIT_NAME, r#"{"edits":[],"path":"notes/to"#);
        assert_eq!(body, String::new());
        assert_eq!(name, "notes/to");
    }

    // A whole call written the other way about - the words to put in before the
    // words to take out, and the name of the file after both - walked a character
    // at a time, as the stream of an answer hands it out. Two things are looked
    // at over every one of those looks: the body of the block never stands over
    // nothing once a half of a change has begun, and the name of the file is read
    // from the character it begins at, whichever half of the call it stands in.
    #[test]
    fn a_call_written_backwards_is_read_a_character_at_a_time() {
        let call = String::from(concat!(
            r#"{"edits":[{"new_string":"uno","old_string":"one"},"#,
            r#"{"new_string":"dos","old_string":"two"}],"path":"notes/todo.txt"}"#,
        ));
        let opened = r#""new_string":""#;
        let named = r#""path":""#;
        // The body and the name of the file at every length of the call, from
        // nothing at all to the whole of it.
        let mut bodies: Vec<String> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for cut in 0..=call.len() {
            let (body, name) = live_params(EDIT_NAME, &call[..cut]);
            bodies.push(body);
            names.push(name);
        }
        // The whole of the call reads as the whole of any other call does.
        assert_eq!(bodies[call.len()].as_str(), "one\n> uno\ntwo\n> dos");
        assert_eq!(names[call.len()].as_str(), "notes/todo.txt");
        // From the first character after the mark that opens the words to put in,
        // every look holds a word of the change. This is the stretch the user
        // used to sit out: the block stood blank, and the words of the change
        // popped in whole when the two halves of it happened to meet.
        let begun = call.find(opened).expect("the call opens its first words");
        for (cut, body) in bodies.iter().enumerate().skip(begun + opened.len()) {
            assert!(
                !body.is_empty(),
                "the body of the block stood blank {} characters into the call",
                cut
            );
        }
        // The name of the file is nothing until it is begun, and then every
        // character of it as far as it has come, which is how a command of the
        // terminal tool is read.
        let at = call.find(named).expect("the call names its file at the last") + named.len();
        for (cut, name) in names.iter().enumerate().take(at) {
            assert!(
                name.is_empty(),
                "a file was named before its name began: {} characters in",
                cut
            );
        }
        let whole = "notes/todo.txt";
        for (come, name) in names.iter().enumerate().skip(at) {
            let gone = (come - at).min(whole.len());
            assert_eq!(name.as_str(), &whole[..gone], "the name of the file fell behind");
        }
    }

    // `replace_all` is read strictly. The word false between quotes is off, and
    // a word that is neither of the two is refused rather than read as on.
    #[test]
    fn replace_all_is_read_strictly() {
        assert_eq!(strict_bool(None), Some(false));
        assert_eq!(strict_bool(Some(&json!(true))), Some(true));
        assert_eq!(strict_bool(Some(&json!("false"))), Some(false));
        assert_eq!(strict_bool(Some(&json!("true"))), Some(true));
        assert_eq!(strict_bool(Some(&json!(1))), Some(true));
        assert_eq!(strict_bool(Some(&json!("maybe"))), None);
        assert_eq!(strict_bool(Some(&json!([]))), None);
        // A call that gives the word between quotes is not read as a change of
        // every place in the file.
        let root = room_of("chat");
        let place = root.join("many.txt");
        std::fs::write(&place, "xa xa xa\n").unwrap();
        let out = edit(
            &root,
            r#"{"path":"many.txt","edits":[{"old_string":"xa","new_string":"xb","replace_all":"false"}]}"#,
        );
        assert!(!out.ok, "{}", out.output);
        assert_eq!(std::fs::read_to_string(&place).unwrap(), "xa xa xa\n");
    }

    // A file that changed while the call was making itself ready is not set back
    // to the words of the call, and a write that is refused leaves no other
    // version of the file standing beside it.
    #[test]
    fn a_file_changed_in_the_middle_is_left_alone() {
        let root = room_of("chat");
        let place = root.join("race.txt");
        std::fs::write(&place, "one\n").unwrap();
        let standing = Standing {
            words: "two\n".into(),
            newline: String::from("\n"),
            bom: String::new(),
            bytes: Vec::new(),
        };
        let the_room = room(&root);
        // The words the change was worked out of are still the words of the file.
        assert!(write_words(&the_room, &place, &standing, b"one\n").is_ok());
        assert_eq!(std::fs::read_to_string(&place).unwrap(), "two\n");
        // Now they are not, so this write is refused.
        let out = write_words(&the_room, &place, &standing, b"different\n");
        assert!(out.is_err());
        assert!(out.unwrap_err().contains("nothing was written"));
        assert_eq!(std::fs::read_to_string(&place).unwrap(), "two\n");
        let left: Vec<String> = std::fs::read_dir(&root)
            .expect("the room can be read")
            .filter_map(|entry| entry.ok().map(|e| e.file_name().to_string_lossy().to_string()))
            .collect();
        assert!(
            !left.iter().any(|name| name.contains("natter")),
            "a refused write left a file behind: {:?}",
            left
        );
    }

    // A file of bytes, and a file that is not there, are said plainly.
    #[test]
    fn a_file_of_bytes_is_not_changed() {
        let root = room_of("chat");
        std::fs::write(root.join("bytes.bin"), [b'a', 0, b'b']).unwrap();
        let out = edit(&root, r#"{"path":"bytes.bin","edits":[{"old_string":"a","new_string":"b"}]}"#);
        assert!(!out.ok);
        assert!(out.output.contains("bytes"), "{}", out.output);
        let out = edit(&root, r#"{"path":"gone.txt","edits":[{"old_string":"a","new_string":"b"}]}"#);
        assert!(!out.ok);
        assert!(out.output.contains("create_file"), "{}", out.output);
        // A call of no shape at all is answered with the shape.
        let out = edit(&root, "notes.txt");
        assert!(!out.ok);
        assert!(out.output.contains("edits"), "{}", out.output);
    }
}
