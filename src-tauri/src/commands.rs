// The calls the window makes into the engine.
//
// The page of the window asks for the state with a short poll, in the same way
// as the browser page of the remote mode, and it sends the actions of the user
// as named calls. Every command is async and takes the engine by reference, so
// Tauri runs it on a worker thread and a slow answer never stops the window
// from drawing. A command that takes a reference must return a Result, so each
// one does. None of them fail on purpose: a bad value answers with a small
// "not ok" record, and the window reads the state again.

use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use tauri::State;

use autumn_natter_core::attach;

use autumn_natter_core::config::{Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use autumn_natter_core::{llama, web};

// The state as JSON. The page sends the number it already has (since). When
// nothing changed, the answer stays small.
#[tauri::command]
pub async fn get_state(engine: State<'_, Engine>, since: Option<u64>) -> Result<Value, String> {
    let snap = engine.snapshot();
    Ok(web::state(&snap, since))
}

// One action of the user. The name is the same as the route of the remote page,
// so the two share the mapping in web::action.
#[tauri::command]
pub async fn action(
    engine: State<'_, Engine>,
    path: String,
    body: Option<Value>,
) -> Result<Value, String> {
    let text = body.map(|value| value.to_string()).unwrap_or_default();
    Ok(web::action(&engine, &path, &text))
}

// One attachment of a message, for the block the window draws. The page
// asks by the id the state gave with the message, and gets back the name,
// the kind, the data URL of a picture, or the words of a file.
#[tauri::command]
pub async fn get_attachment(engine: State<'_, Engine>, id: String) -> Result<Value, String> {
    match engine.attachment(&id) {
        Some(record) => Ok(json!({
            "name": record.name,
            "mime": record.mime,
            "data": record.data,
            "text": record.text,
        })),
        None => Err("that attachment is not here".to_string()),
    }
}

// The settings of the program, for the window of settings.
#[tauri::command]
pub async fn get_config(engine: State<'_, Engine>) -> Result<Value, String> {
    Ok(serde_json::to_value(engine.config()).unwrap_or_else(|_| json!({})))
}

// Store new settings, and refresh what depends on them.
#[tauri::command]
pub async fn save_config(engine: State<'_, Engine>, config: Value) -> Result<Value, String> {
    match serde_json::from_value::<Config>(config) {
        Ok(cfg) => {
            engine.save_settings(cfg);
            Ok(web::done())
        }
        Err(problem) => Ok(web::refused(&format!(
            "the settings could not be read: {}",
            problem
        ))),
    }
}

// Every repository on disk, most recently used first. The window of settings
// shows all of them, while the top bar shows five.
#[tauri::command]
pub async fn list_repositories(engine: State<'_, Engine>) -> Result<Value, String> {
    let repos = engine.repositories();
    Ok(Value::Array(
        repos
            .iter()
            .map(|repo| json!({ "id": repo.id, "name": repo.name }))
            .collect(),
    ))
}

// Everything the window of one model needs: the settings it holds, the devices
// of the machine, and the three lists of choices.
#[tauri::command]
pub async fn get_model_settings(engine: State<'_, Engine>, id: String) -> Result<Value, String> {
    let snap = engine.snapshot();
    let (settings, gpus) = engine.model_settings(&id);
    let (name, kind) = snap
        .models
        .iter()
        .find(|model| model.id() == id)
        .map(|model| (model.name.clone(), model.kind.to_string()))
        .unwrap_or_default();
    Ok(json!({
        "id": id,
        "name": name,
        "kind": kind,
        "settings": serde_json::to_value(&settings).unwrap_or_else(|_| json!({})),
        "gpus": gpus
            .iter()
            .map(|card| json!({
                "device": card.device,
                "name": card.name,
                "memory_mb": card.memory_mb,
                "enabled": card.enabled,
                "is_offload": card.is_offload,
                "memory_text": card.memory_text(),
            }))
            .collect::<Vec<_>>(),
        "kinds": llama::KV_TYPES.to_vec(),
        "loads": llama::LOAD_MODES.to_vec(),
        "specs": llama::SPEC_TYPES.to_vec(),
    }))
}

// Save the settings of one model. A running server is stopped, because it was
// started with the old values.
#[tauri::command]
pub async fn save_model_settings(
    engine: State<'_, Engine>,
    id: String,
    settings: Value,
) -> Result<Value, String> {
    match serde_json::from_value::<ModelSettings>(settings) {
        Ok(made) => {
            engine.save_model_settings(id, made);
            Ok(web::done())
        }
        Err(problem) => Ok(web::refused(&format!(
            "the model settings could not be read: {}",
            problem
        ))),
    }
}

// The command line the next answer will use, read again whenever a field of the
// window of one model moves. Nothing is started: only the arguments are built.
#[tauri::command]
pub async fn preview_command(
    engine: State<'_, Engine>,
    id: String,
    settings: Value,
) -> Result<String, String> {
    let snap = engine.snapshot();
    let Some(model) = snap.models.iter().find(|m| m.id() == id) else {
        return Ok(String::new());
    };
    let Ok(held) = serde_json::from_value::<ModelSettings>(settings) else {
        return Ok(String::new());
    };
    // The switches of the form win over the switches the model was saved with.
    let (_, devices) = engine.model_settings(&id);
    let mut cards = devices;
    for (place, on) in held.gpu_enabled.iter().enumerate() {
        if let Some(card) = cards.get_mut(place) {
            card.enabled = *on;
        }
    }
    let mut notes = Vec::new();
    let args = llama::build_args(
        std::path::Path::new(""),
        model,
        &held,
        &cards,
        0,
        snap.code_enabled,
        snap.thinking_mode.as_str(),
        // The line shown in the window is the line of the chat in hand, so the
        // choice of keeping the thinking of the earlier turns stands in it too.
        snap.thinking_preserve_available
            .then(|| snap.thinking_preserve),
        &mut notes,
    );
    let mut line = String::from("llama-server ");
    for arg in args {
        if arg.contains(' ') {
            line.push_str(&format!("\"{arg}\""));
        } else {
            line.push_str(&arg);
        }
        line.push(' ');
    }
    for note in notes {
        line.push('\n');
        line.push_str(&note);
    }
    Ok(line.trim_end().to_string())
}

// ---------------------------------------------------------------- files of the machine
//
// The window takes a file the way the system hands one over, which is not the
// way a browser page does it. The web view of the system on Linux is
// WebKitGTK, and WebKitGTK does not bring a file let go over the window down
// to the page at all (the web view of the system keeps its list of the drop
// targets to a handful of kinds of words, and a file is none of them), and a
// paste of a picture brings no file either. Both roads therefore end on this
// side: the window is told of the drop by Tauri itself, and the clipboard of
// the machine is asked through the toolkit of the system.
//
// What comes back is the shape the page hangs over the chat box: the whole
// name with its ending, the kind of file, the data URL of a picture, or the
// words of a file of words. A file of neither carries neither, for nothing
// else of it would mean anything to the model.

// The most bytes of one file the window reads from the machine. A file bigger
// than this stands on the message by its name alone, for nothing of it would
// fit a chat anyway.
const MOST_FILE_BYTES: u64 = 12 * 1024 * 1024;

// The files of a list of places, in the order they were given, and no more of
// them than one message may carry.
fn taken(paths: &[String]) -> Vec<Value> {
    paths
        .iter()
        .take(attach::MAX_ATTACHMENTS)
        .filter_map(|place| take_one(place))
        .collect()
}

// One file of the places, read as the page holds it. A place that is no file
// gives nothing at all. A file too big to be read, or one whose kind is neither
// a picture nor a file of words, stands by its name alone.
fn take_one(place: &str) -> Option<Value> {
    let name = attach::tidy_name(place);
    if name.is_empty() {
        return None;
    }
    // A place that is no file - a folder of the machine, or a place that is not
    // there at all - gives nothing to hang on a message.
    if !std::fs::metadata(place).ok()?.is_file() {
        return None;
    }
    let mime = attach::mime_of(&name).to_string();
    let mut kind = attach::kind_of(&name, &mime);
    let mut data = String::new();
    let mut words = String::new();
    // Only a picture and a file of words are worth opening. The rest goes to
    // the model by its name, for nothing else of it would mean anything.
    if kind != "file" {
        if let Some(bytes) = read_small(Path::new(place)) {
            if kind == "image" {
                let made = format!("data:{mime};base64,{}", STANDARD.encode(&bytes));
                // A picture that no longer fits a message once it is written
                // out goes by its name too, and the reader is told no lie
                // about what stands on the message.
                if made.len() <= attach::MAX_DATA_CHARS {
                    data = made;
                }
            } else {
                match String::from_utf8(bytes) {
                    Ok(all) => {
                        if all.chars().count() > attach::MAX_TEXT_CHARS {
                            let kept: String = all.chars().take(attach::MAX_TEXT_CHARS).collect();
                            words = format!("{}\n{}", kept, attach::CUT_NOTE);
                        } else {
                            words = all;
                        }
                    }
                    // Words that turn out not to be words: the file stands by
                    // its name, as any other file of bytes does.
                    Err(_) => kind = "file",
                }
            }
        }
    }
    Some(json!({
        "name": name,
        "mime": mime,
        "kind": kind,
        "data": data,
        "text": words,
    }))
}

// The bytes of a file, when it is a file and no bigger than the room of one
// attachment. Nothing comes back for a folder, for a place that is not there,
// or for a file too big to be taken in.
fn read_small(path: &Path) -> Option<Vec<u8>> {
    let size = std::fs::metadata(path).ok()?.len();
    if size == 0 || size > MOST_FILE_BYTES {
        return None;
    }
    std::fs::read(path).ok()
}

// The files of a drop the window saw. The window is told the places of the
// files, and the reader put each of them in front of the program by letting go
// of it, so they are read as they stand.
#[tauri::command]
pub async fn take_files(paths: Vec<String>) -> Result<Value, String> {
    Ok(json!({ "files": taken(&paths) }))
}

// The clipboard of the machine, asked on the side of the window. The places of
// a file copied from the file manager come first, for a place keeps the name
// of the file; a picture of a viewer or of a screen tool comes as no place at
// all, and is read as it stands.
//
// The answer always says which road the window took, so the page never asks
// twice for the same thing: "machine" means the clipboard was read here, and
// an error means it was not, and the page may ask the web view instead.
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn clipboard_take() -> Result<Value, String> {
    // GTK is the work of one thread, which is the thread of the window. A call
    // that came in on another thread answers that it did nothing, rather than
    // ask GTK of a thread it does not belong to.
    if !gtk::glib::MainContext::default().is_owner() {
        return Err("the clipboard of the machine is read on the thread of the window".to_string());
    }
    let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
    // The clipboard is first asked what it holds, and only then reached for.
    // Every ask of the clipboard is a wait on whatever program holds it, so the
    // window asks once for the list of kinds, and then for the one thing it is
    // after.
    let kinds: Vec<String> = clipboard
        .wait_for_targets()
        .map(|targets| {
            targets
                .iter()
                .map(|target| target.name().as_str().to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let holds = |kind: &str| kinds.iter().any(|one| one.as_str() == kind);

    // A file copied from the file manager comes as a list of places, and a
    // place keeps the whole name of the file. That is worth more than the
    // picture a viewer may have put beside the same file, so the places of a
    // copy are asked for before a picture.
    if holds("text/uri-list") || holds(LIST_OF_PLACES) {
        let mut places: Vec<String> = Vec::new();
        if holds("text/uri-list") {
            places = clipboard
                .wait_for_uris()
                .iter()
                .filter_map(|uri| uri_to_place(uri.as_str()))
                .collect();
        }
        if places.is_empty() {
            places = icon_list(&clipboard);
        }
        return Ok(json!({ "files": taken(&places), "road": "machine" }));
    }
    // Then a picture, which the clipboard holds as pixels of its own. They are
    // written out as a portable network picture, which is what a picture the
    // model is shown has to be.
    if kinds.iter().any(|kind| kind.starts_with("image/")) {
        if let Some(picture) = clipboard.wait_for_image() {
            let png = picture.save_to_bufferv("png", &[]).map_err(|problem| {
                format!(
                    "the picture of the clipboard could not be written: {}",
                    problem
                )
            })?;
            let data = format!("data:image/png;base64,{}", STANDARD.encode(&png));
            if data.len() <= attach::MAX_DATA_CHARS {
                return Ok(json!({
                    "files": [{
                        "name": "pasted-picture.png",
                        "mime": "image/png",
                        "kind": "image",
                        "data": data,
                        "text": "",
                    }],
                    "road": "machine",
                }));
            }
        }
    }
    // Words of the clipboard are the business of the chat box, which was given
    // them by the paste itself. Nothing of the kind was asked for here.
    Ok(json!({ "files": [], "road": "machine" }))
}

// The clipboard of the machine of the other systems is read by the page, which
// the web view of those systems does allow.
#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub fn clipboard_take() -> Result<Value, String> {
    Err("the clipboard of the machine is read by the page on this system".to_string())
}

// The place on the disk that an address of the clipboard stands for. The
// addresses come with the odd characters of a name written out in parts.
#[cfg(target_os = "linux")]
pub(crate) fn uri_to_place(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let place = percent_encoding::percent_decode_str(rest)
        .decode_utf8_lossy()
        .into_owned();
    (!place.is_empty()).then_some(place)
}

// The list of places the file manager of GNOME copies with, for it does not
// always put the plain list of addresses beside it.
#[cfg(target_os = "linux")]
const LIST_OF_PLACES: &str = "x-special/gnome-icon-list";

// The places of that list. The list stands one address to a line, and the
// manager ends it with a nothing of its own.
#[cfg(target_os = "linux")]
fn icon_list(clipboard: &gtk::Clipboard) -> Vec<String> {
    let atom = gtk::gdk::Atom::intern(LIST_OF_PLACES);
    let Some(held) = clipboard.wait_for_contents(&atom) else {
        return Vec::new();
    };
    let words = String::from_utf8_lossy(&held.data()).into_owned();
    words
        .split(['\n', '\0'])
        .filter_map(|line| uri_to_place(line.trim()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // A folder of its own for the files of one test, for the tests run together
    // and none of them may look at another's work.
    fn room() -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let place = std::env::temp_dir().join(format!(
            "atnatter-window-test-{}-{}",
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&place);
        std::fs::create_dir_all(&place).expect("a test place for the files can be made");
        place
    }

    // A picture of the machine comes as a data URL, a file of words as its
    // words, and a file of neither by its name alone.
    #[test]
    fn a_file_of_the_machine_comes_as_its_kind() {
        let place = room();
        std::fs::write(place.join("swatch.png"), b"not really a picture, but bytes")
            .expect("a picture of the test can be written");
        std::fs::write(place.join("note.md"), "the words of the note")
            .expect("a file of words of the test can be written");
        std::fs::write(place.join("model.bin"), b"\x00\x01 by bytes")
            .expect("a file of bytes of the test can be written");
        let places = vec![
            place.join("swatch.png").to_string_lossy().into_owned(),
            place.join("note.md").to_string_lossy().into_owned(),
            place.join("model.bin").to_string_lossy().into_owned(),
        ];
        let made = taken(&places);
        assert_eq!(made.len(), 3, "every file of the places was looked at");
        assert_eq!(made[0]["kind"], "image");
        assert!(made[0]["data"]
            .as_str()
            .unwrap_or("")
            .starts_with("data:image/png;base64,"));
        assert_eq!(made[1]["kind"], "text");
        assert_eq!(made[1]["text"], "the words of the note");
        assert_eq!(made[2]["kind"], "file");
        assert_eq!(made[2]["data"], "");
        assert_eq!(made[2]["text"], "");
        let _ = std::fs::remove_dir_all(&place);
    }

    // A file with words that are not words, and a file too big to be taken in,
    // both stand by their names. A folder gives nothing at all, and neither
    // does a place that is not there.
    #[test]
    fn a_file_that_cannot_be_read_gives_its_name_only() {
        let place = room();
        std::fs::write(place.join("words.txt"), b"\xff\xfe not words")
            .expect("a file of no words can be written");
        let big = place.join("big.png");
        let made = std::fs::File::create(&big).expect("a big file can be made");
        made.set_len(MOST_FILE_BYTES + 1)
            .expect("a big file can be stretched");
        let mut places = vec![
            place.join("words.txt").to_string_lossy().into_owned(),
            big.to_string_lossy().into_owned(),
            place.to_string_lossy().into_owned(),
            place.join("nowhere.txt").to_string_lossy().into_owned(),
        ];
        let made = taken(&places);
        assert_eq!(made.len(), 2, "a folder and a place of nowhere give nothing");
        assert_eq!(made[0]["kind"], "file", "words that are not words are a file");
        assert_eq!(made[1]["kind"], "image");
        assert_eq!(made[1]["data"], "", "a picture too big for the room is not read");
        places.clear();
        let _ = std::fs::remove_dir_all(&place);
    }
}
