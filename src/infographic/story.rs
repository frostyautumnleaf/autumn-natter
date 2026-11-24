// The composition of a video: what is on the screen, and at what moment it is there.
//
// A composition is one file of json in the folder of the project, `composition.json`. It holds
// the header of the film - the size of the canvas, the frames of the second, how long the film
// runs, the ground the scenes stand on, the sound of the film - and the list of the layers.
// Nothing of a composition is kept in the memory of the program, so a chat may be closed between
// one call and the next and the film is still there, and a model may read the file itself with a
// command when it wants to see the whole of it.
//
// A layer is one thing on the screen: a heading, a table, a bar chart, a shape, a photograph, a
// film, a bed of sound. Every layer owns a stretch of the timeline, `from` and `to`, in seconds
// of the finished film, so a title may stand from 0.5 to 3.2 and a lower third from 4 to 6.5
// while the film under it runs the whole way. Two layers may hold the same moment, and the one
// with the higher `z` is drawn over the other. That is what an edit is made of: a base film, a
// graphic that arrives at a named second, a face cut out of a green screen beside it, and a caption
// under all of it.
//
// The place of a layer is given in parts of the canvas, not in pixels: `x` and `y` are the centre
// of the box from the top left of the screen, `w` and `h` are its width and height, all from 0.0
// to 1.0. A model therefore says "a fifth of the screen wide, in the middle of the left third"
// and the words of it do not change between 720p and 4k.
//
// This file reads and writes that shape, and says what is wrong with it. No pixels are drawn
// here: see `paint` and `stage`.

use std::path::Path;

use serde_json::{json, Value};

// The file a composition stands in, and the folder under the working directory of the chat that
// holds the projects.
pub const COMPOSITION_FILE: &str = "composition.json";
pub const ROOT: &str = "infographics";
pub const DEFAULT_PROJECT: &str = "main";
// The frame size a film gets when nothing was said about one.
pub const DEFAULT_CANVAS: &str = "1080p";
pub const DEFAULT_FPS: u32 = 24;
pub const DEFAULT_PALETTE: &str = "ink";

// The bounds a composition is kept inside. A film of an hour at sixty frames of the second is
// beyond what this machine should be asked to draw by a call of a tool.
pub const GREATEST_FPS: u32 = 60;
pub const LEAST_FPS: u32 = 1;
pub const GREATEST_LENGTH: f64 = 3600.0;
pub const GREATEST_LAYERS: usize = 240;
pub const GREATEST_KEYS: usize = 64;
pub const GREATEST_EFFECTS: usize = 12;

// The size of the canvas, in pixels. A name is accepted as well as the measure, so a model may
// say "1080p" and mean it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Canvas {
    pub word: &'static str,
    pub width: u32,
    pub height: u32,
}

pub const CANVASES: &[Canvas] = &[
    Canvas { word: "square", width: 1080, height: 1080 },
    Canvas { word: "story", width: 1080, height: 1920 },
    Canvas { word: "portrait", width: 1080, height: 1350 },
    Canvas { word: "720p", width: 1280, height: 720 },
    Canvas { word: "1080p", width: 1920, height: 1080 },
    Canvas { word: "1440p", width: 2560, height: 1440 },
    Canvas { word: "4k", width: 3840, height: 2160 },
    Canvas { word: "wide", width: 2560, height: 1080 },
    Canvas { word: "film", width: 2048, height: 858 },
];

// The measure a canvas word means: the name of one of the set, or 1280x720 as it stands.
pub fn canvas_of(word: &str) -> Option<(u32, u32)> {
    let said = word.trim().to_ascii_lowercase();
    if said.is_empty() {
        return None;
    }
    if let Some(found) = CANVASES.iter().find(|one| one.word.to_ascii_lowercase() == said) {
        return Some((found.width, found.height));
    }
    // 1280x720, 1280 X 720, 1280 by 720, 1280/720.
    let digits: Vec<String> = said
        .split(|c: char| !c.is_ascii_digit())
        .filter(|one| !one.is_empty())
        .map(|one| one.to_string())
        .collect();
    if digits.len() == 2 {
        let width = digits[0].parse::<u32>().ok()?;
        let height = digits[1].parse::<u32>().ok()?;
        if (64..=7680).contains(&width) && (64..=7680).contains(&height) {
            return Some((width, height));
        }
    }
    None
}

// The measure a canvas word means, or the measure of a film that named none.
pub fn size_of(project: &Project) -> (u32, u32) {
    canvas_of(&project.canvas).unwrap_or((1920, 1080))
}

// The words a model may put in `canvas`, for the prompt of the suite.
pub fn canvas_words() -> String {
    CANVASES
        .iter()
        .map(|one| format!("{} ({}x{})", one.word, one.width, one.height))
        .collect::<Vec<String>>()
        .join(", ")
}

// The colours a graphic is drawn in. A composition may name a set, or give its own colours in
// `colours`, which is how a brand of a client is honoured.
#[derive(Clone, Debug)]
pub struct Palette {
    pub ink: String,
    pub ground: String,
    pub soft: String,
    pub accent: String,
    pub series: Vec<String>,
}

impl Default for Palette {
    fn default() -> Self {
        Palette {
            ink: String::from("#12161c"),
            ground: String::from("#f7f5f0"),
            soft: String::from("#5d6672"),
            accent: String::from("#c2410c"),
            series: vec![
                String::from("#c2410c"),
                String::from("#0f766e"),
                String::from("#b45309"),
                String::from("#334155"),
                String::from("#7c2d12"),
                String::from("#4d7c0f"),
            ],
        }
    }
}

// The sets of colours a composition may name: the name of the set, the four colours of the
// graphic (ink on ground, the muted colour of notes, and the accent), and the series of a chart.
pub const PALETTES: &[(&str, [&str; 4], &[&str])] = &[
    (
        "ink",
        ["#12161c", "#f7f5f0", "#5d6672", "#c2410c"],
        &["#c2410c", "#0f766e", "#b45309", "#334155", "#7c2d12", "#4d7c0f"],
    ),
    (
        "night",
        ["#f2f4f8", "#0d1117", "#9aa7b8", "#f59e0b"],
        &["#f59e0b", "#38bdf8", "#a3e635", "#f472b6", "#fbbf24", "#818cf8"],
    ),
    (
        "leaf",
        ["#22211c", "#fdf9f1", "#6d6455", "#b45309"],
        &["#b45309", "#65a30d", "#a16207", "#3f6212", "#c2410c", "#0f766e"],
    ),
    (
        "sea",
        ["#0b1f2b", "#f1f7fa", "#4c6675", "#0e7490"],
        &["#0e7490", "#155e75", "#0369a1", "#0f766e", "#475569", "#7c3aed"],
    ),
    (
        "berry",
        ["#1d1119", "#fdf2f6", "#6b5560", "#be185d"],
        &["#be185d", "#7c3aed", "#c2410c", "#0f766e", "#b45309", "#334155"],
    ),
    (
        "mono",
        ["#101010", "#ffffff", "#6a6a6a", "#101010"],
        &["#101010", "#3d3d3d", "#6a6a6a", "#969696", "#c2c2c2", "#e0e0e0"],
    ),
];

pub fn palette_of(word: &str) -> Option<Palette> {
    let said = word.trim().to_ascii_lowercase();
    PALETTES.iter().find(|(one, _, _)| *one == said).map(|(_, four, series)| Palette {
        ink: String::from(four[0]),
        ground: String::from(four[1]),
        soft: String::from(four[2]),
        accent: String::from(four[3]),
        series: series.iter().map(|one| String::from(*one)).collect(),
    })
}

pub fn palette_names() -> String {
    PALETTES
        .iter()
        .map(|(one, _, _)| (*one).to_string())
        .collect::<Vec<String>>()
        .join(", ")
}

// The colours of a project: the set it names, with any colour the project changed.
pub fn colours_of(project: &Project) -> Palette {
    let mut made = palette_of(&project.palette).unwrap_or_default();
    let given = &project.colours;
    if !given.ink.is_empty() {
        made.ink = given.ink.clone();
    }
    if !given.ground.is_empty() {
        made.ground = given.ground.clone();
    }
    if !given.soft.is_empty() {
        made.soft = given.soft.clone();
    }
    if !given.accent.is_empty() {
        made.accent = given.accent.clone();
    }
    if !given.series.is_empty() {
        made.series = given.series.clone();
    }
    made
}

// The colour of the series of a chart, going round the set when there are more parts than colours.
pub fn series_colour(palette: &Palette, place: usize) -> String {
    if palette.series.is_empty() {
        return String::from("#000000");
    }
    palette.series[place % palette.series.len()].clone()
}

// A film: the header, and the layers of it in the order they were put in.
#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    pub about: String,
    pub canvas: String,
    pub fps: u32,
    // The length of the film in seconds. When it is 0.0 the film runs as long as its last layer.
    pub length: f64,
    pub palette: String,
    pub colours: Colours,
    pub font: String,
    pub heading_weight: String,
    pub background: Background,
    pub audio: Option<Audio>,
    pub tracks: Vec<Track>,
    pub layers: Vec<Layer>,
}

// A film with nothing in it but the header a call did not have to name: 1080p, 24 frames of the
// second, drawn in ink.
impl Default for Project {
    fn default() -> Self {
        Self {
            name: String::new(),
            about: String::new(),
            canvas: String::from(DEFAULT_CANVAS),
            fps: DEFAULT_FPS,
            length: 0.0,
            palette: String::from(DEFAULT_PALETTE),
            colours: Colours::default(),
            font: String::new(),
            heading_weight: String::new(),
            background: Background::default(),
            audio: None,
            tracks: Vec::new(),
            layers: Vec::new(),
        }
    }
}

// Colours a project sets over the set it named.
#[derive(Clone, Debug, Default)]
pub struct Colours {
    pub ink: String,
    pub ground: String,
    pub soft: String,
    pub accent: String,
    pub series: Vec<String>,
}

