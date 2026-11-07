// The words of an infographic video: what is to be seen, and when.
//
// A video is one project. A project stands in a folder of the room of the chat,
// and the whole of it is one file of json, `script.json`. The model writes that
// file through the tools of this module, never with a file tool, so the words it
// writes are read as they come in and a fault is told to it at once instead of
// being seen a minute later in a picture of nothing.
//
// The shape is kept small on purpose. A scene is a box of blocks; a block is one
// thing of the screen with a place on the grid and a way of coming in. The model
// names a place by the grid -- `col`, `row`, `w`, `h` of twenty-four columns and
// sixteen rows -- and not by pixels, for the grid reads the same at every size of
// canvas and never asks the model to do arithmetic. A place in pixels is still
// let through, for a picture that hangs off the edge of the grid is a thing a
// model sometimes wants.
//
// Nothing here draws. `script.rs` keeps words, `draw.rs` turns them into a
// picture, and `encode.rs` turns pictures into a file.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// The name of the file the whole of a project stands in.
pub const SCRIPT_FILE: &str = "script.json";

// The name of the folder of one project, under `infographics` in the room of
// the chat.
pub const ROOT: &str = "infographics";

// The name of the project a tool takes when it is given none.
pub const DEFAULT_PROJECT: &str = "main";

// The grid every block is placed on. Twenty-four columns and sixteen rows fit a
// wide canvas, a square one and a tall one alike, and give a block a place fine
// enough for any layout without asking for pixels.
pub const COLS: i32 = 24;
pub const ROWS: i32 = 16;

// How long a scene may be, and how many of them one video may hold. The two
// ceilings keep a run of the renderer inside a time a user will wait for.
pub const GREATEST_SCENE_SECONDS: f64 = 120.0;
pub const GREATEST_SCENES: usize = 80;
pub const GREATEST_FPS: u32 = 60;

// ---------------------------------------------------------------- canvas

// One size of picture. The word the model uses is kept, so the script reads as
// the model wrote it and not as a list of numbers.
#[derive(Clone, Debug)]
pub struct Canvas {
    pub word: &'static str,
    pub width: u32,
    pub height: u32,
}

// The sizes a canvas word may name. A model that wants another size writes
// "2048x858" and gets it.
pub const CANVASES: &[Canvas] = &[
    Canvas { word: "1080p", width: 1920, height: 1080 },
    Canvas { word: "720p", width: 1280, height: 720 },
    Canvas { word: "vertical", width: 1080, height: 1920 },
    Canvas { word: "square", width: 1080, height: 1080 },
    Canvas { word: "tall", width: 1080, height: 1350 },
    Canvas { word: "wide", width: 2560, height: 1080 },
];

// The size of a canvas word. A word nothing knows is answered with nothing, and
// the caller says so. A word of the shape "1280x720" is taken, and both numbers
// are kept inside the ceiling of the renderer.
pub fn canvas_of(word: &str) -> Option<Canvas> {
    let said = word.trim().to_ascii_lowercase();
    if said.is_empty() {
        return CANVASES[0].clone().into();
    }
    for one in CANVASES {
        if one.word == said {
            return Some(one.clone());
        }
    }
    let (w, h) = said
        .split_once('x')
        .or_else(|| said.split_once('*'))
        .or_else(|| said.split_once('\u{00d7}'))?;
    let width = w.trim().parse::<u32>().ok()?;
    let height = h.trim().parse::<u32>().ok()?;
    if width < 320 || height < 320 || width > 3840 || height > 3840 {
        return None;
    }
    Some(Canvas { word: "custom", width, height })
}

// The words of the sizes, for the guide and for a fault that names them.
pub fn canvas_words() -> String {
    let mut out: Vec<String> = CANVASES
        .iter()
        .map(|one| format!("{} ({}x{})", one.word, one.width, one.height))
        .collect();
    out.push(String::from("or \"1280x720\" for a size of your own"));
    out.join(", ")
}

// ---------------------------------------------------------------- palette

// The colours of one video. Every block takes its colours from here unless it
// names its own, so a video is re-skinned by one word.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
    pub name: String,
    pub bg: String,
    pub panel: String,
    pub ink: String,
    pub muted: String,
    pub line: String,
    pub accent: String,
    pub accent2: String,
    pub good: String,
    pub bad: String,
    pub series: Vec<String>,
}

impl Default for Palette {
    fn default() -> Self {
        palette_of("ink").unwrap_or_else(|| palette_of("paper").unwrap())
    }
}

