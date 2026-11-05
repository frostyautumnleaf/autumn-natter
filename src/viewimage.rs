// The tool that hands a picture to the model.
//
// A picture may stand in the room of the chat, or it may be addressed straight
// on the web. Both roads end in the same place: the bytes are read, the kind of
// the picture is read out of the bytes and not out of the end of the name, and
// the picture is written as a record in the folder of the attachments of the
// chat. The message of the run carries that record, and the words the model is
// sent carry the picture itself after the short receipt of the run. That is how
// a picture reaches a vision model here: as a part of the answer of the tool,
// and not as a run of base64 in the middle of a sentence.
//
// The block the user sees holds the receipt and nothing else. The bytes go to
// the model alone, so no block of the chat can ever hold a wall of base64.
//
// A picture can only be shown to a model that can look at pictures: the vision
// switch of the model settings must stand on, and a projector file must stand
// next to the model for the server to take. Without both there is nothing to
// hand a picture to, so the tool says so plainly and reads no bytes at all.
//
// The kind of the picture is read from the marks at its front, for a name may
// say anything. Only the kinds the projector of llama.cpp can turn into pixels
// are let through: PNG, JPEG, GIF and BMP. A WebP or a TIFF of a trusting name
// would be nothing over there, so it is refused here, where a reason can still
// be given.

use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::editfile;
use crate::tools::{self, with_rule};
use crate::util;

// The name the model uses to have pictures shown to it. The tool was named for
// one picture, and is named for the two of them now that one call may carry more
// than one. A call that comes in under the name of one is still read, for a model
// that read the list of tools of the talk above may write that name again.
pub const NAME: &str = "view_images";
pub const ONE_NAME: &str = "view_image";

// True when a call of this name is a look at pictures, under either of the two
// names the tool has had.
pub fn is_look(name: &str) -> bool {
    name == NAME || name == ONE_NAME
}

// The most bytes of one picture. A picture of more than this is a wall of
// pixels, and a projector of this size makes little of a wall.
const MAX_PICTURE_BYTES: usize = 12 * 1024 * 1024;

// The most pictures one call may carry. Every picture is paid for in the room of
// the talk, and a call that hands over a hundred of them leaves nothing to say
// about them.
const MOST_PICTURES: usize = 8;

// The most pixels of one picture. Past this the picture holds more pixels than
// a projector uses, and every one of them is paid for in the room of the talk.
const MOST_PIXELS: u64 = 40_000_000;

// The words of the call, and the words the user is shown. The room the files may
// stand in follows the state of the sandbox, as it does for the two tools of the
// files: the wall over the room of the chat is the sandbox, and only the sandbox.
pub fn definition(sandbox: bool) -> Value {
    let said = if sandbox {
        "While the sandbox stands, every file looked at is a file of the working directory of the chat."
    } else {
        "A file may stand anywhere on this machine, and its whole name is then given."
    };
    let paths_said = if sandbox {
        "The names of the files to look at, each relative to the working directory of the chat."
    } else {
        "The names of the files to look at, each relative to the working directory of the chat, or whole when the file stands elsewhere on this machine."
    };
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": with_rule(format!(
                "Send one or more images to yourself, so that you can be asked about them. Give the names of files in the chat as a list, as [\"notes/cat.png\", \"notes/dog.png\"], or the addresses of images on the web as a list, as [\"https://example.org/cat.png\"]. One name or a list of names is read either way, and one call may carry up to eight images, which arrive in the order you named them. {} Each image must be of a kind the projector reads: PNG, JPEG, GIF or BMP, and no more than twelve megabytes. An image can only be shown while vision is turned on for the model; without it the call says so and nothing is read. The images are not stored after the conversation.",
                said
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": paths_said
                    },
                    "urls": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "The addresses of the images to look at, instead of or beside the files."
                    }
                },
                "required": []
            }
        }
    })
}

// The parts of a call that is finished: the names of the files, and the
// addresses of the pictures. A model writes a name other than the one that was
// asked for more often than one would think, so the other names a local model
// reaches for are read as well, and a part that holds one picture is read as a
// list of one.
pub fn parts_of(raw: &str) -> (Vec<String>, Vec<String>) {
    let Some(call) = object(raw) else {
        let text = raw.trim_start();
        return (
            some(tools::partial_text(text, "path")),
            some(tools::partial_text(text, "url")),
        );
    };
    (names_of(&call, &PATH_KEYS), names_of(&call, &URL_KEYS))
}