// The ground every layer stands on.
#[derive(Clone, Debug, Default)]
pub struct Background {
    // "solid", "gradient", "image" or "none".
    pub kind: String,
    pub colour: String,
    pub from: String,
    pub to: String,
    pub angle: f64,
    // 0.0 to 1.0: the darkening towards the corners of the screen.
    pub vignette: f64,
    pub media: String,
    // How an image of the ground fills the canvas.
    pub fit: String,
}

// The sound of the film, taken from one file. The sound of the layers of kind "audio", and of the
// video layers that are asked to keep theirs, is mixed under this.
#[derive(Clone, Debug, Default)]
pub struct Audio {
    pub media: String,
    pub from: f64,
    pub length: f64,
    pub volume: f64,
    pub fade_in: f64,
    pub fade_out: f64,
}

// A row of the list of tracks. Tracks are a name the model gives its own order, and a layer may
// say which track it stands on.
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub name: String,
    pub note: String,
}

// One thing on the screen, and the stretch of the timeline it owns.
#[derive(Clone, Debug)]
pub struct Layer {
    // The name the model calls the layer by later, when it changes it or takes it away.
    pub id: String,
    pub name: String,
    // The place of the layer among the layers drawn over one another. Bigger is nearer the eye.
    pub z: i32,
    // The track, by the name the model gave it. Only for the reading of the list.
    pub track: String,
    // "text", "table", "chart", "shape", "image", "video" or "audio".
    pub kind: String,
    // The stretch of the film the layer is on screen for, in seconds. `to` of 0.0 means the end.
    pub from: f64,
    pub to: f64,
    // The file an image, a video or an audio layer reads. Absolute, or in the folder of the
    // project, or in the working directory of the chat.
    pub media: String,
    // Where in the file the layer starts reading, and where it stops. 0.0 for the stop means the
    // end of the file.
    pub in_at: f64,
    pub out_at: f64,
    // 1.0 is the speed of the file; 2.0 plays it twice as fast on the screen.
    pub speed: f64,
    // Whether a film or a sound runs round again to fill the stretch of the layer.
    pub loop_media: bool,
    // Whether the last frame of a film is held to the end of the stretch.
    pub hold: bool,
    // The sound of a video layer, and of an audio layer. 0.0 is silence.
    pub volume: f64,
    // The box of the layer, in parts of the canvas: the centre of the box at x, y from the top
    // left, and its width and height.
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    // The turn of the box about its centre, in degrees.
    pub rot: f64,
    // "cover", "contain" or "stretch" - how the media fills the box.
    pub fit: String,
    // Parts of the media cut away before it is placed: left, top, right, bottom.
    pub crop_l: f64,
    pub crop_t: f64,
    pub crop_r: f64,
    pub crop_b: f64,
    pub opacity: f64,
    // "normal", "screen", "multiply", "overlay", "add", "lighten", "darken", "difference".
    pub blend: String,
    pub style: Style,
    pub enter: Option<Motion>,
    pub exit: Option<Motion>,
    pub anim: Vec<Key>,
    pub effects: Vec<Effect>,
}

impl Default for Layer {
    fn default() -> Self {
        Layer {
            id: String::new(),
            name: String::new(),
            z: 0,
            track: String::new(),
            kind: String::from("text"),
            from: 0.0,
            to: 0.0,
            media: String::new(),
            in_at: 0.0,
            out_at: 0.0,
            speed: 1.0,
            loop_media: false,
            hold: true,
            volume: 0.0,
            x: 0.5,
            y: 0.5,
            w: 0.6,
            h: 0.2,
            rot: 0.0,
            fit: String::from("contain"),
            crop_l: 0.0,
            crop_t: 0.0,
            crop_r: 0.0,
            crop_b: 0.0,
            opacity: 1.0,
            blend: String::from("normal"),
            style: Style::default(),
            enter: None,
            exit: None,
            anim: Vec::new(),
            effects: Vec::new(),
        }
    }
}

// The look of a layer. The parts of all the kinds stand here together, and each kind reads the
// parts of its own. A part of another kind is not an error, and is not read.
#[derive(Clone, Debug, Default)]
pub struct Style {
    // ---- text ----
    // The words. `**bold**` and `*italic*` are read, and `{#c2410c}this{}` takes a colour.
    pub text: String,
    // The size of the letters, in parts of the height of the canvas, so a heading of 0.06 is
    // six hundredths of the screen and reads the same at 720p and at 4k.
    pub size: f64,
    // "sans", "serif", "mono", or the name of a family of this machine.
    pub family: String,
    // "normal", "bold" or "black".
    pub weight: String,
    pub italic: bool,
    pub colour: String,
    // "left", "center", "right".
    pub align: String,
    pub line_height: f64,
    pub letter_space: f64,
    // A panel behind the words: "none", "box", "bar".
    pub panel: String,
    pub panel_colour: String,
    pub radius: f64,
    pub pad: f64,
    // A line about the letters, which is how a caption reads over a bright film.
    pub stroke_colour: String,
    pub stroke_width: f64,
    // A shadow under the words.
    pub shadow: f64,
    // The colour of the shadow.
    pub shadow_colour: String,
    // The height of the box a caption may take before it is cut, in lines. 0 is no limit.
    pub max_lines: usize,
    // "none", "dot", "check", "cross", "number", "dash" - the mark before each line.
    pub bullet: String,
    // ---- table ----
    // The row of the heads, and the rows of the table.
    pub heads: Vec<String>,
    pub rows: Vec<Vec<String>>,
    // "grid", "lines", "plain", "zebra", "bars".
    pub form: String,
    // How each column stands: "left", "center", "right". Repeated if it is short.
    pub aligns: Vec<String>,
    // The colours of the head row, of the rows of the ground, and of the lines of the grid.
    pub head_colour: String,
    pub head_ground: String,
    pub row_ground: String,
    pub row_ground2: String,
    pub line_colour: String,
    // The width of each column, in parts of the box. Equal when not given.
    pub widths: Vec<f64>,
    // A bar behind a number, whose length is the number against the biggest of its column.
    pub show_bars: bool,
    pub bar_colour: String,
    // ---- chart ----
    // "bars", "hbars", "line", "area", "pie", "donut", "progress", "stat", "timeline", "steps",
    // "scatter".
    pub chart: String,
    // The parts of the chart: a label, a value, and a colour of its own when the set is not meant.
    pub items: Vec<Datum>,
    // Several lines or bars of the same chart.
    pub series: Vec<Series>,
    pub title: String,
    pub note: String,
    pub unit_pre: String,
    pub unit_post: String,
    pub decimals: i32,
    pub show_values: bool,
    pub show_key: bool,
    pub min: f64,
    pub max: f64,
    // The hole in the middle of a donut, in parts of the radius.
    pub hole: f64,
    // ---- shape ----
    // "rect", "round", "circle", "ellipse", "triangle", "arrow", "line", "star", "plus", "path",
    // "blob".
    pub shape: String,
    // The drawing of a path, in the words of SVG: "M10 10 L90 10", and the like.
    pub path: String,
    pub fill: String,
    pub stroke: String,
    pub stroke_size: f64,
    // A shape drawn as a wire of the given thickness, for rings and arcs.
    pub arc: f64,
    // The measure a path is drawn in, so its numbers may be written small and scaled up.
    pub view: f64,
    // Whether a shape is drawn behind the words of its own layer.
    pub behind: bool,
    // ---- every kind ----
    // How far the content of the layer is shown: 1.0 is all of it. A table reveals row by row, a
    // chart grows to its values, a film of words types itself.
    pub reveal: f64,
}

// One part of a chart.
#[derive(Clone, Debug, Default)]
pub struct Datum {
    pub label: String,
    pub value: f64,
    pub colour: String,
    pub note: String,
}

// A line or a set of bars of a chart.
#[derive(Clone, Debug, Default)]
pub struct Series {
    pub name: String,
    pub colour: String,
    pub values: Vec<f64>,
    pub labels: Vec<String>,
}

// The way a layer arrives, or leaves.
#[derive(Clone, Debug, Default)]
pub struct Motion {
    // "fade", "rise", "drop", "left", "right", "grow", "shrink", "pop", "wipe-left",
    // "wipe-right", "wipe-up", "wipe-down", "type", "rows", "values", "none".
    pub kind: String,
    // How long the motion takes, in seconds, from the start of the layer or before its end.
    pub seconds: f64,
    pub ease: String,
    // How far the layer travels, in parts of the canvas, for the kinds that travel.
    pub distance: f64,
}

// One line of a property that changes with time.
#[derive(Clone, Debug, Default)]
pub struct Key {
    // "x", "y", "w", "h", "scale", "rot", "opacity", "size", "colour", "stroke_size", "reveal",
    // "volume", "crop", "fill".
    pub prop: String,
    // The moments and the values: [[0.0, 0.05], [1.2, 0.5]]. The moments are seconds of the film.
    // A property of more than one number, such as "crop", takes more than one value per moment.
    pub keys: Vec<Value>,
    pub ease: String,
}

// Something done to the pixels of a layer: a key colour taken out, a blur, a mask.
#[derive(Clone, Debug, Default)]
pub struct Effect {
    // "chroma", "despill", "blur", "shadow", "glow", "mask", "edge", "grey", "invert", "tint",
    // "brighten", "contrast", "sharpen".
    pub kind: String,
    // The colour a key is taken against, or the colour of a tint. "auto" finds the colour of the
    // border of the picture, which is what a green screen asks for.
    pub colour: String,
    // How far from the key colour a pixel may be and still be taken out, from 0.0 to 1.0.
    pub similarity: f64,
    // How soft the edge of the key is, from 0.0 to 1.0.
    pub blend: f64,
    // How much of the key colour is taken out of the colours next to the edge.
    pub spill: f64,
    // The size of a blur or a shadow, in parts of the height of the canvas.
    pub radius: f64,
    pub strength: f64,
    // "rect", "round", "ellipse", "circle", "triangle", "star", "path", "fade-up", "fade-down",
    // "fade-left", "fade-right", "vignette".
    pub shape: String,
    pub path: String,
    // The offset of a shadow: x, y, in parts of the canvas.
    pub x: f64,
    pub y: f64,
    // Whether the border of a mask or a key is kept soft, in pixels of the canvas.
    pub feather: f64,
}

