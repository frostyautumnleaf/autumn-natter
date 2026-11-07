// Infographic videos: the tools the model works through, and the folder a
// project lives in.
//
// A video is not written in one call. The model makes a project, puts scenes
// into it, has the layout checked, and then has the frames made into a file.
// Each step answers with what it found, so a fault is put right while it is
// still one line of json and not a minute of nothing on the screen.
//
// The whole of a project is one file, `script.json`, in a folder of the room of
// the chat. Nothing of it is kept in the memory of the program, so a chat may be
// closed between the steps and the work stands.
//
// The five tools:
//
//   * `infographic_guide` - the shape of a script, the kinds of the blocks, the
//     colours, the sizes and the marks. The model reads this before it writes.
//   * `infographic_plan` - the head of a project and the list of its scenes.
//   * `infographic_scene` - one scene, with its blocks.
//   * `infographic_check` - the layout of the scenes, with every fault of it.
//   * `infographic_render` - the frames, made into a file.

pub mod draw;
pub mod encode;
pub mod fonts;
pub mod layout;
pub mod marks;
pub mod script;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde_json::{json, Value};

use crate::tools::with_rule;

pub const GUIDE_NAME: &str = "infographic_guide";
pub const PLAN_NAME: &str = "infographic_plan";
pub const SCENE_NAME: &str = "infographic_scene";
pub const CHECK_NAME: &str = "infographic_check";
pub const RENDER_NAME: &str = "infographic_render";

// True for the five names of this module.
pub fn is_tool(name: &str) -> bool {
    matches!(
        name,
        GUIDE_NAME | PLAN_NAME | SCENE_NAME | CHECK_NAME | RENDER_NAME
    )
}

// The words of a call, made into an object. The five tools of this suite take
// their parts as a json object and not as a command of the terminal, so the
// object is read as it stands, and through a fenced block when the model put one
// about it. It used to be read with `parse_arguments`, whose second answer is the
// reason a model gives for a terminal command: that reason is empty here, so
// every call of this suite came in with no part at all, and a plan was made of
// nothing but the parts the call left out.
fn object(raw: &str) -> Value {
    let text = raw.trim();
    let cut = crate::tools::extract_object(text).unwrap_or(text);
    match serde_json::from_str::<Value>(cut) {
        Ok(given) if given.is_object() => given,
        _ => json!({}),
    }
}

// The words of the state line for one call: the tool, and the project it works
// on when the model named one.
pub fn shown(name: &str, raw: &str) -> String {
    let head = match name {
        GUIDE_NAME => "the guide of a script",
        PLAN_NAME => "the plan of a video",
        SCENE_NAME => "a scene of a video",
        CHECK_NAME => "the layout of a video",
        RENDER_NAME => "the frames of a video",
        _ => "an infographic",
    };
    let given = object(raw);
    let named = given.get("project").and_then(Value::as_str).unwrap_or("");
    if named.trim().is_empty() {
        head.to_string()
    } else {
        format!("{head} ({named})")
    }
}

