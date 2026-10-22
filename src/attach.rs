// Attachments: the files a reader hangs on a message.
//
// A reader drags files on to the chat box, or pastes them, and each file
// becomes an attachment. The page reads a picture as a data URL and a file
// of words as its words, and sends both with the message. The engine keeps
// the words of one attachment in a record of its own, and keeps the id, the
// name and the kind in the message.
//
// The records stand under a folder of the chat that holds them: the folder
// `.attachments` inside the repository of the chat, or inside the working
// folder the program gave a chat with no repository. A chat and its files
// therefore stand together, so the files of a chat are still there when the
// chat is opened again the next day, and they go with the repository when the
// reader moves it. A command of the tool sees them too, for the folder stands
// in the room the command works in.
//
// The words that go to the model name every attachment with its place in
// the order the reader attached it, and its whole file name, ending
// included. Pictures go out in that same order, each one spoken of by its
// number just before it stands. A reader can then write about the third
// image, and the model knows which picture that is. A file of words goes
// out as its words. A file of neither goes out by its name alone, for
// nothing else of it would mean anything to the model.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config;
use crate::store;

// The most attachments one message may carry. llama.cpp has a room of its
// own for pictures in one message, and a chat box with more blocks than
// this says less, not more.
pub const MAX_ATTACHMENTS: usize = 16;

// The most words of one attachment that go to the model. A file of more is
// cut, and the model is told that it was cut.
pub const MAX_TEXT_CHARS: usize = 200_000;

// The longest file name kept. A name longer than this is cut at the end.
pub const MAX_NAME_CHARS: usize = 200;

// The words that stand after a file of words the model was not given whole.
// The same words stand wherever a file is cut, so the model is never told a
// whole file was given when it was not.
pub const CUT_NOTE: &str = "(the file was cut here, because it held more text than this program sends)";

// The most bytes one attachment may hold, counted in the base64 form the
// page sends. A file larger than this has no business standing in a chat.
pub const MAX_DATA_CHARS: usize = 24 * 1024 * 1024;

// Endings that stand for a picture the vision projector can read.
const IMAGE_ENDINGS: [&str; 8] = [
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff",
];

// Endings that stand for a file of words. The model reads these as they
// are, so they go out among the words of the message.
const TEXT_ENDINGS: [&str; 29] = [
    "txt", "md", "markdown", "rs", "py", "js", "ts", "jsx", "tsx", "json", "toml", "yaml",
    "yml", "ini", "cfg", "conf", "sh", "bash", "csv", "log", "sql", "html", "css", "c", "h",
    "cpp", "hpp", "go", "java",
];

// One attachment as the page sends it: the whole file name with its ending,
// the kind of the file, the data URL of a picture, or the words of a file
// of words. A file that is neither carries neither.
#[derive(Clone, Debug, Default)]
pub struct Incoming {
    pub name: String,
    pub mime: String,
    pub data: String,
    pub text: String,
}

// What stands in the record of one attachment on disk.
#[derive(Clone, Debug, Default)]
pub struct Stored {
    pub name: String,
    pub mime: String,
    pub data: String,
    pub text: String,
}

// The name of the folder of the records, inside the room of the chat.
pub const FOLDER: &str = ".attachments";

// The folder that holds the records of the attachments of one chat. It stands
// inside the room that chat works in, which is its repository, or its own
// folder under `work/` when it has no repository.
pub fn shelf(workdir: &Path) -> PathBuf {
    workdir.join(FOLDER)
}

// The folder an older run of the program kept the records in: one flat folder
// under the data directory, with no room of the chat in it. It is looked at
// once, and only to move what stands in it away.
pub fn old_dir() -> PathBuf {
    config::paths().data.join("attachments")
}

// Move the records of these attachments out of the old folder into the folder
// of the chat that holds them. A record that stands in the new place already
// is left where it is, and a record that is in neither place is nothing to
// move. The number of what moved comes back, so the caller can say whether the
// old folder is empty now.
pub fn move_old(place: &Path, ids: &[String]) -> usize {
    move_from(&old_dir(), place, ids)
}

