// How a layer changes while it is on the screen.
//
// A layer holds a set of numbers: where its box stands, how big it is, how turned, how seen, how
// big its letters are, what colour they are, and how much of its content is let out. A frame is
// drawn with one set of those numbers, and `pose` answers with the set for a moment of the film.
//
// Three things move them. The layer's own values are the ground. A motion of arrival or of
// departure - `enter` and `exit` - is a shape the numbers go through at the start and at the end
// of the stretch the layer owns: a title rises into place over half a second, a caption fades out
// before the shot changes. A line of keyframes - `anim` - is the whole of the freedom: the value
// of a property at named moments of the film, with an easing between them. A keyframe is written
// in seconds of the finished film, so the same numbers that say "the graphic arrives at 4.5 s"
// say what the graphic is doing at 5.2 s.
//
// The drawing reads the pose and never looks at a keyframe itself. That is what keeps one rule
// for the order of the moving parts, and why a graphic that arrives and moves by keyframe at once
// is not a contradiction.

use serde_json::Value;

use crate::infographic::story::{self, Layer};

// One number a property may take: a plain number, or a colour as three parts from 0.0 to 1.0.
#[derive(Clone, Copy, Debug)]
enum Sample {
    Number(f64),
    Colour([f64; 3]),
}

// One moment of a line of keyframes.
struct Mark {
    at: f64,
    parts: Vec<Sample>,
}

// Which way a wipe goes, and how far it has come.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edge {
    Left,
    Right,
    Up,
    Down,
}

// The numbers a frame is drawn with.
#[derive(Clone, Debug)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rot: f64,
    pub opacity: f64,
    // The size of the letters, in parts of the height of the canvas.
    pub size: f64,
    // The colour of the content, when a keyframe or the style gave one.
    pub colour: Option<String>,
    // The colour of a fill of a shape, when one was given.
    pub fill: Option<String>,
    pub stroke_size: f64,
    // How much of the content of the layer is let out: 1.0 is all of it.
    pub reveal: f64,
    pub volume: f64,
    pub crop: [f64; 4],
    // A wipe, which the compositor cuts into the pixels of the layer.
    pub wipe: Option<(Edge, f64)>,
    // The turn of a scale about the centre of the box, for the motions that grow and shrink.
    pub scale: f64,
}

// The pose of a layer at a moment of the film.
pub fn pose(layer: &Layer, at: f64, whole: f64) -> Pose {
    let mut made = base(layer);
    // The arrival, then the departure, then the keyframes: a model that writes a keyframe for a
    // property means that keyframe, so it has the last word.
    if let Some(motion) = &layer.enter {
        let done = moving(at - layer.from, motion.seconds);
        if done < 1.0 {
            apply(&mut made, motion, 1.0 - ease_for(&motion.ease)(done));
        }
    }
    if let Some(motion) = &layer.exit {
        let end = if layer.to > 0.0 { layer.to } else { whole };
        let done = 1.0 - moving(end - at, motion.seconds);
        if done > 0.0 {
            apply(&mut made, motion, ease_for(&motion.ease)(done));
        }
    }
    for key in &layer.anim {
        if let Some(parts) = sample_at(key, at) {
            set(&mut made, &key.prop, &parts);
        }
    }
    made
}

// The numbers of the layer as it stands, with the defaults the style did not name.
fn base(layer: &Layer) -> Pose {
    Pose {
        x: layer.x,
        y: layer.y,
        w: layer.w,
        h: layer.h,
        rot: layer.rot,
        opacity: layer.opacity,
        size: layer.style.size,
        colour: Some(layer.style.colour.clone()).filter(|one| !one.is_empty()),
        fill: Some(layer.style.fill.clone()).filter(|one| !one.is_empty()),
        stroke_size: if layer.style.stroke_size > 0.0 {
            layer.style.stroke_size
        } else {
            layer.style.stroke_width
        },
        reveal: if layer.style.reveal > 0.0 { layer.style.reveal } else { 1.0 },
        volume: layer.volume,
        crop: [layer.crop_l, layer.crop_t, layer.crop_r, layer.crop_b],
        wipe: None,
        scale: 1.0,
    }
}