// The colours a name may ask for. Nine of them, each a whole skin of its own:
// two pale, six dark, one of grey alone. The words of a series are six colours
// that stand apart from one another in a chart of any of these skins.
pub fn palette_of(word: &str) -> Option<Palette> {
    let said = word.trim().to_ascii_lowercase();
    let made = match said.as_str() {
        "" | "ink" => Palette {
            name: String::from("ink"),
            bg: String::from("#12100f"),
            panel: String::from("#1e1a17"),
            ink: String::from("#f7f1e8"),
            muted: String::from("#a89e92"),
            line: String::from("#3b342e"),
            accent: String::from("#ED7401"),
            accent2: String::from("#DF6101"),
            good: String::from("#4ec26a"),
            bad: String::from("#e0563f"),
            series: colours(&["#ED7401", "#4ec26a", "#f2c14e", "#5aa9e6", "#c56bd6", "#8fd6c9"]),
        },
        "paper" => Palette {
            name: String::from("paper"),
            bg: String::from("#faf7f0"),
            panel: String::from("#ffffff"),
            ink: String::from("#1c1a17"),
            muted: String::from("#6d675f"),
            line: String::from("#ddd6c9"),
            accent: String::from("#C53C01"),
            accent2: String::from("#ED7401"),
            good: String::from("#2e7d4f"),
            bad: String::from("#b3261e"),
            series: colours(&["#C53C01", "#2e7d4f", "#1f6fb2", "#b8860b", "#7b4fa6", "#0f7f7f"]),
        },
        "midnight" => Palette {
            name: String::from("midnight"),
            bg: String::from("#0b1020"),
            panel: String::from("#151c30"),
            ink: String::from("#eef2fb"),
            muted: String::from("#93a0bd"),
            line: String::from("#2a3450"),
            accent: String::from("#6ea8fe"),
            accent2: String::from("#22d3ee"),
            good: String::from("#34d399"),
            bad: String::from("#f87171"),
            series: colours(&["#6ea8fe", "#34d399", "#fbbf24", "#a78bfa", "#22d3ee", "#f472b6"]),
        },
        "graphite" => Palette {
            name: String::from("graphite"),
            bg: String::from("#17181a"),
            panel: String::from("#222426"),
            ink: String::from("#f5f6f7"),
            muted: String::from("#9aa0a6"),
            line: String::from("#35383b"),
            accent: String::from("#e6e8ea"),
            accent2: String::from("#b6bcc3"),
            good: String::from("#b7d7b4"),
            bad: String::from("#e3b0a8"),
            series: colours(&["#f5f6f7", "#c3c8cd", "#9aa0a6", "#7c8288", "#5e646a", "#dedfe1"]),
        },
        "forest" => Palette {
            name: String::from("forest"),
            bg: String::from("#0d1512"),
            panel: String::from("#17241d"),
            ink: String::from("#eef6f0"),
            muted: String::from("#9db4a8"),
            line: String::from("#27402f"),
            accent: String::from("#58c98a"),
            accent2: String::from("#d9c26a"),
            good: String::from("#58c98a"),
            bad: String::from("#e07a5f"),
            series: colours(&["#58c98a", "#d9c26a", "#7fb2e5", "#c98fb8", "#8fd6c9", "#e0a45e"]),
        },
        "sunset" => Palette {
            name: String::from("sunset"),
            bg: String::from("#1a0f0c"),
            panel: String::from("#291612"),
            ink: String::from("#fff3ea"),
            muted: String::from("#c39a86"),
            line: String::from("#45211a"),
            accent: String::from("#ff7a45"),
            accent2: String::from("#ffc53d"),
            good: String::from("#7ad4a0"),
            bad: String::from("#ff5c5c"),
            series: colours(&["#ff7a45", "#ffc53d", "#7ad4a0", "#8ab6ff", "#e58fd0", "#ff9f7a"]),
        },
        "neon" => Palette {
            name: String::from("neon"),
            bg: String::from("#07070b"),
            panel: String::from("#121218"),
            ink: String::from("#f4f6ff"),
            muted: String::from("#8d8fa6"),
            line: String::from("#24242f"),
            accent: String::from("#7cff59"),
            accent2: String::from("#22d3ee"),
            good: String::from("#7cff59"),
            bad: String::from("#ff4d6d"),
            series: colours(&["#7cff59", "#22d3ee", "#ffd166", "#c77dff", "#ff6b9d", "#4dd2ff"]),
        },
        "mono" => Palette {
            name: String::from("mono"),
            bg: String::from("#ffffff"),
            panel: String::from("#f2f2f2"),
            ink: String::from("#111111"),
            muted: String::from("#666666"),
            line: String::from("#d5d5d5"),
            accent: String::from("#111111"),
            accent2: String::from("#555555"),
            good: String::from("#333333"),
            bad: String::from("#8a8a8a"),
            series: colours(&["#111111", "#4d4d4d", "#808080", "#b3b3b3", "#2b2b2b", "#9a9a9a"]),
        },
        _ => return None,
    };
    Some(made)
}

// The names of the palettes, for the guide.
pub fn palette_names() -> String {
    let mut out: Vec<&str> = Vec::new();
    for word in ["ink", "paper", "midnight", "graphite", "forest", "sunset", "neon", "mono"] {
        out.push(word);
    }
    out.join(", ")
}

fn colours(words: &[&str]) -> Vec<String> {
    words.iter().map(|one| String::from(*one)).collect()
}

// One colour out of the palette, with the colour of a series taken by its
// place. A number past the end of the series wraps, so a chart of nine bars
// still gives every bar a colour of its own.
pub fn series_colour(palette: &Palette, place: usize) -> String {
    if palette.series.is_empty() {
        return palette.accent.clone();
    }
    palette.series[place % palette.series.len()].clone()
}

// ---------------------------------------------------------------- the script

// One video.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    // The name of the project, which is the name of its folder.
    pub name: String,
    // A line for the user about what the video is of. Not drawn.
    pub about: String,
    // The size of the picture: a word of the list, or "WxH".
    pub canvas: String,
    // Frames of the second.
    pub fps: u32,
    // The colours of the whole of the video.
    pub palette: String,
    // The colours the palette gives way to, block by block. Empty for none.
    pub colours: Colours,
    // The words of the type: "sans", "serif", "mono", or the name of a family
    // the machine has.
    pub font: String,
    // How heavy the heads are: "normal", "bold", "black".
    pub heading_weight: String,
    // The ground every scene stands on unless the scene names its own.
    pub background: Background,
    // The scenes, in the order they are shown in.
    pub scenes: Vec<Scene>,
}

