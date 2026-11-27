// The calls the window makes into the engine.
//
// The page of the window requests the state with a short poll, in the same way as the browser
// page of the remote mode, and it sends the actions of the user as named calls. Every command
// is async and takes the engine by reference, so Tauri runs it on a worker thread and a slow
// answer never stops the window from drawing. A command that takes a reference must return a
// Result, so each one does. None of them fail on purpose: a bad value answers with a small "not
// ok" record, and the window reads the state again.

use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use autumn_natter_core::attach;

use autumn_natter_core::engine::Engine;
use autumn_natter_core::logs;
use autumn_natter_core::web;

// The state as JSON. The page sends the number it already has (since). When nothing changed,
// the answer stays small.
#[tauri::command]
pub async fn get_state(engine: State<'_, Engine>, since: Option<u64>) -> Result<Value, String> {
    let snap = engine.snapshot();
    Ok(web::state(&snap, since))
}

#[tauri::command]
pub async fn get_browser_page(engine: State<'_, Engine>, tab: u64) -> Result<Value, String> {
    engine
        .browser_page(tab)
        .map(|(url, html)| json!({"url": url, "html": html}))
        .ok_or_else(|| String::from("That browser page is no longer available."))
}

// One action of the user. The name is the same as the route of the remote page, so the two
// share the mapping in web::action.
#[tauri::command]
pub async fn action(
    engine: State<'_, Engine>,
    path: String,
    body: Option<Value>,
) -> Result<Value, String> {
    let text = body.map(|value| value.to_string()).unwrap_or_default();
    Ok(web::action(&engine, &path, &text))
}

// Check the address before opening the remote program in a separate window. The page there uses
// the same HTTP API as a browser, and the local program stays open in its own window.
#[tauri::command]
pub async fn connect_remote(app: AppHandle, address: String) -> Result<Value, String> {
    let address = address.trim();
    let address = if address.contains("://") {
        address.to_string()
    } else {
        format!("http://{address}")
    };
    let mut target = tauri::Url::parse(&address)
        .map_err(|problem| format!("Enter a valid web address: {problem}"))?;
    if !matches!(target.scheme(), "http" | "https")
        || target.host_str().is_none()
        || !target.username().is_empty()
        || target.password().is_some()
        || target.path() != "/"
        || target.query().is_some()
        || target.fragment().is_some()
    {
        return Err(
            "Enter the web address of the remote program, without a path or sign-in details."
                .into(),
        );
    }
    target.set_path("/");

    let state_url = target
        .join("api/state?since=-1")
        .map_err(|problem| format!("The remote address is not valid: {problem}"))?;
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let response = agent.get(state_url.as_str()).call().map_err(|problem| {
        format!("Could not reach an Autumn Natter remote program at this address: {problem}")
    })?;
    let state: Value = response.into_json().map_err(|problem| {
        format!("The address did not return a readable program state: {problem}")
    })?;
    if !state.get("rev").is_some_and(Value::is_number)
        || !state.get("models").is_some_and(Value::is_array)
    {
        return Err("That address did not return an Autumn Natter remote program.".into());
    }

    if let Some(window) = app.get_webview_window("remote-program") {
        window
            .navigate(target.clone())
            .map_err(|problem| format!("The remote window could not be opened: {problem}"))?;
        window
            .set_focus()
            .map_err(|problem| format!("The remote window could not be focused: {problem}"))?;
    } else {
        WebviewWindowBuilder::new(&app, "remote-program", WebviewUrl::External(target.clone()))
            .title("Autumn Natter — Remote")
            .inner_size(1180.0, 760.0)
            .min_inner_size(620.0, 420.0)
            .build()
            .map_err(|problem| format!("The remote window could not be opened: {problem}"))?;
    }

    Ok(json!({ "ok": true, "address": target.as_str() }))
}

// One attachment of a message, for the block the window draws. The page asks by the id the
// state gave with the message, and gets back the name, the type, the data URL of an image, or
// the text of a file.
#[tauri::command]
pub async fn get_attachment(engine: State<'_, Engine>, id: String) -> Result<Value, String> {
    match engine.attachment(&id) {
        Some(record) => Ok(json!({
            "name": record.name,
            "mime": record.mime,
            "data": record.data,
            "text": record.text,
        })),
        None => Err("That attachment is not here".to_string()),
    }
}

