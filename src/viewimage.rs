// The tool that pointers an image to the model.
//
// An image may be in the working directory of the chat, or it may be addressed straight on the web.
// Both paths end in the same place: the bytes are read, the type of the image is extracted from
// the bytes and not out of the end of the name, and the image is written as a record in the
// directory of the attachments of the chat. The message of the run carries that record, and the
// text the model is sent carry the image itself after the short result of the run. That is how
// an image reaches a vision model here: as a part of the answer of the tool, and not as a run
// of base64 in the middle of a sentence.
//
// The block the user sees holds the result and nothing else. The bytes go to the model alone,
// so no block of the chat can ever hold a wall of base64.
//
// An image can only be shown to a model that can look at images: the vision switch of the model
// settings must be on, and a projector file must remain next to the model for the server to
// take. Without both there is nothing to hand an image to, so the tool records it plainly and
// reads no bytes at all.
//
// The type of the image is read from the markers at its front, for a name may say anything.
// Only the types the projector of llama.cpp can turn into pixels are let through: PNG, JPEG,
// GIF, BMP and WebP.
//
// A WebP is decoded by this program and sent as a PNG. The projector reads PNG, JPEG, GIF and
// BMP with stb_image, and decodes a WebP only when the server was built with video support and
// ffmpeg is in the path of the computer. Sending the WebP whole would therefore work on one
// computer and fail on the next, so the image is decoded here, where a reason can still be
// given, and re-encoded. A TIFF is refused, for nothing on either side reads it.

use serde_json::{json, Value};
use std::io::{Cursor, Read};
use std::path::Path;
use std::time::Duration;

use crate::editfile;
use crate::tools::{self, with_rule};
use crate::util;

// The name the model uses to have images shown to it. The tool was named for one image, and is
// named for the two of them now that one call may carry more than one. A call that arrives
// under the name of one is still read, for a model that read the list of tools of the
// conversation above may write that name again.
pub const NAME: &str = "view_images";
pub const ONE_NAME: &str = "view_image";

// True when a call of this name is a look at images, under either of the two names the tool has
// had.
pub fn is_look(name: &str) -> bool {
    name == NAME || name == ONE_NAME
}

// The most bytes of one image. An image of more than this is a wall of pixels, and a projector
// of this size makes little of a wall.
const MAX_PICTURE_BYTES: usize = 12 * 1024 * 1024;

// The most images one call may carry. Every image is paid for in the size of the context, and a
// call that pointers over a hundred of them leaves nothing to say about them.
const MOST_PICTURES: usize = 8;

// The most pixels of one image. Past this the image holds more pixels than a projector uses,
// and all of them is paid for in the size of the context.
const MOST_PIXELS: u64 = 40_000_000;

// The text of the call, and the text the user is shown. The space the files may be in
// follows the state of the sandbox, as it does for the two tools of the files: the wall over
// the working directory of the chat is the sandbox, and only the sandbox.
pub fn definition(sandbox: bool) -> Value {
    let said = if sandbox {
        "While the sandbox is on, every file checked is a file of the working directory of the chat."
    } else {
        "A file may remain anywhere on this computer, and its whole name is then given."
    };
    let paths_said = if sandbox {
        "The names of the files to look at, each relative to the working directory of the chat."
    } else {
        "The names of the files to look at, each relative to the working directory of the chat, or whole when the file remains elsewhere on this computer."
    };
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": with_rule(format!(
                "Send one or more images to yourself, so that you can be asked about them. Give the names of files in the chat as a list, as [\"notes/cat.png\", \"notes/dog.png\"], or the addresses of images on the web as a list, as [\"https://example.org/cat.png\"]. One name or a list of names is read either way, and one call may carry up to eight images, which arrive in the order you named them. {} Each image must be of a kind the projector reads: PNG, JPEG, GIF, BMP or WebP, and no more than twelve megabytes. A WebP is decoded and sent to you as a PNG. An image can only be shown while vision is turned on for the model; without it the call says so and nothing is read. The images are not stored after the conversation.",
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

// The parts of a call that is finished: the names of the files, and the addresses of the
// images. A model writes a name other than the one that was requested more often than one would
// think, so the other names a local model reaches for are read as well, and a part that holds
// one image is read as a list of one.
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

// The keys a model uses for the files of a call, and the keys it uses for the addresses of the
// web.
const PATH_KEYS: [&str; 8] =
    ["path", "paths", "file", "files", "file_path", "filepath", "filename", "name"];
const URL_KEYS: [&str; 7] = ["url", "urls", "uri", "uris", "href", "link", "image_url"];

// One piece of text, in a list of them, when it has text in it.
fn some(text: String) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    vec![text.trim().to_string()]
}