// The parts a project may be given, and the parts a layer may be given. They are kept as lists so
// a part nothing knows is answered with the nearest of what is known.
pub const PROJECT_PARTS: &[&str] = &[
    "about", "canvas", "fps", "length", "palette", "colours", "font", "heading_weight",
    "background", "audio", "tracks", "layers",
];

pub const LAYER_PARTS: &[&str] = &[
    "id", "name", "z", "track", "kind", "from", "to", "media", "in", "out", "speed", "loop",
    "hold", "volume", "x", "y", "w", "h", "rot", "fit", "crop", "opacity", "blend", "style",
    "enter", "exit", "anim", "effects",
];

pub const STYLE_PARTS: &[&str] = &[
    "text", "size", "family", "weight", "italic", "colour", "align", "line_height", "letter_space",
    "panel", "panel_colour", "radius", "pad", "stroke_colour", "stroke_width", "shadow",
    "shadow_colour", "max_lines", "bullet", "heads", "rows", "form", "aligns", "head_colour",
    "head_ground", "row_ground", "row_ground2", "line_colour", "widths", "show_bars", "bar_colour",
    "chart", "items", "series", "title", "note", "unit_pre", "unit_post", "decimals",
    "show_values", "show_key", "min", "max", "hole", "shape", "path", "fill", "stroke",
    "stroke_size", "arc", "view", "behind", "reveal",
];

pub const KINDS: &[&str] = &[
    "text", "table", "chart", "shape", "image", "video", "audio",
];

pub fn is_kind(word: &str) -> bool {
    KINDS.contains(&word)
}

pub fn kind_names() -> String {
    KINDS.join(", ")
}

// The motions a layer may arrive or leave by.
pub const MOTIONS: &[&str] = &[
    "none", "fade", "rise", "drop", "left", "right", "grow", "shrink", "pop", "wipe-left",
    "wipe-right", "wipe-up", "wipe-down", "type", "rows", "values",
];

// The properties a keyframe may move.
pub const PROPS: &[&str] = &[
    "x", "y", "w", "h", "scale", "rot", "opacity", "size", "colour", "stroke_size", "reveal",
    "volume", "crop", "fill", "radius", "pad",
];

// The kinds of effect.
pub const EFFECTS: &[&str] = &[
    "chroma", "despill", "blur", "shadow", "glow", "mask", "edge", "grey", "invert", "tint",
    "brighten", "contrast", "sharpen",
];

// The blend modes of a layer over what stands under it.
pub const BLENDS: &[&str] = &[
    "normal", "screen", "multiply", "overlay", "add", "lighten", "darken", "difference",
];

// The easings of a keyframe or a motion.
pub const EASES: &[&str] = &[
    "linear", "smooth", "ease", "ease-in", "ease-out", "ease-in-out", "quad-in", "quad-out",
    "quad-in-out", "cubic-in", "cubic-out", "cubic-in-out", "expo-out", "expo-in", "back-out",
    "back-in", "elastic-out", "bounce-out", "hold",
];

// ---------------------------------------------------------------- reading

// A composition read from the words of a call, with what was wrong with it.
pub struct Read {
    pub project: Project,
    // What the reader refused to take, and what it mended on its own. The faults of the second
    // kind are reported to the model, and the composition stands.
    pub faults: Vec<String>,
    pub notes: Vec<String>,
    // True when the words held a header at all. A call that changes one part of a film that
    // stands is not answered as though the rest of it had been emptied.
    pub given: bool,
}

// The header of a project, out of the words of a call. Only the parts the call gave are taken;
// what the call did not name keeps what stood.
pub fn merge_project(had: &Project, given: &Value) -> Read {
    let mut made = had.clone();
    let mut faults: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut count = 0usize;

    for key in ["about", "canvas", "palette", "font", "heading_weight"] {
        if let Some(said) = text_of(given, key) {
            count += 1;
            match key {
                "canvas" => {
                    if canvas_of(&said).is_none() {
                        faults.push(format!(
                            "\"{said}\" is not a size of the canvas. The names are {}; or give \
                             the measure as 1280x720.",
                            canvas_words()
                        ));
                        continue;
                    }
                    made.canvas = said;
                }
                "palette" => {
                    if said != "custom" && palette_of(&said).is_none() {
                        faults.push(format!(
                            "\"{said}\" is not a set of colours. The sets are {}; or give the \
                             colours themselves under \"colours\".",
                            palette_names()
                        ));
                        continue;
                    }
                    made.palette = said;
                }
                other => match other {
                    "about" => made.about = said,
                    "font" => made.font = said,
                    _ => made.heading_weight = said,
                },
            }
        }
    }

    if let Some(fps) = number_of(given, "fps") {
        count += 1;
        let whole = fps.round() as i64;
        if !(LEAST_FPS as i64..=GREATEST_FPS as i64).contains(&whole) {
            faults.push(format!(
                "{fps} is not a frame rate in the range of {LEAST_FPS} to {GREATEST_FPS}."
            ));
        } else {
            made.fps = whole as u32;
        }
    }
    if let Some(length) = number_of(given, "length") {
        count += 1;
        if length < 0.0 || length > GREATEST_LENGTH {
            faults.push(format!(
                "a length of {length} seconds is out of the range of 0 (the length of the last \
                 layer) to {GREATEST_LENGTH}."
            ));
        } else {
            made.length = length;
        }
    }

    if let Some(ground) = given.get("background") {
        count += 1;
        made.background = read_background(ground, &made.background, &mut faults);
    }
    if let Some(sound) = given.get("audio") {
        count += 1;
        made.audio = Some(read_audio(sound, &mut faults));
    }
    if let Some(list) = given.get("colours") {
        count += 1;
        made.colours = read_colours(list);
    }
    if let Some(list) = given.get("tracks") {
        count += 1;
        if let Some(rows) = list.as_array() {
            made.tracks = Vec::new();
            for one in rows {
                if let Some(name) = text_of(one, "name") {
                    made.tracks.push(Track { name, note: text_of(one, "note").unwrap_or_default() });
                } else if let Value::String(name) = one {
                    made.tracks.push(Track { name: name.clone(), note: String::new() });
                }
            }
        }
    }
    // The layers of a call stand in the place of the layers of the film. One layer is put in,
    // changed or taken away with the tool of the layers, not here.
    if let Some(list) = given.get("layers") {
        count += 1;
        if let Some(rows) = list.as_array() {
            let (made_layers, layer_faults) = read_layers(rows);
            faults.extend(layer_faults);
            made.layers = made_layers;
        } else {
            faults.push("\"layers\" has to be a list of layers.".to_string());
        }
    }

    note_unknown(given, PROJECT_PARTS, "of a film", &mut faults, &mut notes);

    Read { project: made, faults, notes, given: count > 0 }
}

fn read_colours(given: &Value) -> Colours {
    let mut made = Colours::default();
    if let Some(one) = text_of(given, "ink") {
        made.ink = one;
    }
    if let Some(one) = text_of(given, "ground") {
        made.ground = one;
    }
    if let Some(one) = text_of(given, "soft") {
        made.soft = one;
    }
    if let Some(one) = text_of(given, "accent") {
        made.accent = one;
    }
    if let Some(list) = given.get("series").and_then(Value::as_array) {
        made.series = list.iter().filter_map(|one| one.as_str().map(String::from)).collect();
    }
    made
}

fn read_audio(given: &Value, faults: &mut Vec<String>) -> Audio {
    let mut made = Audio::default();
    if let Some(media) = text_of(given, "media") {
        made.media = media;
    }
    if let Some(one) = number_of(given, "from") {
        made.from = one.max(0.0);
    }
    if let Some(one) = number_of(given, "length") {
        made.length = one.max(0.0);
    }
    if let Some(one) = number_of(given, "volume") {
        made.volume = one.clamp(0.0, 4.0);
    }
    if let Some(one) = number_of(given, "fade_in") {
        made.fade_in = one.max(0.0);
    }
    if let Some(one) = number_of(given, "fade_out") {
        made.fade_out = one.max(0.0);
    }
    if made.media.is_empty() {
        faults.push("the sound of the film needs a file under \"media\".".to_string());
    }
    made
}

fn read_background(given: &Value, had: &Background, faults: &mut Vec<String>) -> Background {
    let mut made = had.clone();
    if let Some(kind) = text_of(given, "kind") {
        if !matches!(kind.as_str(), "solid" | "gradient" | "image" | "none") {
            faults.push(format!(
                "\"{kind}\" is not a kind of ground. Take solid, gradient, image or none."
            ));
        } else {
            made.kind = kind;
        }
    }
    for (key, field) in
        [("colour", 0), ("from", 1), ("to", 2), ("media", 3), ("fit", 4)]
    {
        if let Some(said) = text_of(given, key) {
            match field {
                0 => made.colour = said,
                1 => made.from = said,
                2 => made.to = said,
                3 => made.media = said,
                _ => {
                    if !matches!(said.as_str(), "cover" | "contain" | "stretch") {
                        faults.push(format!(
                            "\"{said}\" is not a way to fill the ground. Take cover, contain or \
                             stretch."
                        ));
                    } else {
                        made.fit = said;
                    }
                }
            }
        }
    }
    if let Some(angle) = number_of(given, "angle") {
        made.angle = angle;
    }
    if let Some(one) = number_of(given, "vignette") {
        made.vignette = one.clamp(0.0, 1.0);
    }
    made
}