// The keys a model uses for the files of a call, and the keys it uses for the
// addresses of the web.
const PATH_KEYS: [&str; 8] =
    ["path", "paths", "file", "files", "file_path", "filepath", "filename", "name"];
const URL_KEYS: [&str; 7] = ["url", "urls", "uri", "uris", "href", "link", "image_url"];

// One piece of words, in a list of them, when it has words in it.
fn some(text: String) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    vec![text.trim().to_string()]
}

// The names under the keys of a call. A key holds one picture or a list of them,
// a name of nothing is passed over, and a name written twice is looked at once.
fn names_of(call: &Value, keys: &[&str]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for key in keys {
        match call.get(*key) {
            Some(Value::String(one)) => hold_name(&mut found, one),
            Some(Value::Array(list)) => {
                for one in list {
                    if let Some(text) = one.as_str() {
                        hold_name(&mut found, text);
                    }
                }
            }
            _ => {}
        }
    }
    found
}

// One name more, when it has words in it and has not been held already.
fn hold_name(found: &mut Vec<String>, text: &str) {
    let one = text.trim();
    if one.is_empty() || found.iter().any(|held| held == one) {
        return;
    }
    found.push(one.to_string());
}

// The one thing the call named, as far as it has come. The user watches this
// while the call is written. A name of a file is worth showing whole, for the
// address may not have come yet, and a list is watched from its first name.
pub fn partial_words(raw: &str) -> String {
    let text = raw.trim_start();
    for key in ["path", "paths", "file", "files"] {
        let name = tools::partial_text(text, key);
        if !name.is_empty() {
            return name;
        }
    }
    for key in ["url", "urls"] {
        let name = tools::partial_text(text, key);
        if !name.is_empty() {
            return name;
        }
    }
    first_of_a_list(text, &["paths", "path", "urls", "url"])
}

// The first name inside a list that is still being written. The mark of the list
// came, and the words of the first name of it are what the user should see.
fn first_of_a_list(text: &str, keys: &[&str]) -> String {
    for key in keys {
        let Some(at) = text.find(&format!("\"{}\"", key)) else { continue };
        let rest = &text[at + key.len() + 2..];
        let Some(open) = rest.find('[') else { continue };
        let after = &rest[open + 1..];
        let Some(quote) = after.find('"') else { continue };
        let tail = &after[quote + 1..];
        let mut name = String::new();
        for one in tail.chars() {
            if one == '"' || one == '\\' {
                break;
            }
            name.push(one);
        }
        if !name.is_empty() {
            return name;
        }
    }
    String::new()
}

// A picture that was read, in the shape the folder of the attachments wants: the
// whole file name, the kind of the file, and the bytes as a data URL.
pub struct Picture {
    pub name: String,
    pub mime: String,
    pub data: String,
}

// What one call came to: the receipt the model is given, and the pictures that
// go with it, when there are any to give. More than one picture goes in the order
// the call named them, and a picture the call named but the program could not
// read is not among them.
pub struct Opening {
    pub report: tools::Report,
    pub pictures: Vec<Picture>,
}