// The names under the keys of a call. A key holds one image or a list of them, a name of
// nothing is passed over, and a name written twice is checked once.
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

// One name more, when it has text in it and has not been held already.
fn hold_name(found: &mut Vec<String>, text: &str) {
    let one = text.trim();
    if one.is_empty() || found.iter().any(|held| held == one) {
        return;
    }
    found.push(one.to_string());
}

// The one item the call named, as far as it has come. The user watches this while the call is
// written. A name of a file is worth showing whole, for the address may not have come yet, and
// a list is watched from its first name.
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

// The first name inside a list that is still being written. The marker of the list came, and
// the text of the first name of it are what the user should see.
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

// An image that was read, in the form the directory of the attachments wants: the whole file
// name, the type of the file, and the bytes as a data URL.
pub struct Picture {
    pub name: String,
    pub mime: String,
    pub data: String,
}

// What one call came to: the result the model is given, and the images that go with it, when
// there are any to give. More than one image goes in the order the call named them, and an
// image the call named but the program could not read is not among them.
pub struct Opening {
    pub report: tools::Report,
    pub pictures: Vec<Picture>,
}

// Look at images. `vision_on` is the answer of the engine about the model in use: whether an
// image can be shown to it at all. `in_room` is the sandbox boundary of the chat: while the
// sandbox remains, a name of a file is taken inside the working directory of the chat and nowhere else,
// and with the sandbox off it reaches the computer as a command of the terminal tool does.
pub fn open(workdir: &Path, raw: &str, vision_on: bool, in_room: bool) -> Opening {
    let (wanted_paths, wanted_urls) = parts_of(raw);
    if wanted_paths.is_empty() && wanted_urls.is_empty() {
        return refused(
            "The call needs the name of one or more files in the chat, or the address of one or more images on the web",
        );
    }
    // With nothing to hand an image to there is no work to do, so no byte is read. The text
    // reports what is in the way, for the model is to know whether to ask again of another
    // model or to say what the image was for.
    if !vision_on {
        return refused(
            "the images cannot be shown, because this model has no vision: turn the vision setting of the model on and put a projector file next to the model, or describe in text what you need from the images",
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
                    "'{}' is not an image this program can send. The projector reads PNG, JPEG, GIF, BMP and WebP. The type is read from the first bytes of the file, not from the end of its name.",
                    short(&name)
                ));
                continue;
            }
        };
        // A WebP is decoded here and sent as a PNG, so that the answer does not depend on
        // whether the server was built with ffmpeg.
        let decoded = if kind.mime == WEBP.mime {
            match webp_to_png(&bytes) {
                Ok(one) => Some(one),
                Err(problem) => {
                    not_shown.push(format!("'{}' is a WebP and {}.", short(&name), problem));
                    continue;
                }
            }
        } else {
            None
        };
        let mime: &str = if decoded.is_some() { PNG.mime } else { kind.mime };
        let out: &[u8] = decoded
            .as_ref()
            .map(|one| one.bytes.as_slice())
            .unwrap_or(&bytes);
        let size = match decoded.as_ref() {
            Some(one) => Some((one.wide, one.high)),
            None => pixels(kind, &bytes),
        };
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
                called(kind, decoded.is_some()),
                util::count(wide),
                util::count(high),
                weight(out.len())
            ),
            None => format!("{}, {}", called(kind, decoded.is_some()), weight(out.len())),
        };
        said.push(format!("{}: {} ({})", name, measure, part + 1));
        pictures.push(Picture {
            mime: String::from(mime),
            data: format!("data:{};base64,{}", mime, util::b64(out)),
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

// An image of the chat. The work-directory rule of the file tools applies to this path: while the
// sandbox remains, the name is joined onto the working directory of the chat and a path that goes up
// out of it is refused, and with the sandbox off the name reaches the computer as a command of
// the terminal tool does. The records of the attachments are kept out either way.
fn from_room(workdir: &Path, name: &str, in_room: bool) -> Result<(String, Vec<u8>), String> {
    let place = editfile::place_kept(workdir, name, in_room)?;
    let label = place
        .file_name()
        .map(|one| one.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    // The size is requested before the bytes are taken in, so a file of a thousand megabytes is
    // refused and not carried about.
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

// An image of the web. It is fetched here, in the program, and not by a command: the sandbox of
// a command has no network.
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

// The bytes of one address, stopped at the ceiling of an image plus a byte, so a page that is
// no image and goes on for ever is not carried about.
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

// The text the browser of the computer reports. An address of the web is asked as a browser
// would ask it, or a house of images answers with a test instead.
const AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

// Say when an image is past the ceiling of bytes.
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

// The name of the image an address means: the last part of the path in front of the marker of a
// query, turned back into the characters it was written with. An address of nothing to name is
// named for what it is.
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

// How the result names an image. A WebP is sent as a PNG, and the user is informed both: what
// was on the disk, and what went to the model.
fn called(kind: &Kind, converted: bool) -> String {
    if converted {
        return format!("{}, sent as PNG", kind.name);
    }
    String::from(kind.name)
}

// One type of image, with the text the result calls it and the marker the data URL of it
// carries.
struct Kind {
    name: &'static str,
    mime: &'static str,
}

const PNG: Kind = Kind { name: "PNG", mime: "image/png" };
const JPEG: Kind = Kind { name: "JPEG", mime: "image/jpeg" };
const GIF: Kind = Kind { name: "GIF", mime: "image/gif" };
const BMP: Kind = Kind { name: "BMP", mime: "image/bmp" };
// A WebP is recognised so that it can be turned into a PNG. The mime of it is never sent to the
// model: the projector reads it only when the server was built with video support and ffmpeg is
// in the path.
const WEBP: Kind = Kind { name: "WebP", mime: "image/webp" };

// The type of an image, from the markers at its front. Nothing returns for a file that is none
// of the types the projector can be given.
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
    // A WebP is a RIFF page that names itself WEBP twelve bytes in.
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some(&WEBP);
    }
    None
}

// A WebP prepared for the model: the PNG bytes, and the measure of the image as its own decoder
// read them.
struct Decoded {
    bytes: Vec<u8>,
    wide: u64,
    high: u64,
}

// Turn a WebP into a PNG. The projector of llama.cpp reads PNG, JPEG, GIF and BMP with
// stb_image, and reaches a WebP only through ffmpeg, which is in the path on some machines and
// not on others. Decoding here keeps one answer for every computer. An animated WebP gives its
// first frame, which is the image a model can be asked about.
fn webp_to_png(bytes: &[u8]) -> Result<Decoded, String> {
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes))
        .map_err(|one| format!("the file could not be read as a WebP ({one})"))?;
    let (wide, high) = decoder.dimensions();
    let room = decoder.output_buffer_size().ok_or_else(|| {
        String::from("the image is too large for this program to hold in memory")
    })?;
    let mut pixels = vec![0u8; room];
    decoder
        .read_image(&mut pixels)
        .map_err(|one| format!("its pixels could not be decoded ({one})"))?;
    let colour = if decoder.has_alpha() {
        png::ColorType::Rgba
    } else {
        png::ColorType::Rgb
    };
    let mut out: Vec<u8> = Vec::new();
    {
        let mut writer = png::Encoder::new(&mut out, wide, high);
        writer.set_color(colour);
        // The image is written for a projector to read once, not kept for long, so the fastest
        // compression is the one that costs the least time.
        writer.set_compression(png::Compression::Fast);
        let mut page = writer
            .write_header()
            .map_err(|one| format!("a PNG of it could not be begun ({one})"))?;
        page.write_image_data(&pixels)
            .map_err(|one| format!("a PNG of it could not be written ({one})"))?;
    }
    Ok(Decoded {
        bytes: out,
        wide: u64::from(wide),
        high: u64::from(high),
    })
}