// The list of the layers, out of the words of a call.
pub fn read_layers(rows: &[Value]) -> (Vec<Layer>, Vec<String>) {
    let mut made: Vec<Layer> = Vec::new();
    let mut faults: Vec<String> = Vec::new();
    for (place, one) in rows.iter().enumerate() {
        if let Some(layer) = read_layer(one, place + 1, &mut faults) {
            made.push(layer);
        }
    }
    (made, faults)
}

// One layer, out of the words of a call. A layer whose kind is not known is refused, for the
// parts of a text layer do not mean anything in a chart.
pub fn read_layer(given: &Value, number: usize, faults: &mut Vec<String>) -> Option<Layer> {
    let mut made = Layer::default();
    let kind = text_of(given, "kind").unwrap_or_default();
    if !kind.is_empty() {
        if !is_kind(&kind) {
            match nearest(&kind, KINDS) {
                Some(found) => {
                    faults.push(format!(
                        "layer {number}: \"{kind}\" is not a kind of layer; \"{found}\" was \
                         taken. The kinds are {}.",
                        kind_names()
                    ));
                    made.kind = found;
                }
                None => {
                    faults.push(format!(
                        "layer {number}: \"{kind}\" is not a kind of layer. The kinds are {}.",
                        kind_names()
                    ));
                    return None;
                }
            }
        } else {
            made.kind = kind;
        }
    }
    if let Some(id) = text_of(given, "id") {
        made.id = id;
    }
    if let Some(name) = text_of(given, "name") {
        made.name = name;
    }
    if let Some(track) = given.get("track") {
        made.track = match track {
            Value::String(one) => one.clone(),
            Value::Number(one) => one.to_string(),
            _ => String::new(),
        };
    }
    if let Some(z) = number_of(given, "z") {
        made.z = z.round() as i32;
    }
    if let Some(one) = number_of(given, "from") {
        made.from = one.max(0.0);
    }
    if let Some(one) = number_of(given, "to") {
        made.to = one.max(0.0);
    }
    if let Some(one) = text_of(given, "media") {
        made.media = one;
    }
    if let Some(one) = number_of(given, "in") {
        made.in_at = one.max(0.0);
    }
    if let Some(one) = number_of(given, "out") {
        made.out_at = one.max(0.0);
    }
    if let Some(one) = number_of(given, "speed") {
        if one.abs() < 0.01 {
            faults.push(format!("layer {number}: a speed of {one} stops the media; 1.0 was taken."));
        } else {
            made.speed = one.clamp(-8.0, 8.0);
        }
    }
    if let Some(one) = bool_of(given, "loop") {
        made.loop_media = one;
    }
    if let Some(one) = bool_of(given, "hold") {
        made.hold = one;
    }
    if let Some(one) = number_of(given, "volume") {
        made.volume = one.clamp(0.0, 4.0);
    }
    for (key, field) in [("x", 0), ("y", 1), ("w", 2), ("h", 3), ("rot", 4)] {
        if let Some(one) = number_of(given, key) {
            match field {
                0 => made.x = one,
                1 => made.y = one,
                2 => made.w = one.max(0.001),
                3 => made.h = one.max(0.001),
                _ => made.rot = one,
            }
        }
    }
    if let Some(one) = text_of(given, "fit") {
        if !matches!(one.as_str(), "cover" | "contain" | "stretch") {
            faults.push(format!(
                "layer {number}: \"{one}\" is not a way to fill a box. Take cover, contain or \
                 stretch."
            ));
        } else {
            made.fit = one;
        }
    }
    if let Some(list) = given.get("crop").and_then(Value::as_array) {
        let parts: Vec<f64> = list.iter().filter_map(Value::as_f64).map(|one| one.clamp(0.0, 0.95)).collect();
        if parts.len() == 4 {
            made.crop_l = parts[0];
            made.crop_t = parts[1];
            made.crop_r = parts[2];
            made.crop_b = parts[3];
        } else {
            faults.push(format!(
                "layer {number}: \"crop\" has to be four parts - left, top, right, bottom."
            ));
        }
    }
    if let Some(one) = number_of(given, "opacity") {
        made.opacity = one.clamp(0.0, 1.0);
    }
    if let Some(one) = text_of(given, "blend") {
        if !BLENDS.contains(&one.as_str()) {
            match nearest(&one, BLENDS) {
                Some(found) => {
                    faults.push(format!(
                        "layer {number}: \"{one}\" is not a way to blend; \"{found}\" was taken."
                    ));
                    made.blend = found;
                }
                None => faults.push(format!(
                    "layer {number}: \"{one}\" is not a way to blend. The ways are {}.",
                    BLENDS.join(", ")
                )),
            }
        } else {
            made.blend = one;
        }
    }
    if let Some(style) = given.get("style") {
        made.style = read_style(style, number, faults);
    }
    if let Some(motion) = given.get("enter") {
        made.enter = Some(read_motion(motion, number, faults));
    }
    if let Some(motion) = given.get("exit") {
        made.exit = Some(read_motion(motion, number, faults));
    }
    if let Some(list) = given.get("anim") {
        if let Some(rows) = list.as_array() {
            if rows.len() > GREATEST_KEYS {
                faults.push(format!(
                    "layer {number}: {GREATEST_KEYS} lines of animation is the most; the rest \
                     were left out."
                ));
            }
            for one in rows.iter().take(GREATEST_KEYS) {
                if let Some(key) = read_key(one, number, faults) {
                    made.anim.push(key);
                }
            }
        } else {
            faults.push(format!("layer {number}: \"anim\" has to be a list of lines."));
        }
    }
    if let Some(list) = given.get("effects") {
        if let Some(rows) = list.as_array() {
            if rows.len() > GREATEST_EFFECTS {
                faults.push(format!(
                    "layer {number}: {GREATEST_EFFECTS} effects is the most; the rest were left \
                     out."
                ));
            }
            for one in rows.iter().take(GREATEST_EFFECTS) {
                if let Some(effect) = read_effect(one, number, faults) {
                    made.effects.push(effect);
                }
            }
        } else {
            faults.push(format!("layer {number}: \"effects\" has to be a list of effects."));
        }
    }
    Some(made)
}

fn read_style(given: &Value, number: usize, faults: &mut Vec<String>) -> Style {
    let mut made = Style::default();
    for key in given.as_object().into_iter().flat_map(|one| one.keys()) {
        if !STYLE_PARTS.contains(&key.as_str()) {
            match nearest(key, STYLE_PARTS) {
                Some(found) => faults.push(format!(
                    "layer {number}: there is no \"{key}\" in a style; \"{found}\" was meant?"
                )),
                None => faults.push(format!(
                    "layer {number}: there is no \"{key}\" in a style. The parts are listed in \
                     the prompt of the suite."
                )),
            }
        }
    }
    for key in [
        "text", "family", "weight", "colour", "align", "panel", "panel_colour", "form", "chart",
        "title", "note", "unit_pre", "unit_post", "shape", "path", "fill", "stroke", "bullet",
        "head_colour", "head_ground", "row_ground", "row_ground2", "line_colour", "bar_colour",
    ] {
        if let Some(said) = text_of(given, key) {
            match key {
                "text" => made.text = said,
                "family" => made.family = said,
                "weight" => made.weight = said,
                "colour" => made.colour = said,
                "align" => made.align = said,
                "panel" => made.panel = said,
                "panel_colour" => made.panel_colour = said,
                "form" => made.form = said,
                "chart" => made.chart = said,
                "title" => made.title = said,
                "note" => made.note = said,
                "unit_pre" => made.unit_pre = said,
                "unit_post" => made.unit_post = said,
                "shape" => made.shape = said,
                "path" => made.path = said,
                "fill" => made.fill = said,
                "stroke" => made.stroke = said,
                "bullet" => made.bullet = said,
                "head_colour" => made.head_colour = said,
                "head_ground" => made.head_ground = said,
                "row_ground" => made.row_ground = said,
                "row_ground2" => made.row_ground2 = said,
                "line_colour" => made.line_colour = said,
                _ => made.bar_colour = said,
            }
        }
    }
    for key in [
        "size", "line_height", "letter_space", "radius", "pad", "stroke_width", "shadow",
        "stroke_size", "arc", "view", "reveal", "min", "max", "hole",
    ] {
        if let Some(one) = number_of(given, key) {
            match key {
                "size" => made.size = one,
                "line_height" => made.line_height = one,
                "letter_space" => made.letter_space = one,
                "radius" => made.radius = one,
                "pad" => made.pad = one,
                "stroke_width" => made.stroke_width = one,
                "shadow" => made.shadow = one,
                "stroke_size" => made.stroke_size = one,
                "arc" => made.arc = one,
                "view" => made.view = one,
                "reveal" => made.reveal = one,
                "min" => made.min = one,
                "max" => made.max = one,
                _ => made.hole = one,
            }
        }
    }
    if let Some(one) = bool_of(given, "italic") {
        made.italic = one;
    }
    if let Some(one) = bool_of(given, "show_values") {
        made.show_values = one;
    }
    if let Some(one) = bool_of(given, "show_key") {
        made.show_key = one;
    }
    if let Some(one) = bool_of(given, "behind") {
        made.behind = one;
    }
    if let Some(one) = bool_of(given, "show_bars") {
        made.show_bars = one;
    }
    if let Some(one) = number_of(given, "decimals") {
        made.decimals = one.round() as i32;
    }
    if let Some(one) = number_of(given, "max_lines") {
        made.max_lines = one.max(0.0) as usize;
    }
    if let Some(list) = given.get("heads").and_then(Value::as_array) {
        made.heads = list.iter().filter_map(text_of_value).collect();
    }
    if let Some(list) = given.get("aligns").and_then(Value::as_array) {
        made.aligns = list.iter().filter_map(text_of_value).collect();
    }
    if let Some(list) = given.get("widths").and_then(Value::as_array) {
        made.widths = list.iter().filter_map(Value::as_f64).collect();
    }
    if let Some(list) = given.get("rows").and_then(Value::as_array) {
        made.rows = list
            .iter()
            .map(|one| match one {
                Value::Array(cells) => cells.iter().filter_map(text_of_value).collect(),
                Value::String(one) => one.split('\t').map(String::from).collect(),
                other => vec![written(other)],
            })
            .collect();
    }
    if let Some(list) = given.get("items").and_then(Value::as_array) {
        made.items = list
            .iter()
            .map(|one| {
                let mut datum = Datum::default();
                if let Some(label) = text_of(one, "label") {
                    datum.label = label;
                } else if let Value::String(one) = one {
                    datum.label = one.clone();
                }
                if let Some(value) = number_of(one, "value") {
                    datum.value = value;
                } else if let Value::Number(one) = one {
                    datum.value = one.as_f64().unwrap_or(0.0);
                }
                if let Some(colour) = text_of(one, "colour") {
                    datum.colour = colour;
                }
                if let Some(note) = text_of(one, "note") {
                    datum.note = note;
                }
                datum
            })
            .collect();
    }
    if let Some(list) = given.get("series").and_then(Value::as_array) {
        made.series = list
            .iter()
            .map(|one| {
                let mut line = Series::default();
                if let Some(name) = text_of(one, "name") {
                    line.name = name;
                }
                if let Some(colour) = text_of(one, "colour") {
                    line.colour = colour;
                }
                if let Some(values) = one.get("values").and_then(Value::as_array) {
                    line.values = values.iter().filter_map(Value::as_f64).collect();
                }
                if let Some(labels) = one.get("labels").and_then(Value::as_array) {
                    line.labels = labels.iter().filter_map(text_of_value).collect();
                }
                line
            })
            .collect();
    }
    made
}