// The five definitions, for the list of tools of an ask.
pub fn definitions() -> Vec<Value> {
    let kinds = script::kind_names();
    let canvases = script::canvas_words();
    let colours = script::palette_names();
    vec![
        json!({
            "type": "function",
            "function": {
                "name": GUIDE_NAME,
                "description": with_rule(String::from(
                    "Read how an infographic script is built: the parts of a project, the parts of a scene, every kind of block with the words it takes, the colour sets, the sizes of the picture, and the marks that may be drawn. Read this once before you write a script, and read it again when a check says a word is not known."
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "description": "One kind of block to have the whole of its words for, such as 'bars' or 'steps'. Leave it out to have the guide of everything."
                        }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": PLAN_NAME,
                "description": with_rule(format!(
                    "Make or change the head of an infographic project and the list of its scenes. The head carries the name, the size of the picture, the frames of the second, the colours and the type. The scenes are given as a list of heads and lengths, and each one is filled later with the scene tool. A part you do not name is kept as it was. A list of scenes you give stands in place of the list that stood, but a scene head that names no blocks keeps the blocks that stood at its place, so coming back to the plan costs nothing. The sizes are: {canvases}. The colour sets are: {colours}. The project stands in the folder infographics/<name>/ of the room of this chat."
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "The name of the project. Leave it out for the one project of this chat." },
                        "about": { "type": "string", "description": "One line about what the video is of." },
                        "canvas": { "type": "string", "description": "The size of the picture." },
                        "fps": { "type": "integer", "description": "Frames of the second, 1 to 60. 24 is the rule." },
                        "palette": { "type": "string", "description": "The colour set." },
                        "font": { "type": "string", "description": "'sans', 'serif', 'mono', or the name of a family of this machine." },
                        "heading_weight": { "type": "string", "description": "'normal', 'bold' or 'black'." },
                        "background": { "type": "object", "description": "The ground of the scenes: kind, from, to, angle, vignette." },
                        "scenes": {
                            "type": "array",
                            "description": "The list of the scenes, in the order they are shown. Each one is a head and a length; the blocks come with the scene tool. Given at all, this list is what stands.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "title": { "type": "string" },
                                    "note": { "type": "string" },
                                    "seconds": { "type": "number" }
                                }
                            }
                        }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": SCENE_NAME,
                "description": with_rule(format!(
                    "Put one scene into an infographic project, or change one that stands. A scene is a head, a length in seconds, the way it comes in, and the blocks on the screen. Every block names its kind and its place on a grid of 24 columns and 16 rows: col, row, w, h. The kinds are: {kinds}. The words of a block are read as they come in and a fault is told to you at once, so write one scene at a time and mend what is said. Read the guide for the whole of the words of a kind."
                )),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "The name of the project." },
                        "at": { "type": "integer", "description": "The place of the scene, from 0. A place past the end adds a scene." },
                        "scene": { "type": "object", "description": "The scene: title, note, seconds, transition, blocks, background." }
                    },
                    "required": ["scene"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": CHECK_NAME,
                "description": with_rule(
                    "Lay out the scenes of an infographic project and say what is wrong: a block off the board, words that do not fit their box, a chart with no numbers, two blocks on one another, a scene with nothing in it. Nothing is drawn and nothing is written. Do this before you render, and mend what it says.".to_string()
                ),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "The name of the project." },
                        "at": { "type": "integer", "description": "One scene by its place, when only that scene is to be looked at." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": RENDER_NAME,
                "description": with_rule(
                    "Draw the frames of an infographic project and write the file: an MP4 when this machine has ffmpeg, an animated GIF when it has not, or one still PNG of a scene. The file is written into the folder of the project. A video of a minute takes a while; the run tells how far it has come.".to_string()
                ),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "The name of the project." },
                        "form": { "type": "string", "description": "'mp4', 'gif' or 'png'. An mp4 falls back to a gif when the machine has no ffmpeg." },
                        "at": { "type": "number", "description": "The moment in seconds of the still, for a png. 0 is the first frame." }
                    }
                }
            }
        }),
    ]
}

// The folder of one project.
pub fn room_of(workdir: &Path, name: &str) -> PathBuf {
    let clean = if name.trim().is_empty() {
        script::DEFAULT_PROJECT
    } else {
        name.trim()
    };
    workdir.join(script::ROOT).join(clean)
}

// The project out of its folder. Nothing comes back when there is no project
// yet, and the caller says so.
fn read(room: &Path) -> Option<script::Project> {
    let text = std::fs::read_to_string(room.join(script::SCRIPT_FILE)).ok()?;
    script::from_words(&text)
}

// The project written into its folder.
fn write(room: &Path, project: &script::Project) -> Result<(), String> {
    std::fs::create_dir_all(room)
        .map_err(|why| format!("the folder of the project could not be made: {why}"))?;
    std::fs::write(room.join(script::SCRIPT_FILE), script::words(project))
        .map_err(|why| format!("the script could not be written: {why}"))
}