// Look at pictures. `vision_on` is the answer of the engine about the model in
// use: whether a picture can be shown to it at all. `in_room` is the wall of the
// chat: while the jail stands, a name of a file is taken inside the room of the
// chat and nowhere else, and with the jail off it reaches the machine as a
// command of the terminal tool does.
pub fn open(workdir: &Path, raw: &str, vision_on: bool, in_room: bool) -> Opening {
    let (wanted_paths, wanted_urls) = parts_of(raw);
    if wanted_paths.is_empty() && wanted_urls.is_empty() {
        return refused(
            "The call needs the name of one or more files in the chat, or the address of one or more images on the web",
        );
    }
    // With nothing to hand a picture to there is no work to do, so no byte is
    // read. The words say what stands in the way, for the model is to know
    // whether to ask again of another model or to say what the picture was for.
    if !vision_on {
        return refused(
            "the images cannot be shown, because this model has no vision: turn the vision setting of the model on and put a projector file next to the model, or describe in words what you need from the images",
        );
    }
    let mut pictures: Vec<Picture> = Vec::new();
    let mut said: Vec<String> = Vec::new();
    let mut not_shown: Vec<String> = Vec::new();
    let mut left_out = 0usize;
    let whole = wanted_paths.len() + wanted_urls.len();
    for part in 0..whole {
        let address = part >= wanted_paths.len();
        let what = if address {
            wanted_urls[part - wanted_paths.len()].clone()
        } else {
            wanted_paths[part].clone()
        };
        if pictures.len() + not_shown.len() >= MOST_PICTURES {
            left_out += 1;
            continue;
        }
        let read =
            if address { from_address(&what) } else { from_room(workdir, &what, in_room) };
        let (name, bytes) = match read {
            Ok(made) => made,
            Err(problem) => {
                not_shown.push(problem);
                continue;
            }
        };
        let kind = match kind_of(&bytes) {
            Some(kind) => kind,
            None => {
                not_shown.push(format!(
                    "'{}' is not an image this program can send. The projector reads PNG, JPEG, GIF and BMP. The type is read from the first bytes of the file, not from the end of its name.",
                    short(&name)
                ));
                continue;
            }
        };
        let size = pixels(kind, &bytes);
        if let Some((wide, high)) = size {
            if wide.saturating_mul(high) > MOST_PIXELS {
                not_shown.push(format!(
                    "'{}' is {} by {} pixels, which is more than this program sends ({} pixels at the most). The image would fill the context window and the projector would use little of it.",
                    short(&name),
                    util::count(wide),
                    util::count(high),
                    util::count(MOST_PIXELS)
                ));
                continue;
            }
        }
        let measure = match size {
            Some((wide, high)) => format!(
                "{}, {} by {} pixels, {}",
                kind.name,
                util::count(wide),
                util::count(high),
                weight(bytes.len())
            ),
            None => format!("{}, {}", kind.name, weight(bytes.len())),
        };
        said.push(format!("{}: {} ({})", name, measure, part + 1));
        pictures.push(Picture {
            mime: String::from(kind.mime),
            data: format!("data:{};base64,{}", kind.mime, util::b64(&bytes)),
            name,
        });
    }
    if pictures.is_empty() {
        let reason = if not_shown.is_empty() {
            format!(
                "The call named {} images, which is more than this program sends in one call ({} at the most). Ask again for fewer of them.",
                whole, MOST_PICTURES
            )
        } else {
            not_shown.join(" ")
        };
        return refused(&reason);
    }
    let mut output = String::new();
    if pictures.len() == 1 && not_shown.is_empty() && left_out == 0 {
        output.push_str(&format!(
            "Opened an image: {}. The image itself is sent to the model with this text, so you may be asked what it shows.",
            said[0]
        ));
    } else {
        output.push_str(&format!(
            "Opened {} images, in this order:",
            pictures.len()
        ));
        for (place, one) in said.iter().enumerate() {
            output.push_str(&format!("\n{}. {}", place + 1, one));
        }
        output.push_str("\nThe images themselves are sent to the model with this text, in the same order, so you may be asked what each of them shows.");
        if !not_shown.is_empty() {
            output.push_str(&format!("\nNot shown: {}", not_shown.join(" ")));
        }
        if left_out > 0 {
            output.push_str(&format!(
                "\nLeft out of the call: {} of them, because one call carries at most {} images.",
                left_out, MOST_PICTURES
            ));
        }
    }
    Opening {
        report: tools::Report { ok: true, output },
        pictures,
    }
}