fn read_motion(given: &Value, number: usize, faults: &mut Vec<String>) -> Motion {
    let mut made = Motion {
        kind: String::new(),
        seconds: 0.5,
        distance: 0.06,
        ease: String::from("ease-out"),
    };
    if let Some(kind) = text_of(given, "kind") {
        if !MOTIONS.contains(&kind.as_str()) {
            match nearest(&kind, MOTIONS) {
                Some(found) => {
                    faults.push(format!(
                        "layer {number}: there is no motion of \"{kind}\"; \"{found}\" was taken."
                    ));
                    made.kind = found;
                }
                None => faults.push(format!(
                    "layer {number}: there is no motion of \"{kind}\". The motions are {}.",
                    MOTIONS.join(", ")
                )),
            }
        } else {
            made.kind = kind;
        }
    }
    if let Some(one) = number_of(given, "seconds") {
        made.seconds = one.clamp(0.01, 30.0);
    }
    if let Some(one) = number_of(given, "distance") {
        made.distance = one;
    }
    if let Some(one) = text_of(given, "ease") {
        if !EASES.contains(&one.as_str()) {
            match nearest(&one, EASES) {
                Some(found) => {
                    faults.push(format!(
                        "layer {number}: there is no easing of \"{one}\"; \"{found}\" was taken."
                    ));
                    made.ease = found;
                }
                None => faults.push(format!(
                    "layer {number}: there is no easing of \"{one}\". The easings are {}.",
                    EASES.join(", ")
                )),
            }
        } else {
            made.ease = one;
        }
    }
    for key in given.as_object().into_iter().flat_map(|one| one.keys()) {
        if !matches!(key.as_str(), "kind" | "seconds" | "distance" | "ease") {
            faults.push(format!(
                "layer {number}: there is no \"{key}\" in a motion. The parts are kind, seconds, \
                 ease and distance."
            ));
        }
    }
    made
}

fn read_key(given: &Value, number: usize, faults: &mut Vec<String>) -> Option<Key> {
    let prop = text_of(given, "prop").unwrap_or_default();
    if !PROPS.contains(&prop.as_str()) {
        match nearest(&prop, PROPS) {
            Some(found) => {
                faults.push(format!(
                    "layer {number}: nothing moves by \"{prop}\"; \"{found}\" was taken."
                ));
                return read_key_of(&found, given, faults);
            }
            None => {
                faults.push(format!(
                    "layer {number}: nothing moves by \"{prop}\". What moves is {}.",
                    PROPS.join(", ")
                ));
                return None;
            }
        }
    }
    let mut made = read_key_of(&prop, given, faults)?;
    if let Some(one) = text_of(given, "ease") {
        if !EASES.contains(&one.as_str()) {
            match nearest(&one, EASES) {
                Some(found) => {
                    faults.push(format!(
                        "layer {number}: there is no easing of \"{one}\"; \"{found}\" was taken."
                    ));
                    made.ease = found;
                }
                None => faults.push(format!(
                    "layer {number}: there is no easing of \"{one}\". The easings are {}.",
                    EASES.join(", ")
                )),
            }
        } else {
            made.ease = one;
        }
    }
    Some(made)
}

fn read_key_of(prop: &str, given: &Value, faults: &mut Vec<String>) -> Option<Key> {
    let rows = given.get("keys").and_then(Value::as_array)?;
    if rows.len() < 2 {
        faults.push(format!(
            "a line of animation for \"{prop}\" needs at least two moments to move between."
        ));
        return None;
    }
    let mut made = Key {
        prop: String::from(prop),
        keys: rows.clone(),
        ease: String::from("ease-out"),
    };
    if let Some(one) = text_of(given, "ease") {
        if EASES.contains(&one.as_str()) {
            made.ease = one;
        }
    }
    Some(made)
}

fn read_effect(given: &Value, number: usize, faults: &mut Vec<String>) -> Option<Effect> {
    let kind = text_of(given, "kind").unwrap_or_default();
    if !EFFECTS.contains(&kind.as_str()) {
        match nearest(&kind, EFFECTS) {
            Some(found) => {
                faults.push(format!(
                    "layer {number}: there is no effect of \"{kind}\"; \"{found}\" was taken."
                ));
                return effect_of(&found, given, faults);
            }
            None => {
                faults.push(format!(
                    "layer {number}: there is no effect of \"{kind}\". The effects are {}.",
                    EFFECTS.join(", ")
                ));
                return None;
            }
        }
    }
    effect_of(&kind, given, faults)
}

fn effect_of(kind: &str, given: &Value, _faults: &mut Vec<String>) -> Option<Effect> {
    let mut made = Effect::default();
    made.kind = String::from(kind);
    if let Some(one) = text_of(given, "colour") {
        made.colour = one;
    }
    if let Some(one) = text_of(given, "shape") {
        made.shape = one;
    }
    if let Some(one) = text_of(given, "path") {
        made.path = one;
    }
    if let Some(one) = number_of(given, "similarity") {
        made.similarity = one.clamp(0.0, 1.0);
    }
    if let Some(one) = number_of(given, "blend") {
        made.blend = one.clamp(0.0, 1.0);
    }
    if let Some(one) = number_of(given, "spill") {
        made.spill = one.clamp(0.0, 1.0);
    }
    if let Some(one) = number_of(given, "radius") {
        made.radius = one;
    }
    if let Some(one) = number_of(given, "strength") {
        made.strength = one.clamp(-1.0, 4.0);
    }
    if let Some(one) = number_of(given, "x") {
        made.x = one;
    }
    if let Some(one) = number_of(given, "y") {
        made.y = one;
    }
    if let Some(one) = number_of(given, "feather") {
        made.feather = one.max(0.0);
    }
    Some(made)
}

// ---------------------------------------------------------------- writing

// The words of a composition, as it stands on the disk.
pub fn words(project: &Project) -> String {
    let mut out = json!({
        "name": project.name,
        "about": project.about,
        "canvas": project.canvas,
        "fps": project.fps,
        "length": trim(project.length),
        "palette": project.palette,
        "font": project.font,
        "heading_weight": project.heading_weight,
        "background": {
            "kind": project.background.kind,
            "colour": project.background.colour,
            "from": project.background.from,
            "to": project.background.to,
            "angle": trim(project.background.angle),
            "vignette": trim(project.background.vignette),
            "media": project.background.media,
            "fit": project.background.fit,
        },
        "audio": project.audio.as_ref().map(|one| json!({
            "media": one.media,
            "from": trim(one.from),
            "length": trim(one.length),
            "volume": trim(one.volume),
            "fade_in": trim(one.fade_in),
            "fade_out": trim(one.fade_out),
        })),
        "tracks": project.tracks.iter().map(|one| json!({
            "name": one.name,
            "note": one.note,
        })).collect::<Vec<Value>>(),
        "layers": project.layers.iter().map(layer_words).collect::<Vec<Value>>(),
    });
    if let Some(map) = out.as_object_mut() {
        if !project.colours.ink.is_empty()
            || !project.colours.ground.is_empty()
            || !project.colours.soft.is_empty()
            || !project.colours.accent.is_empty()
            || !project.colours.series.is_empty()
        {
            map.insert(
                "colours".to_string(),
                json!({
                    "ink": project.colours.ink,
                    "ground": project.colours.ground,
                    "soft": project.colours.soft,
                    "accent": project.colours.accent,
                    "series": project.colours.series,
                }),
            );
        }
    }
    let mut text = serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string());
    text.push('\n');
    text
}