// All of the plan of the open chat, for the window that reads it at a click on the heading over
// the conversation.
#[tauri::command]
pub async fn get_plan(engine: State<'_, Engine>) -> Result<Value, String> {
    match engine.plan() {
        Some(words) => Ok(json!({ "words": words })),
        None => Err("This chat has no plan".to_string()),
    }
}

// A file the user was proposed by the download tool, and all of such an propose wound into one
// archive. The window has no web server between it and the engine, so the bytes return as a
// data URL and the window hangs them on a link; the remote page requests the same two items
// over http. Only the working directory of the open chat is opened for either, which is the rule the
// file tools work to.
#[tauri::command]
pub async fn get_gift(engine: State<'_, Engine>, path: String) -> Result<Value, String> {
    Ok(web::gift_value(&engine, &path))
}

#[tauri::command]
pub async fn get_gift_zip(
    engine: State<'_, Engine>,
    paths: Vec<String>,
) -> Result<Value, String> {
    Ok(web::gift_zip_value(&engine, &paths))
}

// The contents of one file of a show, for the window that checks a file whole. The bytes come
// by the name the block of the show carries, under the same rule of the working directory as the call
// that made the block: while the sandbox of the chat remains only the working directory of the chat is
// opened, and with the sandbox off a file of the computer may be read. A type the window has no
// way of showing is refused.
#[tauri::command]
pub async fn get_displayed(engine: State<'_, Engine>, path: String) -> Result<Value, String> {
    Ok(web::display_value(&engine, &path))
}

// The settings of the program, for the settings panel. The same three calls are behind the
// routes of the remote page, in `web`.
#[tauri::command]
pub async fn get_config(engine: State<'_, Engine>) -> Result<Value, String> {
    Ok(web::config_value(&engine))
}

// Store new settings, and refresh what depends on them.
#[tauri::command]
pub async fn save_config(engine: State<'_, Engine>, config: Value) -> Result<Value, String> {
    Ok(web::save_config(&engine, config))
}

// Every repository on disk, most recently used first. The settings panel shows all of them,
// while the top bar shows five.
#[tauri::command]
pub async fn list_repositories(engine: State<'_, Engine>) -> Result<Value, String> {
    Ok(web::repositories_value(&engine))
}

// The pre-written prompts of the Program tab. Without an id the call gives the lines of the
// table; with one it gives the text of that prompt, for the user asked to read it.
#[tauri::command]
pub async fn get_prompts(id: String) -> Result<Value, String> {
    Ok(web::prompts_value(&id))
}

// Everything the Model tab needs: the settings it holds, the devices of the computer, and the
// three lists of choices.
#[tauri::command]
pub async fn get_model_settings(engine: State<'_, Engine>, id: String) -> Result<Value, String> {
    Ok(web::model_settings_value(&engine, &id))
}

// Save the settings of one model. A running server is stopped, because it was started with the
// old values.
#[tauri::command]
pub async fn save_model_settings(
    engine: State<'_, Engine>,
    id: String,
    settings: Value,
) -> Result<Value, String> {
    Ok(web::save_model_settings(&engine, &id, settings))
}

// The servers of the user's own, for the MCP tab of the settings panel. The state carries their
// names, their icons and their tools; all of a server, its code included, comes from here.
#[tauri::command]
pub async fn get_mcp(engine: State<'_, Engine>) -> Result<Value, String> {
    Ok(web::mcp_value(&engine))
}

// The embedding model of the sources, for the section at the top of the Program tab: the choice
// of the model, its quantization and its vision projector, which of the files are in the folder,
// whether the server that counts the words runs, and how a download is going.
#[tauri::command]
pub async fn get_embed_state(engine: State<'_, Engine>) -> Result<Value, String> {
    Ok(engine.embedding_state())
}

// The sources of the open chat, for the pop-up of the sources: the files and their state, the
// folder they are in, the number of them, and the job that may be going on over them.
#[tauri::command]
pub async fn get_sources(engine: State<'_, Engine>) -> Result<Value, String> {
    Ok(engine.sources_value())
}

// The logs, for the Logs tab of the settings panel: the output of llama-server, and the log the
// program writes of its own work. The list is small: the name of every log of that type that is
// kept, the date and the time it is at, its size, and which of them this run of the program is
// writing. The type comes with the request, and a word that is neither type is read as the log
// of llama-server.
#[tauri::command]
pub async fn get_logs(kind: Option<String>) -> Result<Value, String> {
    Ok(web::logs_value(logs::Kind::of_word(
        kind.as_deref().unwrap_or_default(),
    )))
}