// A picture of the chat. The room rule of the file tools stands over this road:
// while the jail stands, the name is joined onto the room of the chat and a road
// that goes up out of it is refused, and with the jail off the name reaches the
// machine as a command of the terminal tool does. The records of the attachments
// are kept out either way.
fn from_room(workdir: &Path, name: &str, in_room: bool) -> Result<(String, Vec<u8>), String> {
    let place = editfile::place_kept(workdir, name, in_room)?;
    let label = place
        .file_name()
        .map(|one| one.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    // The size is asked for before the bytes are taken in, so a file of a
    // thousand megabytes is refused and not carried about.
    if let Ok(made) = std::fs::metadata(&place) {
        over_the_ceiling(name, made.len() as usize)?;
    }
    let bytes = match std::fs::read(&place) {
        Ok(made) => made,
        Err(_) => {
            return Err(format!(
                "'{}' does not exist. Give the name of an image in the working directory of the chat, or the address of one on the web.",
                short(&label)
            ));
        }
    };
    over_the_ceiling(&label, bytes.len())?;
    Ok((label, bytes))
}

// A picture of the web. It is fetched here, in the program, and not by a command:
// the jail of a command has no network.
fn from_address(address: &str) -> Result<(String, Vec<u8>), String> {
    if !address.starts_with("http://") && !address.starts_with("https://") {
        return Err(format!(
            "\"{}\" is no address to read. An address starts with http:// or https://",
            short(address)
        ));
    }
    let bytes = fetch(address)?;
    over_the_ceiling(address, bytes.len())?;
    Ok((name_of_address(address), bytes))
}

// The bytes of one address, stopped at the ceiling of a picture plus a byte, so a
// page that is no picture and goes on for ever is not carried about.
fn fetch(address: &str) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(20))
        .redirects(5)
        .build();
    let made = agent
        .get(address)
        .set("user-agent", AGENT)
        .set("accept", "image/png, image/jpeg, image/gif, image/bmp")
        .call();
    let response = match made {
        Ok(response) => response,
        Err(problem) => return Err(format!("the image at {} could not be read: {}", address, first_words(&problem.to_string(), 200))),
    };
    let mut bytes: Vec<u8> = Vec::new();
    let mut stream = response.into_reader();
    let _ = stream
        .by_ref()
        .take((MAX_PICTURE_BYTES + 1) as u64)
        .read_to_end(&mut bytes);
    Ok(bytes)
}

// The words the browser of the machine says. An address of the web is asked as a
// browser would ask it, or a house of pictures answers with a test instead.
const AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

// Say when a picture is past the ceiling of bytes.
fn over_the_ceiling(name: &str, bytes: usize) -> Result<(), String> {
    if bytes <= MAX_PICTURE_BYTES {
        return Ok(());
    }
    Err(format!(
        "'{}' is larger than the {} limit for one image. Use a smaller image.",
        short(name),
        weight(MAX_PICTURE_BYTES)
    ))
}

// The name of the picture an address stands for: the last part of the road in
// front of the mark of a query, turned back into the characters it was written
// with. An address of nothing to name is named for what it is.
fn name_of_address(address: &str) -> String {
    let road = address.split(['?', '#']).next().unwrap_or(address);
    let last = road.rsplit('/').next().unwrap_or("").trim();
    if last.is_empty() {
        return String::from("an image from the web");
    }
    let cut = urlencoding::decode(last).unwrap_or(std::borrow::Cow::Borrowed(last));
    let name = first_words(cut.trim(), 200);
    if name.is_empty() {
        String::from("an image from the web")
    } else {
        name
    }
}

// One kind of picture, with the words the receipt calls it and the mark the data
// URL of it carries.
struct Kind {
    name: &'static str,
    mime: &'static str,
}

const PNG: Kind = Kind { name: "PNG", mime: "image/png" };
const JPEG: Kind = Kind { name: "JPEG", mime: "image/jpeg" };
const GIF: Kind = Kind { name: "GIF", mime: "image/gif" };
const BMP: Kind = Kind { name: "BMP", mime: "image/bmp" };

// The kind of a picture, from the marks at its front. Nothing comes back for a
// file that is none of the kinds the projector reads.
fn kind_of(bytes: &[u8]) -> Option<&'static Kind> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(&PNG);
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Some(&JPEG);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(&GIF);
    }
    if bytes.starts_with(b"BM") {
        return Some(&BMP);
    }
    None
}

// How wide and how high a picture is, for the ceiling of pixels. A kind whose
// height and width are not to be read from the front of it gives nothing, and the
// ceiling of bytes stands over it alone.
fn pixels(kind: &Kind, bytes: &[u8]) -> Option<(u64, u64)> {
    if kind.mime == PNG.mime {
        // The marks of the front, the length of the first part, the name of it,
        // and then the width and the height in forty eight bits each.
        let wide = big32(bytes.get(16..20)?)?;
        let high = big32(bytes.get(20..24)?)?;
        return Some((wide, high));
    }
    if kind.mime == GIF.mime {
        // The two numbers of sixteen bits after the six marks of the front, of
        // which the little end comes first.
        let wide = little16(bytes.get(6..8)?)?;
        let high = little16(bytes.get(8..10)?)?;
        return Some((wide, high));
    }
    if kind.mime == BMP.mime {
        // The header of a Windows picture carries the two numbers of thirty two
        // bits, little end first, and a negative height only says which way the
        // rows of pixels run.
        let wide = little32(bytes.get(18..22)?)?;
        let high = (little32(bytes.get(22..26)?)? as i32).unsigned_abs() as u64;
        return Some((wide, high));
    }
    jpeg_pixels(bytes)
}

