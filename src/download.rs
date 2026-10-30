// The tool that hands a file of the chat to the reader.
//
// The model names a file that stands in the room of the chat, and the program
// puts a block of its own into the talk: the mark of the kind of file, the name
// of it, the extension under the name, and a button. The reader presses the
// button and the file comes down. Nothing is fetched, and nothing is written:
// the file stands where the model put it, and the block only names it.
//
// The block is read again after a restart of the program, and in the page of
// the remote mode. The bytes are asked for by the id of the message the block
// stands in, and are let through only when the file still stands in the room of
// that chat. A file outside the room of the chat is not handed out at all, which
// is the rule the file tools have always worked to.
//
// The tool is offered under the badge of the tools, and falls under it as the
// other internal tools do: the badge is the switch over all of them, and the
// drop-down of the badge holds the switch of this one.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::tools::{self, with_rule};

// Name the model uses to ask for a file to be handed to the reader.
pub const NAME: &str = "download_file";

// The longest name of a file the block may show. A name above this is cut in
// the middle, so the block keeps its shape.
pub const GREATEST_NAME: usize = 90;

// The most bytes of one file that are kept for the reader. A file above this is
// named but not handed out, for a block that fills the memory of the window is
// no use to anyone.
pub const GREATEST_FILE: u64 = 64 * 1024 * 1024;

// The tool as the model is told of it.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": with_rule(String::from(
                "Put a download block into the chat for one file of the working directory, so the reader can take that file with the button of the block. Use it when the work left a file the reader asked for: a document, a picture, an archive of the work. The block shows the kind of the file, its name without the extension, the extension under the name, and the button. Call it with nothing else around it: the block stands in the chat on its own. A file that is not there, or that is bigger than 64 MiB, is answered and no block stands.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file to hand to the reader. The name is relative to the working directory of the chat, for example 'report.md' or 'build/picture.png'."
                    },
                    "note": {
                        "type": "string",
                        "description": "A short line for the block, in the words of the chat, such as 'The report of the run'. It is shown under the name of the file. Nothing is put there when it is left out."
                    }
                },
                "required": ["path"]
            }
        }
    })
}

// What became of one ask. The block stands in the chat when `held` is true, and
// the words of the receipt go back to the model either way.
pub struct Made {
    pub report: crate::tools::Report,
    // True when the file was there and the block may stand.
    pub held: bool,
    // The name of the file as the block shows it, with the extension off.
    pub name: String,
    // The extension of the file, without the point. Empty when the name has
    // none.
    pub extension: String,
    // The kind of the file, which says which mark the block draws. See `kind`.
    pub kind: String,
    // The words under the name, when the model gave any.
    pub note: String,
}

// Answer one call. The file is looked for in the room of the chat, and only in
// that room.
pub fn offer(workdir: &Path, arguments: &str) -> Made {
    let value: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let asked = value.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let note = trim_note(value.get("note").and_then(|v| v.as_str()).unwrap_or(""));
    if asked.trim().is_empty() {
        return none("No file was named, so nothing is offered to the reader");
    }
    let Some(inside) = place_of(workdir, asked) else {
        return none(
            "That file is outside the working directory of the chat, so it is not offered",
        );
    };
    if !inside.is_file() {
        return none(&format!(
            "There is no file at {}, so nothing is offered",
            first_line_of(asked)
        ));
    }
    let size = std::fs::metadata(&inside).map(|m| m.len()).unwrap_or(0);
    if size > GREATEST_FILE {
        return none(&format!(
            "{} is {} mebibytes, above the 64 MiB one block may carry",
            first_line_of(asked),
            size / (1024 * 1024)
        ));
    }
    let name = inside
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| asked.to_string());
    let (stem, tail) = split_name(&name);
    let kind = kind(&tail);
    Made {
        report: crate::tools::Report {
            ok: true,
            output: format!(
                "The file {} is offered to the reader in a download block. The reader takes it with the button of the block.",
                name
            ),
        },
        held: true,
        name: stem,
        extension: tail,
        kind: kind.to_string(),
        note,
    }
}

// The words of a receipt for a call that gave no block.
fn none(words: &str) -> Made {
    Made {
        report: crate::tools::Report {
            ok: false,
            output: words.to_string(),
        },
        held: false,
        name: String::new(),
        extension: String::new(),
        kind: String::new(),
        note: String::new(),
    }
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

// The first line of what the model wrote, for a receipt.
fn first_line_of(words: &str) -> String {
    words.lines().next().unwrap_or("").trim().to_string()
}

// The name of a file, cut into the part the block shows big and the extension
// it shows under it. A point at the start of a name is not an extension, for a
// name of that shape is a hidden file of the machine.
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
// these, and the reader knows the mark without reading the extension.
pub fn kind(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif" | "tif" | "tiff" => {
            "image"
        }
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" => "sound",
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" => "picture of a film",
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "zst" | "tgz" => "archive",
        "rs" | "py" | "js" | "ts" | "go" | "c" | "h" | "cpp" | "java" | "rb" | "php" | "sh"
        | "json" | "toml" | "yaml" | "yml" | "xml" | "sql" | "css" | "html" => "code",
        "md" | "txt" | "log" | "csv" | "pdf" | "doc" | "docx" | "odt" | "rtf" | "tex" => {
            "document"
        }
        "exe" | "sh" | "run" | "appimage" | "msi" | "deb" | "rpm" => "program",
        _ => "file",
    }
}

// True when this name of a tool is the download tool.
pub fn is_tool(name: &str) -> bool {
    name == NAME
}

// The words of the head of the block while the call is being written: the name
// of the file as far as the model has written it.
pub fn partial_words(arguments: &str) -> String {
    editfile::partial_words(arguments)
}

// The road of a file of the chat, kept inside the room of the chat. A name that
// steps out of the room, or that leads into the records of the attachments, is
// refused, which is the rule the file tools work to as well.
fn place_of(workdir: &Path, asked: &str) -> Option<PathBuf> {
    let held = editfile::place_in(workdir, asked).ok()?;
    Some(held)
}

// The road of a file of the chat, out of the message that holds its block. The
// road is kept inside the room of the chat, which is what the reader of the
// bytes holds it to. A name that tries to leave the room gives nothing.
pub fn road_in(workdir: &Path, asked: &str) -> Option<PathBuf> {
    place_of(workdir, asked)
}