// Colours the user of the tool puts over the palette. Every part is optional.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Colours {
    pub bg: String,
    pub panel: String,
    pub ink: String,
    pub muted: String,
    pub line: String,
    pub accent: String,
    pub accent2: String,
    pub good: String,
    pub bad: String,
    pub series: Vec<String>,
}

// The ground of a scene.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Background {
    // "solid", "gradient", "grid", "dots", "none".
    pub kind: String,
    // The two colours of a gradient, or the one colour of a solid ground.
    pub from: String,
    pub to: String,
    // The way a gradient runs, in degrees from the left edge to the right.
    pub angle: f64,
    // How dark the four corners are made: 0 for nothing, 1 for a great deal.
    pub vignette: f64,
    // How far apart the lines of a grid stand, in parts of the width.
    pub spacing: f64,
}

// One scene of the video.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    // The head of the scene in the storyboard. Not drawn unless a block draws
    // it; it is what the model and the user read the scene by.
    pub title: String,
    // What the scene is for, in the words of the model. Never drawn.
    pub note: String,
    // How long the scene stands, in seconds.
    pub seconds: f64,
    // How the scene comes in from the one before it.
    pub transition: Transition,
    // The things on the screen.
    pub blocks: Vec<Block>,
    // The ground of this scene, when it is not the ground of the video.
    pub background: Option<Background>,
}

// How one scene follows the one before it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Transition {
    // "cut", "fade", "slide", "wipe", "zoom".
    pub kind: String,
    // How long the change takes, in milliseconds.
    pub ms: f64,
    // For a slide and a wipe: "left", "right", "up", "down".
    pub dir: String,
}

// One thing on the screen. Every kind of block is one shape with many parts,
// and a part that a kind does not use is left out. `check_keys` says which parts
// a kind knows, so a part written wrong is told to the model rather than
// silently passed over.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Block {
    // What this block is. See `KINDS`.
    pub kind: String,

    // Where it stands, on the grid. Zero means the part was not named.
    pub col: i32,
    pub row: i32,
    pub w: i32,
    pub h: i32,
    // Where it stands, in pixels, when the grid is not wanted.
    pub x: f64,
    pub y: f64,
    pub pw: f64,
    pub ph: f64,

    // The words.
    pub text: String,
    pub items: Vec<Item>,
    pub spans: Vec<Span>,
    // The two sides of a compare, each with a head and lines of its own.
    pub groups: Vec<Group>,
    // The rows of a table, each a list of cells.
    pub rows: Vec<Vec<String>>,
    // "left", "center", "right".
    pub align: String,
    // "top", "middle", "bottom".
    pub valign: String,
    // The size of the words, in parts of the height of the canvas, so the same
    // script fills a 720p picture and a 1080p one alike. 0 takes the size the
    // kind brings with it.
    pub size: f64,
    // "normal", "bold", "black".
    pub weight: String,
    // How tall a line of words stands, as a share of the size.
    pub leading: f64,
    // How far apart the lines of a list stand, in parts of the height.
    pub gap: f64,
    // For a list: "dot", "check", "cross", "arrow", "number", "none".
    pub marker: String,

    // Colours and the panel behind the block.
    pub colour: String,
    pub bg: String,
    pub border: String,
    // The round of a panel's corner, in parts of the height.
    pub radius: f64,
    // The air inside a panel, in parts of the height.
    pub pad: f64,
    pub opacity: f64,

    // Numbers, for a stat and for a chart.
    pub value: f64,
    pub values: Vec<f64>,
    pub labels: Vec<String>,
    pub series: Vec<Series>,
    pub prefix: String,
    pub suffix: String,
    pub decimals: i32,
    // "vertical" or "horizontal" for a bar chart; "row" or "column" for a
    // process.
    pub direction: String,
    // The share of a ring left empty in the middle, and the share a progress bar
    // stands at.
    pub hole: f64,
    // Whether the numbers of a chart stand on it, and whether the axis does.
    pub show_values: Option<bool>,
    pub show_axis: Option<bool>,
    pub show_legend: Option<bool>,
    // Whether a number counts up while the scene runs, and whether the bars of a
    // chart grow.
    pub count: Option<bool>,
    pub grow: Option<bool>,
    // How long the growth takes, in milliseconds.
    pub grow_ms: f64,

    // A picture, a shape, or a piece of the model's own drawing.
    pub path: String,
    // "contain" or "cover".
    pub fit: String,
    // "rect", "circle", "ring", "panel".
    pub shape: String,
    // The name of a mark of the set the program draws: see `MARKS`.
    pub mark: String,
    // The thickness of a line, in parts of the height.
    pub stroke: f64,
    // The raw words of a picture of the model's own, for a block of kind "svg".
    pub svg: String,

    // How the block comes in.
    // "none", "fade", "rise", "left", "right", "pop", "wipe", "type".
    pub reveal: String,
    // When it starts, in milliseconds from the start of the scene.
    pub delay: f64,
    // How much later each line or item of it comes in.
    pub stagger: f64,
    // How long the coming in takes, in milliseconds.
    pub reveal_ms: f64,
}

// One line of a list, one step of a process, one point of a timeline.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub text: String,
    // A second, smaller line under the first.
    pub sub: String,
    // A head of its own, for a step and for a point of a timeline.
    pub title: String,
    // When it stands, for a point of a timeline.
    pub when: String,
    // A number, for a step that carries one and for a chart drawn from items.
    pub value: f64,
    pub colour: String,
    // The mark beside the line, for a list that names its own marks.
    pub mark: String,
    // How far along a timeline this point stands, from 0 to 1.
    pub at: f64,
}

// One side of a compare: the head of it, and the lines under it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub title: String,
    pub text: String,
    pub items: Vec<Item>,
    pub colour: String,
}