// The width and the height of a JPEG. The parts of one stand one behind another,
// each with its own mark, and the part that says how big the picture is is the
// one that begins with the mark of a start of frame.
fn jpeg_pixels(bytes: &[u8]) -> Option<(u64, u64)> {
    let mut at = 2usize;
    while at + 4 <= bytes.len() {
        if bytes[at] != 0xff {
            return None;
        }
        let mark = bytes[at + 1];
        // A mark of nothing but a fill, and a mark of a start of frame with no
        // length of its own, carry no length.
        if mark == 0xff || (0xd0..=0xd9).contains(&mark) || mark == 0x01 {
            at += 2;
            continue;
        }
        let length = big16(bytes.get(at + 2..at + 4)?)?;
        // The marks of a start of frame, of either of the two orders, and of the
        // ones of a lossy or a lossless coding.
        let of_frame = (0xc0..=0xcf).contains(&mark) && !matches!(mark, 0xc4 | 0xc8 | 0xcc);
        if of_frame && at + 9 <= bytes.len() {
            let high = big16(bytes.get(at + 5..at + 7)?)?;
            let wide = big16(bytes.get(at + 7..at + 9)?)?;
            return Some((wide, high));
        }
        at += 2 + usize::try_from(length).unwrap_or(0);
    }
    None
}

// A number of sixteen bits, written with the great end first, as a JPEG writes
// the length of a part and the size of a picture.
fn big16(bytes: &[u8]) -> Option<u64> {
    if bytes.len() != 2 {
        return None;
    }
    Some(((bytes[0] as u64) << 8) | bytes[1] as u64)
}

// A number of sixteen bits, written with the little end first.
fn little16(bytes: &[u8]) -> Option<u64> {
    if bytes.len() != 2 {
        return None;
    }
    Some(((bytes[1] as u64) << 8) | bytes[0] as u64)
}

// A number of thirty two bits, written with the great end first, as the front of
// a PNG writes the width and the height.
fn big32(bytes: &[u8]) -> Option<u64> {
    if bytes.len() != 4 {
        return None;
    }
    Some(
        ((bytes[0] as u64) << 24)
            | ((bytes[1] as u64) << 16)
            | ((bytes[2] as u64) << 8)
            | bytes[3] as u64,
    )
}

// A number of thirty two bits, written with the little end first, as the header
// of a Windows picture writes them.
fn little32(bytes: &[u8]) -> Option<u64> {
    if bytes.len() != 4 {
        return None;
    }
    Some(
        ((bytes[3] as u64) << 24)
            | ((bytes[2] as u64) << 16)
            | ((bytes[1] as u64) << 8)
            | bytes[0] as u64,
    )
}

// How big a picture is, in the words of the receipt.
fn weight(bytes: usize) -> String {
    const KILO: usize = 1024;
    if bytes < 64 * KILO {
        return format!("{} bytes", util::count(bytes as u64));
    }
    let kilo = bytes as f64 / KILO as f64;
    if kilo < 1024.0 {
        return format!("{:.0} KiB", kilo);
    }
    format!("{:.1} MiB", kilo / 1024.0)
}

// A call that came to nothing, with the reason in it.
fn refused(problem: &str) -> Opening {
    Opening {
        report: tools::Report {
            ok: false,
            output: problem.to_string(),
        },
        pictures: Vec::new(),
    }
}

// Words cut short, so a name of a file of a thousand characters does not fill
// the reason of a refusal.
fn short(text: &str) -> String {
    first_words(text, 200)
}

// The first words of a run, cut at the length asked for, with a mark that says
// the rest was left out.
fn first_words(text: &str, most: usize) -> String {
    let out: String = text.chars().take(most).collect();
    if out.chars().count() < text.chars().count() {
        return format!("{} ...", out);
    }
    out
}

