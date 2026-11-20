// Infographic video tools: the tools a model uses, and the folder a composition lives in.
//
// A video is built in several calls, not one. The model writes a composition - a header, plus a
// list of layers, each layer owning a range of the timeline - checks it, previews a chroma key,
// and then has the frames encoded into a file. Every call answers with what it found, so a fault
// is fixed while it is still one line of JSON instead of a minute of nothing on the screen. A
// render returns stills of the video to the model, so it sees what it made instead of guessing
// from the numbers it wrote.
//
// The five tools:
//
// * `infographic_project` - the header of a composition, and the whole composition read back.
// * `infographic_layer` - one layer: added, changed, deleted, or read.
// * `infographic_cutout` - a subject keyed out of an image or a frame of a video, so the key can
//   be looked at before it is drawn into every frame.
// * `infographic_check` - every fault in a composition, with no file written.
// * `infographic_render` - the frames, encoded as MP4, GIF, or a single PNG.
//
// The whole vocabulary - canvases, colour palettes, layer kinds, style fields, motion types,
// keyframe properties, effects - is in `system_prompt`, which is added to the system messages
// while the line of the tool badge is on. It is deliberately not repeated in the tool
// descriptions: a model that is offered a tool of this suite has that prompt above it, and the
// descriptions have to fit beside every other tool of the request.
//
// A composition is one file, `composition.json`, in a folder of the working directory of the
// chat. None of it is held in the memory of the program, so the work survives closing the chat.
// Pixels are not cached either: one frame is drawn, handed to the file writer, and dropped, so
// the memory cost of a video is the cost of one frame.
//
// The arguments of a call arrive as a JSON object, plain or inside a fenced block, as they do for
// every tool of this program that is not a terminal command.

pub mod anim;
pub mod encode;
pub mod fonts;
pub mod media;
pub mod paint;
pub mod prompts;
pub mod stage;
pub mod story;
pub mod text;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};

use crate::infographic::story::{Layer, Project};
use crate::tools::{self, with_rule};
use crate::util;
use crate::viewimage::Picture;

pub const PROJECT_NAME: &str = "infographic_project";
pub const LAYER_NAME: &str = "infographic_layer";
pub const CUTOUT_NAME: &str = "infographic_cutout";
pub const CHECK_NAME: &str = "infographic_check";
pub const RENDER_NAME: &str = "infographic_render";

// True for the five names of this suite.
pub fn is_tool(name: &str) -> bool {
    matches!(
        name,
        PROJECT_NAME | LAYER_NAME | CUTOUT_NAME | CHECK_NAME | RENDER_NAME
    )
}

// The most stills one call returns to the model. Every image costs context space, and three
// frames are enough to judge a video.
const MOST_STILLS: usize = 3;

// The folder, inside the folder of a composition, that the still frames of the work in progress
// are written into.
pub const FRAMES_DIR: &str = "frames";

// The result of one call: the text the model reads back, and the images that go with it.
pub struct Made {
    pub report: tools::Report,
    pub pictures: Vec<Picture>,
}

// The result of a call that succeeded.
fn told(words: String) -> Made {
    Made { report: tools::Report { ok: true, output: words }, pictures: Vec::new() }
}

// The result of a call that succeeded and is shown to the model as pictures as well.
fn told_with(words: String, pictures: Vec<Picture>) -> Made {
    Made { report: tools::Report { ok: true, output: words }, pictures }
}

// The result of a call that could not be done, or was done only in part.
fn refused(why: String) -> Made {
    Made { report: tools::Report { ok: false, output: why }, pictures: Vec::new() }
}

// The state line above the tool block: what the call does, and which composition it works on.
pub fn shown(name: &str, raw: &str) -> String {
    let head = match name {
        PROJECT_NAME => "Setting up a composition",
        LAYER_NAME => "Editing a layer",
        CUTOUT_NAME => "Previewing a key",
        CHECK_NAME => "Checking a composition",
        RENDER_NAME => "Rendering a video",
        _ => "Working on a composition",
    };
    let given = object(raw);
    let said = given
        .get("project")
        .and_then(Value::as_str)
        .map(|one| one.trim().to_string())
        .filter(|one| !one.is_empty());
    match said {
        Some(one) => format!("{head} ({one})"),
        None => String::from(head),
    }
}

// The arguments of a call, as an object.
fn object(raw: &str) -> Value {
    let text = raw.trim();
    let cut = crate::tools::extract_object(text).unwrap_or(text);
    match serde_json::from_str::<Value>(cut) {
        Ok(given) if given.is_object() => given,
        _ => json!({}),
    }
}

// ---------------------------------------------------------------- the tools