// One run of words with colours of its own, for a text block that puts a word
// in the accent colour.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Span {
    pub text: String,
    pub colour: String,
    pub weight: String,
}

// One line of a chart, or one set of bars of it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Series {
    pub name: String,
    pub values: Vec<f64>,
    pub colour: String,
    // Whether the space under a line is filled, and whether the line is broken.
    pub fill: Option<bool>,
    pub dashed: Option<bool>,
    // The thickness of the line, in parts of the height.
    pub width: f64,
}

// The kinds of block a scene may hold, with the parts each of them reads. A
// part outside this list is a fault of writing, and the model is told so.
pub const KINDS: &[(&str, &[&str])] = &[
    ("head", &["text", "spans", "align", "valign", "size", "weight", "colour", "leading", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "stagger", "reveal_ms"]),
    ("text", &["text", "spans", "align", "valign", "size", "weight", "colour", "leading", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "stagger", "reveal_ms"]),
    ("bullets", &["items", "text", "values", "labels", "align", "valign", "size", "weight", "colour", "leading", "gap", "marker", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "stagger", "reveal_ms"]),
    ("stat", &["value", "text", "prefix", "suffix", "decimals", "align", "valign", "size", "weight", "colour", "gap", "count", "grow_ms", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "reveal_ms"]),
    ("bars", &["labels", "values", "series", "items", "direction", "colour", "align", "valign", "size", "decimals", "prefix", "suffix", "show_values", "show_axis", "show_legend", "grow", "grow_ms", "gap", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "reveal_ms"]),
    ("line", &["labels", "values", "series", "items", "colour", "align", "valign", "size", "decimals", "prefix", "suffix", "show_values", "show_axis", "show_legend", "grow", "grow_ms", "stroke", "hole", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "reveal_ms"]),
    ("pie", &["labels", "values", "series", "items", "hole", "colour", "align", "valign", "size", "decimals", "prefix", "suffix", "show_values", "show_legend", "grow", "grow_ms", "gap", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "reveal_ms"]),
    ("compare", &["items", "groups", "text", "labels", "values", "align", "valign", "size", "weight", "colour", "leading", "gap", "marker", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "stagger", "reveal_ms"]),
    ("steps", &["items", "labels", "text", "direction", "align", "valign", "size", "weight", "colour", "leading", "gap", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "stagger", "reveal_ms"]),
    ("timeline", &["items", "labels", "direction", "align", "valign", "size", "weight", "colour", "leading", "gap", "stroke", "grow", "grow_ms", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "reveal_ms"]),
    ("quote", &["text", "spans", "items", "align", "valign", "size", "weight", "colour", "leading", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "reveal_ms"]),
    ("image", &["path", "fit", "text", "align", "valign", "size", "colour", "radius", "border", "opacity", "reveal", "delay", "reveal_ms"]),
    ("icon", &["mark", "colour", "size", "stroke", "bg", "radius", "border", "pad", "align", "valign", "opacity", "reveal", "delay", "reveal_ms"]),
    ("shape", &["shape", "colour", "bg", "border", "stroke", "radius", "opacity", "hole", "reveal", "delay", "reveal_ms"]),
    ("rule", &["colour", "stroke", "direction", "opacity", "reveal", "delay", "reveal_ms"]),
    ("pill", &["text", "colour", "bg", "border", "stroke", "size", "weight", "radius", "pad", "align", "valign", "gap", "opacity", "reveal", "delay", "stagger", "reveal_ms"]),
    ("progress", &["value", "text", "labels", "colour", "bg", "border", "stroke", "size", "weight", "radius", "pad", "prefix", "suffix", "decimals", "show_values", "gap", "grow", "grow_ms", "align", "valign", "opacity", "reveal", "delay", "reveal_ms"]),
    ("table", &["items", "labels", "rows", "values", "series", "align", "valign", "size", "weight", "colour", "leading", "gap", "stroke", "bg", "border", "radius", "pad", "opacity", "reveal", "delay", "stagger", "reveal_ms"]),
    ("code", &["text", "colour", "bg", "border", "radius", "pad", "size", "align", "valign", "opacity", "reveal", "delay", "reveal_ms"]),
    ("svg", &["svg", "colour", "align", "valign", "opacity", "reveal", "delay", "reveal_ms"]),
];

// The parts every block may name, beside the parts of its kind.
pub const COMMON: &[&str] = &["kind", "col", "row", "w", "h", "x", "y", "pw", "ph"];

// The words of the kinds, for the guide and for a fault that names them.
pub fn kind_names() -> String {
    KINDS.iter().map(|(one, _)| *one).collect::<Vec<&str>>().join(", ")
}

// The whole of the words of one kind: the parts it reads, and the place it is
// put on. The model reads this before it writes a block of that kind.
pub fn kind_words(kind: &str) -> String {
    let Some(parts) = known_parts(kind) else {
        return format!("\"{kind}\" is not a kind of block. The kinds are: {}.", kind_names());
    };
    let mut out = format!(
        "A block of kind \"{kind}\" reads these parts:\n  {}\nand these, like every block:\n  {}",
        parts.join(", "),
        COMMON.join(", ")
    );
    out.push_str(&format!(
        "\nThe place is named on a grid of {COLS} columns and {ROWS} rows: col, row, w, h.\
         A part outside these lists is answered as a fault, so do not invent one."
    ));
    out
}