// How far a motion has to go: 0.0 is the beginning of it, 1.0 the end.
fn moving(into: f64, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        return 0.0;
    }
    (into / seconds).clamp(0.0, 1.0)
}

// The state of a motion, as how much of the "away" state is still on the layer: 0.0 is the layer
// at rest, 1.0 is the layer as far out of its place as the motion takes it. An arrival begins
// fully away and ends at rest; a departure begins at rest and ends fully away, which is why one
// function serves both.
fn apply(made: &mut Pose, motion: &story::Motion, away: f64) {
    match motion.kind.as_str() {
        "fade" => made.opacity *= 1.0 - away,
        "rise" => made.y += motion.distance * away,
        "drop" => made.y -= motion.distance * away,
        "left" => made.x -= motion.distance * away,
        "right" => made.x += motion.distance * away,
        "grow" => made.scale = 1.0 + 0.25 * away,
        "shrink" => made.scale = 1.0 - 0.25 * away,
        "pop" => {
            // A little more than the size it is going to, so the layer lands with a beat.
            made.scale = 1.0 + 0.35 * away - 0.35 * away * away;
        }
        "wipe-left" => made.wipe = Some((Edge::Left, 1.0 - away)),
        "wipe-right" => made.wipe = Some((Edge::Right, 1.0 - away)),
        "wipe-up" => made.wipe = Some((Edge::Up, 1.0 - away)),
        "wipe-down" => made.wipe = Some((Edge::Down, 1.0 - away)),
        "type" | "rows" | "values" => made.reveal *= 1.0 - away,
        _ => {}
    }
}

// The value of a line of keyframes at a moment.
fn sample_at(key: &story::Key, at: f64) -> Option<Vec<Sample>> {
    let marks = read(key)?;
    if marks.is_empty() {
        return None;
    }
    if at <= marks.first()?.at {
        return Some(marks.first()?.parts.clone());
    }
    if at >= marks.last()?.at {
        return Some(marks.last()?.parts.clone());
    }
    for pair in marks.windows(2) {
        let (one, next) = (&pair[0], &pair[1]);
        if at < one.at || at > next.at {
            continue;
        }
        let span = next.at - one.at;
        let part = if span <= 0.0 { 1.0 } else { (at - one.at) / span };
        let eased = ease_for(&key.ease)(part);
        let mut made: Vec<Sample> = Vec::new();
        for place in 0..one.parts.len().min(next.parts.len()) {
            made.push(mix(one.parts[place], next.parts[place], eased));
        }
        if !made.is_empty() {
            return Some(made);
        }
    }
    None
}

// The moments of a line, in the order they were written. A keyframe out of order is put right.
fn read(key: &story::Key) -> Option<Vec<Mark>> {
    let mut marks: Vec<Mark> = Vec::new();
    for row in &key.keys {
        let parts: Vec<Value> = match row {
            Value::Array(rows) => rows.clone(),
            _ => vec![row.clone()],
        };
        if parts.len() < 2 {
            continue;
        }
        let at = number(&parts[0])?;
        let mut held: Vec<Sample> = Vec::new();
        for one in &parts[1..] {
            match one {
                Value::String(words) => {
                    if let Some(rgb) = colour_of(words) {
                        held.push(Sample::Colour(rgb));
                    } else if let Ok(value) = words.trim().parse::<f64>() {
                        held.push(Sample::Number(value));
                    }
                }
                Value::Array(rows) => {
                    for part in rows.iter().filter_map(number) {
                        held.push(Sample::Number(part));
                    }
                }
                _ => {
                    if let Some(value) = number(one) {
                        held.push(Sample::Number(value));
                    }
                }
            }
        }
        if held.is_empty() {
            continue;
        }
        marks.push(Mark { at, parts: held });
    }
    marks.sort_by(|one, other| one.at.partial_cmp(&other.at).unwrap_or(std::cmp::Ordering::Equal));
    Some(marks)
}