// The same move, between two folders that are named. This is what the move of
// the old records comes down to, and it is what a test can try without
// touching the place the program keeps its data in.
pub fn move_from(from: &Path, place: &Path, ids: &[String]) -> usize {
    if !from.is_dir() {
        return 0;
    }
    let mut moved = 0usize;
    for id in ids {
        if !looks_like_id(id) {
            continue;
        }
        let name = format!("{}.json", id);
        let was = from.join(&name);
        let now = place.join(&name);
        if !was.is_file() || now.exists() {
            continue;
        }
        if !place.is_dir() && fs::create_dir_all(place).is_err() {
            break;
        }
        if fs::rename(&was, &now).is_ok() {
            moved += 1;
        }
    }
    moved
}

// The record of one attachment, looked for under a list of rooms. Every room
// holds chats, and the record of a chat stands in a folder of that chat, so
// this walks the rooms and opens only the record it was asked for. It is the
// slow way, for when the chat that holds the record is not known.
pub fn find_in(rooms: &[PathBuf], id: &str) -> Option<Stored> {
    if !looks_like_id(id) {
        return None;
    }
    for room in rooms {
        let Ok(entries) = fs::read_dir(room) else {
            continue;
        };
        for entry in entries.flatten() {
            let chat = entry.path();
            if !chat.is_dir() {
                continue;
            }
            if let Some(record) = read_at(&shelf(&chat), id) {
                return Some(record);
            }
        }
    }
    None
}

// Take the old folder away when it holds nothing left.
pub fn drop_old_dir() {
    let _ = fs::remove_dir(old_dir());
}

// True for the ten hexadecimal characters of an id, and for nothing else. A
// caller from the web may hand over anything, so nothing that smells like a
// road is opened.
fn looks_like_id(id: &str) -> bool {
    id.len() == 10 && id.chars().all(|c| c.is_ascii_hexdigit())
}

// True when the attachment is a picture the model can be shown. An unknown
// kind falls back to the ending of the file name.
pub fn is_image(mime: &str, name: &str) -> bool {
    if mime.starts_with("image/") {
        // The web page kind of a picture is words, and this program has no
        // reader for it. It stands among the files of words instead.
        return !mime.contains("svg");
    }
    if mime.starts_with("text/") || !mime.is_empty() && mime != "application/octet-stream" {
        return false;
    }
    match ending(name) {
        Some(end) => IMAGE_ENDINGS.contains(&end.as_str()),
        None => false,
    }
}

// True when the attachment is a file of words.
pub fn is_text(mime: &str, name: &str) -> bool {
    if mime.contains("svg") {
        return true;
    }
    if mime.is_empty() || mime == "application/octet-stream" {
        return match ending(name) {
            Some(end) => TEXT_ENDINGS.contains(&end.as_str()),
            None => false,
        };
    }
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/x-sh"
                | "application/javascript"
                | "application/typescript"
                | "application/xml"
                | "application/x-yaml"
                | "application/toml"
        )
}

// Which of the three kinds a file is, said in the word the page and the
// window both use for a block over the chat box: a picture, a file of words,
// or a file that can only be named.
pub fn kind_of(name: &str, mime: &str) -> &'static str {
    if is_image(mime, name) {
        "image"
    } else if is_text(mime, name) {
        "text"
    } else {
        "file"
    }
}

// The kind of file the ending of its name says it is. The window needs this
// when it takes a file from the machine by its place, for a place on the disk
// carries no kind of its own. A name with no ending, or an ending this
// program does not know, stands as a stream of bytes.
pub fn mime_of(name: &str) -> &'static str {
    match ending(name).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("tif") | Some("tiff") => "image/tiff",
        Some("svg") => "image/svg+xml",
        Some("txt") | Some("log") => "text/plain",
        Some("md") | Some("markdown") => "text/markdown",
        Some("html") | Some("htm") => "text/html",
        Some("css") => "text/css",
        Some("csv") => "text/csv",
        Some("js") | Some("mjs") => "text/javascript",
        Some("json") => "application/json",
        Some("toml") => "application/toml",
        Some("yaml") | Some("yml") => "application/x-yaml",
        Some("xml") => "application/xml",
        Some("sh") | Some("bash") => "application/x-sh",
        _ => "application/octet-stream",
    }
}

