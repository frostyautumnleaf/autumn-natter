// The tool that hands a file of the chat to the user.
//
// The model names one or more files that stand in the room of the chat, and the
// program puts blocks of its own into the talk: the mark of the kind of file,
// the name of it, the extension under the name, and a button. The user presses
// the button and the file comes down. Nothing is fetched, and nothing is written:
// the file stands where the model put it, and the block only names it.
//
// The blocks are read again after a restart of the program, and in the page of
// the remote mode. The bytes are asked for by the name the file has in the room
// of the chat, and are let through only when that room is the room of the chat
// that is open. A file outside the room of the chat is not handed out at all,
// which is the rule the file tools have always worked to: the sandbox of a
// command and the reach of a download are the same room.
//
// The tool is offered under the badge of the tools, and falls under it as the
// other internal tools do: the badge is the switch over all of them, and the
// drop-down of the badge holds the switch of this one.
//
// A block is never drawn while the model is still writing, and the blocks of an
// answer stand at its end: a file the model was offering is not finished work
// until the answer that made it is over.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::tools::with_rule;

// Name the model uses to ask for a file to be handed to the user.
pub const NAME: &str = "offer_files";

// The longest name of a file the block may show. A name above this is cut in
// the middle, so the block keeps its shape.
pub const GREATEST_NAME: usize = 90;

// The most files one call may offer. A model that wants to hand over the whole of
// a repository was not asked to, and a block for every file of it is not a thing
// a user looks at.
pub const GREATEST_FILES: usize = 8;

// The most bytes of one file that are kept for the user. A file above this is
// named but not handed out, for a block that fills the memory of the window is no
// use to anyone.
pub const GREATEST_FILE: u64 = 64 * 1024 * 1024;

// The tool as the model is told of it.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": with_rule(String::from(
                "Offer one or more files of the working directory to the user, so the user can take each of them with the button of a block in the chat. Use it when the work left a file the user asked for: a page, a document, a picture, an archive of the work. Call it at the end of the answer, once the work is done. The blocks are placed BELOW your answer, at its foot: they are never above it, and never stand while it is still being written. So never tell the user the files are above them - say the files are below, or say only that they are offered. The block shows the kind of the file, its name without the extension, the extension under the name, and the button. A file that is not there, or that is bigger than 64 MiB, is answered and no block stands for it.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "The files to offer to the user. Every name is relative to the working directory of the chat, for example 'page.html' or 'reports/summary.md'. One name is enough; up to eight are taken."
                    },
                    "path": {
                        "type": "string",
                        "description": "One file to offer, when there is only one and no array is given."
                    },
                    "note": {
                        "type": "string",
                        "description": "A short line for the blocks, in the words of the chat, such as 'The page you asked for'. It is shown above them. Nothing is put there when it is left out."
                    }
                },
                "required": ["paths"]
            }
        }
    })
}

// One file a block offers. The record of the message holds these, so the blocks
// are drawn again when the chat is opened another day, and in the page of the
// remote mode. `path` is the name the file has in the room of the chat, which is
// what the user's click asks the program for.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Gift {
    pub name: String,
    pub extension: String,
    pub kind: String,
    pub path: String,
    pub size: u64,
}

// What became of one call. The blocks stand in the chat when `gifts` holds
// anything, and the words of the receipt go back to the model either way.
pub struct Made {
    pub report: crate::tools::Report,
    // The files that may stand as blocks, in the order the model named them.
    pub gifts: Vec<Gift>,
    // The words the model gave for the blocks, cut to one short line.
    pub note: String,
}