// One call of one of the five tools. The answer is the words the model reads
// back; a fault is answered with the fault and nothing written.
pub fn run(
    name: &str,
    raw: &str,
    workdir: &Path,
    cancel: &AtomicBool,
    mut told: impl FnMut(&str),
) -> String {
    let given = object(raw);
    let room = room_of(workdir, given.get("project").and_then(Value::as_str).unwrap_or(""));
    match name {
        GUIDE_NAME => guide(&given),
        PLAN_NAME => plan(&room, &given),
        SCENE_NAME => scene(&room, &given),
        CHECK_NAME => check(&room, &given),
        RENDER_NAME => render(&room, &given, cancel, &mut told),
        other => format!("\"{other}\" is not a tool of the infographic suite"),
    }
}

// The shape of a script, in the words the model reads.
fn guide(given: &Value) -> String {
    let asked = given.get("kind").and_then(Value::as_str).unwrap_or("");
    if !asked.is_empty() {
        if !script::is_kind(asked) {
            return format!(
                "\"{asked}\" is not a kind of block. The kinds are: {}.",
                script::kind_names()
            );
        }
        return script::kind_words(asked);
    }
    script::guide()
}

// The head of a project and the list of its scenes.
fn plan(room: &Path, given: &Value) -> String {
    let name = room
        .file_name()
        .map(|one| one.to_string_lossy().to_string())
        .unwrap_or_else(|| script::DEFAULT_PROJECT.to_string());
    let had = read(room).unwrap_or_else(|| script::Project {
        name: name.clone(),
        about: String::new(),
        canvas: String::from("1080p"),
        fps: 24,
        palette: String::from("ink"),
        colours: Default::default(),
        font: String::new(),
        heading_weight: String::new(),
        background: Default::default(),
        scenes: Vec::new(),
    });
    // The head is made of the parts a call may name, and of nothing else. The
    // name of a project is the name of its folder, which the call does not write
    // into the script, and the scenes are read here as much as anywhere else, so
    // that a head of a scene is answered with the words of `read_scene` and comes
    // with a way of coming in, as every scene does.
    let mut head = serde_json::Map::new();
    for part in script::PROJECT_PARTS {
        if let Some(one) = given.get(*part) {
            head.insert((*part).to_string(), one.clone());
        }
    }
    let mut made = match script::merge(&had, &Value::Object(head)) {
        Ok(one) => one,
        Err(faults) => return faults.join("; "),
    };
    // The name of the project is the name of the folder it stands in.
    made.name = name;
    if let Err(why) = write(room, &made) {
        return why;
    }
    format!(
        "The project \"{}\" stands at {}.\n{}\nThe video is {} long.",
        made.name,
        room.display(),
        script::storyboard(&made),
        script::trim_time(script::seconds(&made))
    )
}

// One scene, with its blocks.
fn scene(room: &Path, given: &Value) -> String {
    let Some(body) = given.get("scene") else {
        return "the call carries no scene".to_string();
    };
    let Some(mut had) = read(room) else {
        return "there is no project here yet; make one with infographic_plan first".to_string();
    };
    let at = given
        .get("at")
        .and_then(Value::as_i64)
        .unwrap_or(had.scenes.len() as i64);
    if at < 0 || at as usize > script::GREATEST_SCENES {
        return format!("the place of a scene is between 0 and {}", script::GREATEST_SCENES);
    }
    // The blocks are looked at before the scene is made, so a word out of place
    // is answered with the word and not with a fault of the shape.
    if let Some(blocks) = body.get("blocks").and_then(Value::as_array) {
        let faults = script::block_faults(blocks);
        if !faults.is_empty() {
            return faults.join("; ");
        }
    }
    let made = match serde_json::from_value::<script::Scene>(body.clone()) {
        Ok(one) => one,
        Err(why) => return format!("the scene does not read: {why}"),
    };
    if made.blocks.is_empty() {
        return "the scene carries no block; a scene of nothing draws nothing".to_string();
    }
    if made.seconds <= 0.0 || made.seconds > script::GREATEST_SCENE_SECONDS {
        return format!(
            "the length of a scene is between 0 and {} seconds",
            script::trim_time(script::GREATEST_SCENE_SECONDS)
        );
    }
    let at = at as usize;
    if at < had.scenes.len() {
        had.scenes[at] = made;
    } else {
        had.scenes.push(made);
    }
    if let Err(why) = write(room, &had) {
        return why;
    }
    let (fonts, _) = pair();
    let (faults, drawn) = look(&had, Some(at), &fonts);
    let mut out = format!(
        "Scene {} of \"{}\" stands: {}.\n{}",
        at + 1,
        had.name,
        script::trim_time(had.scenes[at].seconds),
        script::storyboard(&had)
    );
    if drawn > 0 {
        out.push_str(&format!("\n{drawn} blocks were laid out."));
    }
    if !faults.is_empty() {
        out.push_str("\nThe layout says:\n");
        out.push_str(&faults.join("\n"));
    }
    out
}