// The words of the call, made into a thing with parts in it.
fn object(raw: &str) -> Option<Value> {
    let text = raw.trim();
    let cut = tools::extract_object(text).unwrap_or(text);
    serde_json::from_str::<Value>(cut)
        .ok()
        .filter(|value| value.is_object())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // The room of a chat, made for one test and taken away again. The tests take
    // turns through a count, so two of them never work in one room.
    static TURN_OF_TEST: AtomicUsize = AtomicUsize::new(0);

    fn room_of(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "autumn-natter-view-image-{}-{}",
            std::process::id(),
            TURN_OF_TEST.fetch_add(1, Ordering::SeqCst)
        ));
        let place = root.join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&place).expect("the working directory of the chat could not be created");
        place
    }

    // A picture of one pixel, written the way a PNG is written: the marks of the
    // front, the header of it, the words of one pixel, and the mark of the end.
    // It is a real PNG, so the tool is tried against a picture and not against a
    // shape made up for the test.
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xf7, 0x03, 0x41, 0x43, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    // A PNG header of a picture of the size asked for, with nothing behind it.
    fn png_of(wide: u32, high: u32) -> Vec<u8> {
        let mut made = TINY_PNG[..24].to_vec();
        made[16..20].copy_from_slice(&wide.to_be_bytes());
        made[20..24].copy_from_slice(&high.to_be_bytes());
        made
    }

    // The room the pictures may stand in is said in both states, as it is for the
    // tools of the files. With the wall of the sandbox up, every file looked at is
    // a file of the working directory of the chat. With the wall down, a file of
    // this machine may be named whole, and the wall that is not there is not named
    // either.
    #[test]
    fn the_room_of_the_pictures_is_said_in_both_states() {
        let said = |made: &Value| {
            format!(
                "{} {}",
                made["function"]["description"].as_str().unwrap_or_default(),
                made["function"]["parameters"]["properties"]["paths"]["description"]
                    .as_str()
                    .unwrap_or_default()
            )
        };
        let up = said(&definition(true));
        assert!(up.contains("While the sandbox stands"), "{}", up);
        assert!(up.contains("working directory of the chat"), "{}", up);
        let down = said(&definition(false));
        assert!(down.contains("anywhere on this machine"), "{}", down);
        assert!(down.contains("whole"), "{}", down);
        assert!(!down.to_lowercase().contains("sandbox"), "{}", down);
    }

    // The call needs one of its two parts, and says so without naming the other
    // program.
    #[test]
    fn the_call_needs_one_of_the_two_things() {
        let room = room_of("nothing");
        let made = open(&room, "{}", true, true);
        assert!(!made.report.ok);
        assert!(made.report.output.contains("name of one or more files"), "{}", made.report.output);
        assert!(made.pictures.is_empty());
    }

    // Without a projector there is nothing to hand a picture to, so the bytes of
    // the picture are not even read.
    #[test]
    fn no_picture_is_read_while_vision_is_off() {
        let room = room_of("blind");
        std::fs::write(room.join("cat.png"), TINY_PNG).expect("the picture could not be written");
        let made = open(&room, "{\"paths\":[\"cat.png\"]}", false, true);
        assert!(!made.report.ok);
        assert!(
            made.report.output.contains("have no vision") || made.report.output.contains("has no vision"),
            "{}",
            made.report.output
        );
        assert!(made.pictures.is_empty());
    }

    // A picture of the room comes back as a record of an attachment: the file
    // name, the kind of it, and the bytes of it in the shape a data URL wants.
    #[test]
    fn a_picture_of_the_room_is_handed_over() {
        let room = room_of("view");
        std::fs::write(room.join("cat.png"), TINY_PNG).expect("the picture could not be written");
        let made = open(&room, "{\"path\":\"cat.png\"}", true, true);
        assert!(made.report.ok, "{}", made.report.output);
        assert!(made.report.output.contains("1 by 1 pixels"), "{}", made.report.output);
        assert!(made.report.output.contains("69 bytes"), "{}", made.report.output);
        assert_eq!(made.pictures.len(), 1);
        let picture = &made.pictures[0];
        assert_eq!(picture.name, "cat.png");
        assert_eq!(picture.mime, "image/png");
        assert!(picture.data.starts_with("data:image/png;base64,"), "{}", picture.data);
        // The words of a picture of one pixel are short, and nothing of the
        // picture stands in the receipt.
        assert!(!made.report.output.contains("base64"), "{}", made.report.output);
    }

    // One call carries more than one picture, in the order the call named them.
    // A picture of the two that is no picture does not stop the other.
    #[test]
    fn one_call_hands_over_more_than_one_picture() {
        let room = room_of("many");
        std::fs::write(room.join("one.png"), TINY_PNG).expect("the first picture");
        std::fs::create_dir_all(room.join("notes")).expect("the folder of the pictures");
        std::fs::write(room.join("notes/two.png"), TINY_PNG).expect("the second picture");
        std::fs::write(room.join("notes/three.txt"), b"words").expect("the file of words");
        let made = open(
            &room,
            "{\"paths\":[\"one.png\",\"notes/two.png\",\"notes/three.txt\"]}",
            true,
            true,
        );
        assert!(made.report.ok, "{}", made.report.output);
        assert_eq!(made.pictures.len(), 2);
        assert_eq!(made.pictures[0].name, "one.png");
        assert_eq!(made.pictures[1].name, "two.png");
        assert!(made.report.output.contains("Opened 2 images"), "{}", made.report.output);
        assert!(made.report.output.contains("three.txt"), "{}", made.report.output);
        // The order of the receipt is the order of the pictures, so the model is
        // told which of the two it is being shown.
        let first = made.report.output.find("one.png").expect("the first name");
        let second = made.report.output.find("two.png").expect("the second name");
        assert!(first < second, "{}", made.report.output);
    }

    // A call of more pictures than one call may carry says so, and the first of
    // them are still handed over.
    #[test]
    fn a_call_of_too_many_pictures_says_so() {
        let room = room_of("crowd");
        let mut asked: Vec<String> = Vec::new();
        for n in 0..MOST_PICTURES + 3 {
            let name = format!("pic{}.png", n);
            std::fs::write(room.join(&name), TINY_PNG).expect("a picture could not be written");
            asked.push(format!("\"{}\"", name));
        }
        let made = open(&room, &format!("{{\"paths\":[{}]}}", asked.join(",")), true, true);
        assert!(made.report.ok);
        assert_eq!(made.pictures.len(), MOST_PICTURES);
        assert!(made.report.output.contains("Left out of the call: 3"), "{}", made.report.output);
    }

    // A name that leads up out of the room of the chat is refused while the wall
    // of the chat stands, as it is by the tools that work on files. With the wall
    // down the same name is a file of the machine like any other, which is what
    // the user chose by turning the sandbox off.
    #[test]
    fn a_name_that_leads_out_of_the_room_is_refused_while_the_wall_stands() {
        let room = room_of("out");
        let made = open(&room, "{\"path\":\"../../etc/passwd\"}", true, true);
        assert!(!made.report.ok);
        assert!(made.report.output.contains("outside the working directory"), "{}", made.report.output);
        // The same name with the wall down is read as a file, and a file of words
        // is answered for what it is: not a picture.
        let free = open(&room, "{\"path\":\"../../etc/hostname\"}", true, false);
        assert_eq!(free.pictures.len(), 0);
        assert!(free.report.output.contains("does not exist")
            || free.report.output.contains("not an image"), "{}", free.report.output);
        // A name that leads up out of the room is not refused at the door of the
        // room when the wall is down.
        assert!(!free.report.output.contains("outside the working directory"), "{}", free.report.output);
    }

    // A file of words is no picture, and the answer says what kinds are read.
    #[test]
    fn a_file_of_words_is_no_picture() {
        let room = room_of("words");
        std::fs::write(room.join("notes.txt"), b"one two three").expect("the file could not be written");
        let made = open(&room, "{\"path\":\"notes.txt\"}", true, true);
        assert!(!made.report.ok);
        assert!(made.report.output.contains("PNG, JPEG, GIF and BMP"), "{}", made.report.output);
    }

    // A picture of more pixels than the ceiling is refused, before the model is
    // made to pay for all of them.
    #[test]
    fn a_picture_of_too_many_pixels_is_refused() {
        let room = room_of("huge");
        std::fs::write(room.join("wall.png"), png_of(9000, 9000)).expect("the header could not be written");
        let made = open(&room, "{\"path\":\"wall.png\"}", true, true);
        assert!(!made.report.ok);
        assert!(made.report.output.contains("pixels"), "{}", made.report.output);
        assert!(made.report.output.contains("wall.png"), "{}", made.report.output);
    }

    // The kind of a picture is read from the marks at its front, and a kind the
    // projector cannot turn into pixels is not let through.
    #[test]
    fn the_kind_is_read_from_the_marks_at_the_front() {
        assert_eq!(kind_of(&png_of(2, 2)).map(|k| k.mime), Some("image/png"));
        assert_eq!(
            kind_of(&[0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46]).map(|k| k.mime),
            Some("image/jpeg")
        );
        assert_eq!(
            kind_of(b"GIF89a\x01\x00\x01\x00").map(|k| k.mime),
            Some("image/gif")
        );
        assert_eq!(kind_of(b"BM\x00\x00\x00\x00\x00\x00").map(|k| k.mime), Some("image/bmp"));
        // A WebP names itself as a RIFF page, and the projector has no reader
        // for it, so it is no picture here.
        assert!(kind_of(b"RIFF\x00\x00\x00\x00WEBPVP8 ").is_none());
        assert!(kind_of(b"not a picture at all").is_none());
    }

    // The number of pixels of a header, of each of the four kinds.
    #[test]
    fn the_pixels_of_a_header_are_read() {
        assert_eq!(pixels(&PNG, &png_of(640, 480)), Some((640, 480)));
        assert_eq!(pixels(&GIF, b"GIF89a\x80\x02\xe0\x01"), Some((640, 480)));
        let mut bmp: Vec<u8> = b"BM\x00\x00\x00\x00\x00\x00\x00\x00\x36\x00\x00\x00".to_vec();
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&640u32.to_le_bytes());
        bmp.extend_from_slice(&480u32.to_le_bytes());
        assert_eq!(pixels(&BMP, &bmp), Some((640, 480)));
        // A header of a picture is the start of a run of parts, each of them
        // named and given a length of its own.
        let mut jpeg: Vec<u8> = vec![0xff, 0xd8, 0xff, 0xe0];
        jpeg.extend_from_slice(&[0x00, 0x10]);
        jpeg.extend_from_slice(b"JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00");
        jpeg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&480u16.to_be_bytes());
        jpeg.extend_from_slice(&640u16.to_be_bytes());
        assert_eq!(pixels(&JPEG, &jpeg), Some((640, 480)));
    }

    // A name of a file may come in any of the shapes a model reaches for, in the
    // old one of one picture and in the new one of a list, and the user is shown
    // the name of the picture while the call is written.
    #[test]
    fn the_names_a_model_uses_for_its_two_parts() {
        assert_eq!(parts_of("{\"file\":\"cat.png\"}").0, vec![String::from("cat.png")]);
        assert_eq!(
            parts_of("{\"image_url\":\"https://e.org/c.png\"}").1,
            vec![String::from("https://e.org/c.png")]
        );
        // A list of names, and a name written twice.
        assert_eq!(
            parts_of("{\"paths\":[\"one.png\",\"two.png\",\"one.png\",\" \"]}"),
            (vec![String::from("one.png"), String::from("two.png")], Vec::new())
        );
        // The part of one is read as a list of one, so a model that keeps to the
        // old shape is served as well.
        assert_eq!(
            parts_of("{\"path\":\"cat.png\",\"url\":\"https://e.org/c.png\"}"),
            (
                vec![String::from("cat.png")],
                vec![String::from("https://e.org/c.png")]
            )
        );
        assert_eq!(partial_words("{\"path\":\"notes/cat"), "notes/cat");
        assert_eq!(partial_words("{\"url\":\"https://e.org/c"), "https://e.org/c");
        // The first name of a list that is still being written.
        assert_eq!(partial_words("{\"paths\":[\"one.pn"), "one.pn");
    }

    // The address of a picture gives its name, and an address of a house of the
    // web with nothing at the end of it is no picture to read.
    #[test]
    fn the_name_of_a_picture_of_the_web() {
        assert_eq!(name_of_address("https://e.org/pics/Cat%20Big.png?v=2"), "Cat Big.png");
        assert_eq!(name_of_address("https://e.org/"), "an image from the web");
    }
}