// The guide of a whole script: the parts of a project, the parts of a scene,
// the kinds, the colours, the sizes and the marks.
pub fn guide() -> String {
    let mut out = String::new();
    out.push_str("An infographic video is one project. The shape is this:\n\n");
    out.push_str("PROJECT\n  ");
    out.push_str(&PROJECT_PARTS.join(", "));
    out.push_str("\n\nSCENE\n  ");
    out.push_str(&SCENE_PARTS.join(", "));
    out.push_str("\n\nGROUND\n  ");
    out.push_str(&BACKGROUND_PARTS.join(", "));
    out.push_str("\n\nEvery block names a kind and a place on a grid of ");
    out.push_str(&COLS.to_string());
    out.push_str(" columns and ");
    out.push_str(&ROWS.to_string());
    out.push_str(" rows (col, row, w, h).\nThe kinds are: ");
    out.push_str(&kind_names());
    out.push_str(".\nAsk the guide with one kind named to have the whole of its words.\n\n");
    out.push_str("Sizes: ");
    out.push_str(&canvas_words());
    out.push_str("\nColour sets: ");
    out.push_str(&palette_names());
    out.push_str("\n\nMarks an icon block may name: ");
    out.push_str(&crate::infographic::marks::about());
    out.push_str("\n\nCome in: a block names reveal (fade, up, down, left, right, grow, type, pop),\
         how long it waits (delay), how long it takes (reveal_ms), and how the items\
         after the first are held apart (stagger). A scene names how it comes from the\
         one before it (transition: cut, fade, wipe, push, slide) and how long that takes.\n\n\
         Work in this order: plan, scene, check, render. The check lays the words out\
         for real and says what does not fit; mend what it says before you render.");
    out
}

// True when a kind is one of the program.
pub fn is_kind(word: &str) -> bool {
    KINDS.iter().any(|(one, _)| *one == word)
}

// The parts a kind reads, and the parts every block reads. Nothing is answered
// for a kind nothing knows.
fn known_parts(kind: &str) -> Option<&'static [&'static str]> {
    KINDS.iter().find(|(one, _)| *one == kind).map(|(_, parts)| *parts)
}

// The faults of writing in a list of blocks: a part of a name that no kind
// reads, a kind that is not one of the program, a list with nothing in it. The
// words are for the model, so each says which block it is talking about and
// names the part it thinks was meant.
pub fn block_faults(blocks: &[Value]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (place, one) in blocks.iter().enumerate() {
        let number = place + 1;
        let map = match one.as_object() {
            Some(map) => map,
            None => {
                out.push(format!(
                    "block {number} is not a set of parts. A block is one object with a \"kind\" in it."
                ));
                continue;
            }
        };
        let kind = map.get("kind").and_then(Value::as_str).unwrap_or("");
        if kind.is_empty() {
            out.push(format!(
                "block {number} has no \"kind\". The kinds are: {}.",
                kind_names()
            ));
            continue;
        }
        let parts = match known_parts(kind) {
            Some(parts) => parts,
            None => {
                out.push(format!(
                    "block {number} names the kind \"{kind}\", which is not a kind of this program. The kinds are: {}.",
                    kind_names()
                ));
                continue;
            }
        };
        for key in map.keys() {
            if COMMON.contains(&key.as_str()) || parts.contains(&key.as_str()) {
                continue;
            }
            match nearest(key, parts) {
                Some(said) => out.push(format!(
                    "block {number} of kind \"{kind}\" has no part \"{key}\". The part it reads is \"{said}\"."
                )),
                None => out.push(format!(
                    "block {number} of kind \"{kind}\" has no part \"{key}\". The parts of that kind are: {}.",
                    parts.join(", ")
                )),
            }
        }
    }
    out
}