// Answer one call. Every file is looked for in the room of the chat, and only in
// that room. A file that is not there is reported and skipped; a call with
// nothing usable in it gives no block at all.
pub fn offer(workdir: &Path, arguments: &str) -> Made {
    let value: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let note = trim_note(value.get("note").and_then(|v| v.as_str()).unwrap_or(""));
    let asked = asked_paths(&value);
    if asked.is_empty() {
        return Made {
            report: crate::tools::Report {
                ok: false,
                output: String::from("No file was named, so nothing is offered to the user"),
            },
            gifts: Vec::new(),
            note,
        };
    }
    let mut gifts: Vec<Gift> = Vec::new();
    let mut missed: Vec<String> = Vec::new();
    for one in asked.iter().take(GREATEST_FILES) {
        let Some(inside) = place_of(workdir, one) else {
            missed.push(format!(
                "{} is outside the working directory of the chat",
                first_line_of(one)
            ));
            continue;
        };
        if !inside.is_file() {
            missed.push(format!("there is no file at {}", first_line_of(one)));
            continue;
        }
        let size = std::fs::metadata(&inside).map(|m| m.len()).unwrap_or(0);
        if size > GREATEST_FILE {
            missed.push(format!(
                "{} is {} mebibytes, above the 64 MiB one block may carry",
                first_line_of(one),
                size / (1024 * 1024)
            ));
            continue;
        }
        let name = inside
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| one.to_string());
        let (stem, tail) = split_name(&name);
        let mark = kind(&tail).to_string();
        gifts.push(Gift {
            name: stem,
            extension: tail,
            kind: mark,
            path: clean_of(workdir, &inside),
            size,
        });
    }
    if gifts.is_empty() {
        let said = if missed.is_empty() {
            String::from("No file was named, so nothing is offered to the user")
        } else {
            format!(
                "Nothing is offered: {}. Check the names and the sizes.",
                missed.join("; ")
            )
        };
        return Made {
            report: crate::tools::Report {
                ok: false,
                output: said,
            },
            gifts: Vec::new(),
            note,
        };
    }
    let shown: Vec<String> = gifts.iter().map(full_name).collect();
    let mut output = if gifts.len() == 1 {
        format!(
            "The file {} is offered to the user in a download block.",
            shown[0]
        )
    } else {
        format!(
            "{} files are offered to the user in download blocks: {}.",
            gifts.len(),
            shown.join(", ")
        )
    };
    if !missed.is_empty() {
        output.push_str(&format!(" Not offered: {}.", missed.join("; ")));
    }
    if asked.len() > GREATEST_FILES {
        output.push_str(&format!(
            " Only the first {} of the {} names were read.",
            GREATEST_FILES,
            asked.len()
        ));
    }
    Made {
        report: crate::tools::Report {
            ok: true,
            output,
        },
        gifts,
        note,
    }
}

// The names the model gave. `paths` is what the tool asks for; `path` is read as
// well, for a model that has one file to hand often reaches for it.
fn asked_paths(value: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(list) = value.get("paths").and_then(|v| v.as_array()) {
        for one in list.iter() {
            if let Some(text) = one.as_str() {
                if !text.trim().is_empty() {
                    out.push(text.trim().to_string());
                }
            }
        }
    }
    if let Some(text) = value.get("path").and_then(|v| v.as_str()) {
        if !text.trim().is_empty() {
            out.push(text.trim().to_string());
        }
    }
    out
}

// The name of a file as the block shows it, with the extension back on it.
fn full_name(one: &Gift) -> String {
    if one.extension.is_empty() {
        return one.name.clone();
    }
    format!("{}.{}", one.name, one.extension)
}

// The name of a file of the chat, as the block of the chat asks for it again:
// relative to the room of the chat, with the separator of the machine turned into
// the one the page uses.
fn clean_of(workdir: &Path, inside: &Path) -> String {
    // The room of the chat is followed through its links before the name is cut
    // off it, for the file tools work on the room they found that way, and a room
    // that was come at by another road would otherwise leave the whole of the road
    // standing in the name the block shows.
    let room = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf());
    let tail = inside
        .strip_prefix(&room)
        .or_else(|_| inside.strip_prefix(workdir))
        .unwrap_or(inside);
    tail.to_string_lossy().replace('\\', "/")
}

// The first line of what the model wrote, for a receipt.
fn first_line_of(words: &str) -> String {
    words.lines().next().unwrap_or("").trim().to_string()
}

// A note of the model, cut to one short line.
fn trim_note(words: &str) -> String {
    let one = words
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    let mut out = String::new();
    for ch in one.chars() {
        if out.chars().count() >= 120 {
            out.push('\u{2026}');
            break;
        }
        out.push(ch);
    }
    out
}