// The ending of a file name, in lower case. A name with no ending gives
// nothing.
fn ending(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    if dot + 1 >= name.len() || dot == 0 {
        return None;
    }
    Some(name[dot + 1..].to_ascii_lowercase())
}

// The file name the model is given: no folder road, no control characters,
// and cut at the length a block can show.
pub fn tidy_name(raw: &str) -> String {
    // A road from another system may come with the name. The last part of
    // it is the name of the file.
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let mut out = String::new();
    for ch in last.chars() {
        if ch.is_control() {
            continue;
        }
        out.push(ch);
        if out.chars().count() >= MAX_NAME_CHARS {
            break;
        }
    }
    out.trim().to_string()
}

// The attachments of a send call, in the order the page sent them. An entry
// with no name is dropped, and so is a picture or a text that is larger
// than the room. The order of what is left stands as the reader made it.
pub fn parse_incoming(value: Value) -> Vec<Incoming> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<Incoming> = Vec::new();
    for item in items.iter().take(MAX_ATTACHMENTS) {
        let name = tidy_name(item.get("name").and_then(|v| v.as_str()).unwrap_or(""));
        if name.is_empty() {
            continue;
        }
        let mime = item
            .get("mime")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let data = item
            .get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let text = item
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if data.len() > MAX_DATA_CHARS || text.chars().count() > MAX_TEXT_CHARS {
            continue;
        }
        out.push(Incoming {
            name,
            mime,
            data,
            text,
        });
    }
    out
}

// Write the records of new attachments in the folder of the chat that holds
// them, and give back the marks the message keeps. A record that cannot be
// written loses that attachment, and the reader is told of it by the count
// that comes back short.
pub fn save_at(place: &Path, files: &[Incoming]) -> Vec<store::Attachment> {
    let _ = fs::create_dir_all(place);
    let mut out: Vec<store::Attachment> = Vec::new();
    for file in files {
        let id = store::new_id();
        let record = json!({
            "name": file.name,
            "mime": file.mime,
            "data": file.data,
            "text": file.text,
        });
        let text = serde_json::to_string(&record).unwrap_or_else(|_| String::from("{}"));
        if fs::write(record_path(place, &id), text).is_err() {
            continue;
        }
        out.push(store::Attachment {
            id,
            name: file.name.clone(),
            mime: file.mime.clone(),
            image: is_image(&file.mime, &file.name),
        });
    }
    out
}

// The words of one stored attachment, for the model. A record that is gone
// says that it is gone, so the model hears the truth and not a blank space.
fn stored_text_at(place: &Path, id: &str) -> String {
    match read_at(place, id) {
        Some(record) if !record.text.is_empty() => {
            let mut words = record.text.as_str();
            if words.chars().count() > MAX_TEXT_CHARS {
                let cut = words
                    .char_indices()
                    .nth(MAX_TEXT_CHARS)
                    .map(|(at, _)| at)
                    .unwrap_or(words.len());
                words = &words[..cut];
                return format!("{}\n{}", words, CUT_NOTE);
            }
            words.to_string()
        }
        Some(_) => String::from("(the file held no text)"),
        None => String::from("(the file was missing when the model was asked again)"),
    }
}

// The record of one attachment, read from the folder of the chat.
pub fn read_at(place: &Path, id: &str) -> Option<Stored> {
    if !looks_like_id(id) {
        return None;
    }
    let path = record_path(place, id);
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let text_of = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    // A record with no name is not one of ours.
    let _ = value.get("name").and_then(|v| v.as_str())?;
    Some(Stored {
        name: text_of("name"),
        mime: text_of("mime"),
        data: text_of("data"),
        text: text_of("text"),
    })
}

// Take one attachment away from the disk. The chat that held it is gone, so
// its records may go too.
pub fn remove_at(place: &Path, ids: &[String]) {
    for id in ids {
        if looks_like_id(id) {
            let _ = fs::remove_file(record_path(place, id));
        }
    }
}