// The part of a list a written word was nearest to: a part that starts the same
// way, or one that shares most of its letters. Nothing comes back when the two
// are nothing alike, and the caller then names the whole list.
fn nearest(written: &str, parts: &[&'static str]) -> Option<&'static str> {
    let word = written.to_ascii_lowercase();
    let mut best: Option<(usize, &'static str)> = None;
    for one in parts {
        let said = one.to_ascii_lowercase();
        let score = if said.starts_with(&word) || word.starts_with(&said) {
            0
        } else {
            let shared = shared_letters(&word, &said);
            if shared * 2 < said.len() {
                continue;
            }
            1 + shared.abs_diff(said.len())
        };
        match best {
            Some((was, _)) if was <= score => {}
            _ => best = Some((score, *one)),
        }
    }
    best.map(|(_, one)| one)
}

// How many letters two words share. A rough measure is enough: the answer is
// only used to say which part the model may have meant.
fn shared_letters(one: &str, other: &str) -> usize {
    let mut left: Vec<char> = other.chars().collect();
    let mut count = 0;
    for c in one.chars() {
        if let Some(place) = left.iter().position(|have| *have == c) {
            left.remove(place);
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------- reading it

// The project out of the words of a tool call. The parts a call leaves out keep
// what the project had, so a call that changes the palette does not empty the
// scenes. A fault of writing is answered with the list of faults and no project.
pub fn merge(
    had: &Project,
    value: &Value,
) -> Result<Project, Vec<String>> {
    let mut faults: Vec<String> = Vec::new();
    let mut made = had.clone();
    if made.fps == 0 {
        made.fps = 24;
    }
    if made.canvas.is_empty() {
        made.canvas = String::from("1080p");
    }
    if made.palette.is_empty() {
        made.palette = String::from("ink");
    }
    if made.font.is_empty() {
        made.font = String::from("sans");
    }
    if made.heading_weight.is_empty() {
        made.heading_weight = String::from("bold");
    }
    if made.background.kind.is_empty() {
        made.background.kind = String::from("solid");
    }
    if made.background.angle == 0.0 {
        made.background.angle = 135.0;
    }

    if let Some(map) = value.as_object() {
        for key in map.keys() {
            if !PROJECT_PARTS.contains(&key.as_str()) {
                match nearest(key, PROJECT_PARTS) {
                    Some(said) => faults.push(format!(
                        "the project has no part \"{key}\". The part it reads is \"{said}\"."
                    )),
                    None => faults.push(format!(
                        "the project has no part \"{key}\". The parts are: {}.",
                        PROJECT_PARTS.join(", ")
                    )),
                }
            }
        }
    }
    let text = |key: &str| -> Option<String> {
        value.get(key).and_then(Value::as_str).map(|one| one.trim().to_string())
    };
    let number = |key: &str| -> Option<f64> {
        value.get(key).and_then(|one| {
            one.as_f64().or_else(|| one.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
        })
    };
    if let Some(one) = text("about") {
        made.about = one;
    }
    if let Some(one) = text("canvas") {
        if canvas_of(&one).is_none() {
            faults.push(format!(
                "\"{one}\" is not a size of canvas. The sizes are: {}.",
                canvas_words()
            ));
        } else {
            made.canvas = one;
        }
    }
    if let Some(one) = number("fps") {
        let whole = one.round() as u32;
        if whole < 1 || whole > GREATEST_FPS {
            faults.push(format!(
                "fps {one} is out of a time. The frames of a second stand between 1 and {GREATEST_FPS}."
            ));
        } else {
            made.fps = whole;
        }
    }
    if let Some(one) = text("palette") {
        if palette_of(&one).is_none() {
            faults.push(format!(
                "\"{one}\" is not a palette of this program. The palettes are: {}.",
                palette_names()
            ));
        } else {
            made.palette = one;
        }
    }
    if let Some(one) = value.get("colours") {
        match serde_json::from_value::<Colours>(one.clone()) {
            Ok(held) => made.colours = held,
            Err(why) => faults.push(format!("the part \"colours\" does not read: {why}")),
        }
    }
    if let Some(one) = text("font") {
        made.font = one;
    }
    if let Some(one) = text("heading_weight") {
        if !["normal", "bold", "black"].contains(&one.as_str()) {
            faults.push(format!(
                "\"{one}\" is not a weight of a heading. The weights are: normal, bold, black."
            ));
        } else {
            made.heading_weight = one;
        }
    }
    if let Some(one) = value.get("background") {
        match read_background(one) {
            Ok(held) => made.background = held,
            Err(why) => faults.push(format!("the part \"background\" does not read: {why}")),
        }
    }
    if let Some(list) = value.get("scenes") {
        let rows = match list.as_array() {
            Some(rows) => rows,
            None => {
                faults.push(String::from("\"scenes\" is not a list."));
                &Vec::new()
            }
        };
        let mut held: Vec<Scene> = Vec::new();
        for (place, one) in rows.iter().enumerate() {
            let number = place + 1;
            match read_scene(one, number, &mut faults) {
                Some(scene) => held.push(scene),
                None => {}
            }
        }
        if held.len() > GREATEST_SCENES {
            faults.push(format!(
                "{} scenes are more than one video may hold. The ceiling is {GREATEST_SCENES}.",
                held.len()
            ));
            held.truncate(GREATEST_SCENES);
        }
        made.scenes = held;
    }
    if faults.is_empty() {
        Ok(made)
    } else {
        Err(faults)
    }
}

// The parts of the project a call may name.
pub const PROJECT_PARTS: &[&str] = &[
    "about",
    "canvas",
    "fps",
    "palette",
    "colours",
    "font",
    "heading_weight",
    "background",
    "scenes",
];

// The ground of a scene, read with the parts it leaves out filled in.
fn read_background(value: &Value) -> Result<Background, String> {
    let word = |key: &str| -> String {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    if let Some(text) = value.as_str() {
        // A ground named by one word: "solid", "gradient", "none", or a colour.
        let said = text.trim().to_ascii_lowercase();
        let named = ["solid", "gradient", "grid", "dots", "none"].contains(&said.as_str());
        return Ok(Background {
            kind: if named { said } else { String::from("solid") },
            from: if named { String::new() } else { String::from(text.trim()) },
            angle: 135.0,
            ..Default::default()
        });
    }
    let map = value
        .as_object()
        .ok_or_else(|| String::from("a ground is a word or a set of parts"))?;
    for key in map.keys() {
        if !BACKGROUND_PARTS.contains(&key.as_str()) {
            return Err(format!("a ground has no part \"{key}\""));
        }
    }
    let kind = word("kind");
    if !kind.is_empty() && !["solid", "gradient", "grid", "dots", "none"].contains(&kind.as_str())
    {
        return Err(format!(
            "\"{kind}\" is not a kind of ground. The kinds are: solid, gradient, grid, dots, none."
        ));
    }
    Ok(Background {
        kind,
        from: word("from"),
        to: word("to"),
        angle: value.get("angle").and_then(Value::as_f64).unwrap_or(135.0),
        vignette: value.get("vignette").and_then(Value::as_f64).unwrap_or(0.0),
        spacing: value.get("spacing").and_then(Value::as_f64).unwrap_or(0.0),
    })
}

pub const BACKGROUND_PARTS: &[&str] = &["kind", "from", "to", "angle", "vignette", "spacing"];

// One scene of the `scenes` list of a call. A scene given as one word or as one
// line of text is a scene with a head and nothing else, which is a thing worth
// letting the model do quickly.
fn read_scene(value: &Value, number: usize, faults: &mut Vec<String>) -> Option<Scene> {
    if let Some(text) = value.as_str() {
        let line = text.trim();
        if line.is_empty() {
            faults.push(format!("scene {number} is a line of nothing"));
            return None;
        }
        return Some(Scene {
            title: String::from(line),
            note: String::new(),
            background: None,
            seconds: 4.0,
            transition: Transition {
                kind: String::from("fade"),
                ms: 450.0,
                dir: String::from("left"),
            },
            blocks: vec![Block {
                kind: String::from("head"),
                col: 2,
                row: 6,
                w: 20,
                h: 4,
                text: String::from(line),
                reveal: String::from("rise"),
                ..Default::default()
            }],
        });
    }
    let map = match value.as_object() {
        Some(map) => map,
        None => {
            faults.push(format!("scene {number} is neither a line of words nor a set of parts"));
            return None;
        }
    };
    for key in map.keys() {
        if !SCENE_PARTS.contains(&key.as_str()) {
            match nearest(key, SCENE_PARTS) {
                Some(said) => faults.push(format!(
                    "scene {number} has no part \"{key}\". The part it reads is \"{said}\"."
                )),
                None => faults.push(format!(
                    "scene {number} has no part \"{key}\". The parts are: {}.",
                    SCENE_PARTS.join(", ")
                )),
            }
        }
    }
    let mut scene = Scene {
        title: map
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        note: map
            .get("note")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        seconds: map.get("seconds").and_then(Value::as_f64).unwrap_or(4.0),
        ..Default::default()
    };
    if scene.seconds <= 0.0 {
        faults.push(format!("scene {number} stands for no time. \"seconds\" is a number above 0."));
        scene.seconds = 4.0;
    }
    if scene.seconds > GREATEST_SCENE_SECONDS {
        faults.push(format!(
            "scene {number} stands for {} seconds, which is over the ceiling of {GREATEST_SCENE_SECONDS}.",
            scene.seconds
        ));
        scene.seconds = GREATEST_SCENE_SECONDS;
    }
    if let Some(one) = map.get("transition") {
        scene.transition = read_transition(one, number, faults);
    } else {
        scene.transition = Transition {
            kind: String::from("fade"),
            ms: 450.0,
            dir: String::from("left"),
        };
    }
    match map.get("blocks") {
        Some(Value::Array(rows)) => {
            let bad = block_faults(rows);
            for one in bad {
                faults.push(format!("scene {number}: {one}"));
            }
            for row in rows {
                match serde_json::from_value::<Block>(row.clone()) {
                    Ok(block) => scene.blocks.push(block),
                    Err(why) => faults.push(format!("scene {number}: a block does not read: {why}")),
                }
            }
        }
        Some(_) => faults.push(format!("scene {number}: \"blocks\" is not a list.")),
        None => {}
    }
    if let Some(one) = map.get("background") {
        match read_background(one) {
            Ok(held) => scene.background = Some(held),
            Err(why) => faults.push(format!("scene {number}: the ground does not read: {why}")),
        }
    }
    Some(scene)
}

pub const SCENE_PARTS: &[&str] = &["title", "note", "seconds", "transition", "blocks", "background"];

// How one scene follows another.
fn read_transition(value: &Value, number: usize, faults: &mut Vec<String>) -> Transition {
    let word = match value.as_str() {
        Some(text) => String::from(text.trim()),
        None => {
            let map = match value.as_object() {
                Some(map) => map,
                None => {
                    faults.push(format!("scene {number}: a transition is a word or a set of parts"));
                    return Transition {
                        kind: String::from("fade"),
                        ms: 450.0,
                        dir: String::from("left"),
                    };
                }
            };
            for key in map.keys() {
                if !["kind", "ms", "dir"].contains(&key.as_str()) {
                    faults.push(format!("scene {number}: a transition has no part \"{key}\""));
                }
            }
            String::from(
                map.get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("fade")
                    .trim(),
            )
        }
    };
    if !["cut", "fade", "slide", "wipe", "zoom"].contains(&word.as_str()) {
        faults.push(format!(
            "scene {number}: \"{word}\" is not a way of coming in. The ways are: cut, fade, slide, wipe, zoom."
        ));
    }
    let ms = match value.as_object() {
        Some(map) => map.get("ms").and_then(Value::as_f64).unwrap_or(450.0),
        None => 450.0,
    };
    let dir = match value.as_object() {
        Some(map) => String::from(
            map.get("dir")
                .and_then(Value::as_str)
                .unwrap_or("left")
                .trim(),
        ),
        None => String::from("left"),
    };
    if !["left", "right", "up", "down"].contains(&dir.as_str()) {
        faults.push(format!(
            "scene {number}: \"{dir}\" is not a way a slide or a wipe runs. The ways are: left, right, up, down."
        ));
    }
    if ms < 0.0 || ms > 4000.0 {
        faults.push(format!(
            "scene {number}: a change of {ms} milliseconds is out of a time. The change stands between 0 and 4000."
        ));
    }
    Transition { kind: word, ms, dir }
}

// The whole of the project as the words of its file.
pub fn words(project: &Project) -> String {
    serde_json::to_string_pretty(project).unwrap_or_else(|_| String::from("{}"))
}

// The project out of the words of its file. Nothing comes back when the file
// does not read, for the caller says so to the model.
pub fn from_words(text: &str) -> Option<Project> {
    serde_json::from_str::<Project>(text).ok()
}

// The storyboard of a project in the words the model and the user read: one line
// per scene, with the number, the head, the time, and the kinds of the blocks.
pub fn storyboard(project: &Project) -> String {
    if project.scenes.is_empty() {
        return String::from("No scene stands in the project yet.");
    }
    let mut out: Vec<String> = Vec::new();
    for (place, scene) in project.scenes.iter().enumerate() {
        let head = if scene.title.is_empty() {
            first_head(scene).unwrap_or_else(|| String::from("(no heading)"))
        } else {
            scene.title.clone()
        };
        let kinds: Vec<&str> = scene.blocks.iter().map(|one| one.kind.as_str()).collect();
        out.push(format!(
            "{}. {} [{} s, {}] {}",
            place + 1,
            head,
            trim_time(scene.seconds),
            if kinds.is_empty() {
                String::from("no block")
            } else {
                kinds.join(", ")
            },
            if scene.note.is_empty() {
                String::new()
            } else {
                format!("-- {}", scene.note)
            }
        ));
    }
    out.join("\n")
}

// The first line of the first head of a scene, for a scene that names no title.
fn first_head(scene: &Scene) -> Option<String> {
    for block in &scene.blocks {
        if block.kind == "head" || block.kind == "quote" {
            let line = block.text.lines().next().unwrap_or_default().trim();
            if !line.is_empty() {
                return Some(String::from(line));
            }
        }
    }
    None
}

// A number of seconds without the nothing after the point.
pub fn trim_time(seconds: f64) -> String {
    if (seconds - seconds.round()).abs() < 0.05 {
        format!("{}", seconds.round() as i64)
    } else {
        format!("{:.1}", seconds)
    }
}

// How long the whole of the video stands.
pub fn seconds(project: &Project) -> f64 {
    project.scenes.iter().map(|one| one.seconds).sum()
}

// The colours of a project: the palette the word names, with every part the
// project puts over it laid on top.
pub fn colours_of(project: &Project) -> Palette {
    let mut made = palette_of(&project.palette).unwrap_or_default();
    let held = &project.colours;
    if !held.bg.is_empty() {
        made.bg = held.bg.clone();
    }
    if !held.panel.is_empty() {
        made.panel = held.panel.clone();
    }
    if !held.ink.is_empty() {
        made.ink = held.ink.clone();
    }
    if !held.muted.is_empty() {
        made.muted = held.muted.clone();
    }
    if !held.line.is_empty() {
        made.line = held.line.clone();
    }
    if !held.accent.is_empty() {
        made.accent = held.accent.clone();
    }
    if !held.accent2.is_empty() {
        made.accent2 = held.accent2.clone();
    }
    if !held.good.is_empty() {
        made.good = held.good.clone();
    }
    if !held.bad.is_empty() {
        made.bad = held.bad.clone();
    }
    if !held.series.is_empty() {
        made.series = held.series.clone();
    }
    made
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_scene_of_one_line_grows_a_head() {
        let mut faults = Vec::new();
        let scene = read_scene(&json!("Why attention costs"), 1, &mut faults).expect("a scene");
        assert!(faults.is_empty(), "{faults:?}");
        assert_eq!(scene.title, "Why attention costs");
        assert_eq!(scene.blocks.len(), 1);
        assert_eq!(scene.blocks[0].kind, "head");
        assert_eq!(scene.blocks[0].text, "Why attention costs");
    }

    #[test]
    fn a_part_written_wrong_names_the_part_meant() {
        let list: Vec<Value> =
            serde_json::from_value(json!([{"kind": "bars", "lablels": ["a"], "values": [1]}]))
                .expect("a list of blocks");
        let bad = block_faults(&list);
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].contains("labels"), "{}", bad[0]);
    }

    #[test]
    fn a_kind_nothing_knows_names_the_kinds() {
        let list: Vec<Value> =
            serde_json::from_value(json!([{"kind": "histogram", "values": [1]}]))
                .expect("a list of blocks");
        let bad = block_faults(&list);
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].contains("bars"), "{}", bad[0]);
    }

    #[test]
    fn a_merge_keeps_what_a_call_does_not_name() {
        let had = Project {
            name: String::from("main"),
            canvas: String::from("720p"),
            fps: 30,
            palette: String::from("forest"),
            scenes: vec![Scene {
                title: String::from("One"),
                seconds: 3.0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let made = merge(&had, &json!({"palette": "neon"})).expect("a project");
        assert_eq!(made.palette, "neon");
        assert_eq!(made.canvas, "720p");
        assert_eq!(made.fps, 30);
        assert_eq!(made.scenes.len(), 1);
    }

    #[test]
    fn a_size_of_its_own_is_taken_and_a_size_out_of_a_time_is_not() {
        assert_eq!(canvas_of("2048x858").map(|one| one.width), Some(2048));
        assert!(canvas_of("64x64").is_none(), "a size below the least is refused");
        assert!(canvas_of("nonsense").is_none(), "a size nothing knows is refused");
        assert_eq!(canvas_of("").map(|one| one.word), Some("1080p"));
    }

    #[test]
    fn a_palette_outside_the_nine_is_told() {
        let had = Project::default();
        let bad = merge(&had, &json!({"palette": "bubblegum"}));
        let faults = bad.unwrap_err();
        assert!(faults[0].contains("graphite"), "{}", faults[0]);
    }

    #[test]
    fn the_script_of_a_project_reads_back_as_the_project() {
        let had = Project {
            name: String::from("main"),
            canvas: String::from("square"),
            fps: 15,
            palette: String::from("paper"),
            font: String::from("serif"),
            scenes: vec![Scene {
                title: String::from("One"),
                seconds: 2.5,
                blocks: vec![Block {
                    kind: String::from("stat"),
                    value: 42.0,
                    text: String::from("tokens"),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let back = from_words(&words(&had)).expect("the script reads");
        assert_eq!(back.canvas, "square");
        assert_eq!(back.scenes[0].blocks[0].value, 42.0);
    }
}