fn number(given: &Value) -> Option<f64> {
    match given {
        Value::Number(one) => one.as_f64(),
        Value::String(words) => words.trim().parse::<f64>().ok(),
        _ => None,
    }
}

// Two samples of the same shape, mixed by a part.
fn mix(one: Sample, next: Sample, part: f64) -> Sample {
    match (one, next) {
        (Sample::Number(a), Sample::Number(b)) => Sample::Number(a + (b - a) * part),
        (Sample::Colour(a), Sample::Colour(b)) => Sample::Colour([
            a[0] + (b[0] - a[0]) * part,
            a[1] + (b[1] - a[1]) * part,
            a[2] + (b[2] - a[2]) * part,
        ]),
        // A number and a colour are not the same shape. The nearer of the two stands.
        (a, _) if part < 0.5 => a,
        (_, b) => b,
    }
}

// The value of a line put where the property of the layer lives.
fn set(made: &mut Pose, prop: &str, parts: &[Sample]) {
    let first = parts.first().copied();
    let value = match first {
        Some(Sample::Number(one)) => Some(one),
        _ => None,
    };
    let colour = match first {
        Some(Sample::Colour(rgb)) => Some(rgb_text(rgb)),
        _ => None,
    };
    match prop {
        "x" => made.x = value.unwrap_or(made.x),
        "y" => made.y = value.unwrap_or(made.y),
        "w" => made.w = value.unwrap_or(made.w).max(0.001),
        "h" => made.h = value.unwrap_or(made.h).max(0.001),
        "scale" => made.scale = value.unwrap_or(made.scale),
        "rot" => made.rot = value.unwrap_or(made.rot),
        "opacity" => made.opacity = value.unwrap_or(made.opacity).clamp(0.0, 1.0),
        "size" => made.size = value.unwrap_or(made.size),
        "stroke_size" => made.stroke_size = value.unwrap_or(made.stroke_size),
        "reveal" => made.reveal = value.unwrap_or(made.reveal).clamp(0.0, 1.0),
        "volume" => made.volume = value.unwrap_or(made.volume).clamp(0.0, 4.0),
        "radius" | "pad" => {}
        "colour" => {
            if let Some(said) = colour {
                made.colour = Some(said);
            } else if let Some(one) = value {
                made.colour = Some(rgb_text([one, one, one]));
            }
        }
        "fill" => {
            if let Some(said) = colour {
                made.fill = Some(said);
            }
        }
        "crop" => {
            if parts.len() >= 4 {
                let four: Vec<f64> = parts
                    .iter()
                    .filter_map(|one| match one {
                        Sample::Number(value) => Some(*value),
                        Sample::Colour(_) => None,
                    })
                    .take(4)
                    .collect();
                if four.len() == 4 {
                    made.crop = [four[0], four[1], four[2], four[3]];
                }
            } else if let Some(one) = value {
                made.crop = [one, one, one, one];
            }
        }
        _ => {}
    }
}

// A colour of three parts as the text a style carries.
fn rgb_text(rgb: [f64; 3]) -> String {
    let part = |one: f64| (one.clamp(0.0, 1.0) * 255.0).round() as i32;
    format!("#{:02x}{:02x}{:02x}", part(rgb[0]), part(rgb[1]), part(rgb[2]))
}