// One log, from its end. The page asks again by itself while the update of the tab is on live
// or on five seconds, and once when the user presses the button of it. A name that is not the
// name of a log returns the text of an error, as the page of the remote mode does.
#[tauri::command]
pub async fn get_log(id: String, tail: Option<u64>, kind: Option<String>) -> Result<Value, String> {
    Ok(web::log_value(
        logs::Kind::of_word(kind.as_deref().unwrap_or_default()),
        &id,
        tail,
    ))
}

// The command line the next answer will use, read again whenever a field of the Model tab
// moves. Nothing is started: only the arguments are built.
#[tauri::command]
pub async fn preview_command(
    engine: State<'_, Engine>,
    id: String,
    settings: Value,
) -> Result<String, String> {
    Ok(web::preview_line(&engine, &id, settings)
        .as_str()
        .unwrap_or_default()
        .to_string())
}

// ---------------------------------------------------------------- files of the machine
//
// The window takes a file the way the system pointers one over, which is not the way a browser
// page does it. The web view of the system on Linux is WebKitGTK, and WebKitGTK does not bring
// a file let go over the window down to the page at all (the web view of the system keeps its
// list of the drop targets to a handful of types of text, and a file is none of them), and a
// paste of an image brings no file either. Both paths therefore end on this side: the window is
// informed of the drop by Tauri itself, and the clipboard of the computer is asked through the
// toolkit of the system.
//
// What returns is the form the page hangs over the chat composer: the whole name with its
// ending, the type of file, the data URL of an image, or the text of a text file. A file of
// neither carries neither, for nothing else of it would mean anything to the model.

// The most bytes of one file the window reads from the computer. A file bigger than this is on
// the message by name only, for nothing of it would fit a chat anyway.
const MOST_FILE_BYTES: u64 = 12 * 1024 * 1024;

// The files of a list of places, in the order they were given, and no more of them than one
// message may carry.
fn taken(paths: &[String]) -> Vec<Value> {
    paths
        .iter()
        .take(attach::MAX_ATTACHMENTS)
        .filter_map(|place| take_one(place))
        .collect()
}