// The five tool definitions offered to a model.
pub fn definitions() -> Vec<Value> {
    let canvases = story::canvas_words();
    let palettes = story::palette_names();
    vec![
        json!({
            "type": "function",
            "function": {
                "name": PROJECT_NAME,
                "description": with_rule(String::from(
                    "Create a video composition, change its header, or read the whole of it. The \
                     header sets the frame size, the frame rate, the duration, the colour palette \
                     and the background. Called with only the name of a composition, it answers \
                     with the state of the video: the header and one line per layer. Given \
                     `layers`, that list becomes the whole layer list and the previous layers are \
                     dropped; use the layer tool to change a single layer. Given `preview`, the \
                     frames named are drawn from the composition as it stands, shown to me as \
                     images, and written into the `frames` folder of the composition. The full \
                     vocabulary is in the system prompt of this suite."
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "The name of the composition. It is also the name of the folder the work is in. Leave it out for the default composition of this chat." },
                        "about": { "type": "string", "description": "One line about what the video is of." },
                        "canvas": { "type": "string", "description": format!("The frame size. Named sizes: {canvases}. Or as 1280x720.") },
                        "fps": { "type": "integer", "description": "Frames per second, 1 to 60. 24 is the default." },
                        "length": { "type": "number", "description": "Duration in seconds. 0 means the end of the last layer." },
                        "palette": { "type": "string", "description": format!("The colour palette. The sets are {palettes}.") },
                        "colours": { "type": "object", "description": "Colours for this video only: ink, ground, soft, accent, series." },
                        "font": { "type": "string", "description": "'sans', 'serif', 'mono', or the name of a font family on this machine." },
                        "heading_weight": { "type": "string", "description": "'normal', 'bold' or 'black'." },
                        "background": { "type": "object", "description": "The background: kind (solid, gradient, image, none), colour, from, to, angle, vignette, media, fit." },
                        "audio": { "type": "object", "description": "The audio track under the whole video: media, from, length, volume, fade_in, fade_out." },
                        "tracks": { "type": "array", "description": "Names of the timeline rows. They affect the listing only.", "items": { "type": "string" } },
                        "layers": { "type": "array", "description": "The whole layer list. Given at all, it replaces the layers that were there." },
                        "preview": { "type": "string", "description": "Frames of the work in progress to show me as images, drawn from the composition before any render: one second, or 'now', or 'film' for three across the film, or 'none'. Up to 3. They are written into the frames folder of the composition as well." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": LAYER_NAME,
                "description": with_rule(String::from(
                    "Add a layer to a composition, change one, delete one, or read one back. `one` \
                     selects the layer by its id, its name, or its position in the list starting \
                     at 1. `layer` carries the fields to change, and they are merged into the \
                     layer that stands: a field the call does not name keeps its value. Without \
                     `one`, a new layer is added, at `at` when a position is given. With `remove` \
                     set to true the layer is deleted. Without `layer` or `remove`, the layer is \
                     returned as the JSON it is stored as, and with no `one` either the whole \
                     composition is returned. Problems with the layer - text that does not fit its \
                     box, a file that is missing - are reported in the answer. Given `preview`, the \
                     frame named is drawn with this layer in place - 'now' is the middle of the time \
                     the layer owns - shown to me as an image, and written into the `frames` folder."
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "The name of the composition." },
                        "one": { "type": "string", "description": "Which layer: its id, its name, or its position in the list starting at 1." },
                        "at": { "type": "integer", "description": "The position in the list, from 0, for a layer being added." },
                        "layer": { "type": "object", "description": "The fields of the layer. The system prompt of this suite lists what a layer may set." },
                        "remove": { "type": "boolean", "description": "Delete the layer instead of changing it." },
                        "preview": { "type": "string", "description": "The frame to show me after the change: 'now' for the middle of this layer's time, a second, or 'film' for three across the film. Up to 3. Written into the frames folder of the composition as well." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": CUTOUT_NAME,
                "description": with_rule(String::from(
                    "Key a subject out of an image or of one frame of a video, and show the result \
                     to me. The screen colour is detected from the border of the picture when \
                     `colour` is 'auto', which is what a green or blue screen needs. The file is \
                     not modified: the key is drawn into the folder of the composition as a PNG \
                     for inspection, and the settings that produced it are returned, to be put on \
                     the layer as an `effects` entry of kind 'chroma'. Run this once per subject \
                     before the video is rendered."
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "The name of the composition." },
                        "media": { "type": "string", "description": "The image or video file." },
                        "at": { "type": "number", "description": "The second of the video to key from. 0 is the first frame." },
                        "colour": { "type": "string", "description": "'auto' to detect the border colour, or the colour itself: green, #00ff00." },
                        "similarity": { "type": "number", "description": "How far a colour may be from the key colour and still be removed, 0.0 to 1.0. Default 0.24." },
                        "blend": { "type": "number", "description": "How wide the gradient of the edge is, 0.0 to 1.0. Default 0.12." },
                        "spill": { "type": "number", "description": "How much of the screen colour is suppressed in the edges of the subject, 0.0 to 1.0. Default 0.6." },
                        "feather": { "type": "number", "description": "Softness of the edge, in pixels of the frame." },
                        "choke": { "type": "number", "description": "How far the edge of the key is pulled in, to remove a fringe. -0.5 to 0.5." },
                        "despill": { "type": "number", "description": "Suppress the screen colour across the whole image, 0.0 to 1.0." },
                        "show": { "type": "string", "description": "'cut' for the subject on a transparent background, 'matte' for the black and white alpha, 'over' for the subject over a colour." }
                    },
                    "required": ["media"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": CHECK_NAME,
                "description": with_rule(String::from(
                    "Report every fault in a composition: text that does not fit its box, a file \
                     that is missing or is a different media type than the layer states, a layer \
                     that is never on screen, a video with nothing visible in it. Every layer is \
                     drawn at the middle of its own time range to find text that overflows. Run \
                     this before a render and fix what it reports. Given `at` or `preview`, the \
                     frames are drawn and shown to me as images as well, and written into the \
                     `frames` folder of the composition, so the work may be judged before it is \
                     rendered."
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "The name of the composition." },
                        "at": { "type": "number", "description": "One second of the video, when only that frame is to be checked." },
                        "preview": { "type": "string", "description": "Frames to show me as images: 'now' for the frame being checked, a second, or 'film' for three across the video. Up to 3." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": RENDER_NAME,
                "description": with_rule(String::from(
                    "Draw the frames of a composition and write the file: an MP4 with audio when \
                     this machine has ffmpeg, an animated GIF when it does not, or one still PNG of \
                     one second. The file is written into the folder of the composition. Stills of \
                     the video are returned to me as images in the answer, so I can see the result: \
                     `preview` lists the seconds to show, and three are taken across the video when \
                     none are named. Rendering a one minute video takes about a minute, and the \
                     state line reports the progress."
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "The name of the composition." },
                        "form": { "type": "string", "description": "'mp4', 'gif' or 'png'. An mp4 is refused with a note about the gif when the machine has no ffmpeg." },
                        "fps": { "type": "integer", "description": "Frames per second of the output file. The value from the composition when none is given." },
                        "name": { "type": "string", "description": "The name of the file, without its extension. The name of the composition when none is given." },
                        "at": { "type": "number", "description": "The second of the video the still PNG is drawn at, for form 'png'." },
                        "preview": { "type": "array", "description": "The seconds of the video to return as stills, up to 3. 'none' for no stills.", "items": { "type": "number" } },
                        "sound": { "type": "boolean", "description": "Whether the audio of the composition is mixed into the video. True when not given." }
                    }
                }
            }
        }),
    ]
}

// ---------------------------------------------------------------- one call

// One call of one of the five tools. `vision` reports whether the model in use can be shown an
// image at all; `jailed` is the sandbox of the chat, which keeps a file name inside the working
// directory in the same way a terminal command is kept there.
pub fn run(
    name: &str,
    raw: &str,
    workdir: &Path,
    vision: bool,
    jailed: bool,
    cancel: &AtomicBool,
    mut told: impl FnMut(&str),
) -> Made {
    let given = object(raw);
    let room = room_of(workdir, given.get("project").and_then(Value::as_str).unwrap_or(""));
    match name {
        PROJECT_NAME => header(&room, workdir, &given, vision),
        LAYER_NAME => layer(&room, workdir, &given, vision),
        CUTOUT_NAME => cutout(&room, workdir, &given, vision, jailed),
        CHECK_NAME => check(&room, &given, vision),
        RENDER_NAME => render(&room, &given, vision, cancel, &mut told),
        other => refused(format!(
            "\"{other}\" is not a tool of the composition suite. The five are \
             {PROJECT_NAME}, {LAYER_NAME}, {CUTOUT_NAME}, {CHECK_NAME} and {RENDER_NAME}."
        )),
    }
}

// The folder of one composition, inside the working directory of the chat.
pub fn room_of(workdir: &Path, name: &str) -> PathBuf {
    let given = name.trim();
    let clean: String = given
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let clean = clean.trim_matches('-');
    let head = if clean.is_empty() { story::DEFAULT_PROJECT } else { clean };
    workdir.join(story::ROOT).join(head)
}

// The composition from its folder. None is returned when there is no composition yet.
fn read(room: &Path) -> Option<Project> {
    let text = std::fs::read_to_string(room.join(story::COMPOSITION_FILE)).ok()?;
    story::from_words(&text)
}

// The composition written to its folder.
fn write(room: &Path, project: &Project) -> Result<(), String> {
    std::fs::create_dir_all(room)
        .map_err(|why| format!("the folder of the composition could not be created: {why}"))?;
    std::fs::write(room.join(story::COMPOSITION_FILE), story::words(project))
        .map_err(|why| format!("the composition could not be written: {why}"))
}

// ---------------------------------------------------------------- the header

// The header of a composition, and the whole composition read back.
fn header(room: &Path, workdir: &Path, given: &Value, vision: bool) -> Made {
    let name = room
        .file_name()
        .map(|one| one.to_string_lossy().to_string())
        .unwrap_or_else(|| story::DEFAULT_PROJECT.to_string());
    let had = read(room);
    let base = match &had {
        None => Project { name: name.clone(), ..Project::default() },
        Some(one) => one.clone(),
    };
    let read = story::merge_project(&base, given);
    if !read.given {
        // Nothing was given, so the call is a read of the composition.
        return match had {
            None => refused(format!(
                "there is no composition in {} yet. Create one by naming the frame size and what \
                 the video is about, as \"{}\", \"about\": \"...\", \"canvas\": \"1080p\".",
                room.display(),
                name
            )),
            Some(held) => told(story::account(&held)),
        };
    }
    let mut project = read.project;
    // The name of a composition is the name of its folder, which a call does not write into the
    // file.
    project.name = name;
    let mut notes = read.notes;
    let mut faults = read.faults;
    // A file named by a layer is looked for in the folder of the composition and then in the
    // working directory, and it is stored in the form the drawing finds it in, so every render
    // reads the same file.
    fix_media(&mut project, room, workdir, &mut faults);
    if project.layers.len() > story::GREATEST_LAYERS {
        faults.push(format!(
            "{} layers is more than the {} a composition allows.",
            project.layers.len(),
            story::GREATEST_LAYERS
        ));
        project.layers.truncate(story::GREATEST_LAYERS);
    }
    if let Err(why) = write(room, &project) {
        return refused(why);
    }
    let (fonts, said) = pair();
    if !said.is_empty() {
        notes.push(said);
    }
    let mut out = format!(
        "The composition \"{}\" is saved in {}.\n{}",
        project.name,
        room.display(),
        story::account(&project)
    );
    out.push_str(&said_faults(&faults, &notes));
    // The frames the call asked to look at, which is how the work is judged before a single second
    // of the film has been rendered out.
    let (pictures, seen) = peek(room, &project, given, vision, None, &fonts);
    out.push_str(&still_words(&seen));
    told_with(out, pictures)
}

// The fields that were refused and the values that were corrected, under the state of the video.
// A fault is not a rejection: what was read stands, and what was not is reported.
fn said_faults(faults: &[String], notes: &[String]) -> String {
    let mut out = String::new();
    if !faults.is_empty() {
        out.push_str("\nNot accepted:\n");
        out.push_str(&faults.join("\n"));
    }
    if !notes.is_empty() {
        out.push_str("\nCorrected:\n");
        out.push_str(notes.join("\n").as_str());
    }
    out
}

// ---------------------------------------------------------------- one layer

// One layer of a composition: added, changed, deleted, or read.
fn layer(room: &Path, workdir: &Path, given: &Value, vision: bool) -> Made {
    let Some(mut project) = read(room) else {
        return refused(format!(
            "there is no composition in {} yet. Create one first, with {PROJECT_NAME}.",
            room.display()
        ));
    };
    // The layer is named by its id, its name, or its place in the list, which a model often
    // writes as a number rather than as a word.
    let named = story::text_of(given, "one").unwrap_or_default();
    let place = if named.is_empty() {
        None
    } else {
        match story::find(&project.layers, &named) {
            Some(one) => Some(one),
            None => {
                return refused(format!(
                    "there is no layer \"{named}\" in this composition. The layers are: {}",
                    list_of(&project)
                ));
            }
        }
    };
    // A deleted layer.
    if given.get("remove").and_then(Value::as_bool).unwrap_or(false) {
        let Some(at) = place.or_else(|| number(given, "at")) else {
            return refused(format!(
                "name the layer to delete, in `one` or in `at`. The layers are: {}",
                list_of(&project)
            ));
        };
        let gone = project.layers.remove(at);
        if let Err(why) = write(room, &project) {
            return refused(why);
        }
        return told(format!(
            "{} was deleted.\n{}",
            story::name_of(&gone),
            story::account(&project)
        ));
    }
    // A read layer. A read may ask for a frame as well, which is how a model looks at the work
    // after a change made somewhere else.
    if given.get("layer").is_none() {
        let mut out = match place {
            Some(at) => {
                let words = serde_json::to_string_pretty(&story::layer_words(&project.layers[at]))
                    .unwrap_or_else(|_| "{}".to_string());
                format!(
                    "{} of \"{}\", as it stands:\n{words}",
                    story::name_of(&project.layers[at]),
                    project.name
                )
            }
            None => story::account(&project),
        };
        if given.get("preview").is_some() {
            let whole = story::seconds(&project);
            let hint = place.map(|at| mid_of(&project.layers[at], whole));
            let (fonts, _) = pair();
            let (pictures, seen) = peek(room, &project, given, vision, hint, &fonts);
            out.push_str(&still_words(&seen));
            return told_with(out, pictures);
        }
        return told(out);
    }
    // A changed or an added layer. The fields of the call are merged into the fields that stand,
    // so a call that moves a caption does not empty its text.
    let body = given.get("layer").cloned().unwrap_or(Value::Null);
    let mut faults: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let at = match place {
        Some(one) => one,
        None => number(given, "at").unwrap_or(project.layers.len()).min(project.layers.len()),
    };
    let made = match place {
        None => story::read_layer(&body, at + 1, &mut faults),
        Some(at) => {
            // The layer that stands is written out, the new fields replace the old ones, and the
            // result is read back as one layer.
            let mut held = story::layer_words(&project.layers[at]);
            let new = object_of(&body);
            if let (Some(map), Some(given)) = (held.as_object_mut(), new.as_object()) {
                for (key, value) in given {
                    map.insert(key.clone(), value.clone());
                }
            }
            story::read_layer(&held, at + 1, &mut faults)
        }
    };
    let Some(mut made) = made else {
        return refused(if faults.is_empty() {
            "the layer does not parse".to_string()
        } else {
            faults.join("; ")
        });
    };
    if made.id.trim().is_empty() {
        made.id = story::next_id(&project);
    }
    if made.kind == "audio" && made.volume <= 0.0 {
        made.volume = 1.0;
        notes.push("an audio layer set no volume, so it is set to full.".to_string());
    }
    fix_one_media(&mut made, room, workdir, &mut faults);
    if place.is_none() && made.z == 0 {
        made.z = story::next_z(&project);
    }
    if at < project.layers.len() {
        project.layers[at] = made;
    } else {
        project.layers.push(made);
    }
    if let Err(why) = write(room, &project) {
        return refused(why);
    }
    let (fonts, said) = pair();
    if !said.is_empty() {
        notes.push(said);
    }
    let mut shots = fresh_shots(&project);
    let trouble = trouble_of(&project, at, &fonts, room, &mut shots);
    let whole = story::seconds(&project);
    let mut out = format!(
        "{} saved: {}.\n{}",
        story::name_of(&project.layers[at]),
        story::one_line(&project.layers[at], whole),
        story::headline(&project)
    );
    if !trouble.is_empty() {
        out.push_str("\nProblems with this layer:\n");
        out.push_str(&trouble.join("\n"));
    }
    out.push_str(&said_faults(&faults, &notes));
    // The frame the call asked for, at the middle of the time this layer owns, so the layer may be
    // seen where it stands over the rest of the film.
    let hint = mid_of(&project.layers[at], story::seconds(&project));
    let (pictures, seen) = peek(room, &project, given, vision, Some(hint), &fonts);
    out.push_str(&still_words(&seen));
    told_with(out, pictures)
}

// The lines of the still frames, under the state of the video.
fn still_words(lines: &[String]) -> String {
    let mut held: Vec<String> = lines.to_vec();
    held.sort();
    held.dedup();
    if held.is_empty() {
        return String::new();
    }
    format!("\n{}", held.join("\n"))
}

// The layers of a video, one per line, so a fault can name what may be named.
fn list_of(project: &Project) -> String {
    if project.layers.is_empty() {
        return String::from("none yet");
    }
    project
        .layers
        .iter()
        .map(|one| format!("{} [{}]", story::name_of(one), one.kind))
        .collect::<Vec<String>>()
        .join(", ")
}

// A number of a call, when it is present and is a number.
fn number(given: &Value, key: &str) -> Option<usize> {
    given
        .get(key)
        .and_then(Value::as_f64)
        .filter(|one| *one >= 0.0)
        .map(|one| one as usize)
}

// The object of a call, or an empty object when the value is not an object.
fn object_of(given: &Value) -> Value {
    match given {
        Value::Object(_) => given.clone(),
        _ => json!({}),
    }
}

// ---------------------------------------------------------------- the cut-out

// A subject keyed out of an image or of a frame of a video, written into the folder of the
// composition, shown to the model, with the settings that made it.
fn cutout(room: &Path, workdir: &Path, given: &Value, vision: bool, jailed: bool) -> Made {
    let said = given.get("media").and_then(Value::as_str).unwrap_or("").trim();
    if said.is_empty() {
        return refused(format!(
            "the key needs a file name in `media`. The file is read from {} or from the working \
             directory of this chat.",
            room.display()
        ));
    }
    let path = match found(workdir, room, said, jailed) {
        Ok(one) => one,
        Err(why) => return refused(why),
    };
    let at = given.get("at").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
    let cap = canvas_of(given, room);
    let info = match media::probe(&path) {
        Ok(one) => one,
        Err(why) => return refused(why),
    };
    let mut frame = match info.kind {
        media::Kind::Picture => match media::read_still(&path, cap) {
            Ok(one) => one,
            Err(why) => return refused(why),
        },
        media::Kind::Film => {
            let rate = if info.fps > 0.0 { info.fps } else { 25.0 };
            match media::Clip::open(&path, cap, rate) {
                Err(why) => return refused(why),
                Ok(mut clip) => match clip.frame(at) {
                    Ok(Some(one)) => one,
                    Ok(None) => {
                        return refused(format!(
                            "there is no frame at {} of {}",
                            story::trim_time(at),
                            path.display()
                        ))
                    }
                    Err(why) => return refused(why),
                },
            }
        }
        media::Kind::Sound => {
            return refused("an audio file has no pixels to key out".to_string())
        }
    };
    // The screen colour: named, or taken from the border of the picture.
    let named = given.get("colour").and_then(Value::as_str).unwrap_or("auto").trim().to_string();
    let colour = match key_colour(&frame, &named) {
        Ok(one) => one,
        Err(why) => return refused(why),
    };
    let agreement = media::border_agreement(&frame, colour);
    // The bounds of the ramp, measured from this picture, stand under the numbers of the call. A
    // number of the call is honoured; a number left out is the number the picture asked for.
    let (measured, measured_blend) = media::key_bounds(&frame, colour, 16).unwrap_or((0.08, 0.1));
    let similarity = given
        .get("similarity")
        .and_then(Value::as_f64)
        .unwrap_or(measured);
    let blend = given.get("blend").and_then(Value::as_f64).unwrap_or(measured_blend);
    let spill = given.get("spill").and_then(Value::as_f64).unwrap_or(0.6);
    let feather = given.get("feather").and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as u32;
    let choke = given.get("choke").and_then(Value::as_f64).unwrap_or(0.0);
    let despill = given.get("despill").and_then(Value::as_f64).unwrap_or(0.0);
    media::key_out(&mut frame, colour, similarity, blend, spill);
    if feather > 0 {
        media::feather(&mut frame, feather);
    }
    if choke.abs() > 0.001 {
        media::choke(&mut frame, choke.clamp(-0.5, 0.5));
    }
    if despill > 0.0 {
        media::despill(&mut frame, colour, despill.clamp(0.0, 1.0));
    }
    // What the preview shows: the keyed subject, the alpha matte, or the subject over a colour,
    // which is where a fringe of the screen colour is easiest to see.
    let show = given.get("show").and_then(Value::as_str).unwrap_or("cut").trim().to_string();
    let preview = match show.as_str() {
        "matte" => matte_of(&frame),
        "over" => {
            let against = anim::colour_of(
                given.get("against").and_then(Value::as_str).unwrap_or("#101418"),
            )
            .unwrap_or([0.06, 0.08, 0.09]);
            over(&frame, against)
        }
        _ => frame.clone(),
    };
    let stem = path
        .file_name()
        .map(|one| one.to_string_lossy().to_string())
        .unwrap_or_else(|| String::from("cut"));
    let file = format!("cut-{}.png", stem.replace('.', "-"));
    let made = room.join(&file);
    let _ = std::fs::create_dir_all(room);
    if let Err(why) = media::write_png(&made, &preview) {
        return refused(why);
    }
    let kept = frame.kept();
    let key = if named.eq_ignore_ascii_case("auto") { hex_of(colour) } else { named.clone() };
    let mut out = format!(
        "The key of {} is saved at {}.\nKey colour: {} ({:.0}% of the border of the picture \
         matches it). Kept: {:.1}% of the image.\n{}\nPut these effects on the layer: \
         {{\"kind\":\"chroma\",\"colour\":\"{key}\",\"similarity\":{},\"blend\":{},\"spill\":{}}}{}",
        stem,
        made.display(),
        hex_of(colour),
        agreement * 100.0,
        kept * 100.0,
        bounds_words(&frame),
        story::trim(similarity),
        story::trim(blend),
        story::trim(spill),
        if despill > 0.0 {
            format!(
                ", and {{\"kind\":\"despill\",\"colour\":\"{key}\",\"strength\":{}}}",
                story::trim(despill)
            )
        } else {
            String::new()
        }
    );
    out.push_str(&advice(kept, agreement));
    let mut pictures = Vec::new();
    if vision {
        pictures.push(picture(&preview, file));
    } else {
        out.push_str(
            "\nThe image is not returned, because this model has no vision. The file is in the \
             folder of the composition for the user to open.",
        );
    }
    Made { report: tools::Report { ok: true, output: out }, pictures }
}

// The colour a key is taken against, and the reason it could not be found.
fn key_colour(frame: &media::Frame, named: &str) -> Result<[f64; 3], String> {
    if !named.is_empty() && !named.eq_ignore_ascii_case("auto") {
        return anim::colour_of(named).ok_or_else(|| {
            format!("\"{named}\" is not a colour. Name it as green, or as #00ff00.")
        });
    }
    let found = media::border_colour(frame)
        .ok_or_else(|| "the colour of the border of the image could not be read".to_string())?;
    let agreement = media::border_agreement(frame, found);
    if agreement < 0.4 {
        return Err(format!(
            "the border of the image is not one colour - only {:.0}% of it matches the colour \
             detected - so there is no screen key to remove. Name the colour in `colour`, or cut \
             the subject out with a mask of shape \"path\".",
            agreement * 100.0
        ));
    }
    Ok(found)
}

// A keyed frame as its alpha matte: white where the subject is, black where the screen was.
fn matte_of(frame: &media::Frame) -> media::Frame {
    let mut made = frame.clone();
    for share in made.rgba.chunks_exact_mut(4) {
        let value = share[3];
        share[0] = value;
        share[1] = value;
        share[2] = value;
        share[3] = 255;
    }
    made
}

// A keyed frame composited over a colour, which is where a fringe of screen colour is seen.
fn over(frame: &media::Frame, colour: [f64; 3]) -> media::Frame {
    let part = |one: f64| (one.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (back_r, back_g, back_b) = (part(colour[0]), part(colour[1]), part(colour[2]));
    let mut made = frame.clone();
    for share in made.rgba.chunks_exact_mut(4) {
        let alpha = share[3] as f64 / 255.0;
        share[0] = ((share[0] as f64) * alpha + (back_r as f64) * (1.0 - alpha)).round() as u8;
        share[1] = ((share[1] as f64) * alpha + (back_g as f64) * (1.0 - alpha)).round() as u8;
        share[2] = ((share[2] as f64) * alpha + (back_b as f64) * (1.0 - alpha)).round() as u8;
        share[3] = 255;
    }
    made
}

// Where the subject of a key sits in the frame.
fn bounds_words(frame: &media::Frame) -> String {
    match frame.bounds() {
        Some((left, top, width, height)) => format!(
            "The subject occupies {}x{} at {},{} of the {}x{} of the image.",
            width,
            height,
            left,
            top,
            frame.width,
            frame.height
        ),
        None => String::from("Nothing of the image was kept."),
    }
}

// What the numbers of a key mean, in the words a model can act on.
fn advice(kept: f64, agreement: f64) -> String {
    let mut out = String::new();
    if kept > 0.97 {
        out.push_str(
            "\nAlmost the whole image was kept, so the screen is still visible. Raise \
             `similarity`, or name the screen colour in `colour`.",
        );
    }
    if kept < 0.03 {
        out.push_str(
            "\nNothing of the image was kept. Lower `similarity`, or name a colour closer to the \
             screen than the one being removed.",
        );
    }
    if agreement < 0.55 && kept > 0.03 && kept < 0.97 {
        out.push_str(
            "\nThe screen colour was detected and the border of the image does not match it \
             closely, so parts of the subject may have been removed. Name the screen colour in \
             `colour`.",
        );
    }
    out
}

// A colour as its hex form.
fn hex_of(colour: [f64; 3]) -> String {
    let part = |one: f64| (one.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", part(colour[0]), part(colour[1]), part(colour[2]))
}

// ---------------------------------------------------------------- the check

// Every fault in a composition. The faults of the layers are found by drawing them and reading
// what the drawing says; nothing of the check is written except the still frames the call asked
// for, which go back to the model as images.
fn check(room: &Path, given: &Value, vision: bool) -> Made {
    let Some(project) = read(room) else {
        return refused(format!("there is no composition in {} yet.", room.display()));
    };
    if project.layers.is_empty() {
        return refused(format!("the composition has no layer. Add one with {LAYER_NAME}."));
    }
    let (fonts, said) = pair();
    let mut shots = fresh_shots(&project);
    let whole = story::seconds(&project);
    let mut faults: Vec<String> = Vec::new();
    if !said.is_empty() {
        faults.push(said);
    }
    // What is wrong with the media files, reported once for the video.
    faults.extend(stage::notes(&project, room, &mut shots));
    if let Some(at) = given.get("at").and_then(Value::as_f64) {
        // One frame, drawn complete: every layer that is on screen at that second. The frame being
        // checked is also the frame the model is shown, unless the call asked for others.
        let held = at.clamp(0.0, whole.max(0.0));
        let _ = stage::frame(&project, &fonts, room, held, &mut shots);
        faults.extend(shots.notes());
        let mut asked = given.clone();
        if asked.get("preview").is_none() && !whole.is_nan() && whole > 0.0 {
            asked["preview"] = json!("now");
        }
        let (pictures, seen) = peek(room, &project, &asked, vision, Some(held), &fonts);
        return answer(&project, faults, whole, pictures, seen);
    }
    if whole <= 0.0 {
        faults.push(
            "the video has no duration: no layer sets the second it ends at, and the header set \
             no length. Set `length` on the video, or `to` on the layers."
                .to_string(),
        );
    }
    let mut visible = 0usize;
    for place in 0..project.layers.len() {
        let one = &project.layers[place];
        if one.kind != "audio" {
            if whole > 0.0 && !story::shows(one, mid_of(one, whole), whole) {
                faults.push(format!(
                    "{}: it is never on screen, because the video runs for {} and it starts at {}. \
                     Set `from` before the end of the video.",
                    story::name_of(one),
                    story::trim_time(whole),
                    story::trim(one.from)
                ));
                continue;
            }
            visible += 1;
        }
        faults.extend(trouble_of(&project, place, &fonts, room, &mut shots));
    }
    if visible == 0 {
        faults.push(
            "nothing in the video is visible: every layer is an audio layer. Add a text, shape or \
             image layer."
                .to_string(),
        );
    }
    faults.extend(shots.notes());
    let (pictures, seen) = peek(room, &project, given, vision, None, &fonts);
    answer(&project, faults, whole, pictures, seen)
}

// The answer of the check. The still frames the call asked for ride on the answer even when the
// check found faults, for a composition is mended from what is seen of it.
fn answer(
    project: &Project,
    mut faults: Vec<String>,
    whole: f64,
    pictures: Vec<Picture>,
    seen: Vec<String>,
) -> Made {
    faults.sort();
    faults.dedup();
    let said = format!("{} layers, {} long.", project.layers.len(), story::trim_time(whole));
    let looked = still_words(&seen);
    if faults.is_empty() {
        return told_with(format!("Nothing is wrong. {said} It can be rendered.{looked}"), pictures);
    }
    let mut out = format!(
        "{} fault{} in the composition, {said}:\n{}",
        faults.len(),
        if faults.len() == 1 { "" } else { "s" },
        faults.join("\n")
    );
    out.push_str(&looked);
    Made { report: tools::Report { ok: false, output: out }, pictures }
}

// The middle of the time range a layer owns, which is where its text is measured.
fn mid_of(one: &Layer, whole: f64) -> f64 {
    let end = if one.to > 0.0 { one.to } else { whole };
    let end = end.max(one.from + 0.04);
    (one.from + end) / 2.0
}

// What one layer has wrong with it: text that does not fit its box, and a file that is missing or
// is a different media type than the layer states.
fn trouble_of(
    project: &Project,
    place: usize,
    fonts: &fonts::Fonts,
    room: &Path,
    shots: &mut stage::Shots,
) -> Vec<String> {
    let Some(one) = project.layers.get(place) else { return Vec::new() };
    let whole = story::seconds(project);
    let (width, height) = story::size_of(project);
    let palette = story::colours_of(project);
    let at = mid_of(one, whole);
    let mut out: Vec<String> = Vec::new();
    if matches!(one.kind.as_str(), "image" | "video" | "audio") {
        if one.media.trim().is_empty() {
            out.push(format!(
                "{}: a layer of kind {} names no file.",
                story::name_of(one),
                one.kind
            ));
            return out;
        }
        let path = stage::media_key(room, &one.media);
        match shots.kind(&path) {
            Ok(info) => {
                let want = if one.kind == "audio" {
                    media::Kind::Sound
                } else if one.kind == "video" {
                    media::Kind::Film
                } else {
                    media::Kind::Picture
                };
                // A still image in a video layer is allowed: it is drawn as a still frame.
                if info.kind != want && !(one.kind == "video" && info.kind == media::Kind::Picture)
                {
                    out.push(format!(
                        "{}: {} is {} and the layer states {}.",
                        story::name_of(one),
                        one.media,
                        said_kind(info.kind),
                        one.kind
                    ));
                }
            }
            Err(why) => out.push(format!("{}: {why}", story::name_of(one))),
        }
        return out;
    }
    // Text, tables, charts and shapes, drawn at the middle of their own time range.
    let pose = anim::pose(one, at, whole);
    let box_w = (pose.w * width as f64).max(1.0).round() as u32;
    let box_h = (pose.h * height as f64).max(1.0).round() as u32;
    let sheet = paint::draw(one, &pose, box_w, box_h, (width, height), fonts, &palette);
    for fault in sheet.faults {
        out.push(format!("{}: {fault}", story::name_of(one)));
    }
    out
}

// What a file was detected as, in words.
fn said_kind(kind: media::Kind) -> &'static str {
    match kind {
        media::Kind::Picture => "a still image",
        media::Kind::Film => "a video",
        media::Kind::Sound => "an audio file",
    }
}

// ---------------------------------------------------------------- the render

// The frames of a composition, encoded into one file.
fn render(
    room: &Path,
    given: &Value,
    vision: bool,
    cancel: &AtomicBool,
    told: &mut dyn FnMut(&str),
) -> Made {
    let Some(project) = read(room) else {
        return refused(format!("there is no composition in {} yet.", room.display()));
    };
    if project.layers.iter().all(|one| one.kind == "audio") {
        return refused(
            "the composition has nothing visible in it, so there is no video to draw.".to_string(),
        );
    }
    let want = given.get("form").and_then(Value::as_str).unwrap_or("mp4");
    let Some(form) = encode::form_of(want) else {
        return refused(format!("\"{want}\" is not a file form. Take mp4, gif or png."));
    };
    let (fonts, said) = pair();
    if !said.is_empty() {
        told(&said);
    }
    let (width, height) = story::size_of(&project);
    let whole = story::seconds(&project);
    let fps = given
        .get("fps")
        .and_then(Value::as_f64)
        .map(|one| one as u32)
        .unwrap_or(project.fps)
        .clamp(story::LEAST_FPS, story::GREATEST_FPS);
    let name = given
        .get("name")
        .and_then(Value::as_str)
        .filter(|one| !one.trim().is_empty())
        .unwrap_or(&project.name)
        .to_string();
    let mut notes: Vec<String> = Vec::new();
    if form == encode::Form::Mp4 && !encode::has_ffmpeg() {
        return refused(media::need_ffmpeg());
    }
    // The stills of the video, which are how the model judges the work without opening the file.
    let moments = stills_of(given, whole, vision);
    if form == encode::Form::Png {
        let at = given.get("at").and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, whole.max(0.0));
        let mut shots = fresh_shots(&project);
        let made = match stage::frame(&project, &fonts, room, at, &mut shots) {
            Ok(one) => one,
            Err(why) => return refused(why),
        };
        notes.extend(shots.notes());
        let mut writer = match encode::Writer::open(room, &name, form, width, height, fps, &[]) {
            Ok(one) => one,
            Err(why) => return refused(why),
        };
        if let Err(why) = writer.push(&made, at) {
            return refused(why);
        }
        let path = match writer.close() {
            Ok(one) => one,
            Err(why) => return refused(why),
        };
        let size = std::fs::metadata(&path).map(|one| one.len()).unwrap_or(0);
        let file = path
            .file_name()
            .map(|one| one.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut out = format!(
            "The still of {} is saved at {} ({} kB).",
            story::trim_time(at),
            path.display(),
            size / 1024
        );
        out.push_str(&fault_words(&notes));
        let mut pictures = Vec::new();
        if vision {
            match std::fs::read(&path) {
                Ok(bytes) => pictures.push(picture_of(bytes, file)),
                Err(why) => out.push_str(&format!("\nThe still could not be read back: {why}")),
            }
        }
        return Made { report: tools::Report { ok: true, output: out }, pictures };
    }
    if whole <= 0.0 {
        return refused(
            "the video has no duration, so there is nothing to draw. Set `length` on the \
             composition, or `to` on its layers.".to_string(),
        );
    }
    // The audio of the composition, which only an MP4 carries.
    let mixed = given.get("sound").and_then(Value::as_bool).unwrap_or(true);
    let sounds = if mixed && form == encode::Form::Mp4 {
        sounds_of(&project, room, whole, &mut notes)
    } else {
        Vec::new()
    };
    if form == encode::Form::Gif {
        notes.push("a GIF carries no audio, so the audio of the composition was left out.".to_string());
    }
    let frames = ((whole * fps as f64).ceil() as usize).max(1);
    let every = (frames / 20).max(1);
    let mut writer = match encode::Writer::open(room, &name, form, width, height, fps, &sounds) {
        Ok(one) => one,
        Err(why) => {
            return refused(if notes.is_empty() {
                why
            } else {
                format!("{why}\n{}", notes.join("\n"))
            })
        }
    };
    let mut shots = fresh_shots(&project);
    for step in 0..frames {
        if cancel.load(Ordering::Relaxed) {
            writer.abort();
            return refused(format!(
                "the render was stopped after {} of {frames} frames.",
                step + 1
            ));
        }
        let at = step as f64 / fps as f64;
        match stage::frame(&project, &fonts, room, at, &mut shots) {
            Ok(made) => {
                if let Err(why) = writer.push(&made, at) {
                    writer.abort();
                    return refused(why);
                }
            }
            Err(why) => {
                writer.abort();
                return refused(why);
            }
        }
        // The faults of the pixels are taken out of the run as they arrive, so one fault of a
        // file is not repeated for every frame of a video.
        notes.extend(shots.notes());
        if step % every == 0 || step + 1 == frames {
            told(&format!(
                "frame {} of {frames}, at {}",
                step + 1,
                story::trim_time(at)
            ));
        }
    }
    let path = match writer.close() {
        Ok(one) => one,
        Err(why) => {
            if notes.is_empty() {
                return refused(why);
            }
            return refused(format!("{why}\n{}", notes.join("\n")));
        }
    };
    let size = std::fs::metadata(&path).map(|one| one.len()).unwrap_or(0);
    let mut out = format!(
        "The file is at {} ({} kB, {} at {} fps).",
        path.display(),
        size / 1024,
        story::trim_time(whole),
        fps
    );
    out.push_str(&fault_words(&notes));
    // The stills, drawn again at the seconds that were asked for. Three frames of a video of a
    // minute take a moment to draw, and they are the only way the model sees its own work. Each of
    // them is written into the `frames` folder too, so the user may look at the same pictures.
    let mut pictures = Vec::new();
    let mut shots = fresh_shots(&project);
    let dir = room.join(FRAMES_DIR);
    let keepable = std::fs::create_dir_all(&dir).is_ok();
    for at in &moments {
        let held = story::trim_time(*at);
        match stage::frame(&project, &fonts, room, *at, &mut shots) {
            Ok(made) => {
                let written = if keepable {
                    write_still(&dir, &project, &made, *at).ok()
                } else {
                    None
                };
                match written {
                    Some(file) => {
                        if let Ok(bytes) = std::fs::read(dir.join(&file)) {
                            pictures.push(picture_of(bytes, file));
                        }
                    }
                    None => {
                        if let Ok(bytes) = made.encode_png() {
                            pictures
                                .push(picture_of(bytes, format!("still-{}.png", story::trim(*at))));
                        }
                    }
                }
            }
            Err(why) => out.push_str(&format!("\nThe still at {held} could not be drawn: {why}")),
        }
    }
    if !pictures.is_empty() {
        out.push_str(&format!(
            "\n{} stills of the video are returned: {}.",
            pictures.len(),
            moments
                .iter()
                .map(|one| story::trim_time(*one))
                .collect::<Vec<String>>()
                .join(", ")
        ));
    } else if vision {
        out.push_str("\nNo still was returned; ask again with `preview` to look at the video.");
    }
    Made { report: tools::Report { ok: true, output: out }, pictures }
}

// The seconds kept of those that were named: within the film, rounded to a thousandth, in order,
// without repeats, and no more than the most one call returns. An empty list is answered with
// three frames across the film.
fn kept_moments(named: Vec<f64>, whole: f64) -> Vec<f64> {
    let held: Vec<f64> = if named.is_empty() {
        [0.2, 0.5, 0.8].iter().map(|part| whole * *part).collect()
    } else {
        named
    };
    let mut made: Vec<f64> = held
        .into_iter()
        .filter(|one| *one >= 0.0 && *one <= whole)
        .map(|one| (one * 1000.0).round() / 1000.0)
        .collect();
    made.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    made.dedup();
    made.truncate(MOST_STILLS);
    made
}

// The seconds of the film a call asked to look at, while the film is still being built. `preview`
// is one second, a list of seconds, `true` for three frames across the film, 'now' for the moment
// the call is about - which for a layer is the middle of the time the layer owns - 'film' for the
// three frames across the film, and 'none' for nothing at all. A call that names no `preview` is
// shown nothing, so a call that only moves a caption does not fill the context with pictures.
fn peeked(given: &Value, whole: f64, hint: Option<f64>) -> Vec<f64> {
    if whole <= 0.0 {
        return Vec::new();
    }
    let Some(value) = given.get("preview") else {
        return Vec::new();
    };
    let said = value.as_str().unwrap_or("").trim().to_ascii_lowercase();
    if matches!(value, Value::Bool(false)) || said == "none" {
        return Vec::new();
    }
    let named: Vec<f64> = match value {
        Value::Array(rows) => rows.iter().filter_map(|one| one.as_f64()).collect(),
        Value::Number(_) => value.as_f64().into_iter().collect(),
        Value::String(_) if said.parse::<f64>().is_ok() => {
            vec![said.parse::<f64>().unwrap_or(0.0)]
        }
        // `true`, or a word that is no second: the moment the call is about, when it has one.
        _ => match hint {
            Some(one) if said != "film" && said != "across" => vec![one],
            _ => Vec::new(),
        },
    };
    let kept = kept_moments(named, whole);
    // A call that named `preview` at all wants to see something, even when the seconds it named
    // fall outside the film.
    if kept.is_empty() {
        return kept_moments(Vec::new(), whole);
    }
    kept
}

// The frames a call asked for, drawn from the composition as it stands. Every frame is written
// into the `frames` folder of the composition, so the user may look at the same picture the model
// is shown, and the pictures are what a model with vision is sent. This is how the work is judged
// while it is being built, instead of rendering the film out and taking shots from the file.
fn peek(
    room: &Path,
    project: &Project,
    given: &Value,
    vision: bool,
    hint: Option<f64>,
    fonts: &fonts::Fonts,
) -> (Vec<Picture>, Vec<String>) {
    let moments = peeked(given, story::seconds(project), hint);
    if moments.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let dir = room.join(FRAMES_DIR);
    if let Err(why) = std::fs::create_dir_all(&dir) {
        return (
            Vec::new(),
            vec![format!("the folder of the stills could not be made: {why}")],
        );
    }
    let mut shots = fresh_shots(project);
    let mut pictures: Vec<Picture> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut written: Vec<String> = Vec::new();
    for at in moments {
        let held = story::trim_time(at);
        match stage::frame(project, fonts, room, at, &mut shots) {
            Ok(made) => match write_still(&dir, project, &made, at) {
                Ok(file) => {
                    written.push(held.clone());
                    if vision {
                        match std::fs::read(dir.join(&file)) {
                            Ok(bytes) => pictures.push(picture_of(bytes, file)),
                            Err(why) => {
                                notes.push(format!("the still could not be read back: {why}"))
                            }
                        }
                    }
                }
                Err(why) => notes.push(format!("the still at {held} could not be written: {why}")),
            },
            Err(why) => notes.push(format!("the still at {held} could not be drawn: {why}")),
        }
        notes.extend(shots.notes());
    }
    if !written.is_empty() && vision {
        notes.push(format!(
            "{} stills of the work in progress are returned, at {}. They are in {}.",
            written.len(),
            written.join(", "),
            dir.display()
        ));
    } else if !written.is_empty() {
        notes.push(format!(
            "{} stills of the work in progress are written into {} at {}, but they are not \
             returned: this model has no vision.",
            written.len(),
            dir.display(),
            written.join(", ")
        ));
    }
    (pictures, notes)
}

// One frame written into the folder of the stills, under a name that says which second of the film
// it is. The name of the file is answered, for the model to be told where to look.
fn write_still(
    dir: &Path,
    project: &Project,
    made: &resvg::tiny_skia::Pixmap,
    at: f64,
) -> Result<String, String> {
    let head = if project.name.trim().is_empty() {
        story::DEFAULT_PROJECT
    } else {
        project.name.trim()
    };
    let file = format!("{head}-at-{}s.png", story::trim(at));
    let bytes = made
        .encode_png()
        .map_err(|why| format!("the frame could not be encoded: {why}"))?;
    std::fs::write(dir.join(&file), bytes)
        .map_err(|why| format!("the frame could not be written: {why}"))?;
    Ok(file)
}

// The seconds of the video to return as stills. Three are taken across the video when the call
// asked for stills and named no seconds; none are drawn when the model cannot be shown an image,
// or when the call said 'none'.
fn stills_of(given: &Value, whole: f64, vision: bool) -> Vec<f64> {
    if !vision || whole <= 0.0 {
        return Vec::new();
    }
    let named: Vec<f64> = match given.get("preview") {
        Some(Value::Array(rows)) => rows
            .iter()
            .filter_map(|one| {
                one.as_f64().or_else(|| {
                    one.as_str()
                        .filter(|word| word.eq_ignore_ascii_case("none"))
                        .map(|_| -1.0)
                })
            })
            .collect(),
        Some(Value::String(word)) if word.eq_ignore_ascii_case("none") => return Vec::new(),
        Some(Value::Bool(false)) => return Vec::new(),
        Some(one) => one.as_f64().into_iter().collect(),
        None => Vec::new(),
    };
    if named.iter().any(|one| *one < 0.0) {
        return Vec::new();
    }
    let held: Vec<f64> = if named.is_empty() {
        [0.2, 0.5, 0.8].iter().map(|part| whole * *part).collect()
    } else {
        named
    };
    let mut made: Vec<f64> = held
        .into_iter()
        .filter(|one| *one >= 0.0 && *one <= whole)
        .map(|one| (one * 1000.0).round() / 1000.0)
        .collect();
    made.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    made.dedup();
    made.truncate(MOST_STILLS);
    made
}

// The audio that goes under a video: the track the composition named, the audio layers, and the
// video layers whose own audio was asked for.
fn sounds_of(project: &Project, room: &Path, whole: f64, notes: &mut Vec<String>) -> Vec<encode::Sound> {
    let mut out: Vec<encode::Sound> = Vec::new();
    if let Some(bed) = &project.audio {
        if !bed.media.trim().is_empty() {
            let path = stage::media_key(room, &bed.media);
            match media::probe(&path) {
                Ok(info) => {
                    let length = if bed.length > 0.0 {
                        bed.length
                    } else {
                        (info.duration - bed.from).max(0.0)
                    };
                    out.push(encode::Sound {
                        path,
                        from: 0.0,
                        length,
                        in_at: bed.from,
                        speed: 1.0,
                        volume: if bed.volume > 0.0 { bed.volume } else { 1.0 },
                        fade_in: bed.fade_in,
                        fade_out: bed.fade_out,
                    });
                }
                Err(why) => notes.push(format!("the audio of the video: {why}")),
            }
        }
    }
    for one in &project.layers {
        let heard = match one.kind.as_str() {
            "audio" => true,
            // A video layer keeps its own audio only when the layer asks for it, which is a
            // volume above zero.
            "video" => one.volume > 0.0,
            _ => false,
        };
        if !heard || one.media.trim().is_empty() {
            continue;
        }
        let path = stage::media_key(room, &one.media);
        let info = match media::probe(&path) {
            Ok(info) => info,
            Err(why) => {
                notes.push(format!("{}: {why}", story::name_of(one)));
                continue;
            }
        };
        let speed = if one.speed.abs() < 0.01 { 1.0 } else { one.speed.abs() };
        // How much of the file is used, and how long it is audible on screen.
        let from_file = if one.out_at > one.in_at {
            one.out_at - one.in_at
        } else if info.duration > one.in_at {
            info.duration - one.in_at
        } else {
            0.0
        };
        let on_screen = if one.to > one.from {
            one.to - one.from
        } else {
            (whole - one.from).max(0.0)
        };
        let length = if from_file > 0.0 {
            from_file.min(on_screen / speed)
        } else {
            on_screen / speed
        };
        if length <= 0.0 {
            notes.push(format!(
                "{}: it is on screen for no time, so none of its audio is heard.",
                story::name_of(one)
            ));
            continue;
        }
        out.push(encode::Sound {
            path,
            from: one.from.max(0.0),
            length,
            in_at: one.in_at,
            speed,
            volume: if one.volume > 0.0 { one.volume } else { 1.0 },
            fade_in: 0.0,
            fade_out: 0.0,
        });
    }
    out
}

// What was found wrong with the pixels of a video, under the answer.
fn fault_words(notes: &[String]) -> String {
    let mut held: Vec<String> = notes.to_vec();
    held.sort();
    held.dedup();
    if held.is_empty() {
        return String::new();
    }
    format!("\nReported while drawing:\n{}", held.join("\n"))
}

// ---------------------------------------------------------------- helps

// The fonts of one run, with a note for the model when the machine has none.
fn pair() -> (fonts::Fonts, String) {
    let made = fonts::Fonts::new();
    let said = if made.body == "sans-serif" {
        "no fonts were found on this machine, so the letters of the video are substituted"
            .to_string()
    } else {
        String::new()
    };
    (made, said)
}

// The work of drawing a video, set to the frame size of the composition.
fn fresh_shots(project: &Project) -> stage::Shots {
    let (width, height) = story::size_of(project);
    let mut made = stage::Shots::new();
    made.set_cap(width.max(height));
    made
}

// The longer side of the frame a composition is drawn at. The composition is read from its folder
// when it exists, which is how a call that works on files before the composition is complete
// still decodes at the right size.
fn canvas_of(given: &Value, room: &Path) -> u32 {
    if let Some(found) = given.get("canvas").and_then(Value::as_str) {
        if let Some((width, height)) = story::canvas_of(found) {
            return width.max(height);
        }
    }
    if let Some(project) = read(room) {
        let (width, height) = story::size_of(&project);
        return width.max(height);
    }
    1920
}

// Where a file named by a call is: the folder of the composition first, then the working
// directory of the chat, then the machine when the sandbox of the chat is off.
fn found(workdir: &Path, room: &Path, given: &str, jailed: bool) -> Result<PathBuf, String> {
    let named = Path::new(given);
    if named.is_absolute() {
        let path = named.to_path_buf();
        if jailed && !path.starts_with(workdir) {
            return Err(format!(
                "{} is outside the working directory of this chat, and the sandbox of the chat is \
                 on. Copy the file into the working directory, or turn the sandbox off.",
                path.display()
            ));
        }
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("there is no file at {}", path.display()));
    }
    let in_room = room.join(given);
    if in_room.exists() {
        return Ok(in_room);
    }
    let in_work = workdir.join(given);
    if in_work.exists() {
        return Ok(in_work);
    }
    Err(format!(
        "there is no file \"{given}\" in {} or in the working directory of the chat.",
        room.display()
    ))
}

// The path of the file one layer reads, stored in the form the drawing finds it in: the name of a
// file in the folder of the composition is left as it was, and a file of the working directory is
// stored as its whole path. A name nothing was found under is left as it was, and the fault is
// reported.
fn fix_one_media(one: &mut Layer, room: &Path, workdir: &Path, faults: &mut Vec<String>) {
    let named = one.media.trim().to_string();
    if named.is_empty() {
        return;
    }
    if Path::new(&named).is_absolute() {
        one.media = named.clone();
        if !Path::new(&named).exists() {
            faults.push(format!(
                "{}: there is no file at {named}",
                story::name_of(one)
            ));
        }
        return;
    }
    if room.join(&named).exists() {
        one.media = named;
        return;
    }
    let in_work = workdir.join(&named);
    if in_work.exists() {
        one.media = in_work.display().to_string();
        return;
    }
    faults.push(format!(
        "{}: there is no file \"{named}\" in {} or in the working directory of the chat.",
        story::name_of(one),
        room.display()
    ));
}

// The paths of every file of a composition: every layer that draws pixels, the background and the
// audio track.
fn fix_media(project: &mut Project, room: &Path, workdir: &Path, faults: &mut Vec<String>) {
    for one in &mut project.layers {
        fix_one_media(one, room, workdir, faults);
    }
    let background = project.background.media.trim().to_string();
    if !background.is_empty() {
        let mut held = Layer {
            id: String::from("background"),
            name: String::from("the background"),
            kind: String::from("image"),
            media: background,
            ..Layer::default()
        };
        fix_one_media(&mut held, room, workdir, faults);
        project.background.media = held.media;
    }
    if let Some(bed) = &mut project.audio {
        let sound = bed.media.trim().to_string();
        if !sound.is_empty() {
            let mut held = Layer {
                id: String::from("audio"),
                name: String::from("the audio of the video"),
                kind: String::from("audio"),
                media: sound,
                ..Layer::default()
            };
            fix_one_media(&mut held, room, workdir, faults);
            bed.media = held.media;
        }
    }
}

// A frame of pixels as the image a model is shown.
fn picture(frame: &media::Frame, name: String) -> Picture {
    match frame.png() {
        Ok(bytes) => picture_of(bytes, name),
        Err(_) => Picture {
            name,
            mime: String::from("image/png"),
            data: String::new(),
        },
    }
}

// The bytes of a PNG as the image a model is shown.
fn picture_of(bytes: Vec<u8>, name: String) -> Picture {
    Picture {
        name,
        mime: String::from("image/png"),
        data: format!("data:image/png;base64,{}", util::b64(&bytes)),
    }
}

// ---------------------------------------------------------------- the prompt

// The whole vocabulary of a composition, in the text a model reads. It is added to the system
// messages of an ask while the line of the tool badge is on, which is what lets the five tool
// descriptions stay short.
pub fn system_prompt() -> String {
    let ink = prompts::Films {
        project: PROJECT_NAME,
        layer: LAYER_NAME,
        cutout: CUTOUT_NAME,
        check: CHECK_NAME,
        render: RENDER_NAME,
        file: story::COMPOSITION_FILE,
        frames: FRAMES_DIR,
        canvases: &story::canvas_words(),
        palettes: &story::palette_names(),
        kinds: &story::kind_names(),
        motions: &story::MOTIONS.join(", "),
        props: &story::PROPS.join(", "),
        effects: &story::EFFECTS.join(", "),
        blends: &story::BLENDS.join(", "),
        eases: &story::EASES.join(", "),
    };
    prompts::films(&ink)
}

#[cfg(test)]
mod checks {
    use super::*;

    // A folder for one check, under the name that check is known by.
    fn place(name: &str) -> PathBuf {
        let work = std::env::temp_dir().join(format!("autumn-natter-film-{name}"));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).expect("the folder is created");
        work
    }

    // One call, answered as the model is answered.
    fn call(name: &str, raw: &str, work: &Path) -> Made {
        let cancel = AtomicBool::new(false);
        run(name, raw, work, false, true, &cancel, |_| {})
    }

    // One call, answered as a model with vision is answered.
    fn call_seeing(name: &str, raw: &str, work: &Path) -> Made {
        let cancel = AtomicBool::new(false);
        run(name, raw, work, true, true, &cancel, |_| {})
    }

    // The five names of the suite are the five of the definitions, and the names of the previous
    // version of this suite are gone.
    #[test]
    fn the_five_names_of_the_suite() {
        let said = definitions();
        assert_eq!(said.len(), 5);
        for one in &said {
            let name = one["function"]["name"].as_str().expect("a tool is named");
            assert!(is_tool(name), "\"{name}\" is not known as a tool");
        }
        assert!(!is_tool("infographic_scene"));
        assert!(!is_tool("infographic_plan"));
        assert!(!is_tool("infographic_guide"));
    }

    // A header call with nothing but the name of a composition says there is nothing there yet,
    // and a call that names the fields of the header creates the composition.
    #[test]
    fn a_header_creates_a_composition() {
        let work = place("header");
        let empty = call(PROJECT_NAME, "{\"project\":\"loop\"}", &work);
        assert!(!empty.report.ok, "nothing is there yet: {}", empty.report.output);
        let made = call(
            PROJECT_NAME,
            "{\"project\":\"loop\",\"about\":\"What a loop is\",\"canvas\":\"720p\",\"fps\":30,\"length\":4,\"palette\":\"ink\"}",
            &work,
        );
        assert!(made.report.ok, "the header: {}", made.report.output);
        assert!(made.report.output.contains("What a loop is"), "the about: {}", made.report.output);
        let held = read(&room_of(&work, "loop")).expect("the composition was written");
        assert_eq!(held.canvas, "720p");
        assert_eq!(held.fps, 30);
        assert_eq!(held.length, 4.0);
        assert_eq!(held.name, "loop", "the name of a composition is the name of its folder");
        // A read of the composition is its state, and it says how long the video is.
        let again = call(PROJECT_NAME, "{\"project\":\"loop\"}", &work);
        assert!(again.report.ok, "the read: {}", again.report.output);
        assert!(again.report.output.contains("4 s"), "the duration: {}", again.report.output);
        let _ = std::fs::remove_dir_all(&work);
    }

    // A field that is not understood is refused with the values that are known, and the fields
    // that were understood are saved.
    #[test]
    fn a_field_that_is_not_understood_is_reported() {
        let work = place("fault");
        let made = call(
            PROJECT_NAME,
            "{\"project\":\"fault\",\"canvas\":\"wallpaper\",\"fps\":120,\"palette\":\"ink\"}",
            &work,
        );
        assert!(made.report.output.contains("is not a size"), "{}", made.report.output);
        assert!(made.report.output.contains("frame rate"), "{}", made.report.output);
        let held = read(&room_of(&work, "fault")).expect("the composition was written");
        assert_eq!(held.fps, 24, "the frame rate was not accepted");
        assert_eq!(held.palette, "ink", "the palette was accepted");
        let _ = std::fs::remove_dir_all(&work);
    }

    // A layer is added, changed, read back and deleted, and a change of one field leaves the text
    // of the layer as it was.
    #[test]
    fn a_layer_is_added_changed_and_deleted() {
        let work = place("layer");
        call(PROJECT_NAME, "{\"project\":\"one\",\"canvas\":\"720p\",\"length\":4}", &work);
        let added = call(
            LAYER_NAME,
            "{\"project\":\"one\",\"layer\":{\"id\":\"head\",\"kind\":\"text\",\"from\":0,\"to\":4,\"x\":0.5,\"y\":0.2,\"w\":0.6,\"h\":0.12,\"style\":{\"text\":\"**Autumn**\",\"size\":0.06}}}",
            &work,
        );
        assert!(added.report.ok, "the layer: {}", added.report.output);
        assert!(added.report.output.contains("head"), "{}", added.report.output);
        let changed = call(LAYER_NAME, "{\"project\":\"one\",\"one\":\"head\",\"layer\":{\"y\":0.3}}", &work);
        assert!(changed.report.ok, "the change: {}", changed.report.output);
        let held = read(&room_of(&work, "one")).expect("the composition was written");
        assert_eq!(held.layers.len(), 1);
        assert_eq!(held.layers[0].y, 0.3, "the position was changed");
        assert_eq!(
            held.layers[0].style.text, "**Autumn**",
            "the text of the layer survived a change of its position"
        );
        let read_back = call(LAYER_NAME, "{\"project\":\"one\",\"one\":\"head\"}", &work);
        assert!(read_back.report.output.contains("**Autumn**"), "{}", read_back.report.output);
        let gone = call(LAYER_NAME, "{\"project\":\"one\",\"one\":1,\"remove\":true}", &work);
        assert!(gone.report.ok, "the deletion: {}", gone.report.output);
        let held = read(&room_of(&work, "one")).expect("the composition was written");
        assert!(held.layers.is_empty(), "the layer was deleted");
        let _ = std::fs::remove_dir_all(&work);
    }

    // A layer that names no file is reported by the check with the name of the layer, and a
    // missing file is reported as well.
    #[test]
    fn the_check_reports_what_is_wrong() {
        let work = place("check");
        call(PROJECT_NAME, "{\"project\":\"two\",\"canvas\":\"720p\",\"length\":3}", &work);
        call(
            LAYER_NAME,
            "{\"project\":\"two\",\"layer\":{\"id\":\"photo\",\"kind\":\"image\",\"media\":\"nowhere.png\",\"from\":0,\"to\":3,\"x\":0.5,\"y\":0.5,\"w\":0.6,\"h\":0.4}}",
            &work,
        );
        let said = call(CHECK_NAME, "{\"project\":\"two\"}", &work);
        assert!(!said.report.ok, "the video is not correct");
        assert!(said.report.output.contains("photo"), "{}", said.report.output);
        assert!(said.report.output.contains("no file"), "{}", said.report.output);
        let _ = std::fs::remove_dir_all(&work);
    }

    // A composition of a heading and a shape is checked, drawn as a PNG, and written into the
    // folder of the composition.
    #[test]
    fn a_video_is_drawn_into_a_file() {
        let work = place("render");
        call(
            PROJECT_NAME,
            "{\"project\":\"three\",\"canvas\":\"720p\",\"fps\":12,\"length\":1,\"palette\":\"ink\"}",
            &work,
        );
        call(
            LAYER_NAME,
            "{\"project\":\"three\",\"layer\":{\"id\":\"head\",\"kind\":\"text\",\"from\":0,\"to\":1,\"x\":0.5,\"y\":0.3,\"w\":0.7,\"h\":0.12,\"style\":{\"text\":\"**Three**\",\"size\":0.08}}}",
            &work,
        );
        let made = call(RENDER_NAME, "{\"project\":\"three\",\"form\":\"png\"}", &work);
        assert!(made.report.ok, "the still: {}", made.report.output);
        let path = room_of(&work, "three").join("three.png");
        assert!(path.exists(), "the PNG is written at {}", path.display());
        let bytes = std::fs::read(&path).expect("the file is read");
        assert_eq!(&bytes[0..4], &[0x89, b'P', b'N', b'G'], "the file is a PNG");
        let _ = std::fs::remove_dir_all(&work);
    }

    // A GIF is written by this program whatever the machine has, and the file is a real GIF.
    #[test]
    fn a_gif_is_written_whatever_the_machine_has() {
        let work = place("gif");
        call(
            PROJECT_NAME,
            "{\"project\":\"four\",\"canvas\":\"square\",\"fps\":10,\"length\":0.4}",
            &work,
        );
        call(
            LAYER_NAME,
            "{\"project\":\"four\",\"layer\":{\"id\":\"bar\",\"kind\":\"shape\",\"from\":0,\"to\":0.4,\"x\":0.5,\"y\":0.5,\"w\":0.4,\"h\":0.1,\"style\":{\"shape\":\"rect\",\"fill\":\"#c2410c\"}}}",
            &work,
        );
        let made = call(RENDER_NAME, "{\"project\":\"four\",\"form\":\"gif\"}", &work);
        assert!(made.report.ok, "the GIF: {}", made.report.output);
        let bytes =
            std::fs::read(room_of(&work, "four").join("four.gif")).expect("the file is read");
        assert_eq!(&bytes[0..6], b"GIF89a", "the file is a GIF");
        let _ = std::fs::remove_dir_all(&work);
    }

    // A file is found in the folder of the composition, and a path outside the working directory
    // is refused while the sandbox of the chat is on.
    #[test]
    fn a_file_is_found_in_the_folder_of_the_composition() {
        let work = place("found");
        let room = room_of(&work, "main");
        std::fs::create_dir_all(&room).expect("the folder is created");
        std::fs::write(room.join("plate.png"), b"not a png").expect("a file is written");
        let seen = found(&work, &room, "plate.png", true);
        assert_eq!(seen.expect("the file is found"), room.join("plate.png"));
        assert!(found(&work, &room, "/etc/hosts", true).is_err(), "outside is refused");
        assert!(found(&work, &room, "/etc/hosts", false).is_ok(), "and allowed with it off");
        let _ = std::fs::remove_dir_all(&work);
    }

    // The state line of a call names what the call does and which composition it works on.
    #[test]
    fn the_state_line_names_the_composition() {
        assert_eq!(shown(RENDER_NAME, "{\"project\":\"loop\"}"), "Rendering a video (loop)");
        assert_eq!(shown(CHECK_NAME, "{}"), "Checking a composition");
    }

    // The stills of a video: three when stills were asked for and no seconds named, the seconds
    // that were named, and none when the model cannot be shown an image.
    #[test]
    fn the_stills_of_a_video_are_chosen_from_the_video() {
        let said = stills_of(&json!({}), 10.0, true);
        assert_eq!(said, vec![2.0, 5.0, 8.0], "three across the video");
        let given = json!({ "preview": [0.5, 1.5, 2.5, 3.5, 4.5] });
        assert_eq!(stills_of(&given, 10.0, true), vec![0.5, 1.5, 2.5], "and no more than three");
        let given = json!({ "preview": "none" });
        assert!(stills_of(&given, 10.0, true).is_empty(), "none was asked for");
        assert!(stills_of(&json!({}), 10.0, false).is_empty(), "and none for a model with no vision");
    }

    // The seconds a call asks to look at, while the film is still being built.
    #[test]
    fn the_moments_of_a_look_are_read_from_the_words() {
        // Nothing was asked for, so nothing is drawn: a call that moves a caption sends no picture.
        assert!(peeked(&json!({}), 10.0, Some(5.0)).is_empty());
        let given = json!({ "preview": "now" });
        assert_eq!(peeked(&given, 10.0, Some(1.75)), vec![1.75], "the moment of the call");
        assert_eq!(
            peeked(&given, 10.0, None),
            vec![2.0, 5.0, 8.0],
            "and the whole film when the call is about no moment of its own"
        );
        let given = json!({ "preview": true });
        assert_eq!(peeked(&given, 10.0, Some(2.0)), vec![2.0]);
        assert_eq!(peeked(&given, 10.0, None), vec![2.0, 5.0, 8.0], "true alone is the whole film");
        let given = json!({ "preview": "film" });
        assert_eq!(peeked(&given, 10.0, Some(2.0)), vec![2.0, 5.0, 8.0]);
        let given = json!({ "preview": 4.0 });
        assert_eq!(peeked(&given, 10.0, None), vec![4.0], "a second written as a number");
        let given = json!({ "preview": ["9", 1.0, 9.0] });
        assert_eq!(peeked(&given, 10.0, None), vec![1.0, 9.0], "only the numbers of the list");
        let given = json!({ "preview": "none" });
        assert!(peeked(&given, 10.0, Some(2.0)).is_empty(), "none was asked for");
        let given = json!({ "preview": false });
        assert!(peeked(&given, 10.0, Some(2.0)).is_empty(), "the same in words of a different kind");
        let given = json!({ "preview": 12.0 });
        assert_eq!(peeked(&given, 10.0, None), vec![2.0, 5.0, 8.0], "a second out of the film is not a look at nothing");
        assert!(peeked(&json!({ "preview": true }), 0.0, None).is_empty(), "a film of no length shows nothing");
    }

    // A frame of the work in progress comes back as an image and is written into the folder of the
    // composition, so the model sees what it built without rendering the film out.
    #[test]
    fn a_frame_of_the_work_in_progress_is_shown_and_kept() {
        let work = place("peek");
        call(PROJECT_NAME, "{\"project\":\"look\",\"canvas\":\"720p\",\"length\":4}", &work);
        // A call that names nothing sends no picture.
        let plain = call_seeing(
            LAYER_NAME,
            "{\"project\":\"look\",\"layer\":{\"id\":\"head\",\"kind\":\"text\",\"from\":0,\"to\":4,\"x\":0.5,\"y\":0.2,\"w\":0.6,\"h\":0.12,\"style\":{\"text\":\"**Autumn**\",\"size\":0.06}}}",
            &work,
        );
        assert!(plain.report.ok, "{}", plain.report.output);
        assert!(plain.pictures.is_empty(), "nothing was asked for");
        // The same composition, asked for the frame the layer is on screen for.
        let seen = call_seeing(LAYER_NAME, "{\"project\":\"look\",\"one\":\"head\",\"preview\":\"now\"}", &work);
        assert!(seen.report.ok, "{}", seen.report.output);
        assert_eq!(seen.pictures.len(), 1, "one frame is returned");
        let one = &seen.pictures[0];
        assert_eq!(one.mime, "image/png");
        assert!(one.data.starts_with("data:image/png;base64,"), "the form a model is shown in");
        assert!(seen.report.output.contains("stills of the work in progress"), "{}", seen.report.output);
        // The frame a check looks at is shown as well, for a fault of a composition is mended from
        // what is seen of it.
        let looked = call_seeing(CHECK_NAME, "{\"project\":\"look\",\"at\":1}", &work);
        assert_eq!(looked.pictures.len(), 1, "the frame that was checked: {}", looked.report.output);
        let room = room_of(&work, "look");
        let dir = room.join(FRAMES_DIR);
        let mut kept: Vec<String> = std::fs::read_dir(&dir)
            .expect("the folder of the stills")
            .filter_map(|one| one.ok())
            .map(|one| one.file_name().to_string_lossy().to_string())
            .collect();
        kept.sort();
        assert_eq!(
            kept,
            vec![String::from("look-at-1s.png"), String::from("look-at-2s.png")],
            "the frame of the layer and the frame of the check, written into the folder of the film"
        );
        let bytes = std::fs::read(dir.join("look-at-2s.png")).expect("the still is a file");
        assert_eq!(&bytes[..4], b"\x89PNG", "and a PNG of it");
        // A model with no vision is sent no picture, but the frame is still written for the user.
        let blind = call(LAYER_NAME, "{\"project\":\"look\",\"one\":\"head\",\"preview\":\"film\"}", &work);
        assert!(blind.pictures.is_empty(), "no vision, no images");
        assert!(blind.report.output.contains("no vision"), "{}", blind.report.output);
        assert_eq!(
            std::fs::read_dir(&dir).expect("the folder of the stills").count(),
            4,
            "the frame of the layer, the frame of the check, and the three of the film - two of
             them the same second"
        );
        assert!(dir.join("look-at-0.8s.png").is_file(), "a frame of the film");
        assert!(dir.join("look-at-3.2s.png").is_file(), "and the last of them");
        let _ = std::fs::remove_dir_all(&work);
    }

    // The prompt carries the vocabulary the tool descriptions do not, and no placeholder of the
    // format text is left in it.
    #[test]
    fn the_prompt_carries_the_vocabulary() {
        let said = system_prompt();
        assert!(said.contains("chroma"), "the effects are in the prompt");
        assert!(said.contains("keyframe"), "the keyframes are in the prompt");
        assert!(!said.contains("{{"), "no placeholder of the format text is left");
        assert!(said.contains(PROJECT_NAME), "the names of the tools are in the prompt");
    }

    // A film made of a film: a clip laid under a subject cut out of a green screen, with a heading
    // that travels above both, and the whole drawn out into a video file. This is the work the
    // suite is for, so it is checked against the two files a user gives it - a clip and a picture
    // of a screen - and the frames it makes are left where they can be looked at. The check is
    // asked for by name, and says what it is short of when the files are not named for it.
    //
    //     AN_TEST_STILL=testing/green-screen-testing.jpg AN_TEST_FILM=testing/big-buck-bunny-60fps-1080p.mp4 \
    //       cargo test -p autumn-natter-core --lib -- --ignored --nocapture a_subject_is_keyed
    #[test]
    #[ignore = "needs a clip and a picture of a green screen, which are not in the repository"]
    fn a_subject_is_keyed_onto_a_film() {
        let Ok(still) = std::env::var("AN_TEST_STILL") else {
            println!("SKIP AN_TEST_STILL names no picture of a green screen");
            return;
        };
        let Ok(film) = std::env::var("AN_TEST_FILM") else {
            println!("SKIP AN_TEST_FILM names no clip");
            return;
        };
        let (still, film) = (PathBuf::from(&still), PathBuf::from(&film));
        assert!(still.is_file(), "there is no picture at {}", still.display());
        assert!(film.is_file(), "there is no clip at {}", film.display());
        let work = place("keyed");
        // The picture is copied into the working directory, as a user would put it there. The clip
        // is named by its whole path, for a file of 150 MB is not copied about.
        let given = still.file_name().expect("the picture is named");
        std::fs::copy(&still, work.join(given)).expect("the picture is copied");
        let seeing = |name: &str, raw: &str| {
            let cancel = AtomicBool::new(false);
            run(name, raw, &work, true, false, &cancel, |_| {})
        };
        // The key is measured from the screen itself, and the subject is shown back on a colour
        // and as a matte.
        let cut = seeing(
            CUTOUT_NAME,
            &format!(
                "{{\"project\":\"keyed\",\"media\":\"{}\",\"show\":\"over\"}}",
                given.to_string_lossy()
            ),
        );
        assert!(cut.report.ok, "the key: {}", cut.report.output);
        assert!(cut.report.output.contains("Key colour"), "{}", cut.report.output);
        assert!(!cut.pictures.is_empty(), "the key is shown as an image");
        let similarity = cut
            .report
            .output
            .split("\"similarity\":")
            .nth(1)
            .and_then(|one| one.split(',').next().and_then(|n| n.parse::<f64>().ok()))
            .expect("the answer carries the similarity to take from it");
        // The film: the clip under the keyed subject, and a heading that travels over both.
        let head = seeing(
            PROJECT_NAME,
            "{\"project\":\"keyed\",\"canvas\":\"1080p\",\"fps\":12,\"length\":2,\
             \"about\":\"the presenter over the film\"}",
        );
        assert!(head.report.ok, "the header: {}", head.report.output);
        let clip = seeing(
            LAYER_NAME,
            &format!(
                "{{\"project\":\"keyed\",\"layer\":{{\"id\":\"clip\",\"kind\":\"video\",\
                 \"from\":0,\"to\":2,\"media\":\"{}\",\"in\":34,\"x\":0.5,\"y\":0.5,\"w\":1,\
                 \"h\":1,\"fit\":\"cover\",\"volume\":0}}}}",
                film.display()
            ),
        );
        assert!(clip.report.ok, "the clip: {}", clip.report.output);
        let subject = seeing(
            LAYER_NAME,
            &format!(
                "{{\"project\":\"keyed\",\"layer\":{{\"id\":\"host\",\"kind\":\"image\",\
                 \"from\":0,\"to\":2,\"media\":\"{}\",\"x\":0.76,\"y\":0.68,\"w\":0.44,\
                 \"h\":0.66,\"fit\":\"contain\",\"effects\":[{{\"kind\":\"chroma\",\
                 \"auto\":true,\"similarity\":{},\"blend\":0.12,\"spill\":0.6}}],\
                 \"enter\":{{\"kind\":\"rise\",\"seconds\":0.4}}}}}}",
                given.to_string_lossy(),
                similarity
            ),
        );
        assert!(subject.report.ok, "the subject: {}", subject.report.output);
        let words = seeing(
            LAYER_NAME,
            "{\"project\":\"keyed\",\"layer\":{\"id\":\"head\",\"kind\":\"text\",\"from\":0,\
             \"to\":2,\"x\":0.30,\"y\":0.12,\"w\":0.56,\"h\":0.14,\"style\":{\"text\":\
             \"**Keyed over the film**\",\"size\":0.05},\"anim\":[{\"prop\":\"x\",\
             \"keys\":[[0.0,0.18],[2.0,0.42]],\"ease\":\"smooth\"}]}}",
        );
        assert!(words.report.ok, "the heading: {}", words.report.output);
        // The check finds the faults, and shows the frames it judged. These are the frames of the
        // work in progress: no file has been rendered out yet.
        let checked = seeing(CHECK_NAME, "{\"project\":\"keyed\",\"preview\":\"film\"}");
        println!("check: {}", checked.report.output);
        assert_eq!(checked.pictures.len(), 3, "three frames of the work in progress are shown");
        let room = room_of(&work, "keyed");
        let frames = room.join(FRAMES_DIR);
        assert!(frames.is_dir(), "the frames are kept for the user as well");
        // And the whole film is drawn out into a file, with the sound of the clip left out of it.
        let made = seeing(RENDER_NAME, "{\"project\":\"keyed\",\"form\":\"mp4\",\"sound\":false}");
        let file = room.join("keyed.mp4");
        if media::has_ffmpeg() {
            assert!(made.report.ok, "the render: {}", made.report.output);
            let bytes = std::fs::read(&file).expect("the video is written");
            assert!(!bytes.is_empty(), "the video holds something");
            println!(
                "the film is at {} - {} bytes, {} frames",
                file.display(),
                bytes.len(),
                24
            );
        } else {
            println!("SKIP ffmpeg is not on this machine, so no mp4 was asked for");
        }
        println!("the frames of the work in progress are in {}", frames.display());
        // The folder is left in place, so what was drawn may be looked at after the run.
    }
}