// The words of one layer, as the object they are written as. The tool of the layers reads a
// change on top of these words, which is how one part of a layer is mended and the rest of it
// stands.
pub fn layer_words(one: &Layer) -> Value {
    let mut made = json!({
        "id": one.id,
        "name": one.name,
        "z": one.z,
        "track": one.track,
        "kind": one.kind,
        "from": trim(one.from),
        "to": trim(one.to),
        "media": one.media,
        "in": trim(one.in_at),
        "out": trim(one.out_at),
        "speed": trim(one.speed),
        "loop": one.loop_media,
        "hold": one.hold,
        "volume": trim(one.volume),
        "x": trim(one.x),
        "y": trim(one.y),
        "w": trim(one.w),
        "h": trim(one.h),
        "rot": trim(one.rot),
        "fit": one.fit,
        "crop": [trim(one.crop_l), trim(one.crop_t), trim(one.crop_r), trim(one.crop_b)],
        "opacity": trim(one.opacity),
        "blend": one.blend,
        "style": style_words(one),
    });
    let map = made.as_object_mut().expect("a layer is an object");
    if let Some(motion) = &one.enter {
        map.insert("enter".to_string(), motion_words(motion));
    }
    if let Some(motion) = &one.exit {
        map.insert("exit".to_string(), motion_words(motion));
    }
    if !one.anim.is_empty() {
        map.insert(
            "anim".to_string(),
            Value::Array(
                one.anim
                    .iter()
                    .map(|key| json!({ "prop": key.prop, "keys": key.keys, "ease": key.ease }))
                    .collect(),
            ),
        );
    }
    if !one.effects.is_empty() {
        map.insert(
            "effects".to_string(),
            Value::Array(one.effects.iter().map(effect_words).collect()),
        );
    }
    made
}

fn motion_words(motion: &Motion) -> Value {
    json!({
        "kind": motion.kind,
        "seconds": trim(motion.seconds),
        "ease": motion.ease,
        "distance": trim(motion.distance),
    })
}

fn effect_words(one: &Effect) -> Value {
    let mut made = json!({ "kind": one.kind });
    let map = made.as_object_mut().expect("an effect is an object");
    if !one.colour.is_empty() {
        map.insert("colour".to_string(), json!(one.colour));
    }
    if !one.shape.is_empty() {
        map.insert("shape".to_string(), json!(one.shape));
    }
    if !one.path.is_empty() {
        map.insert("path".to_string(), json!(one.path));
    }
    for (key, value) in [
        ("similarity", one.similarity),
        ("blend", one.blend),
        ("spill", one.spill),
        ("radius", one.radius),
        ("strength", one.strength),
        ("x", one.x),
        ("y", one.y),
        ("feather", one.feather),
    ] {
        if value != 0.0 {
            map.insert(key.to_string(), json!(trim(value)));
        }
    }
    made
}

// The style of a layer, with only the parts that carry something. A composition of a dozen
// layers should read as a script and not as a wall of zeroes.
fn style_words(one: &Layer) -> Value {
    let style = &one.style;
    let mut made = serde_json::Map::new();
    let texts = [
        ("text", &style.text),
        ("family", &style.family),
        ("weight", &style.weight),
        ("colour", &style.colour),
        ("align", &style.align),
        ("panel", &style.panel),
        ("panel_colour", &style.panel_colour),
        ("stroke_colour", &style.stroke_colour),
        ("shadow_colour", &style.shadow_colour),
        ("bullet", &style.bullet),
        ("form", &style.form),
        ("head_colour", &style.head_colour),
        ("head_ground", &style.head_ground),
        ("row_ground", &style.row_ground),
        ("row_ground2", &style.row_ground2),
        ("line_colour", &style.line_colour),
        ("bar_colour", &style.bar_colour),
        ("chart", &style.chart),
        ("title", &style.title),
        ("note", &style.note),
        ("unit_pre", &style.unit_pre),
        ("unit_post", &style.unit_post),
        ("shape", &style.shape),
        ("path", &style.path),
        ("fill", &style.fill),
        ("stroke", &style.stroke),
    ];
    for (key, value) in texts {
        if !value.is_empty() {
            made.insert(key.to_string(), json!(value));
        }
    }
    let numbers = [
        ("size", style.size),
        ("line_height", style.line_height),
        ("letter_space", style.letter_space),
        ("radius", style.radius),
        ("pad", style.pad),
        ("stroke_width", style.stroke_width),
        ("shadow", style.shadow),
        ("stroke_size", style.stroke_size),
        ("arc", style.arc),
        ("view", style.view),
        ("reveal", style.reveal),
        ("min", style.min),
        ("max", style.max),
        ("hole", style.hole),
    ];
    for (key, value) in numbers {
        if value != 0.0 {
            made.insert(key.to_string(), json!(trim(value)));
        }
    }
    if style.italic {
        made.insert("italic".to_string(), json!(true));
    }
    if style.show_values {
        made.insert("show_values".to_string(), json!(true));
    }
    if style.show_key {
        made.insert("show_key".to_string(), json!(true));
    }
    if style.behind {
        made.insert("behind".to_string(), json!(true));
    }
    if style.show_bars {
        made.insert("show_bars".to_string(), json!(true));
    }
    if style.decimals != 0 {
        made.insert("decimals".to_string(), json!(style.decimals));
    }
    if style.max_lines != 0 {
        made.insert("max_lines".to_string(), json!(style.max_lines));
    }
    if !style.heads.is_empty() {
        made.insert("heads".to_string(), json!(style.heads));
    }
    if !style.aligns.is_empty() {
        made.insert("aligns".to_string(), json!(style.aligns));
    }
    if !style.widths.is_empty() {
        made.insert("widths".to_string(), json!(style.widths.iter().map(|one| trim(*one)).collect::<Vec<f64>>()));
    }
    if !style.rows.is_empty() {
        made.insert("rows".to_string(), json!(style.rows));
    }
    if !style.items.is_empty() {
        made.insert(
            "items".to_string(),
            Value::Array(
                style
                    .items
                    .iter()
                    .map(|one| {
                        let mut datum = json!({ "label": one.label, "value": trim(one.value) });
                        if !one.colour.is_empty() {
                            datum["colour"] = json!(one.colour);
                        }
                        if !one.note.is_empty() {
                            datum["note"] = json!(one.note);
                        }
                        datum
                    })
                    .collect(),
            ),
        );
    }
    if !style.series.is_empty() {
        made.insert(
            "series".to_string(),
            Value::Array(
                style
                    .series
                    .iter()
                    .map(|one| {
                        json!({
                            "name": one.name,
                            "colour": one.colour,
                            "values": one.values.iter().map(|v| trim(*v)).collect::<Vec<f64>>(),
                            "labels": one.labels,
                        })
                    })
                    .collect(),
            ),
        );
    }
    let _ = one;
    Value::Object(made)
}