// A colour as three parts, from a name, a hex of three or six digits, or "auto".
pub fn colour_of(words: &str) -> Option<[f64; 3]> {
    let said = words.trim().to_ascii_lowercase();
    if said.is_empty() {
        return None;
    }
    const NAMED: &[(&str, [u8; 3])] = &[
        ("black", [0, 0, 0]),
        ("white", [255, 255, 255]),
        ("red", [220, 38, 38]),
        ("crimson", [190, 24, 93]),
        ("orange", [234, 88, 12]),
        ("amber", [217, 119, 6]),
        ("yellow", [234, 179, 8]),
        ("green", [22, 163, 74]),
        ("lime", [77, 124, 15]),
        ("teal", [13, 148, 136]),
        ("cyan", [8, 145, 178]),
        ("blue", [37, 99, 235]),
        ("sky", [2, 132, 199]),
        ("indigo", [79, 70, 229]),
        ("violet", [124, 58, 237]),
        ("purple", [147, 51, 234]),
        ("pink", [219, 39, 119]),
        ("brown", [120, 72, 40]),
        ("grey", [107, 114, 128]),
        ("gray", [107, 114, 128]),
        ("silver", [190, 190, 195]),
        ("gold", [202, 138, 4]),
    ];
    if let Some((_, rgb)) = NAMED.iter().find(|(one, _)| *one == said) {
        return Some([rgb[0] as f64 / 255.0, rgb[1] as f64 / 255.0, rgb[2] as f64 / 255.0]);
    }
    let hex = said.strip_prefix('#').unwrap_or(&said);
    let digits = (0..hex.len()).step_by(2).filter_map(|at| {
        hex.get(at..at + 2).and_then(|one| u8::from_str_radix(one, 16).ok())
    });
    let parts: Vec<u8> = digits.collect();
    match parts.len() {
        3 => Some([parts[0] as f64 / 255.0, parts[1] as f64 / 255.0, parts[2] as f64 / 255.0]),
        6 => Some([parts[0] as f64 / 255.0, parts[1] as f64 / 255.0, parts[2] as f64 / 255.0]),
        _ => None,
    }
}

// The easing of a name. An easing nothing knows moves in a straight line.
pub fn ease_for(name: &str) -> fn(f64) -> f64 {
    match name {
        "smooth" => smooth,
        "ease" | "ease-in-out" => ease_in_out,
        "ease-in" | "quad-in" => quad_in,
        "ease-out" | "quad-out" => quad_out,
        "cubic-in" => cubic_in,
        "cubic-out" => cubic_out,
        "cubic-in-out" => cubic_in_out,
        "expo-in" => expo_in,
        "expo-out" => expo_out,
        "back-in" => back_in,
        "back-out" => back_out,
        "elastic-out" => elastic_out,
        "bounce-out" => bounce_out,
        "hold" => hold,
        _ => straight,
    }
}

fn straight(part: f64) -> f64 {
    part.clamp(0.0, 1.0)
}

fn hold(part: f64) -> f64 {
    if part >= 1.0 {
        1.0
    } else {
        0.0
    }
}

fn smooth(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    p * p * (3.0 - 2.0 * p)
}

fn quad_in(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    p * p
}

fn quad_out(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    1.0 - (1.0 - p) * (1.0 - p)
}

fn ease_in_out(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    if p < 0.5 {
        2.0 * p * p
    } else {
        1.0 - ((-2.0 * p + 2.0).powi(2)) / 2.0
    }
}

fn cubic_in(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    p * p * p
}

fn cubic_out(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    1.0 - (1.0 - p).powi(3)
}

fn cubic_in_out(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    if p < 0.5 {
        4.0 * p * p * p
    } else {
        1.0 - (-2.0 * p + 2.0).powi(3) / 2.0
    }
}

fn expo_in(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    if p <= 0.0 {
        0.0
    } else {
        2.0f64.powf(10.0 * p - 10.0)
    }
}

fn expo_out(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    if p >= 1.0 {
        1.0
    } else {
        1.0 - 2.0f64.powf(-10.0 * p)
    }
}

fn back_in(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    let c1 = 1.70158f64;
    let c3 = c1 + 1.0;
    c3 * p * p * p - c1 * p * p
}