// The way from the folder of the chat to the record of one attachment. The id
// is ten hexadecimal characters, so nothing that smells like a road is opened.
fn record_path(place: &Path, id: &str) -> PathBuf {
    place.join(format!("{}.json", id))
}

// The content of a user message for the API. With no attachments this is
// the words of the reader, exactly as they were before.
//
// With attachments it is a list of parts. The words of the reader come
// first, and after them the list of the attachments with their numbers.
// Then, in the same order, a file of words follows as a part of its own,
// and a picture follows as an image part with one line of words before it
// that names its number and its file. The pictures therefore stand in the
// order of the list, and each can be named by its number.
pub fn content_at(place: &Path, body: &str, attachments: &[store::Attachment], vision_on: bool) -> Value {
    if attachments.is_empty() {
        return json!(body);
    }
    let mut parts: Vec<Value> = Vec::new();
    let mut lead = String::new();
    if !body.trim().is_empty() {
        lead.push_str(body.trim_end());
        lead.push('\n');
    }
    lead.push('\n');
    lead.push_str(&format!(
        "This message has {} attachments, in this order:\n",
        attachments.len()
    ));
    let mut picture_of = 0usize;
    for (number, one) in attachments.iter().enumerate() {
        let kind = if is_image(&one.mime, &one.name) {
            if !vision_on {
                String::from("an image, which this model cannot see, because vision is off")
            } else {
                // The number of the picture counts only the pictures that
                // really go out, so "the third image" is never a gap.
                picture_of += 1;
                format!("an image, image number {}", picture_of)
            }
        } else if is_text(&one.mime, &one.name) {
            String::from("a text file, given in full below")
        } else {
            String::from("a file that cannot be read here, so only its name is sent to the model")
        };
        lead.push_str(&format!("{}. {} ({})\n", number + 1, one.name, kind));
    }
    lead.push('\n');
    parts.push(json!({ "type": "text", "text": lead }));

    let mut picture_of = 0usize;
    for (number, one) in attachments.iter().enumerate() {
        if is_image(&one.mime, &one.name) {
            if !vision_on {
                continue;
            }
            picture_of += 1;
            let kept = read_at(place, &one.id);
            let data = kept.map(|made| made.data).unwrap_or_default();
            if data.is_empty() {
                parts.push(json!({
                    "type": "text",
                    "text": format!(
                        "Image number {}, attachment {} named {}, was missing when the model was asked again.",
                        picture_of,
                        number + 1,
                        one.name
                    ),
                }));
                continue;
            }
            parts.push(json!({
                "type": "text",
                "text": format!(
                    "Here is image number {}, attachment {} named {}:",
                    picture_of,
                    number + 1,
                    one.name
                ),
            }));
            parts.push(json!({
                "type": "image_url",
                "image_url": { "url": data },
            }));
        } else if is_text(&one.mime, &one.name) {
            parts.push(json!({
                "type": "text",
                "text": format!(
                    "The text of attachment {}, named {}:\n{}",
                    number + 1,
                    one.name,
                    stored_text_at(place, &one.id)
                ),
            }));
        }
    }
    Value::Array(parts)
}