// One file of the places, read as the page holds it. A place that is no file gives nothing at
// all. A file too big to be read, or one whose type is neither an image nor a text file,
// remains by name only.
fn take_one(place: &str) -> Option<Value> {
    let name = attach::tidy_name(place);
    if name.is_empty() {
        return None;
    }
    // A place that is no file - a directory of the computer, or a place that is not there at
    // all - gives nothing to hang on a message.
    if !std::fs::metadata(place).ok()?.is_file() {
        return None;
    }
    let mime = attach::mime_of(&name).to_string();
    let mut kind = attach::kind_of(&name, &mime);
    let mut data = String::new();
    let mut words = String::new();
    // Only an image and a text file are worth opening. The rest goes to the model by its name,
    // for nothing else of it would mean anything.
    if kind != "file" {
        if let Some(bytes) = read_small(Path::new(place)) {
            if kind == "image" {
                // An image is sent whatever its size, for there is no ceiling on the size of a
                // file the user hangs on a message. Only the reading of it is bounded, by the
                // space of one file above.
                data = format!("data:{mime};base64,{}", STANDARD.encode(&bytes));
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
                    // Text that turn out not to be text: the file remains by its name, as any
                    // other file of bytes does.
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

// The bytes of a file, when it is a file and no bigger than the space of one attachment.
// Nothing returns for a folder, for a place that is not there, or for a file too big to be
// taken in.
fn read_small(path: &Path) -> Option<Vec<u8>> {
    let size = std::fs::metadata(path).ok()?.len();
    if size == 0 || size > MOST_FILE_BYTES {
        return None;
    }
    std::fs::read(path).ok()
}

// The files of a drop the window saw. The window is informed the places of the files, and the
// user put each of them to the program by letting go of it, so they are read as they are now.
#[tauri::command]
pub async fn take_files(paths: Vec<String>) -> Result<Value, String> {
    Ok(json!({ "files": taken(&paths) }))
}

// The clipboard of the computer, asked on the side of the window. The places of a file copied
// from the file manager come first, for a place keeps the name of the file; an image of a
// viewer or of a screen tool comes as no place at all, and is read as it is now.
//
// The answer always reports which path the window took, so the page never asks twice for the
// same: "computer" means the clipboard was read here, and an error means it was not, and the
// page may ask the web view instead.
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn clipboard_take() -> Result<Value, String> {
    // GTK is the work of one thread, which is the thread of the window. A call that arrived on
    // another thread answers that it did nothing, rather than ask GTK of a thread it does not
    // belong to.
    if !gtk::glib::MainContext::default().is_owner() {
        return Err("the clipboard of the computer is read on the thread of the window".to_string());
    }
    let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
    // The clipboard is first asked what it holds, and only then reached for. Every ask of the
    // clipboard is a wait on whatever program holds it, so the window asks once for the list of
    // types, and then for the one item it is after.
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

    // A file copied from the file manager comes as a list of places, and a place keeps the
    // whole name of the file. That is worth more than the image a viewer may have put beside
    // the same file, so the places of a copy are requested before an image.
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
        return Ok(json!({ "files": taken(&places), "source": "machine" }));
    }
    // Then an image, which the clipboard holds as pixels of its own. They are written out as a
    // portable network image, which is what an image the model is shown has to be.
    if kinds.iter().any(|kind| kind.starts_with("image/")) {
        if let Some(picture) = clipboard.wait_for_image() {
            let png = picture.save_to_bufferv("png", &[]).map_err(|problem| {
                format!(
                    "the image of the clipboard could not be written: {}",
                    problem
                )
            })?;
            // The image is sent whatever its size, for there is no ceiling on the size of a
            // file the user hangs on a message.
            let data = format!("data:image/png;base64,{}", STANDARD.encode(&png));
            return Ok(json!({
                "files": [{
                    "name": "pasted-picture.png",
                    "mime": "image/png",
                    "kind": "image",
                    "data": data,
                    "text": "",
                }],
                "source": "machine",
            }));
        }
    }
    // Text of the clipboard is the business of the chat composer, which was given it by the
    // paste itself. Nothing of the type was requested here.
    Ok(json!({ "files": [], "source": "machine" }))
}

// The clipboard of the computer of the other systems is read by the page, which the web view of
// those systems does allow.
#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub fn clipboard_take() -> Result<Value, String> {
    Err("the clipboard of the computer is read by the page on this system".to_string())
}

// The place on the disk that an address of the clipboard means. The addresses come with the odd
// characters of a name written out in parts.
#[cfg(target_os = "linux")]
pub(crate) fn uri_to_place(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let place = percent_encoding::percent_decode_str(rest)
        .decode_utf8_lossy()
        .into_owned();
    (!place.is_empty()).then_some(place)
}

// The list of places the file manager of GNOME copies with, for it does not always put the
// plain list of addresses beside it.
#[cfg(target_os = "linux")]
const LIST_OF_PLACES: &str = "x-special/gnome-icon-list";

// The places of that list. The list remains one address to a line, and the manager ends it with
// a nothing of its own.
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

    // A directory of its own for the files of one test, for the tests run together and none of
    // them may look at another's work.
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

    // An image of the computer comes as a data URL, a text file as its text, and a file of
    // neither by name only.
    #[test]
    fn a_file_of_the_machine_comes_as_its_kind() {
        let place = room();
        std::fs::write(place.join("swatch.png"), b"not really an image, but bytes")
            .expect("an image of the test can be written");
        std::fs::write(place.join("note.md"), "the words of the note")
            .expect("a text file of the test can be written");
        std::fs::write(place.join("model.bin"), b"\x00\x01 by bytes")
            .expect("a file of bytes of the test can be written");
        let places = vec![
            place.join("swatch.png").to_string_lossy().into_owned(),
            place.join("note.md").to_string_lossy().into_owned(),
            place.join("model.bin").to_string_lossy().into_owned(),
        ];
        let made = taken(&places);
        assert_eq!(made.len(), 3, "every file of the places was checked");
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

    // A file with text that is not text, and a file too big to be taken in, both remain by
    // their names. A folder gives nothing at all, and neither does a place that is not there.
    #[test]
    fn a_file_that_cannot_be_read_gives_its_name_only() {
        let place = room();
        std::fs::write(place.join("words.txt"), b"\xff\xfe not words")
            .expect("a file of no text can be written");
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
        assert_eq!(made[0]["kind"], "file", "text that is not text is a file");
        assert_eq!(made[1]["kind"], "image");
        assert_eq!(made[1]["data"], "", "an image too big for the space is not read");
        places.clear();
        let _ = std::fs::remove_dir_all(&place);
    }
}