// The layout of the scenes, with every fault of it.
fn check(room: &Path, given: &Value) -> String {
    let Some(project) = read(room) else {
        return "there is no project here yet".to_string();
    };
    if project.scenes.is_empty() {
        return "the project has no scene; put one in with infographic_scene".to_string();
    }
    let at = given.get("at").and_then(Value::as_i64);
    let (fonts, _) = pair();
    let (faults, drawn) = look(&project, at.map(|one| one as usize), &fonts);
    let whole = script::trim_time(script::seconds(&project));
    if faults.is_empty() {
        return format!(
            "Nothing is wrong. {} scenes, {drawn} blocks, {whole} long. It may be rendered.",
            project.scenes.len()
        );
    }
    format!(
        "{} faults in {} scenes, {whole} long:\n{}",
        faults.len(),
        project.scenes.len(),
        faults.join("\n")
    )
}

// The frames, made into a file.
fn render(
    room: &Path,
    given: &Value,
    cancel: &AtomicBool,
    told: &mut dyn FnMut(&str),
) -> String {
    let Some(project) = read(room) else {
        return "there is no project here yet".to_string();
    };
    let want = given.get("form").and_then(Value::as_str).unwrap_or("mp4");
    let Some(form) = encode::form_of(want) else {
        return format!("\"{want}\" is not a form of file. Take mp4, gif or png.");
    };
    if project.scenes.is_empty() {
        return "the project has no scene to draw".to_string();
    }
    let (fonts, said) = pair();
    if !said.is_empty() {
        told(&said);
    }
    // A fault of the layout does not stop a render: the model may want to see
    // what it looks like. The faults are put under the answer all the same.
    let (faults, _) = look(&project, None, &fonts);
    let made = if form == encode::Form::Png {
        let ms = given.get("at").and_then(Value::as_f64).unwrap_or(0.0) * 1000.0;
        encode::png(&project, &fonts, ms, Some(room))
            .map(|bytes| {
                let path = room.join(encode::out_name(&project.name, form));
                let wrote = std::fs::write(&path, bytes);
                match wrote {
                    Ok(_) => path,
                    Err(why) => PathBuf::from(format!("could not be written: {why}")),
                }
            })
    } else {
        encode::video(&project, &fonts, room, form, project.fps, |one| {
            if !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                told(one);
            }
        })
    };
    match made {
        Ok(path) => {
            let size = std::fs::metadata(&path).map(|one| one.len()).unwrap_or(0);
            let mut out = format!(
                "The file stands at {} ({} kB, {} long).",
                path.display(),
                size / 1024,
                script::trim_time(script::seconds(&project))
            );
            if !faults.is_empty() {
                out.push_str("\nThe layout said, and was not mended:\n");
                out.push_str(&faults.join("\n"));
            }
            out
        }
        Err(why) => why,
    }
}

// The fonts of one run, with a line for the log when nothing was found.
fn pair() -> (fonts::Fonts, String) {
    let made = fonts::Fonts::new();
    let said = if made.body == "sans-serif" {
        "no fonts were found on this machine; the letters are guessed".to_string()
    } else {
        String::new()
    };
    (made, said)
}