// The words of a tool answer, with the pictures that hang on the same message
// after them. A tool that opened a picture gives the words of its receipt here
// and the picture itself as a part, so that the model is shown the picture and
// not a run of base64 written out in words. Without the switch for pictures, or
// without a picture on the message, these are the plain words, as they were
// before the tool of pictures came.
pub fn parts_at(
    place: &Path,
    words: &str,
    attachments: &[store::Attachment],
    vision_on: bool,
) -> Value {
    let pictures: Vec<&store::Attachment> = attachments.iter().filter(|one| one.image).collect();
    if !vision_on || pictures.is_empty() {
        return json!(words);
    }
    let mut parts = vec![json!({ "type": "text", "text": words })];
    for one in pictures {
        match read_at(place, &one.id) {
            Some(record) if !record.data.is_empty() => parts.push(json!({
                "type": "image_url",
                "image_url": { "url": record.data },
            })),
            _ => parts.push(json!({
                "type": "text",
                "text": format!(
                    "The image named {} was missing when the model was asked again.",
                    one.name
                )
            })),
        }
    }
    Value::Array(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The records of the attachments stand in a folder of the chat that
    // holds them, so a test makes a place of its own to keep them in. Each
    // test has a folder no other test looks at, for the tests run together.
    fn test_shelf() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let place = std::env::temp_dir().join(format!("atnatter-attach-test-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&place);
        fs::create_dir_all(&place).expect("a test place for the records can be made");
        place
    }

    fn incoming(name: &str, mime: &str, data: &str, text: &str) -> Incoming {
        Incoming {
            name: name.to_string(),
            mime: mime.to_string(),
            data: data.to_string(),
            text: text.to_string(),
        }
    }

    // The picture kind is known from the kind of the file, and from the
    // ending when the kind says nothing. A web picture is words, and the
    // ending of a picture alone does not make a text file a picture.
    #[test]
    fn a_picture_is_known_from_its_kind_or_its_ending() {
        assert!(is_image("image/png", "photo"));
        assert!(is_image("", "photo.PNG"));
        assert!(is_image("application/octet-stream", "shot.jpeg"));
        assert!(!is_image("image/svg+xml", "drawing.svg"));
        assert!(!is_image("text/plain", "notes.txt"));
        assert!(!is_image("application/json", "data.json"));
    }

    // A file of words is known from its kind, or from its ending when the
    // kind says nothing.
    #[test]
    fn a_file_of_words_is_known_from_its_kind_or_its_ending() {
        assert!(is_text("text/plain", "notes"));
        assert!(is_text("application/json", "data"));
        assert!(is_text("image/svg+xml", "drawing.svg"));
        assert!(is_text("", "main.rs"));
        assert!(is_text("application/octet-stream", "cargo.toml"));
        assert!(!is_text("", "photo.png"));
        assert!(!is_text("application/pdf", "paper.pdf"));
    }

    // A name keeps its ending, loses its road, and loses the characters
    // that no file name should hold.
    #[test]
    fn a_name_keeps_its_ending_and_loses_its_road() {
        assert_eq!(tidy_name("/home/user/notes.txt"), "notes.txt");
        assert_eq!(tidy_name("C:\\temp\\photo.jpg"), "photo.jpg");
        assert_eq!(tidy_name("bad\u{0}name.md"), "badname.md");
        assert_eq!(tidy_name("   spaced.txt   "), "spaced.txt");
        let long = "a".repeat(400);
        assert_eq!(tidy_name(&long).chars().count(), MAX_NAME_CHARS);
    }

    // The send call gives its attachments in order. An entry with no name
    // is dropped, and nothing past the ceiling is kept.
    #[test]
    fn the_send_call_gives_its_attachments_in_order() {
        let body = json!([
            { "name": "first.png", "mime": "image/png", "data": "data:image/png;base64,AAA=" },
            { "name": "", "mime": "text/plain", "text": "no name, so nothing" },
            { "name": "second.txt", "mime": "text/plain", "text": "the words" },
        ]);
        let made = parse_incoming(body);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].name, "first.png");
        assert_eq!(made[1].name, "second.txt");
        assert_eq!(made[1].text, "the words");
        // A file over the room is dropped, so a giant cannot stand in a chat.
        let big = json!([{ "name": "big.txt", "mime": "text/plain", "text": "x".repeat(MAX_TEXT_CHARS + 1) }]);
        assert!(parse_incoming(big).is_empty());
    }

    // With no attachments the message goes out exactly as it did before
    // this part of the program existed.
    #[test]
    fn a_message_without_attachments_sends_plain_words() {
        let made = content_at(Path::new("."), "Hello there", &[], true);
        assert_eq!(made, json!("Hello there"));
    }

    // The words that go out name every attachment by its number and its
    // whole file name, count the pictures apart from the files of words,
    // and keep the pictures in the order of the list.
    #[test]
    fn the_attachments_stand_numbered_in_the_order_they_came() {
        let _guard = config::test_turn();
        let shelf = test_shelf();
        let files = vec![
            incoming("photo.png", "image/png", "data:image/png;base64,AAA=", ""),
            incoming("notes.txt", "text/plain", "", "the words of the note"),
            incoming("tool.zip", "application/zip", "", ""),
        ];
        let kept = save_at(&shelf, &files);
        assert_eq!(kept.len(), 3);
        assert!(kept[0].image);
        assert!(!kept[1].image);

        let made = content_at(&shelf, "Look at these", &kept, true);
        let parts = made.as_array().expect("the content is a list of parts");
        let lead = parts[0].get("text").unwrap().as_str().unwrap();
        assert!(lead.starts_with("Look at these"));
        assert!(lead.contains("1. photo.png (an image, image number 1)"));
        assert!(lead.contains("2. notes.txt (a text file, given in full below)"));
        assert!(lead.contains("3. tool.zip (a file that cannot be read here"));
        // The words of the text file go out under their number and name.
        let words = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .find(|t| t.starts_with("The text of attachment 2"))
            .expect("the words of the second attachment");
        assert!(words.contains("named notes.txt"));
        assert!(words.contains("the words of the note"));
        // The picture goes out as an image part, with its number named just
        // before it.
        let image_at = parts
            .iter()
            .position(|p| p.get("type").and_then(|t| t.as_str()) == Some("image_url"))
            .expect("the picture goes out");
        let before = parts[image_at - 1].get("text").unwrap().as_str().unwrap();
        assert!(before.contains("image number 1"));
        assert!(before.contains("photo.png"));
        let url = parts[image_at]["image_url"]["url"].as_str().unwrap();
        assert_eq!(url, "data:image/png;base64,AAA=");
        // The file of neither kind stands by its name alone, and asks for
        // no part of its own.
        assert_eq!(
            parts
                .iter()
                .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("image_url"))
                .count(),
            1
        );

        remove_at(&shelf, &kept.iter().map(|a| a.id.clone()).collect::<Vec<_>>());
    }

    // Two pictures take two numbers in the order the reader attached them.
    // With the vision switch off, no picture goes out, and the list says
    // why.
    #[test]
    fn pictures_keep_their_order_and_their_names() {
        let _guard = config::test_turn();
        let shelf = test_shelf();
        let files = vec![
            incoming("cat.png", "image/png", "data:image/png;base64,CAT", ""),
            incoming("dog.png", "image/png", "data:image/png;base64,DOG", ""),
        ];
        let kept = save_at(&shelf, &files);
        let made = content_at(&shelf, "Which one barks?", &kept, true);
        let parts = made.as_array().unwrap();
        let urls: Vec<&str> = parts
            .iter()
            .filter_map(|p| p.get("image_url").and_then(|u| u.get("url")))
            .filter_map(|u| u.as_str())
            .collect();
        assert_eq!(urls, vec!["data:image/png;base64,CAT", "data:image/png;base64,DOG"]);
        let lead = parts[0].get("text").unwrap().as_str().unwrap();
        assert!(lead.contains("image number 1"));
        assert!(lead.contains("image number 2"));

        let off = content_at(&shelf, "Which one barks?", &kept, false);
        let parts = off.as_array().unwrap();
        assert!(!parts
            .iter()
            .any(|p| p.get("type").and_then(|t| t.as_str()) == Some("image_url")));
        let lead = parts[0].get("text").unwrap().as_str().unwrap();
        assert!(lead.contains("vision is off"));

        remove_at(&shelf, &kept.iter().map(|a| a.id.clone()).collect::<Vec<_>>());
    }

    // The record of an attachment reads back as it was written, and an id
    // that is not an id opens nothing.
    #[test]
    fn a_record_reads_back_and_a_strange_id_opens_nothing() {
        let _guard = config::test_turn();
        let shelf = test_shelf();
        let files = vec![incoming("note.md", "text/markdown", "", "# Head")];
        let kept = save_at(&shelf, &files);
        let made = read_at(&shelf, &kept[0].id).expect("the record reads back");
        assert_eq!(made.name, "note.md");
        assert_eq!(made.text, "# Head");
        assert!(read_at(&shelf, "").is_none());
        assert!(read_at(&shelf, "../../etc/passwd").is_none());
        remove_at(&shelf, &kept.iter().map(|a| a.id.clone()).collect::<Vec<_>>());
        assert!(read_at(&shelf, &kept[0].id).is_none());
    }

    // The records stand in a folder of the room of the chat, under the name
    // `.attachments`, so a chat and its files are found together.
    #[test]
    fn the_records_stand_in_a_folder_of_the_room_of_the_chat() {
        let room = test_shelf();
        let folder = shelf(&room);
        assert_eq!(folder.file_name().and_then(|n| n.to_str()), Some(".attachments"));
        let files = vec![incoming("note.md", "text/markdown", "", "words")];
        let kept = save_at(&folder, &files);
        assert_eq!(kept.len(), 1, "the record was written");
        assert!(record_path(&folder, &kept[0].id).is_file(), "it stands in the folder");
        assert!(read_at(&folder, &kept[0].id).is_some());
        remove_at(&folder, &[kept[0].id.clone()]);
        let _ = fs::remove_dir_all(&room);
    }

    // A run of the program that kept the records in one flat folder moves them
    // into the folder of the chat that carries them. A record that is not
    // asked for, and one that stands in the new place already, stay put.
    #[test]
    fn the_records_of_an_older_run_move_into_the_room_of_the_chat() {
        let rooms = test_shelf();
        let old = rooms.join("of-old-run");
        let room = rooms.join("the-chat");
        let folder = shelf(&room);
        fs::create_dir_all(&old).expect("the old folder can be made");
        let files = vec![
            incoming("a.txt", "text/plain", "", "one"),
            incoming("b.txt", "text/plain", "", "two"),
        ];
        let kept = save_at(&folder, &files);
        let now = record_path(&folder, &kept[0].id);
        let was = record_path(&old, &kept[0].id);
        fs::rename(&now, &was).expect("a record goes back to the old folder");
        let ids = vec![kept[0].id.clone(), kept[1].id.clone()];
        assert_eq!(move_from(&old, &folder, &ids), 1, "one record moved");
        assert!(now.is_file(), "the record stands with the chat now");
        assert!(!was.exists(), "the old folder holds it no longer");
        assert!(read_at(&folder, &kept[0].id).is_some());
        assert_eq!(move_from(&old, &folder, &ids), 0, "nothing left to move");
        let _ = fs::remove_dir_all(&rooms);
    }

    // When the chat that holds a record is not known, the rooms of the chats
    // are walked, and only the record that was asked for is opened.
    #[test]
    fn a_record_is_found_by_walking_the_rooms() {
        let rooms = test_shelf();
        let chat = rooms.join("chat-one");
        let files = vec![incoming("found.txt", "text/plain", "", "here")];
        let kept = save_at(&shelf(&chat), &files);
        let made = find_in(&[rooms.clone()], &kept[0].id).expect("the record is found");
        assert_eq!(made.name, "found.txt");
        assert!(find_in(&[rooms.clone()], "0000000000").is_none());
        assert!(find_in(&[rooms.clone()], "../outside").is_none());
        let _ = fs::remove_dir_all(&rooms);
    }

    // A place on the disk carries no kind of its own, so the window is given
    // one from the ending of the name, and the three kinds of block follow.
    #[test]
    fn a_place_on_disk_is_named_by_its_ending() {
        assert_eq!(mime_of("photo.png"), "image/png");
        assert_eq!(mime_of("photo.JPG"), "image/jpeg");
        assert_eq!(mime_of("notes.md"), "text/markdown");
        assert_eq!(mime_of("no-ending"), "application/octet-stream");
        assert_eq!(kind_of("photo.png", mime_of("photo.png")), "image");
        assert_eq!(kind_of("notes.md", mime_of("notes.md")), "text");
        assert_eq!(kind_of("model.bin", mime_of("model.bin")), "file");
        // A kind of its own outranks the ending, for the machine that handed
        // the file over knows better than a name.
        assert_eq!(kind_of("copy", "image/png"), "image");
    }
}