// The name of a file, cut into the part the block shows big and the extension it
// shows under it. A point at the start of a name is not an extension, for a name
// of that shape is a hidden file of the machine.
pub fn split_name(name: &str) -> (String, String) {
    let path = Path::new(name);
    let Some(point) = path.extension().and_then(|e| e.to_str()) else {
        return (cut(name), String::new());
    };
    if point.is_empty() {
        return (cut(name), String::new());
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let shown = if stem.is_empty() { name.to_string() } else { stem };
    (cut(&shown), point.to_ascii_lowercase())
}

// Cut a name that is longer than the block can show.
fn cut(name: &str) -> String {
    let count = name.chars().count();
    if count <= GREATEST_NAME {
        return name.to_string();
    }
    let mut out = String::new();
    for ch in name.chars().take(GREATEST_NAME) {
        out.push(ch);
    }
    out.push('\u{2026}');
    out
}

// The kind of a file, from its extension. The block draws one mark for each of
// these, so the user knows what is offered without reading the extension. A
// page of a web browser stands on its own, for it is the thing a chat most often
// hands over; a table of numbers does too.
pub fn kind(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif" | "tif" | "tiff"
        | "heic" => "image",
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus" => "sound",
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" => "film",
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "zst" | "tgz" | "rar" => "archive",
        "html" | "htm" | "xhtml" => "page",
        "csv" | "tsv" => "table",
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "c" | "h" | "cpp" | "hpp" | "java"
        | "rb" | "php" | "sh" | "json" | "toml" | "yaml" | "yml" | "xml" | "sql" | "css"
        | "ini" | "conf" => "code",
        "md" | "markdown" | "txt" | "log" | "pdf" | "doc" | "docx" | "odt" | "rtf" | "tex"
        | "epub" => "document",
        "exe" | "run" | "appimage" | "msi" | "deb" | "rpm" | "bat" => "program",
        "ttf" | "otf" | "woff" | "woff2" => "font",
        _ => "file",
    }
}