// The layout of a project, or of one scene of it. The answer is every fault, and
// the count of the blocks that were laid out.
fn look(project: &script::Project, at: Option<usize>, fonts: &fonts::Fonts) -> (Vec<String>, usize) {
    let size = script::canvas_of(&project.canvas)
        .unwrap_or(script::Canvas { word: "1080p", width: 1920, height: 1080 });
    let board = layout::stage(size.width, size.height);
    let palette = script::colours_of(project);
    let mut faults: Vec<String> = Vec::new();
    let mut drawn = 0usize;
    for (place, scene) in project.scenes.iter().enumerate() {
        if let Some(wanted) = at {
            if wanted != place {
                continue;
            }
        }
        // A scene with nothing in it draws nothing, and the video then has a blank
        // in the middle of it. The tool of the check says that it looks for this,
        // so it is looked for here.
        if scene.blocks.is_empty() {
            faults.push(format!(
                "scene {} ({}): the scene holds no block, so it draws nothing. Put the blocks in with infographic_scene, or take the scene out of the plan.",
                place + 1,
                scene.title
            ));
            continue;
        }
        let (made, bad) = layout::scene(project, &palette, fonts, &scene.blocks, board, None);
        drawn += made.len();
        for one in bad {
            faults.push(format!("scene {} ({}): {one}", place + 1, scene.title));
        }
    }
    (faults, drawn)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The room of one test: a folder of its own under the temp place, thrown
    // away when the test is over.
    fn place(what: &str) -> PathBuf {
        let room = std::env::temp_dir().join(format!(
            "atnatter-infographic-{}-{what}-{}",
            std::process::id(),
            std::time::UNIX_EPOCH
                .elapsed()
                .map(|one| one.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&room);
        room
    }

    // One call of one of the five tools, with no word of the way through.
    fn call(which: &str, raw: &str, work: &Path) -> String {
        let stop = AtomicBool::new(false);
        run(which, raw, work, &stop, |_| {})
    }

    // The plan tool answers with the storyboard of a project and writes its
    // script. It used to answer with three faults on every call, whatever the
    // model sent: the name of the project was put among the parts a call may
    // name, every part the call left out was put in as nothing, and the parts the
    // call did name never came at all, because the words of the call were read
    // with the reader of a terminal command.
    #[test]
    fn a_plan_of_scene_heads_stands_and_says_so() {
        let work = place("plan");
        let said = call(
            PLAN_NAME,
            "{\"project\":\"minecraft-explained\",\"about\":\"What Minecraft is\",\"canvas\":\"720p\",\"fps\":30,\"palette\":\"forest\",\"scenes\":[{\"title\":\"A block world\",\"seconds\":4},{\"title\":\"The loop\",\"seconds\":3.5},{\"title\":\"Most sold ever\",\"seconds\":2.5}]}",
            &work,
        );
        assert!(!said.contains("has no part"), "the plan was refused: {said}");
        assert!(!said.contains("does not read"), "the plan was refused: {said}");
        assert!(said.contains("minecraft-explained"), "the name: {said}");
        assert!(said.contains("A block world"), "the storyboard: {said}");
        assert!(said.contains("10"), "the length of the video: {said}");
        let held = read(&room_of(&work, "minecraft-explained")).expect("the script was written");
        assert_eq!(held.name, "minecraft-explained");
        assert_eq!(held.about, "What Minecraft is");
        assert_eq!(held.canvas, "720p");
        assert_eq!(held.fps, 30);
        assert_eq!(held.palette, "forest");
        assert_eq!(held.scenes.len(), 3);
        assert_eq!(held.scenes[1].seconds, 3.5);
        // A scene head comes with a way of coming in, as every scene does.
        assert_eq!(held.scenes[1].transition.kind, "fade");
        let _ = std::fs::remove_dir_all(&work);
    }

    // A plan that names nothing but the project stands too: the parts it says
    // nothing about keep what a project of this program stands on.
    #[test]
    fn a_plan_that_names_nothing_but_the_project_stands() {
        let work = place("bare");
        let said = call(PLAN_NAME, "{\"project\":\"bare\"}", &work);
        assert!(!said.contains("has no part"), "the plan was refused: {said}");
        assert!(!said.contains("does not read"), "the plan was refused: {said}");
        let held = read(&room_of(&work, "bare")).expect("the script was written");
        assert_eq!(held.canvas, "1080p");
        assert_eq!(held.fps, 24);
        assert_eq!(held.palette, "ink");
        assert!(held.scenes.is_empty());
        let _ = std::fs::remove_dir_all(&work);
    }

    // The parts of a call come through a fenced block as they come plain, the
    // guide answers for the one kind named, and the line over the block of the
    // run names the project the call works on.
    #[test]
    fn the_parts_of_a_call_come_through_a_fence() {
        let work = place("fence");
        let said = call(GUIDE_NAME, "```json\n{\"kind\":\"bars\"}\n```", &work);
        assert!(said.contains("A block of kind \"bars\""), "the guide of one kind: {said}");
        // The guide of everything, and the guide of one kind, are not the same.
        let all = call(GUIDE_NAME, "{}", &work);
        assert!(!all.contains("A block of kind \"bars\" reads"), "the whole guide: {all}");
        assert_eq!(
            shown(PLAN_NAME, "{\"project\":\"minecraft-explained\"}"),
            "the plan of a video (minecraft-explained)"
        );
        let _ = std::fs::remove_dir_all(&work);
    }

    // The way the model works, through: a plan of heads, one scene with blocks in
    // it, and the check of the layout. A scene of nothing is named by the check,
    // for it draws nothing and the video would stand on a blank in the middle of
    // it.
    #[test]
    fn a_scene_lands_on_a_plan_and_the_check_reads_the_video() {
        let work = place("flow");
        call(PLAN_NAME, "{\"project\":\"loop\"}", &work);
        let said = call(
            SCENE_NAME,
            "{\"project\":\"loop\",\"at\":0,\"scene\":{\"title\":\"The loop\",\"seconds\":4,\"blocks\":[{\"kind\":\"head\",\"text\":\"MINECRAFT\",\"col\":2,\"row\":2,\"w\":20,\"h\":3},{\"kind\":\"steps\",\"items\":[\"Explore\",\"Mine\",\"Craft\",\"Build\"],\"col\":2,\"row\":7,\"w\":20,\"h\":6}]}}",
            &work,
        );
        assert!(said.starts_with("Scene 1"), "the scene stands: {said}");
        assert!(said.contains("2 blocks were laid out"), "the blocks are laid: {said}");
        let told = call(CHECK_NAME, "{\"project\":\"loop\"}", &work);
        assert!(told.starts_with("Nothing is wrong"), "the check: {told}");
        // A second scene that was planned but never filled is a blank in the
        // video, and the check says so.
        call(
            PLAN_NAME,
            "{\"project\":\"loop\",\"scenes\":[{\"title\":\"The loop\",\"seconds\":4},{\"title\":\"The stats\",\"seconds\":3}]}",
            &work,
        );
        let told = call(CHECK_NAME, "{\"project\":\"loop\"}", &work);
        assert!(told.contains("holds no block"), "the empty scene is named: {told}");
        // Both scenes filled, the check has nothing left to say.
        call(
            SCENE_NAME,
            "{\"project\":\"loop\",\"at\":1,\"scene\":{\"title\":\"The stats\",\"seconds\":3,\"blocks\":[{\"kind\":\"stat\",\"value\":300,\"suffix\":\"m\",\"text\":\"copies sold\",\"col\":4,\"row\":5,\"w\":16,\"h\":6}]}}",
            &work,
        );
        let told = call(CHECK_NAME, "{\"project\":\"loop\"}", &work);
        assert!(told.starts_with("Nothing is wrong"), "the video may be rendered: {told}");
        let _ = std::fs::remove_dir_all(&work);
    }
}