fn back_out(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    let c1 = 1.70158f64;
    let c3 = c1 + 1.0;
    1.0 + c3 * (p - 1.0).powi(3) + c1 * (p - 1.0).powi(2)
}

fn elastic_out(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    if p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }
    let c4 = (2.0 * std::f64::consts::PI) / 3.0;
    2.0f64.powf(-10.0 * p) * ((p * 10.0 - 0.75) * c4).sin() + 1.0
}

fn bounce_out(part: f64) -> f64 {
    let p = part.clamp(0.0, 1.0);
    let n1 = 7.5625f64;
    let d1 = 2.75f64;
    if p < 1.0 / d1 {
        n1 * p * p
    } else if p < 2.0 / d1 {
        let p = p - 1.5 / d1;
        n1 * p * p + 0.75
    } else if p < 2.5 / d1 {
        let p = p - 2.25 / d1;
        n1 * p * p + 0.9375
    } else {
        let p = p - 2.625 / d1;
        n1 * p * p + 0.984375
    }
}

// The words a model is given for the easings, for the prompt of the suite.
pub fn ease_names() -> String {
    story::EASES.join(", ")
}

#[cfg(test)]
mod checks {
    use serde_json::json;

    use crate::infographic::story::{Motion, Style, Layer};

    use super::*;

    fn key(prop: &str, rows: Value, ease: &str) -> story::Key {
        story::Key {
            prop: String::from(prop),
            keys: rows.as_array().cloned().unwrap_or_default(),
            ease: String::from(ease),
        }
    }

    // A keyframe holds its first value before its first moment and its last after the last.
    #[test]
    fn a_keyframe_holds_its_ends() {
        let line = key("y", json!([[1.0, 0.9], [2.0, 0.5]]), "linear");
        assert_eq!(value_of(&line, 0.0), Some(0.9));
        assert_eq!(value_of(&line, 1.5), Some(0.7));
        assert_eq!(value_of(&line, 9.0), Some(0.5));
    }

    fn value_of(one: &story::Key, at: f64) -> Option<f64> {
        match sample_at(one, at)?.first()? {
            Sample::Number(one) => Some(*one),
            Sample::Colour(_) => None,
        }
    }

    // The pose of a still layer is the layer.
    #[test]
    fn a_still_layer_is_its_own_pose() {
        let layer = Layer {
            from: 0.0,
            to: 4.0,
            x: 0.4,
            y: 0.25,
            w: 0.5,
            h: 0.1,
            opacity: 0.8,
            style: Style { size: 0.05, colour: String::from("#c2410c"), ..Style::default() },
            ..Layer::default()
        };
        let made = pose(&layer, 2.0, 4.0);
        assert_eq!((made.x, made.y, made.w, made.h), (0.4, 0.25, 0.5, 0.1));
        assert!((made.opacity - 0.8).abs() < 1e-9);
        assert_eq!(made.size, 0.05);
        assert_eq!(made.colour.as_deref(), Some("#c2410c"));
        assert_eq!(made.wipe, None);
    }

    // An arrival of rise lifts the layer from below and lets it settle at the end of its seconds.
    #[test]
    fn a_layer_rises_into_its_place() {
        let layer = Layer {
            from: 1.0,
            to: 5.0,
            y: 0.5,
            enter: Some(Motion {
                kind: String::from("rise"),
                seconds: 0.5,
                ease: String::from("linear"),
                distance: 0.1,
            }),
            ..Layer::default()
        };
        let begun = pose(&layer, 1.0, 5.0);
        let ended = pose(&layer, 1.5, 5.0);
        assert!((begun.y - 0.6).abs() < 1e-9, "{}", begun.y);
        assert!((ended.y - 0.5).abs() < 1e-9, "{}", ended.y);
    }