// The kind of file to write in the answer, so a browser keeps the name the block
// gave rather than the kind it guessed.
pub fn mime(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "txt" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" | "xhtml" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "json" => "application/json",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

// True when this name of a tool is the download tool.
pub fn is_tool(name: &str) -> bool {
    name == NAME
}

// The words of the head of the block while the call is being written: the last
// name the model has begun to write. The note of the model is not a name, so it
// is passed over, and a run of words the model has not finished quoting is read
// as far as it has gone.
pub fn partial_words(arguments: &str) -> String {
    let letters: Vec<char> = arguments.chars().collect();
    let mut last: Option<String> = None;
    let mut key: Option<String> = None;
    let mut at = 0usize;
    while at < letters.len() {
        if letters[at] != '"' {
            at += 1;
            continue;
        }
        let mut word = String::new();
        let mut read = at + 1;
        let mut closed = false;
        while read < letters.len() {
            let ch = letters[read];
            if ch == '\\' {
                if read + 1 < letters.len() {
                    word.push(letters[read + 1]);
                    read += 2;
                    continue;
                }
                read += 1;
                break;
            }
            if ch == '"' {
                closed = true;
                read += 1;
                break;
            }
            word.push(ch);
            read += 1;
        }
        let names = key.as_deref() != Some("note");
        if !closed {
            if names {
                last = Some(word);
            }
            break;
        }
        let mut probe = read;
        while probe < letters.len() && letters[probe].is_whitespace() {
            probe += 1;
        }
        if probe < letters.len() && letters[probe] == ':' {
            key = Some(word);
        } else {
            if names {
                last = Some(word);
            }
            key = None;
        }
        at = read;
    }
    let text = last.unwrap_or_default();
    text.rsplit('/').next().unwrap_or(&text).to_string()
}

// The road of a file of the chat, kept inside the room of the chat. A name that
// steps out of the room, or that leads into the records of the attachments, is
// refused, which is the rule the file tools work to as well.
fn place_of(workdir: &Path, asked: &str) -> Option<PathBuf> {
    let held = crate::editfile::place_in(workdir, asked).ok()?;
    Some(held)
}

// The bytes of a file the user was offered. The name is read as the tool reads
// it, and only the room of the chat is opened: a name that tries to leave the
// room gives nothing, and neither does a file above the size one block may carry.
// This is the road the button of a block comes down.
pub fn bytes_of(workdir: &Path, asked: &str) -> Option<(String, Vec<u8>)> {
    let inside = place_of(workdir, asked)?;
    if !inside.is_file() {
        return None;
    }
    let size = std::fs::metadata(&inside).map(|m| m.len()).unwrap_or(0);
    if size > GREATEST_FILE {
        return None;
    }
    let bytes = std::fs::read(&inside).ok()?;
    let name = inside.file_name()?.to_string_lossy().to_string();
    Some((name, bytes))
}

// The archive of several files, made in memory and handed to the user under one
// button. The files are kept as they are, without compressing them: the point is
// one download rather than several, and the work of a chat is not what
// compressing is for. Only the name of a file stands in the archive, not the
// folders it came out of, so what the user unpacks is what they were offered.
pub fn zip_of(files: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut book: Vec<(String, u32, u32, u32)> = Vec::new();
    for (name, bytes) in files.into_iter() {
        let clean = name.replace('\\', "/");
        let head = clean
            .rsplit('/')
            .next()
            .filter(|one| !one.is_empty())
            .unwrap_or("file")
            .to_string();
        let crc = crc32(&bytes);
        let size = bytes.len() as u32;
        let place = out.len() as u32;
        push(&mut out, b"PK\x03\x04");
        push16(&mut out, 20);
        push16(&mut out, 0x0800);
        push16(&mut out, 0);
        push16(&mut out, 0);
        push16(&mut out, 0);
        push32(&mut out, crc);
        push32(&mut out, size);
        push32(&mut out, size);
        push16(&mut out, head.len() as u16);
        push16(&mut out, 0);
        out.extend_from_slice(head.as_bytes());
        out.extend_from_slice(&bytes);
        book.push((head, crc, size, place));
    }
    let begin = out.len() as u32;
    for (name, crc, size, place) in book.iter() {
        push(&mut out, b"PK\x01\x02");
        push16(&mut out, 20);
        push16(&mut out, 20);
        push16(&mut out, 0x0800);
        push16(&mut out, 0);
        push16(&mut out, 0);
        push16(&mut out, 0);
        push32(&mut out, *crc);
        push32(&mut out, *size);
        push32(&mut out, *size);
        push16(&mut out, name.len() as u16);
        push16(&mut out, 0);
        push16(&mut out, 0);
        push16(&mut out, 0);
        push16(&mut out, 0);
        push32(&mut out, 0);
        push32(&mut out, *place);
        out.extend_from_slice(name.as_bytes());
    }
    let end = out.len() as u32;
    push(&mut out, b"PK\x05\x06");
    push16(&mut out, 0);
    push16(&mut out, 0);
    push16(&mut out, book.len() as u16);
    push16(&mut out, book.len() as u16);
    push32(&mut out, end - begin);
    push32(&mut out, begin);
    push16(&mut out, 0);
    out
}

fn push(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
}

fn push16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

// The check number of a zip entry. It is worked out bit by bit rather than from a
// table, for the archive of a download holds a few files and the bytes of a chat
// are not many.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for one in bytes {
        crc ^= *one as u32;
        for _ in 0..8 {
            let low = crc & 1;
            crc >>= 1;
            if low == 1 {
                crc ^= 0xedb8_8320;
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // A folder of its own for one test, so the tests of this file never stand on
    // one another's work.
    fn room(what: &str) -> PathBuf {
        static TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let place = std::env::temp_dir().join(format!(
            "autumn-natter-gift-{}-{}-{}",
            std::process::id(),
            what,
            TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&place);
        fs::create_dir_all(&place).unwrap();
        place
    }

    // The kinds the blocks are drawn by. A page of a web browser and a table of
    // numbers stand on their own, for those are two of the things a chat hands
    // over most.
    #[test]
    fn the_kind_of_a_file_is_told_from_its_extension() {
        assert_eq!(kind("png"), "image");
        assert_eq!(kind("HTML"), "page");
        assert_eq!(kind("htm"), "page");
        assert_eq!(kind("csv"), "table");
        assert_eq!(kind("rs"), "code");
        assert_eq!(kind("md"), "document");
        assert_eq!(kind("zip"), "archive");
        assert_eq!(kind("mp3"), "sound");
        assert_eq!(kind("unknownthing"), "file");
        let (stem, tail) = split_name("The Long Way.Round.HTML");
        assert_eq!(stem, "The Long Way.Round");
        assert_eq!(tail, "html");
        let (stem, tail) = split_name("notes");
        assert_eq!(stem, "notes");
        assert_eq!(tail, "");
        assert_eq!(mime("html"), "text/html");
        assert_eq!(mime("nothing"), "application/octet-stream");
    }

    // The tool takes several files at once, keeps the names of the room of the
    // chat, and refuses a name that steps out of that room.
    #[test]
    fn a_call_offers_its_files_and_refuses_the_ones_outside_the_room() {
        let place = room("offer");
        fs::write(place.join("page.html"), "<p>work</p>").unwrap();
        fs::create_dir_all(place.join("reports")).unwrap();
        fs::write(place.join("reports/summary.md"), "done").unwrap();
        let made = offer(
            &place,
            r#"{"paths":["page.html","reports/summary.md"],"note":"The page you asked for"}"#,
        );
        assert!(made.report.ok, "{}", made.report.output);
        assert_eq!(made.gifts.len(), 2);
        assert_eq!(made.gifts[0].name, "page");
        assert_eq!(made.gifts[0].extension, "html");
        assert_eq!(made.gifts[0].kind, "page");
        assert_eq!(made.gifts[0].path, "page.html");
        assert_eq!(made.gifts[1].path, "reports/summary.md");
        assert_eq!(made.gifts[1].kind, "document");
        assert_eq!(made.note, "The page you asked for");
        assert!(made.report.output.contains("2 files"));
        // A name that leaves the room, and a file that is not there: the call says
        // so, and no block stands for either.
        let out = offer(&place, r#"{"paths":["../../etc/passwd"]}"#);
        assert!(!out.report.ok, "{:?}", out.report.output);
        assert!(out.gifts.is_empty());
        let gone = offer(&place, r#"{"paths":["nowhere.txt"]}"#);
        assert!(!gone.report.ok);
        assert!(gone.gifts.is_empty());
        // One file may be named the short way as well, and a name out of the
        // eight is answered rather than kept quiet about.
        let one = offer(&place, r#"{"path":"page.html"}"#);
        assert_eq!(one.gifts.len(), 1);
        let many = format!(
            r#"{{"paths":["page.html",{}]}}"#,
            (0..12)
                .map(|n| format!("\"no{n}.txt\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        let told = offer(&place, &many);
        assert_eq!(told.gifts.len(), 1);
        assert!(told.report.output.contains("first 8"), "{}", told.report.output);
        let _ = fs::remove_dir_all(&place);
    }

    // The bytes of a file are let through only for a name inside the room of the
    // chat, which is the rule of the sandbox over the button of a block too.
    #[test]
    fn the_bytes_of_a_file_come_only_from_the_room_of_the_chat() {
        let place = room("bytes");
        fs::write(place.join("work.md"), "the work").unwrap();
        let (name, bytes) = bytes_of(&place, "work.md").expect("the file is in the room");
        assert_eq!(name, "work.md");
        assert_eq!(bytes, b"the work");
        assert!(bytes_of(&place, "../escape").is_none());
        assert!(bytes_of(&place, "nope.md").is_none());
        let _ = fs::remove_dir_all(&place);
    }

    // The archive of several files is a zip the user can open: it begins with
    // the mark of an entry, holds every name, keeps the bytes of each file whole,
    // and ends with the mark of the list of them.
    #[test]
    fn several_files_go_into_one_archive() {
        let made = zip_of(vec![
            (String::from("page.html"), b"<p>work</p>".to_vec()),
            (String::from("notes.md"), b"done\n".to_vec()),
        ]);
        assert_eq!(&made[0..4], b"PK\x03\x04");
        assert_eq!(&made[made.len() - 22..][..4], b"PK\x05\x06");
        let text = String::from_utf8_lossy(&made);
        assert!(text.contains("page.html"), "{}", text);
        assert!(text.contains("notes.md"), "{}", text);
        assert!(text.contains("<p>work</p>"), "the bytes are not kept");
        // A file from a folder stands under its own name, not under the road it
        // came out of.
        let deep = zip_of(vec![(String::from("reports/summary.md"), b"x".to_vec())]);
        let text = String::from_utf8_lossy(&deep);
        assert!(text.contains("summary.md"), "{}", text);
        assert!(!text.contains("reports/summary.md"), "{}", text);
        // The check number of a known string, so an archive that opens is not a
        // matter of luck.
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }

    // The head of the block has something to say even while the model is writing
    // the first name.
    #[test]
    fn the_head_of_a_block_being_written_names_the_file() {
        assert_eq!(partial_words(r#"{"paths":["shared/pa"#), "pa");
        assert_eq!(partial_words(r#"{"paths":["shared/page.js"],"note":"A page"#), "page.js");
        assert_eq!(partial_words(r#"{"paths":["a.txt"],"note":"say hi"}"#), "a.txt");
        assert_eq!(partial_words(""), "");
    }
}