// A composition out of the words of its file. Nothing is answered when the words are not read:
// the caller says that the project has to be made again.
pub fn from_words(text: &str) -> Option<Project> {
    let given: Value = serde_json::from_str(text).ok()?;
    if !given.is_object() {
        return None;
    }
    let mut faults: Vec<String> = Vec::new();
    let made = Project {
        name: text_of(&given, "name").unwrap_or_default(),
        about: text_of(&given, "about").unwrap_or_default(),
        canvas: text_of(&given, "canvas")
            .filter(|one| canvas_of(one).is_some())
            .unwrap_or_else(|| String::from(DEFAULT_CANVAS)),
        fps: number_of(&given, "fps")
            .map(|one| one as u32)
            .unwrap_or(DEFAULT_FPS),
        length: number_of(&given, "length").unwrap_or(0.0),
        palette: text_of(&given, "palette").unwrap_or_else(|| String::from(DEFAULT_PALETTE)),
        colours: read_colours(&given.get("colours").cloned().unwrap_or(Value::Null)),
        font: text_of(&given, "font").unwrap_or_default(),
        heading_weight: text_of(&given, "heading_weight").unwrap_or_default(),
        background: read_background(
            &given.get("background").cloned().unwrap_or(Value::Null),
            &Background::default(),
            &mut faults,
        ),
        audio: given
            .get("audio")
            .filter(|one| one.is_object())
            .map(|one| read_audio(one, &mut faults)),
        tracks: given
            .get("tracks")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|one| match one {
                        Value::String(name) => Some(Track { name: name.clone(), note: String::new() }),
                        _ => text_of(one, "name").map(|name| Track {
                            name,
                            note: text_of(one, "note").unwrap_or_default(),
                        }),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        layers: given
            .get("layers")
            .and_then(Value::as_array)
            .map(|rows| read_layers(rows).0)
            .unwrap_or_default(),
    };
    Some(made)
}

// ---------------------------------------------------------------- the state of a film

// How long the film runs: the length the project gave, or the last moment a layer is on the
// screen for.
pub fn seconds(project: &Project) -> f64 {
    if project.length > 0.0 {
        return project.length;
    }
    let mut last: f64 = 0.0;
    for one in &project.layers {
        if one.kind == "audio" {
            continue;
        }
        last = last.max(end_of(one));
    }
    if last <= 0.0 {
        // A film with nothing visible still has to run for something, and a still of one frame
        // is the least that can be asked for.
        return 0.0;
    }
    last
}

// The moment a layer leaves the screen. 0.0 for `to` means the end of the media, or the end of
// the film.
pub fn end_of(one: &Layer) -> f64 {
    if one.to > 0.0 {
        return one.to;
    }
    if (one.kind == "video" || one.kind == "image" || one.kind == "audio")
        && one.out_at > one.in_at
        && one.speed.abs() > 0.001
    {
        let span = (one.out_at - one.in_at) / one.speed.abs();
        return one.from + span;
    }
    0.0
}

// The length a layer holds the screen for. 0.0 for `to` means the end of the film, which is
// asked for here as a guess of what the film will be.
pub fn held(one: &Layer) -> f64 {
    let end = end_of(one);
    if end > 0.0 {
        return (end - one.from).max(0.0);
    }
    0.0
}

// Whether a layer is on the screen at a moment of the film.
pub fn shows(one: &Layer, at: f64, whole: f64) -> bool {
    if at + 1e-9 < one.from {
        return false;
    }
    let end = if one.to > 0.0 { one.to } else { whole };
    at < end + 1e-9
}

// The layers in the order they are drawn: the lowest `z` first, and within one `z` the order the
// model put them in.
pub fn ordered(project: &Project) -> Vec<usize> {
    let mut places: Vec<usize> = (0..project.layers.len()).collect();
    places.sort_by_key(|one| (project.layers[*one].z, *one));
    places
}

// The next free `z`, so a layer added without a `z` stands over the ones already there.
pub fn next_z(project: &Project) -> i32 {
    project.layers.iter().map(|one| one.z).max().map(|one| one + 1).unwrap_or(0)
}

// A short account of a layer, for the list a model asks for.
pub fn one_line(one: &Layer, whole: f64) -> String {
    let end = if one.to > 0.0 { trim(one.to) } else { trim(whole) };
    let mut said = format!(
        "{} [{}] {} to {} s - {}",
        name_of(one),
        one.kind,
        trim(one.from),
        end,
        about_of(one),
    );
    if one.opacity < 1.0 {
        said.push_str(&format!(", {:.0}% seen", one.opacity * 100.0));
    }
    if one.z != 0 {
        said.push_str(&format!(", z {}", one.z));
    }
    if !one.track.is_empty() {
        said.push_str(&format!(", track {}", one.track));
    }
    said
}

// The name a layer is called by: what the model gave, or the number it stands at.
pub fn name_of(one: &Layer) -> String {
    if !one.name.is_empty() {
        return one.name.clone();
    }
    if !one.id.is_empty() {
        return one.id.clone();
    }
    String::from("(unnamed)")
}

// What a layer holds, in a few words.
fn about_of(one: &Layer) -> String {
    match one.kind.as_str() {
        "text" => {
            let words: Vec<&str> = one.style.text.split_whitespace().take(8).collect();
            let said = words.join(" ");
            if said.is_empty() {
                String::from("no words")
            } else {
                trim_to(&said, 48)
            }
        }
        "table" => format!(
            "{} rows of {} columns",
            one.style.rows.len(),
            if one.style.heads.is_empty() {
                one.style.rows.first().map(|one| one.len()).unwrap_or(0)
            } else {
                one.style.heads.len()
            }
        ),
        "chart" => format!(
            "{} of {} parts",
            if one.style.chart.is_empty() { "bars" } else { &one.style.chart },
            if !one.style.items.is_empty() {
                one.style.items.len()
            } else {
                one.style.series.len()
            }
        ),
        "shape" => format!("a shape of {}", if one.style.shape.is_empty() { "rect" } else { &one.style.shape }),
        "image" => format!("an image, {}", file_of(one)),
        "video" => format!("a film, {}", file_of(one)),
        "audio" => format!("a sound, {}", file_of(one)),
        other => String::from(other),
    }
}

fn file_of(one: &Layer) -> String {
    match Path::new(&one.media).file_name() {
        Some(name) => name.to_string_lossy().to_string(),
        None => {
            if one.media.is_empty() {
                String::from("nothing given yet")
            } else {
                one.media.clone()
            }
        }
    }
}

// A line of the state of a whole film.
pub fn headline(project: &Project) -> String {
    let (width, height) = size_of(project);
    let whole = seconds(project);
    format!(
        "{}, {width}x{height} at {} fps, {} long, {} layer{}",
        if project.name.is_empty() {
            String::from("a film")
        } else {
            project.name.clone()
        },
        project.fps,
        trim_time(whole),
        project.layers.len(),
        if project.layers.len() == 1 { "" } else { "s" }
    )
}

// The account of a film a model reads back: the header, and every layer on one line.
pub fn account(project: &Project) -> String {
    let whole = seconds(project);
    let mut out = format!("{}\n", headline(project));
    if !project.about.is_empty() {
        out.push_str(&format!("About: {}\n", project.about));
    }
    if !project.background.kind.is_empty() && project.background.kind != "none" {
        out.push_str(&format!("Ground: {}\n", project.background.kind));
    }
    if let Some(sound) = &project.audio {
        out.push_str(&format!("Sound of the film: {}\n", sound.media));
    }
    if !project.tracks.is_empty() {
        out.push_str(&format!(
            "Tracks: {}\n",
            project.tracks.iter().map(|one| one.name.clone()).collect::<Vec<String>>().join(", ")
        ));
    }
    if project.layers.is_empty() {
        out.push_str("No layer stands yet.\n");
        return out;
    }
    out.push_str("Layers, first what is drawn under:\n");
    for place in ordered(project) {
        out.push_str(&format!("  {}\n", one_line(&project.layers[place], whole)));
    }
    out
}

// ---------------------------------------------------------------- small helps

// The text of a part, when it is text.
pub fn text_of(given: &Value, key: &str) -> Option<String> {
    match given.get(key) {
        Some(Value::String(one)) if !one.trim().is_empty() => Some(one.trim().to_string()),
        Some(Value::Number(one)) => Some(one.to_string()),
        _ => None,
    }
}

fn text_of_value(given: &Value) -> Option<String> {
    match given {
        Value::String(one) => Some(one.clone()),
        Value::Number(one) => Some(one.to_string()),
        Value::Bool(one) => Some(if *one { "yes".to_string() } else { "no".to_string() }),
        Value::Null => Some(String::new()),
        _ => None,
    }
}

fn written(given: &Value) -> String {
    text_of_value(given).unwrap_or_else(|| match given {
        Value::Array(rows) => rows
            .iter()
            .filter_map(text_of_value)
            .collect::<Vec<String>>()
            .join(", "),
        _ => String::new(),
    })
}

// The number of a part, when it is a number, or a number written as text.
pub fn number_of(given: &Value, key: &str) -> Option<f64> {
    match given.get(key) {
        Some(Value::Number(one)) => one.as_f64(),
        Some(Value::String(one)) => one.trim().replace(',', "").parse::<f64>().ok(),
        _ => None,
    }
}

fn bool_of(given: &Value, key: &str) -> Option<bool> {
    match given.get(key) {
        Some(Value::Bool(one)) => Some(*one),
        Some(Value::String(one)) => match one.trim().to_ascii_lowercase().as_str() {
            "yes" | "true" | "on" | "1" => Some(true),
            "no" | "false" | "off" | "0" => Some(false),
            _ => None,
        },
        Some(Value::Number(one)) => one.as_f64().map(|one| one != 0.0),
        _ => None,
    }
}

// The parts of an object nothing knows about. A part that is not known is reported but does not
// stop the call: a model that gives a name of its own for a colour should be told, not refused.
fn note_unknown(
    given: &Value,
    known: &[&'static str],
    what: &str,
    faults: &mut Vec<String>,
    notes: &mut Vec<String>,
) {
    let _ = notes;
    for key in given.as_object().into_iter().flat_map(|one| one.keys()) {
        if key == "project" || key == "op" || key == "at" || key == "id" || key == "read" {
            continue;
        }
        if known.contains(&key.as_str()) {
            continue;
        }
        match nearest(key, known) {
            Some(found) => faults.push(format!(
                "there is no \"{key}\" {what}; \"{found}\" was meant? Nothing was taken from \
                 that part."
            )),
            None => notes.push(format!(
                "\"{key}\" is not a part {what}; it was left out."
            )),
        }
    }
}

// The nearest of a list of words, so "oppacity" is answered with "opacity" and not with silence.
// The measure is the number of single-letter changes between the two words, and only a word that
// is one or two changes away is offered: "waffle" is not a mispelling of "table", and a model is
// better served by the list of what is known than by a guess at what was not meant.
pub fn nearest(written: &str, known: &[&str]) -> Option<String> {
    let said = written.trim().to_ascii_lowercase().replace(['-', '_', ' '], "");
    if said.is_empty() {
        return None;
    }
    let mut best: Option<(usize, String)> = None;
    for one in known {
        let held = one.to_ascii_lowercase().replace(['-', '_', ' '], "");
        if held == said {
            return Some((*one).to_string());
        }
        // A short word tolerates one change, a long word two. Two letters of a word that were
        // written the wrong way round cost two changes, so they are let through on their own.
        let most = (held.len() / 6).max(1);
        let steps = changes(&said, &held);
        let swapped = said.len() == held.len() && said.len() >= 4 && letters(&said) == letters(&held);
        if (steps <= most || swapped)
            && best.as_ref().map(|(one, _)| steps < *one).unwrap_or(true)
        {
            best = Some((steps, (*one).to_string()));
        }
    }
    best.map(|(_, one)| one)
}

// The letters of a word, in order, so that two words written with the same letters in a different
// order are known to be the same word misspelt.
fn letters(word: &str) -> String {
    let mut held: Vec<char> = word.chars().collect();
    held.sort_unstable();
    held.into_iter().collect()
}

// The number of single-letter insertions, deletions and replacements that turn one word into
// another. The words here are the names of a composition, so a plain dynamic program of two rows
// is enough and cheap.
fn changes(one: &str, other: &str) -> usize {
    let left: Vec<char> = one.chars().collect();
    let right: Vec<char> = other.chars().collect();
    let mut row: Vec<usize> = (0..=right.len()).collect();
    for (place, one) in left.iter().enumerate() {
        let mut last = row[0];
        row[0] = place + 1;
        for (next, other) in right.iter().enumerate() {
            let held = row[next + 1];
            row[next + 1] = if one == other {
                last
            } else {
                (last.min(row[next]).min(row[next + 1])) + 1
            };
            last = held;
        }
    }
    row[right.len()]
}

// A number without the tail of zeroes json writes.
pub fn trim(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let rounded = (value * 10000.0).round() / 10000.0;
    if (rounded - rounded.round()).abs() < 1e-9 {
        return rounded.round();
    }
    rounded
}

// Seconds as they are read: 4.5 s, 1 min 20 s.
pub fn trim_time(seconds: f64) -> String {
    if seconds <= 0.0 {
        return String::from("not measured");
    }
    if seconds < 90.0 {
        return format!("{} s", trim(seconds));
    }
    let minutes = (seconds / 60.0).floor() as i64;
    let rest = seconds - minutes as f64 * 60.0;
    format!("{minutes} min {} s", trim(rest))
}

// A part of the height of the canvas, in pixels of the canvas. A size of a letter is written in
// parts, so the same words draw the same at 720p and at 4k.
pub fn height_part(part: f64, height: u32) -> f64 {
    part * height as f64
}

// Words cut to a length, for a line of a report.
fn trim_to(words: &str, most: usize) -> String {
    if words.chars().count() <= most {
        return words.to_string();
    }
    let mut out: String = words.chars().take(most).collect();
    out.push('…');
    out
}

// The next free id of a layer, when the model did not name it.
pub fn next_id(project: &Project) -> String {
    let mut number = project.layers.len() + 1;
    loop {
        let tried = format!("l{number}");
        if !project.layers.iter().any(|one| one.id == tried) {
            return tried;
        }
        number += 1;
        if number > 9999 {
            return format!("l{}", std::time::UNIX_EPOCH.elapsed().map(|one| one.as_secs()).unwrap_or(0));
        }
    }
}

// The place of a layer by the name the model gave it: its id, its name, or its number in the
// list from 1.
pub fn find(layers: &[Layer], said: &str) -> Option<usize> {
    let needle = said.trim();
    if needle.is_empty() {
        return None;
    }
    if let Ok(number) = needle.parse::<usize>() {
        if number >= 1 && number <= layers.len() {
            return Some(number - 1);
        }
    }
    let lower = needle.to_ascii_lowercase();
    let by_id = layers
        .iter()
        .position(|one| one.id.to_ascii_lowercase() == lower);
    if by_id.is_some() {
        return by_id;
    }
    layers.iter().position(|one| one.name.to_ascii_lowercase() == lower)
}

#[cfg(test)]
mod checks {
    use super::*;

    fn given(words: &str) -> Value {
        serde_json::from_str(words).expect("the words of the check are read")
    }

    // A header is made of what a call gave, and of nothing else.
    #[test]
    fn a_header_keeps_what_the_call_did_not_name() {
        let had = Project {
            name: String::from("leaf"),
            canvas: String::from("720p"),
            fps: 30,
            about: String::from("the fall of the leaf"),
            ..Project::default()
        };
        let read = merge_project(&had, &given(r#"{"fps": 60, "length": 8}"#));
        assert!(read.faults.is_empty(), "{:?}", read.faults);
        assert_eq!(read.project.fps, 60);
        assert_eq!(read.project.length, 8.0);
        assert_eq!(read.project.canvas, "720p", "the canvas was not named, so it stands");
        assert_eq!(read.project.about, "the fall of the leaf");
        assert!(read.given);
    }

    // A size of the canvas is answered by name or by measure, and a word nothing means is told.
    #[test]
    fn a_canvas_is_read_by_name_or_by_measure() {
        assert_eq!(canvas_of("1080p"), Some((1920, 1080)));
        assert_eq!(canvas_of("854x480"), Some((854, 480)));
        assert!(canvas_of("1080q").is_none());
        let read = merge_project(&Project::default(), &given(r#"{"canvas": "1080q"}"#));
        assert_eq!(read.faults.len(), 1, "{:?}", read.faults);
        assert!(read.faults[0].contains("size of the canvas"), "{}", read.faults[0]);
    }

    // A layer of a kind nothing knows is answered with the nearest kind, and told.
    #[test]
    fn a_kind_is_mended_when_it_is_almost_right() {
        let mut faults = Vec::new();
        let layer = read_layer(&given(r#"{"kind": "tabel"}"#), 1, &mut faults).expect("the layer is read");
        assert_eq!(layer.kind, "table", "{:?}", faults);
        assert_eq!(faults.len(), 1, "{faults:?}");
    }

    // A kind nothing resembles is refused, because the parts of one kind mean nothing in another.
    #[test]
    fn a_kind_that_means_nothing_refuses_the_layer() {
        let mut faults = Vec::new();
        assert!(read_layer(&given(r#"{"kind": "waffle"}"#), 1, &mut faults).is_none());
        assert_eq!(faults.len(), 1, "{faults:?}");
    }

    // The parts of the style are kept under their own names, and a part that is not one of them
    // is answered with the nearest of what is known.
    #[test]
    fn a_style_keeps_its_parts_and_tells_of_the_rest() {
        let mut faults = Vec::new();
        let style = read_style(
            &given(
                r#"{"text": "**Bold** and {#c2410c}rust{}", "size": 0.06, "align": "center",
                    "rows": [["one","2"],["three","4"]], "oppacity": 0.5}"#,
            ),
            1,
            &mut faults,
        );
        assert_eq!(style.text, "**Bold** and {#c2410c}rust{}");
        assert_eq!(style.size, 0.06);
        assert_eq!(style.align, "center");
        assert_eq!(style.rows.len(), 2);
        assert_eq!(style.rows[1][0], "three");
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("no \"oppacity\""), "{faults:?}");
    }

    // A film of layers writes its words and reads them back as the same film.
    #[test]
    fn a_film_written_and_read_again_is_the_same_film() {
        let mut project = Project {
            name: String::from("leaf"),
            about: String::from("what falls"),
            canvas: String::from("1080p"),
            fps: 25,
            length: 12.5,
            palette: String::from("leaf"),
            ..Project::default()
        };
        project.layers.push(Layer {
            id: String::from("l1"),
            name: String::from("heading"),
            kind: String::from("text"),
            from: 0.5,
            to: 4.0,
            x: 0.5,
            y: 0.2,
            w: 0.7,
            h: 0.1,
            style: Style { text: String::from("**Autumn**"), size: 0.07, ..Style::default() },
            enter: Some(Motion { kind: String::from("rise"), seconds: 0.6, ..Motion::default() }),
            anim: vec![Key {
                prop: String::from("y"),
                keys: vec![json!([0.5, 0.62]), json!([1.4, 0.2])],
                ease: String::from("ease-out"),
            }],
            effects: vec![Effect { kind: String::from("shadow"), radius: 0.004, ..Effect::default() }],
            ..Layer::default()
        });
        project.layers.push(Layer {
            id: String::from("l2"),
            kind: String::from("video"),
            media: String::from("/tmp/base.mp4"),
            from: 0.0,
            to: 0.0,
            in_at: 12.0,
            out_at: 20.0,
            volume: 0.7,
            x: 0.75,
            y: 0.7,
            w: 0.3,
            h: 0.25,
            effects: vec![Effect {
                kind: String::from("chroma"),
                colour: String::from("auto"),
                similarity: 0.24,
                blend: 0.12,
                spill: 0.6,
                ..Effect::default()
            }],
            ..Layer::default()
        });
        let written = words(&project);
        let again = from_words(&written).expect("the words are read back");
        assert_eq!(again.name, "leaf");
        assert_eq!(again.fps, 25);
        assert_eq!(again.layers.len(), 2);
        assert_eq!(again.layers[0].style.text, "**Autumn**");
        assert_eq!(again.layers[0].enter.as_ref().map(|one| one.kind.as_str()), Some("rise"));
        assert_eq!(again.layers[0].anim.len(), 1);
        assert_eq!(again.layers[1].in_at, 12.0);
        assert_eq!(again.layers[1].effects[0].kind, "chroma");
        assert_eq!(again.layers[1].effects[0].similarity, 0.24);
    }

    // The length of a film is what its last layer holds, when the header did not name one.
    #[test]
    fn a_film_runs_as_long_as_its_last_layer() {
        let mut project = Project { canvas: String::from("1080p"), ..Project::default() };
        project.layers.push(Layer { from: 0.0, to: 4.0, ..Layer::default() });
        project.layers.push(Layer {
            from: 6.0,
            kind: String::from("video"),
            in_at: 1.0,
            out_at: 5.0,
            speed: 2.0,
            ..Layer::default()
        });
        assert_eq!(seconds(&project), 8.0, "the film of the media ends its own layer");
        let whole = seconds(&project);
        assert!(shows(&project.layers[0], 2.0, whole));
        assert!(!shows(&project.layers[0], 5.0, whole));
        assert!(shows(&project.layers[1], 7.9, whole));
    }

    // A layer is found by its number, its id or its name.
    #[test]
    fn a_layer_is_found_by_any_of_its_names() {
        let layers = vec![
            Layer { id: String::from("l1"), name: String::from("heading"), ..Layer::default() },
            Layer { id: String::from("base"), name: String::new(), ..Layer::default() },
        ];
        assert_eq!(find(&layers, "1"), Some(0));
        assert_eq!(find(&layers, "heading"), Some(0));
        assert_eq!(find(&layers, "BASE"), Some(1));
        assert_eq!(find(&layers, "nothing"), None);
    }

    // The order of the drawing is the order of the layers, with the lowest z under.
    #[test]
    fn the_drawing_order_is_lowest_z_first() {
        let mut project = Project { canvas: String::from("1080p"), ..Project::default() };
        project.layers.push(Layer { id: String::from("over"), z: 5, ..Layer::default() });
        project.layers.push(Layer { id: String::from("under"), z: 1, ..Layer::default() });
        project.layers.push(Layer { id: String::from("middle"), z: 5, ..Layer::default() });
        assert_eq!(ordered(&project), vec![1, 0, 2]);
        assert_eq!(next_z(&project), 6);
    }

    // The nearest of a misspelt word, and nothing when the word is far from all.
    #[test]
    fn a_misspelt_word_is_answered_with_the_nearest() {
        assert_eq!(nearest("oppacity", PROPS).as_deref(), Some("opacity"));
        assert_eq!(nearest("wipe_right", MOTIONS).as_deref(), Some("wipe-right"));
        assert_eq!(nearest("waffle", KINDS), None);
    }

    // A line of a report says the moment the layer is on the screen and what it holds.
    #[test]
    fn a_line_of_the_report_names_the_moment_and_the_thing() {
        let layer = Layer {
            id: String::from("l3"),
            kind: String::from("text"),
            from: 1.25,
            to: 3.0,
            style: Style { text: String::from("the fall of the leaf"), ..Style::default() },
            ..Layer::default()
        };
        let said = one_line(&layer, 10.0);
        assert!(said.starts_with("l3 [text] 1.25 to 3 s"), "{said}");
        assert!(said.contains("the fall of the leaf"), "{said}");
    }
}