    // A departure takes the opacity away in the last seconds of the stretch.
    #[test]
    fn a_layer_fades_away_before_it_ends() {
        let layer = Layer {
            from: 0.0,
            to: 3.0,
            exit: Some(Motion {
                kind: String::from("fade"),
                seconds: 1.0,
                ease: String::from("linear"),
                ..Motion::default()
            }),
            ..Layer::default()
        };
        assert!((pose(&layer, 2.0, 3.0).opacity - 1.0).abs() < 1e-9);
        assert!((pose(&layer, 2.5, 3.0).opacity - 0.5).abs() < 1e-9);
        assert!(pose(&layer, 3.0, 3.0).opacity.abs() < 1e-9);
    }

    // A wipe says which edge it uncovers from and how far it has come.
    #[test]
    fn a_wipe_reports_its_edge() {
        let layer = Layer {
            from: 0.0,
            to: 2.0,
            enter: Some(Motion {
                kind: String::from("wipe-right"),
                seconds: 1.0,
                ease: String::from("linear"),
                ..Motion::default()
            }),
            ..Layer::default()
        };
        let made = pose(&layer, 0.25, 2.0);
        assert_eq!(made.wipe, Some((Edge::Right, 0.25)));
    }

    // A keyframe outranks a motion of the same property.
    #[test]
    fn a_keyframe_has_the_last_word() {
        let mut layer = Layer {
            from: 0.0,
            to: 4.0,
            y: 0.5,
            enter: Some(Motion {
                kind: String::from("rise"),
                seconds: 1.0,
                ease: String::from("linear"),
                distance: 0.4,
            }),
            ..Layer::default()
        };
        layer.anim.push(key("y", json!([[0.0, 0.1], [2.0, 0.3]]), "linear"));
        assert!((pose(&layer, 1.0, 4.0).y - 0.2).abs() < 1e-9);
    }

    // A colour moves between two colours, and a crop moves as four numbers.
    #[test]
    fn a_colour_and_a_crop_move_too() {
        let colour = key("colour", json!([[0.0, "#000000"], [1.0, "#ffffff"]]), "linear");
        let parts = sample_at(&colour, 0.5).expect("a colour is answered");
        match parts.first() {
            Some(Sample::Colour(rgb)) => assert!((rgb[0] - 0.5).abs() < 1e-6, "{rgb:?}"),
            other => panic!("a colour was expected, {other:?}"),
        }
        let crop = key(
            "crop",
            json!([[0.0, [0.0, 0.0, 0.0, 0.0]], [1.0, [0.4, 0.0, 0.0, 0.0]]]),
            "linear",
        );
        let mut layer = Layer { from: 0.0, to: 1.0, ..Layer::default() };
        layer.anim.push(crop);
        let made = pose(&layer, 0.5, 1.0);
        assert!((made.crop[0] - 0.2).abs() < 1e-9, "{:?}", made.crop);
    }

    // The easings keep their ends, and the wild ones go past them where they are meant to.
    #[test]
    fn the_easings_keep_their_ends() {
        for name in story::EASES {
            let made = ease_for(name);
            assert!((made(0.0)).abs() < 1e-6 || *name == "hold", "{name} does not start at nought");
            if *name == "hold" {
                assert!((made(0.5)).abs() < 1e-9);
                assert!((made(1.0) - 1.0).abs() < 1e-9);
                continue;
            }
            assert!((made(1.0) - 1.0).abs() < 1e-6, "{name} does not end at one");
        }
        assert!(back_out(0.6) > 1.0, "back-out goes past the end");
    }

    // A name of a colour, and the hex of it, are the same colour.
    #[test]
    fn a_colour_is_named_or_hexed() {
        assert_eq!(colour_of("white"), Some([1.0, 1.0, 1.0]));
        assert_eq!(colour_of("#c2410c"), colour_of("#C2410C"));
        assert!(colour_of("auto").is_none());
        assert!(colour_of("").is_none());
    }
}