// How wide and how high an image is, for the ceiling of pixels. A type whose height and width
// are not to be read from the start of it gives nothing, and the ceiling of bytes is above it
// alone.
fn pixels(kind: &Kind, bytes: &[u8]) -> Option<(u64, u64)> {
    if kind.mime == PNG.mime {
        // The markers of the front, the length of the first part, the name of it, and then the
        // width and the height in forty eight bits each.
        let wide = big32(bytes.get(16..20)?)?;
        let high = big32(bytes.get(20..24)?)?;
        return Some((wide, high));
    }
    if kind.mime == GIF.mime {
        // The two numbers of sixteen bits after the six markers of the front, of which the
        // little end comes first.
        let wide = little16(bytes.get(6..8)?)?;
        let high = little16(bytes.get(8..10)?)?;
        return Some((wide, high));
    }
    if kind.mime == BMP.mime {
        // The header of a Windows image carries the two numbers of thirty two bits, little end
        // first, and a negative height only reports which way the rows of pixels run.
        let wide = little32(bytes.get(18..22)?)?;
        let high = (little32(bytes.get(22..26)?)? as i32).unsigned_abs() as u64;
        return Some((wide, high));
    }
    jpeg_pixels(bytes)
}

// The width and the height of a JPEG. The parts of one remain one behind another, each with its
// own marker, and the part that shows how big the image is the one that begins with the marker
// of a start of frame.
fn jpeg_pixels(bytes: &[u8]) -> Option<(u64, u64)> {
    let mut at = 2usize;
    while at + 4 <= bytes.len() {
        if bytes[at] != 0xff {
            return None;
        }
        let mark = bytes[at + 1];
        // A marker of nothing but a fill, and a marker of a start of frame with no length of
        // its own, carry no length.
        if mark == 0xff || (0xd0..=0xd9).contains(&mark) || mark == 0x01 {
            at += 2;
            continue;
        }
        let length = big16(bytes.get(at + 2..at + 4)?)?;
        // The markers of a start of frame, of either of the two orders, and of the ones of a
        // lossy or a lossless coding.
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

// A number of sixteen bits, written with the great end first, as a JPEG writes the length of a
// part and the size of an image.
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

// A number of thirty two bits, written with the great end first, as the start of a PNG writes
// the width and the height.
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

// A number of thirty two bits, written with the little end first, as the header of a Windows
// image writes them.
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

// How big an image is, in the text of the result.
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

// Text truncated, so a name of a file of a thousand characters does not fill the reason of a
// refusal.
fn short(text: &str) -> String {
    first_words(text, 200)
}

// The first text of a run, cut at the length requested, with a marker that shows the rest was
// left out.
fn first_words(text: &str, most: usize) -> String {
    let out: String = text.chars().take(most).collect();
    if out.chars().count() < text.chars().count() {
        return format!("{} ...", out);
    }
    out
}

// The text of the call, made into an item with parts in it.
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

    // The working directory of a chat, made for one test and removed again. The tests take turns
    // through a count, so two of them never work in one space.
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

    // An image of one pixel, written the way a PNG is written: the markers of the front, the
    // header of it, the text of one pixel, and the marker of the end. It is a real PNG, so the
    // tool is tried against an image and not against a form made up for the test.
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xf7, 0x03, 0x41, 0x43, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    // A PNG header of an image of the size requested, with nothing behind it.
    fn png_of(wide: u32, high: u32) -> Vec<u8> {
        let mut made = TINY_PNG[..24].to_vec();
        made[16..20].copy_from_slice(&wide.to_be_bytes());
        made[20..24].copy_from_slice(&high.to_be_bytes());
        made
    }

    // The directory the images may be in is reported in both states, as it is for the file
    // tools. With the sandbox on, every file checked is a file of the working directory of the
    // chat. With the sandbox off, a file of this computer may be named in full, and the sandbox
    // that is not there is not named either.
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
        assert!(up.contains("While the sandbox is on"), "{}", up);
        assert!(up.contains("working directory of the chat"), "{}", up);
        let down = said(&definition(false));
        assert!(down.contains("anywhere on this computer"), "{}", down);
        assert!(down.contains("whole"), "{}", down);
        assert!(!down.to_lowercase().contains("sandbox"), "{}", down);
    }

    // The call needs one of its two parts, and records it without naming the other program.
    #[test]
    fn the_call_needs_one_of_the_two_things() {
        let room = room_of("nothing");
        let made = open(&room, "{}", true, true);
        assert!(!made.report.ok);
        assert!(made.report.output.contains("name of one or more files"), "{}", made.report.output);
        assert!(made.pictures.is_empty());
    }

    // Without a projector there is nothing to hand an image to, so the bytes of the image are
    // not even read.
    #[test]
    fn no_picture_is_read_while_vision_is_off() {
        let room = room_of("blind");
        std::fs::write(room.join("cat.png"), TINY_PNG).expect("the image could not be written");
        let made = open(&room, "{\"paths\":[\"cat.png\"]}", false, true);
        assert!(!made.report.ok);
        assert!(
            made.report.output.contains("have no vision") || made.report.output.contains("has no vision"),
            "{}",
            made.report.output
        );
        assert!(made.pictures.is_empty());
    }

    // An image of the directory returns as a record of an attachment: the file name, the type
    // of it, and the bytes of it in the form a data URL wants.
    #[test]
    fn a_picture_of_the_room_is_handed_over() {
        let room = room_of("view");
        std::fs::write(room.join("cat.png"), TINY_PNG).expect("the image could not be written");
        let made = open(&room, "{\"path\":\"cat.png\"}", true, true);
        assert!(made.report.ok, "{}", made.report.output);
        assert!(made.report.output.contains("1 by 1 pixels"), "{}", made.report.output);
        assert!(made.report.output.contains("69 bytes"), "{}", made.report.output);
        assert_eq!(made.pictures.len(), 1);
        let picture = &made.pictures[0];
        assert_eq!(picture.name, "cat.png");
        assert_eq!(picture.mime, "image/png");
        assert!(picture.data.starts_with("data:image/png;base64,"), "{}", picture.data);
        // The text of an image of one pixel are short, and nothing of the image is in the
        // result.
        assert!(!made.report.output.contains("base64"), "{}", made.report.output);
    }

    // One call carries more than one image, in the order the call named them. An image of the
    // two that is no image does not stop the other.
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
        // The order of the result is the order of the images, so the model is informed which of
        // the two it is being shown.
        let first = made.report.output.find("one.png").expect("the first name");
        let second = made.report.output.find("two.png").expect("the second name");
        assert!(first < second, "{}", made.report.output);
    }

    // A call of more images than one call may carry records it, and the first of them are still
    // handed over.
    #[test]
    fn a_call_of_too_many_pictures_says_so() {
        let room = room_of("crowd");
        let mut asked: Vec<String> = Vec::new();
        for n in 0..MOST_PICTURES + 3 {
            let name = format!("pic{}.png", n);
            std::fs::write(room.join(&name), TINY_PNG).expect("an image could not be written");
            asked.push(format!("\"{}\"", name));
        }
        let made = open(&room, &format!("{{\"paths\":[{}]}}", asked.join(",")), true, true);
        assert!(made.report.ok);
        assert_eq!(made.pictures.len(), MOST_PICTURES);
        assert!(made.report.output.contains("Left out of the call: 3"), "{}", made.report.output);
    }

    // A name that leads up out of the working directory of the chat is refused while the sandbox
    // is on, as it is by the file tools. With the sandbox off, the same name is a file of this
    // computer like any other, which is what the user chose by turning the sandbox off.
    #[test]
    fn a_name_that_leads_out_of_the_room_is_refused_while_the_wall_stands() {
        let room = room_of("out");
        let made = open(&room, "{\"path\":\"../../etc/passwd\"}", true, true);
        assert!(!made.report.ok);
        assert!(made.report.output.contains("outside the working directory"), "{}", made.report.output);
        // The same name with the sandbox boundary off is read as a file, and a text file is
        // answered for what it is: not an image.
        let free = open(&room, "{\"path\":\"../../etc/hostname\"}", true, false);
        assert_eq!(free.pictures.len(), 0);
        assert!(free.report.output.contains("does not exist")
            || free.report.output.contains("not an image"), "{}", free.report.output);
        // A name that leads up out of the working directory is not refused at the entry point
        // when the sandbox is off.
        assert!(!free.report.output.contains("outside the working directory"), "{}", free.report.output);
    }

    // A text file is no image, and the answer reports what types are read.
    #[test]
    fn a_file_of_words_is_no_picture() {
        let room = room_of("words");
        std::fs::write(room.join("notes.txt"), b"one two three").expect("the file could not be written");
        let made = open(&room, "{\"path\":\"notes.txt\"}", true, true);
        assert!(!made.report.ok);
        assert!(made.report.output.contains("PNG, JPEG, GIF, BMP and WebP"), "{}", made.report.output);
    }

    // A WebP is let through and sent as a PNG, for the projector of llama.cpp reads a WebP only
    // through ffmpeg. The result names both.
    #[test]
    fn a_webp_is_sent_as_a_png() {
        let room = room_of("webp");
        std::fs::write(room.join("cat.webp"), webp_of(4, 3)).expect("the file could not be written");
        let made = open(&room, "{\"path\":\"cat.webp\"}", true, true);
        assert!(made.report.ok, "{}", made.report.output);
        assert_eq!(made.pictures.len(), 1);
        assert_eq!(made.pictures[0].mime, "image/png", "the model is given a PNG");
        assert!(
            made.pictures[0]
                .data
                .starts_with("data:image/png;base64,iVBORw0KGgo"),
            "the data URL holds a PNG"
        );
        assert!(made.report.output.contains("WebP, sent as PNG"), "{}", made.report.output);
        assert!(made.report.output.contains("4 by 3 pixels"), "{}", made.report.output);
    }

    // A file that names itself a WebP and holds nothing of one is refused with a reason, and
    // the rest of the call is not held up by it.
    #[test]
    fn a_webp_that_will_not_decode_is_refused_with_a_reason() {
        let room = room_of("badwebp");
        std::fs::write(
            room.join("hollow.webp"),
            b"RIFF\x14\x00\x00\x00WEBPVP8 \x0c\x00\x00\x00\x00\x00\x00\x00\x00\x00",
        )
        .expect("the file could not be written");
        std::fs::write(room.join("good.webp"), webp_of(2, 2)).expect("the file could not be written");
        let made = open(
            &room,
            "{\"paths\":[\"hollow.webp\",\"good.webp\"]}",
            true,
            true,
        );
        assert!(made.report.ok, "{}", made.report.output);
        assert_eq!(made.pictures.len(), 1, "the good picture still goes");
        assert!(made.report.output.contains("hollow.webp"), "{}", made.report.output);
        assert!(made.report.output.contains("WebP"), "{}", made.report.output);
    }

    // An image of more pixels than the ceiling is refused, before the model is created to pay
    // for all of them.
    #[test]
    fn a_picture_of_too_many_pixels_is_refused() {
        let room = room_of("huge");
        std::fs::write(room.join("wall.png"), png_of(9000, 9000)).expect("the header could not be written");
        let made = open(&room, "{\"path\":\"wall.png\"}", true, true);
        assert!(!made.report.ok);
        assert!(made.report.output.contains("pixels"), "{}", made.report.output);
        assert!(made.report.output.contains("wall.png"), "{}", made.report.output);
    }

    // The type of an image is read from the markers at its front, and a type the projector
    // cannot turn into pixels is not let through.
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
        // A WebP names itself as a RIFF page, and it is let through: this program decodes it
        // and sends a PNG in its place.
        assert_eq!(
            kind_of(&webp_of(2, 2)).map(|k| k.mime),
            Some("image/webp")
        );
        assert!(kind_of(b"not a picture at all").is_none());
        // A TIFF is refused, for nothing on either side reads it.
        assert!(kind_of(b"II\x2a\x00\x08\x00\x00\x00").is_none());
    }

    // A WebP of one colour, made by the encoder of the same crate the tool reads with, so the
    // test holds a real WebP and not an image of one.
    fn webp_of(wide: u32, high: u32) -> Vec<u8> {
        let mut pixels: Vec<u8> = Vec::new();
        for y in 0..high {
            for x in 0..wide {
                pixels.push((x * 40) as u8);
                pixels.push((y * 60) as u8);
                pixels.push(128);
                pixels.push(255);
            }
        }
        let mut out: Vec<u8> = Vec::new();
        image_webp::WebPEncoder::new(&mut out)
            .encode(&pixels, wide, high, image_webp::ColorType::Rgba8)
            .expect("the image could not be written as a WebP");
        out
    }

    // The number of pixels of a header, of each of the four types.
    #[test]
    fn the_pixels_of_a_header_are_read() {
        assert_eq!(pixels(&PNG, &png_of(640, 480)), Some((640, 480)));
        assert_eq!(pixels(&GIF, b"GIF89a\x80\x02\xe0\x01"), Some((640, 480)));
        let mut bmp: Vec<u8> = b"BM\x00\x00\x00\x00\x00\x00\x00\x00\x36\x00\x00\x00".to_vec();
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&640u32.to_le_bytes());
        bmp.extend_from_slice(&480u32.to_le_bytes());
        assert_eq!(pixels(&BMP, &bmp), Some((640, 480)));
        // A header of an image is the start of a run of parts, each of them named and given a
        // length of its own.
        let mut jpeg: Vec<u8> = vec![0xff, 0xd8, 0xff, 0xe0];
        jpeg.extend_from_slice(&[0x00, 0x10]);
        jpeg.extend_from_slice(b"JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00");
        jpeg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&480u16.to_be_bytes());
        jpeg.extend_from_slice(&640u16.to_be_bytes());
        assert_eq!(pixels(&JPEG, &jpeg), Some((640, 480)));
    }

    // A name of a file may arrive any of the forms a model reaches for, in the old one of one
    // image and in the new one of a list, and the user is shown the name of the image while the
    // call is written.
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
        // The part of one is read as a list of one, so a model that keeps to the old form is
        // served as well.
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

    // The address of an image gives its name, and an address of a house of the web with nothing
    // at the end of it is no image to read.
    #[test]
    fn the_name_of_a_picture_of_the_web() {
        assert_eq!(name_of_address("https://e.org/pics/Cat%20Big.png?v=2"), "Cat Big.png");
        assert_eq!(name_of_address("https://e.org/"), "an image from the web");
    }
}
