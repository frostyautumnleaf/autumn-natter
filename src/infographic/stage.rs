// The laying of one frame: what stands under what, and what happens to the pixels before they are
// laid.
//
// A frame is built in the order the layers are drawn. The ground of the film is laid first, and
// then every layer that is on the screen at the moment, from the lowest of them to the highest:
// its pixels are made, its effects are worked on them - a key colour taken out, an edge softened,
// a blur, a mask cut, a shadow thrown - and they are laid on the frame with the turn, the scale,
// the fading and the blending the pose of the layer carries.
//
// Two kinds of pixels come by two ways. Words, tables, charts and shapes are drawn as a document
// of SVG and painted into pixels by `paint`. A photograph, a film or a sound comes from a file,
// through `media`. After that the two are treated the same, which is why a caption and a face cut
// out of a green screen may both be turned, dimmed, wiped and blended by the same words.
//
// The work of a run is kept in `Shots`: the pictures that were read, the films whose readers are
// open, the colour a key was found to be, and the last drawing of every layer. Frames are drawn in
// the order of the film, so a film is read forward, a picture is read once, and a layer whose pose
// has not moved is not drawn again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use resvg::tiny_skia::{BlendMode, Color, FilterQuality, Pixmap, PixmapPaint, Transform};

use crate::infographic::anim::{self, Edge, Pose};
use crate::infographic::encode;
use crate::infographic::fonts::Fonts;
use crate::infographic::media::{self, Frame};
use crate::infographic::paint;
use crate::infographic::story::{self, Layer, Palette};

// The work of drawing a whole film, kept between the frames.
#[derive(Default)]
pub struct Shots {
    // The still pictures that were read, by the name they were read from.
    stills: HashMap<String, Option<Frame>>,
    // The readers of the films, by the name they were opened from.
    clips: HashMap<String, media::Clip>,
    // What every file was found to be.
    kinds: HashMap<String, media::Media>,
    // The colour a key of a file was found in, when the call said "auto".
    keys: HashMap<String, Key>,
    // The last drawing of a layer, and the words it was drawn from.
    drawn: HashMap<String, (String, Pixmap)>,
    // What went wrong, once per file, so a fault is not repeated for every frame.
    told: HashMap<String, String>,
    // The faults found since the last asking.
    notes: Vec<String>,
    // The longest side a picture is read at, which is the side of the screen it is drawn on.
    cap: u32,
}

impl Shots {
    // Nothing read, nothing drawn.
    pub fn new() -> Self {
        Shots::default()
    }

    // What was found wrong with the pixels since the frames were last drawn.
    pub fn notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    // The measure the pixels of the files are read at, which is the side of the screen they are
    // drawn on. A film of 4k is not decoded at 4k to be shrunk to a 720p screen.
    pub fn set_cap(&mut self, cap: u32) {
        if cap > 0 && self.cap != cap {
            self.cap = cap;
            // The readers and the pictures already read were made at another measure, so they go.
            self.stills.clear();
            self.clips.clear();
        }
    }
}

// One layer, in the pixels it is laid with, and the size it is laid at.
struct Laid {
    pixmap: Pixmap,
    // The size the pixels are drawn at, in pixels of the screen.
    width: f64,
    height: f64,
}

// The path of a file a layer names: an absolute path as it stands, and any other name inside the
// folder of the composition. The composition stores each file name in the form the file was found
// in, so this function does not search.
pub fn media_key(room: &Path, given: &str) -> PathBuf {
    let named = given.trim();
    let path = Path::new(named);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    room.join(named)
}

// One frame of a film, at a moment of it.
pub fn frame(
    project: &story::Project,
    fonts: &Fonts,
    room: &Path,
    at: f64,
    shots: &mut Shots,
) -> Result<Pixmap, String> {
    let (width, height) = story::size_of(project);
    let whole = story::seconds(project);
    let palette = story::colours_of(project);
    shots.cap = width.max(height);
    let mut canvas = Pixmap::new(width, height)
        .ok_or_else(|| "the canvas is too big for this computer".to_string())?;
    ground(project, &palette, fonts, room, width, height, shots, &mut canvas)?;
    for place in story::ordered(project) {
        let layer = &project.layers[place];
        if !story::shows(layer, at, whole) {
            continue;
        }
        if layer.kind == "audio" {
            continue;
        }
        let pose = anim::pose(layer, at, whole);
        if pose.opacity <= 0.0 {
            continue;
        }
        let key = media_key(room, &layer.media);
        match laid(project, layer, &pose, (width, height), &palette, fonts, &key, room, at, shots)
        {
            Ok(Some(made)) => lay(&mut canvas, &made, &pose, layer),
            Ok(None) => {}
            Err(why) => {
                // A layer that cannot be drawn is said once, and the film goes on without it.
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    shots.told.entry(layer.id.clone())
                {
                    let said = format!("{}: {why}", story::name_of(layer));
                    entry.insert(said.clone());
                    shots.notes.push(said);
                }
            }
        }
    }
    Ok(canvas)
}

// The ground every layer stands on.
fn ground(
    project: &story::Project,
    palette: &Palette,
    fonts: &Fonts,
    room: &Path,
    width: u32,
    height: u32,
    shots: &mut Shots,
    canvas: &mut Pixmap,
) -> Result<(), String> {
    let ground = &project.background;
    let held = if ground.colour.is_empty() {
        palette.ground.clone()
    } else {
        ground.colour.clone()
    };
    match ground.kind.as_str() {
        "none" => canvas.fill(Color::from_rgba8(0, 0, 0, 0)),
        "gradient" => {
            let svg = gradient(
                &if ground.from.is_empty() { held.clone() } else { ground.from.clone() },
                &if ground.to.is_empty() { palette.ink.clone() } else { ground.to.clone() },
                ground.angle,
                width,
                height,
            );
            let made = encode::paint(&svg, width, height, fonts)?;
            canvas.data_mut().copy_from_slice(made.data());
        }
        "image" => {
            if ground.media.trim().is_empty() {
                return Err("the ground is a picture but no file was named".to_string());
            }
            let path = media_key(room, &ground.media);
            let read = shots.still(&path)?;
            let made = match read {
                Some(one) => one,
                None => return Ok(()),
            };
            let pixmap = made.to_pixmap().ok_or_else(|| {
                "the picture of the ground is too big for this computer".to_string()
            })?;
            let scale = (width as f64 / pixmap.width() as f64)
                .max(height as f64 / pixmap.height() as f64);
            let paint = PixmapPaint {
                quality: FilterQuality::Bilinear,
                ..PixmapPaint::default()
            };
            let transform = Transform::from_scale(scale as f32, scale as f32);
            canvas.draw_pixmap(0, 0, pixmap.as_ref(), &paint, transform, None);
        }
        // A solid ground, and a ground nothing was said about.
        _ => {
            let colour = colour_of(&held).unwrap_or_else(|| Color::from_rgba8(0, 0, 0, 0));
            canvas.fill(colour);
        }
    }
    if ground.vignette > 0.0 {
        let svg = vignette(ground.vignette, width, height);
        let made = encode::paint(&svg, width, height, fonts)?;
        canvas.draw_pixmap(
            0,
            0,
            made.as_ref(),
            &PixmapPaint::default(),
            Transform::default(),
            None,
        );
    }
    Ok(())
}

// A gradient of two colours across the screen, at an angle.
fn gradient(from: &str, to: &str, angle: f64, width: u32, height: u32) -> String {
    let angle = if angle == 0.0 { 90.0 } else { angle };
    let radians = (angle - 90.0).to_radians();
    let half = ((width as f64 * radians.abs()).max(height as f64 * radians.abs())) / 2.0;
    let (cx, cy) = (width as f64 / 2.0, height as f64 / 2.0);
    let (x1, y1) = (cx - radians.cos() * half, cy - radians.sin() * half);
    let (x2, y2) = (cx + radians.cos() * half, cy + radians.sin() * half);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\"><defs><linearGradient id=\"g\" x1=\"{x1:.1}\" \
         y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\"><stop offset=\"0\" stop-color=\"{from}\"/>\
         <stop offset=\"1\" stop-color=\"{to}\"/></linearGradient></defs>\
         <rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"url(#g)\"/></svg>"
    )
}

// A darkening towards the corners of the screen.
fn vignette(part: f64, width: u32, height: u32) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\"><defs><radialGradient id=\"v\" cx=\"0.5\" cy=\"0.5\" \
         r=\"0.75\"><stop offset=\"0.55\" stop-color=\"#000000\" stop-opacity=\"0\"/>\
         <stop offset=\"1\" stop-color=\"#000000\" stop-opacity=\"{:.3}\"/></radialGradient>\
         </defs><rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"url(#v)\"/></svg>",
        part.clamp(0.0, 1.0)
    )
}

// The pixels of one layer, with its effects worked on them. Nothing is answered for a layer whose
// content is not there yet, and a fault says why.
fn laid(
    project: &story::Project,
    layer: &Layer,
    pose: &Pose,
    canvas: (u32, u32),
    palette: &Palette,
    fonts: &Fonts,
    key: &Path,
    _room: &Path,
    at: f64,
    shots: &mut Shots,
) -> Result<Option<Laid>, String> {
    let (width, height) = canvas;
    let box_width = (pose.w * width as f64).max(1.0);
    let box_height = (pose.h * height as f64).max(1.0);
    let content = match layer.kind.as_str() {
        "image" | "video" => media_laid(layer, key, box_width, box_height, shots, at)?,
        _ => drawn_laid(layer, pose, box_width, box_height, canvas, palette, fonts, shots)?,
    };
    let Some(mut content) = content else { return Ok(None) };
    let radius = (height as f64).max(1.0);
    for effect in &layer.effects {
        match effect.kind.as_str() {
            "chroma" => {
                let found = shots.key_of(&layer.media, key, &effect.colour, &content)?;
                media::key_out(
                    &mut content,
                    found.colour,
                    if effect.similarity > 0.0 { effect.similarity } else { found.similarity },
                    if effect.blend > 0.0 { effect.blend } else { found.blend },
                    if effect.spill > 0.0 { effect.spill } else { 0.6 },
                );
                let feather = effect.feather.max(if effect.blend > 0.0 { 0.0 } else { 1.0 });
                media::feather(&mut content, feather as u32);
            }
            "despill" => {
                let found = shots.key_of(&layer.media, key, &effect.colour, &content)?;
                media::despill(&mut content, found.colour, if effect.strength > 0.0 { effect.strength } else { 0.6 });
            }
            "blur" => media::blur(&mut content, (effect.radius * radius) as u32),
            "grey" | "invert" | "brighten" | "contrast" | "tint" => {
                let colour = colour_parts(&effect.colour).unwrap_or([0.5, 0.5, 0.5]);
                media::adjust(&mut content, &effect.kind, colour, if effect.strength == 0.0 { 1.0 } else { effect.strength });
            }
            "sharpen" => sharpen(&mut content, if effect.strength == 0.0 { 0.6 } else { effect.strength }),
            "mask" => {
                let svg = mask_svg(effect, content.width, content.height, radius);
                let soft = (effect.feather * radius) as u32;
                media::shape_mask(&mut content, &svg, fade_of(effect), soft)?;
            }            "edge" => {
                let soft = (if effect.feather > 0.0 { effect.feather } else { 0.004 } * radius) as u32;
                media::feather(&mut content, soft.max(1));
            }
            // A shadow is laid under the layer, which is the work of `lay`, and needs no pixel
            // work here.
            "shadow" => {}
            other => return Err(format!("\"{other}\" is not an effect")),
        }
    }
    let _ = project;
    // The pixels are drawn at the size the fit means, which is the box for a picture that is
    // stretched or cut to it, and the box within the picture keeps its shape.
    let (drawn_w, drawn_h) = match layer.kind.as_str() {
        "image" | "video" => drawn_size(&content, box_width, box_height, &layer.fit),
        _ => (box_width, box_height),
    };
    Ok(Some(Laid {
        width: drawn_w,
        height: drawn_h,
        pixmap: content.to_pixmap().ok_or_else(|| {
            format!(
                "{} is too big to draw on this computer",
                story::name_of(layer)
            )
        })?,
    }))
}

// The pixels of a layer of words, numbers or shapes.
fn drawn_laid(
    layer: &Layer,
    pose: &Pose,
    box_width: f64,
    box_height: f64,
    canvas: (u32, u32),
    palette: &Palette,
    fonts: &Fonts,
    shots: &mut Shots,
) -> Result<Option<Frame>, String> {
    let width = (box_width.round().max(1.0)) as u32;
    let height = (box_height.round().max(1.0)) as u32;
    // Words do not change between the frames of a turn, so a layer that was drawn from the same
    // words with the same pose is not painted again.
    let sheet = paint::draw(layer, pose, width, height, canvas, fonts, palette);
    let signature = sheet.svg.clone();
    if let Some((held, made)) = shots.drawn.get(&layer.id) {
        if *held == signature {
            return Ok(Some(Frame::from_pixmap(made)));
        }
    }
    let made = encode::paint(&sheet.svg, width, height, fonts)?;
    shots.drawn.insert(layer.id.clone(), (signature, made.clone()));
    Ok(Some(Frame::from_pixmap(&made)))
}

// The pixels of a layer of a photograph or of a film, made ready for the box it stands in.
fn media_laid(
    layer: &Layer,
    key: &Path,
    box_width: f64,
    box_height: f64,
    shots: &mut Shots,
    at: f64,
) -> Result<Option<Frame>, String> {
    if layer.media.trim().is_empty() {
        return Err("the layer names no file".to_string());
    }
    let info = shots.kind(key)?;
    let held = match info.kind {
        media::Kind::Picture => shots
            .still(key)?
            .ok_or_else(|| format!("{} could not be read", key.display()))?,
        media::Kind::Film => {
            let rate = shots.rate(key)?;
            let speed = if layer.speed.abs() < 0.01 { 1.0 } else { layer.speed.abs() };
            let clip = shots.clip(key, rate * speed)?;
            let source = source_of(layer, at, info.duration);
            // A film that has run out holds its last frame while the layer stands, unless the
            // layer said it holds nothing. A layer that holds nothing draws nothing, which is
            // how a cut to black in the middle of a shot is made.
            if !layer.hold && info.duration > 0.0 && source >= info.duration - 0.02 {
                return Ok(None);
            }
            match clip.frame(source)? {
                Some(one) => one,
                None => return Ok(None),
            }
        }
        media::Kind::Sound => return Err("a sound draws nothing".to_string()),
    };
    // The parts of the media that are cut away, and then the fit of what remains into the box.
    let cut = if layer.crop_l > 0.0
        || layer.crop_t > 0.0
        || layer.crop_r > 0.0
        || layer.crop_b > 0.0
    {
        crop(
            &held,
            layer.crop_l,
            layer.crop_t,
            layer.crop_r,
            layer.crop_b,
        )
    } else {
        held
    };
    Ok(Some(fit(&cut, box_width, box_height, &layer.fit)))
}

// The second of a film a layer is reading at a moment of the screen. A film that is shorter than
// the layer that reads it is held at its end, unless the layer asked for it to run round again.
// Where the media itself should stop, what was read, is told by `clip`, which does not run past
// the end of the file.
pub fn source_of(layer: &Layer, at: f64, duration: f64) -> f64 {
    let speed = if layer.speed.abs() < 0.01 { 1.0 } else { layer.speed.abs() };
    let wanted = layer.in_at + (at - layer.from) * speed;
    let last = if layer.out_at > layer.in_at {
        layer.out_at
    } else if duration > 0.0 {
        duration
    } else {
        wanted
    };
    if layer.loop_media && last > layer.in_at {
        let span = last - layer.in_at;
        let round = ((wanted - layer.in_at) % span + span) % span;
        return layer.in_at + round;
    }
    wanted.min(last)
}

// How a picture stands in its box: the whole of it within the box, the whole of it across the box
// with the overhang cut away, or the whole of it stretched to the sides of the box.
fn fit(frame: &Frame, box_width: f64, box_height: f64, said: &str) -> Frame {
    match said {
        // The picture is drawn at the size of the box, whatever its shape.
        "stretch" => frame.clone(),
        // The shape of the box is cut out of the middle of the picture, so what is drawn fills
        // the box and nothing of the picture hangs over its sides.
        "cover" => {
            let (width, height) = (frame.width as f64, frame.height as f64);
            let want = box_width / box_height.max(1.0);
            let held = width / height.max(1.0);
            if held > want {
                // The picture is wider across than the box: the middle of it is kept.
                let keep = (height * want).min(width);
                let away = (width - keep) / 2.0 / width;
                crop(frame, away, 0.0, away, 0.0)
            } else {
                let keep = (width / want).min(height);
                let away = (height - keep) / 2.0 / height;
                crop(frame, 0.0, away, 0.0, away)
            }
        }
        // The whole of the picture, made as big as the box allows.
        _ => frame.clone(),
    }
}

// The size the pixels of a layer are drawn at, in pixels of the screen.
fn drawn_size(frame: &Frame, box_width: f64, box_height: f64, said: &str) -> (f64, f64) {
    match said {
        "stretch" | "cover" => (box_width, box_height),
        _ => {
            let scale = (box_width / frame.width as f64).min(box_height / frame.height as f64);
            (
                frame.width as f64 * scale,
                frame.height as f64 * scale,
            )
        }
    }
}

// The parts of a picture, with the four sides taken away by the parts named.
fn crop(frame: &Frame, left: f64, top: f64, right: f64, bottom: f64) -> Frame {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let x0 = ((left * width as f64).round() as usize).min(width.saturating_sub(1));
    let y0 = ((top * height as f64).round() as usize).min(height.saturating_sub(1));
    let x1 = width - ((right * width as f64).round() as usize).min(width - x0 - 1);
    let y1 = height - ((bottom * height as f64).round() as usize).min(height - y0 - 1);
    let (new_w, new_h) = (x1 - x0, y1 - y0);
    let mut made = Frame::blank(new_w as u32, new_h as u32);
    for y in 0..new_h {
        for x in 0..new_w {
            let from = ((y + y0) * width + (x + x0)) * 4;
            let into = (y * new_w + x) * 4;
            made.rgba[into..into + 4].copy_from_slice(&frame.rgba[from..from + 4]);
        }
    }
    made
}

// The marks of a mask, as a document of SVG: the shape in white upon nothing, which the reader of
// the mask turns into the alpha it multiplies.
fn mask_svg(effect: &story::Effect, width: u32, height: u32, radius: f64) -> String {
    let body = match effect.shape.as_str() {
        "ellipse" => format!(
            "<ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"#ffffff\"/>",
            width as f64 / 2.0,
            height as f64 / 2.0,
            width as f64 / 2.0,
            height as f64 / 2.0
        ),
        "circle" => {
            let half = width.min(height) as f64 / 2.0;
            format!(
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"#ffffff\"/>",
                width as f64 / 2.0,
                height as f64 / 2.0,
                half
            )
        }
        "round" => {
            let held = part_of(effect.radius, 0.02, radius);
            format!(
                "<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" rx=\"{:.1}\" \
                 ry=\"{:.1}\" fill=\"#ffffff\"/>",
                held,
                held
            )
        }
        "triangle" => format!(
            "<polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"#ffffff\"/>",
            width as f64 / 2.0,
            0.0,
            width as f64,
            height as f64,
            0.0,
            height as f64
        ),
        "star" => {
            let points: Vec<String> = (0..10)
                .map(|place| {
                    let angle = (-90.0 + place as f64 * 36.0).to_radians();
                    let (rx, ry) = if place % 2 == 0 {
                        (width as f64 / 2.0, height as f64 / 2.0)
                    } else {
                        (width as f64 * 0.22, height as f64 * 0.22)
                    };
                    format!(
                        "{:.1},{:.1}",
                        width as f64 / 2.0 + angle.cos() * rx,
                        height as f64 / 2.0 + angle.sin() * ry
                    )
                })
                .collect();
            format!("<polygon points=\"{}\" fill=\"#ffffff\"/>", points.join(" "))
        }
        "path" => {
            let view = if effect.radius > 0.0 {
                // The radius of a path mask is the measure the numbers of the path are written in.
                effect.radius
            } else {
                100.0
            };
            format!(
                "<g transform=\"scale({:.4} {:.4})\"><path d=\"{}\" fill=\"#ffffff\"/></g>",
                width as f64 / view,
                height as f64 / view,
                effect.path.replace('"', "'")
            )
        }
        // The rest, which is "rect" and the fades, keeps the whole box: a fade is worked on the
        // alpha after the shape is cut.
        _ => format!(
            "<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"#ffffff\"/>"
        ),
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\">{body}</svg>"
    )
}

// One part of a measure, in pixels.
fn part_of(given: f64, fallback: f64, measure: f64) -> f64 {
    if given > 0.0 {
        given * measure
    } else {
        fallback * measure
    }
}

// The fade of a mask, when the shape of the mask is a fade rather than a figure.
fn fade_of(effect: &story::Effect) -> Option<(Edge, f64)> {
    let held = if effect.strength > 0.0 {
        1.0 - effect.strength.clamp(0.0, 1.0)
    } else {
        1.0
    };
    match effect.shape.as_str() {
        "fade-up" => Some((Edge::Up, held)),
        "fade-down" => Some((Edge::Down, held)),
        "fade-left" => Some((Edge::Left, held)),
        "fade-right" => Some((Edge::Right, held)),
        _ => None,
    }
}

// A picture sharpened: the picture, plus the difference between it and a blurred copy of it.
fn sharpen(frame: &mut Frame, strength: f64) {
    let mut held = Frame { width: frame.width, height: frame.height, rgba: frame.rgba.clone() };
    media::blur(&mut held, 1);
    for (at, share) in frame.rgba.iter_mut().enumerate() {
        if at % 4 == 3 {
            continue;
        }
        let held = held.rgba[at];
        let made = (*share as f64 + (*share as f64 - held as f64) * strength.clamp(0.0, 3.0))
            .clamp(0.0, 255.0);
        *share = made as u8;
    }
}

// One layer laid upon the frame, with the turn, the scale, the fading and the blending it carries.
fn lay(canvas: &mut Pixmap, made: &Laid, pose: &Pose, layer: &Layer) {
    let (width, height) = (canvas.width() as f64, canvas.height() as f64);
    let centre_x = pose.x * width;
    let centre_y = pose.y * height;
    let content_w = made.pixmap.width() as f64;
    let content_h = made.pixmap.height() as f64;
    let scale_x = (made.width * pose.scale / content_w) as f32;
    let scale_y = (made.height * pose.scale / content_h) as f32;
    // A shadow of the layer goes down first, turned and placed as the layer itself is.
    if let Some(effect) = layer.effects.iter().find(|one| one.kind == "shadow") {
        let colour = colour_parts(&effect.colour).unwrap_or([0.0, 0.0, 0.0]);
        let radius = ((if effect.radius > 0.0 { effect.radius } else { 0.008 }) * height) as u32;
        let dx = (effect.x * width) as i32;
        let dy = (effect.y * height) as i32;
        let strength = if effect.strength > 0.0 { effect.strength } else { 0.5 };
        let held = media::frame_of(&made.pixmap);
        let shade = media::shadow(&held, colour, dx, dy, radius.max(1), strength);
        if let Some(shade) = shade.to_pixmap() {
            let paint = PixmapPaint {
                opacity: pose.opacity.clamp(0.0, 1.0) as f32,
                ..PixmapPaint::default()
            };
            let transform = place(content_w, content_h, scale_x, scale_y, pose.rot, centre_x, centre_y);
            canvas.draw_pixmap(0, 0, shade.as_ref(), &paint, transform, None);
        }
    }
    let paint = PixmapPaint {
        opacity: pose.opacity.clamp(0.0, 1.0) as f32,
        blend_mode: blend_of(&layer.blend),
        quality: FilterQuality::Bilinear,
    };
    let transform = place(content_w, content_h, scale_x, scale_y, pose.rot, centre_x, centre_y);
    canvas.draw_pixmap(0, 0, made.pixmap.as_ref(), &paint, transform, None);
}

// The place a layer is drawn at: its pixels are stood with their centre on the point the pose
// carries, turned about it, and drawn at the size the box and the scale of the pose mean.
fn place(
    content_w: f64,
    content_h: f64,
    scale_x: f32,
    scale_y: f32,
    rot: f64,
    centre_x: f64,
    centre_y: f64,
) -> Transform {
    Transform::from_translate(-(content_w / 2.0) as f32, -(content_h / 2.0) as f32)
        .post_scale(scale_x, scale_y)
        .post_rotate(rot as f32)
        .post_translate(centre_x as f32, centre_y as f32)
}

// The way a layer blends with what stands under it.
fn blend_of(said: &str) -> BlendMode {
    match said {
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        "overlay" => BlendMode::Overlay,
        "add" => BlendMode::Plus,
        "lighten" => BlendMode::Lighten,
        "darken" => BlendMode::Darken,
        "difference" => BlendMode::Difference,
        _ => BlendMode::SourceOver,
    }
}

// The colour of a word, as the parts of it.
pub fn colour_parts(said: &str) -> Option<[f64; 3]> {
    anim::colour_of(said)
}

// A colour of a word, as the pixels of it.
fn colour_of(said: &str) -> Option<Color> {
    let parts = colour_parts(said)?;
    let part = |one: f64| (one.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(Color::from_rgba8(part(parts[0]), part(parts[1]), part(parts[2]), 255))
}

// What the files of a film are, and whether they may be drawn at all. The faults are for the tool
// of the check, which reports them before a frame is drawn.
pub fn notes(project: &story::Project, room: &Path, shots: &mut Shots) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for layer in &project.layers {
        if layer.media.trim().is_empty() {
            if matches!(layer.kind.as_str(), "image" | "video" | "audio") {
                out.push(format!(
                    "{}: a layer of {} names no file.",
                    story::name_of(layer),
                    layer.kind
                ));
            }
            continue;
        }
        let path = room.join(&layer.media);
        match shots.kind(&path) {
            Ok(info) => {
                if layer.kind == "video" && info.kind == media::Kind::Picture {
                    out.push(format!(
                        "{}: {} is a still picture; use the kind \"image\".",
                        story::name_of(layer),
                        path.display()
                    ));
                }
                if layer.kind == "image" && info.kind == media::Kind::Film {
                    out.push(format!(
                        "{}: {} is a film; use the kind \"video\".",
                        story::name_of(layer),
                        path.display()
                    ));
                }
                if layer.kind == "audio" && !info.has_sound && info.kind != media::Kind::Sound {
                    out.push(format!(
                        "{}: {} carries no sound.",
                        story::name_of(layer),
                        path.display()
                    ));
                }
                if layer.speed < 0.0 {
                    out.push(format!(
                        "{}: a film is not run backwards. Cut the part you want and put it in \
                         where you want it.",
                        story::name_of(layer)
                    ));
                }
            }
            Err(why) => out.push(format!("{}: {why}", story::name_of(layer))),
        }
        if layer.kind == "video" && layer.out_at > 0.0 && layer.out_at <= layer.in_at {
            out.push(format!(
                "{}: the point the film stops at is before the point it starts at.",
                story::name_of(layer)
            ));
        }
    }
    for layer in &project.layers {
        let _ = layer;
    }
    let _ = shots;
    out
}

impl Shots {
    // What a file is, asked once.
    pub fn kind(&mut self, path: &Path) -> Result<media::Media, String> {
        let name = path.display().to_string();
        if let Some(found) = self.kinds.get(&name) {
            if found.width > 0 || found.kind == media::Kind::Sound {
                return Ok(found.clone());
            }
        }
        let found = media::probe(path)?;
        self.kinds.insert(name, found.clone());
        Ok(found)
    }

    // The frames of the second a film is read at, which is those of the film when the film names
    // none.
    fn rate(&mut self, path: &Path) -> Result<f64, String> {
        let info = self.kind(path)?;
        Ok(if info.fps > 0.0 { info.fps } else { 25.0 })
    }

    // The reader of a film, opened once.
    fn clip(&mut self, path: &Path, rate: f64) -> Result<&mut media::Clip, String> {
        let name = path.display().to_string();
        if !self.clips.contains_key(&name) {
            let opened = media::Clip::open(path, self.cap, rate)?;
            self.clips.insert(name.clone(), opened);
        }
        self.clips
            .get_mut(&name)
            .ok_or_else(|| "the reader of the film is gone".to_string())
    }

    // A still picture, read once.
    fn still(&mut self, path: &Path) -> Result<Option<Frame>, String> {
        let name = path.display().to_string();
        if self.stills.contains_key(&name) {
            return Ok(self.stills.get(&name).and_then(|one| one.as_ref()).cloned());
        }
        let read = media::read_still(path, self.cap);
        match read {
            Ok(one) => {
                self.stills.insert(name, Some(one.clone()));
                Ok(Some(one))
            }
            Err(why) => {
                self.stills.insert(name, None);
                Err(why)
            }
        }
    }

    // The colour a key is taken against, and the bounds of the ramp that picture asks for. The
    // colour is the one named, or the colour of the border of the frame that was read; the bounds
    // are always measured from the picture, for no number guessed away from the picture knows how
    // far the subject is from the screen.
    fn key_of(
        &mut self,
        media: &str,
        path: &Path,
        named: &str,
        frame: &Frame,
    ) -> Result<Key, String> {
        let asked = if named == "auto" || named.is_empty() {
            None
        } else {
            Some(anim::colour_of(named).ok_or_else(|| {
                format!("\"{named}\" is not a colour a key is taken against")
            })?)
        };
        if let Some(held) = self.keys.get(media) {
            let mut held = *held;
            if let Some(found) = asked {
                held.colour = found;
            }
            return Ok(held);
        }
        let found = match asked {
            // A colour named by the model is taken as it is, even when the border of the picture
            // is not that colour: the model may be taking a colour out of an ordinary picture.
            Some(one) => one,
            None => {
                let held = media::border_colour(frame).ok_or_else(|| {
                    format!("the colour of the border of {} could not be read", path.display())
                })?;
                let agreement = media::border_agreement(frame, held);
                if agreement < 0.5 {
                    return Err(format!(
                        "the border of {} is not one colour: only {:.0}% of it is near the colour \
                         found. Name the colour to take out, or cut the subject with a mask \
                         instead.",
                        path.display(),
                        agreement * 100.0
                    ));
                }
                held
            }
        };
        let (similarity, blend) = media::key_bounds(frame, found, 16).unwrap_or((0.08, 0.1));
        let made = Key { colour: found, similarity, blend };
        self.keys.insert(media.to_string(), made);
        Ok(made)
    }
}

// A key: the colour it is taken against, and the bounds of the ramp that the picture asked for.
#[derive(Clone, Copy)]
pub struct Key {
    pub colour: [f64; 3],
    pub similarity: f64,
    pub blend: f64,
}

#[cfg(test)]
mod checks {
    use crate::infographic::story::{Background, Layer, Style};

    use super::*;

    fn project(layers: Vec<Layer>) -> story::Project {
        story::Project {
            name: String::from("test"),
            canvas: String::from("720p"),
            fps: 24,
            length: 4.0,
            palette: String::from("ink"),
            background: Background { kind: String::from("solid"), colour: String::from("#ffffff"), ..Background::default() },
            layers,
            ..story::Project::default()
        }
    }

    // A frame of a solid ground and one heading is drawn, and the heading stands where it was put.
    #[test]
    fn a_frame_of_words_is_laid_on_the_ground() {
        let layer = Layer {
            id: String::from("head"),
            kind: String::from("text"),
            from: 0.0,
            to: 4.0,
            x: 0.5,
            y: 0.2,
            w: 0.6,
            h: 0.12,
            style: Style { text: String::from("**Autumn**"), size: 0.06, ..Style::default() },
            ..Layer::default()
        };
        let project = project(vec![layer]);
        let fonts = Fonts::new();
        let mut shots = Shots::new();
        let made = frame(&project, &fonts, Path::new("/tmp"), 1.0, &mut shots)
            .expect("the frame is drawn");
        assert!(shots.notes.is_empty(), "{:?}", shots.notes);
        assert_eq!((made.width(), made.height()), (1280, 720));
        // The ground is white, and the middle of the heading is not.
        let ground = made.pixel(4, 700).expect("a pixel of the ground");
        assert!(ground.red() > 240 && ground.green() > 240, "{ground:?}");
        // The box of the layer runs from 256 to 1024 across and from 101 to 187 down, for x and y
        // are the middle of the box. The words stand at the left of the box, where they are set.
        let mut ink = None;
        for y in 0..720u32 {
            for x in 0..1280u32 {
                if let Some(one) = made.pixel(x, y) {
                    if one.red() < 200 && one.green() < 200 {
                        ink = Some((x, y));
                        break;
                    }
                }
            }
            if ink.is_some() {
                break;
            }
        }
        let (x, y) = ink.expect("the heading stands on the frame");
        assert!(
            (256..1024).contains(&x) && (101..187).contains(&y),
            "the heading stands in its box, not over it: {x},{y}"
        );
    }

    // A layer that is not on the screen at the moment is not drawn, and one that is, is.
    #[test]
    fn a_layer_out_of_its_time_is_not_laid() {
        let layer = Layer {
            id: String::from("late"),
            kind: String::from("shape"),
            from: 2.0,
            to: 3.0,
            x: 0.5,
            y: 0.5,
            w: 0.4,
            h: 0.4,
            style: Style { shape: String::from("rect"), fill: String::from("#c2410c"), ..Style::default() },
            ..Layer::default()
        };
        let project = project(vec![layer]);
        let fonts = Fonts::new();
        let mut shots = Shots::new();
        let before = frame(&project, &fonts, Path::new("/tmp"), 1.0, &mut shots).expect("drawn");
        let during = frame(&project, &fonts, Path::new("/tmp"), 2.5, &mut shots).expect("drawn");
        assert_eq!(before.pixel(640, 360).expect("a pixel").red(), 255, "nothing stands yet");
        assert!(during.pixel(640, 360).expect("a pixel").red() > 150, "the shape stands");
        assert!(during.pixel(640, 360).expect("a pixel").green() < 120, "and in its own colour");
    }

    // The faults of the pixels of a film of no files say which layer has none.
    #[test]
    fn a_layer_of_no_file_is_told() {
        let layer = Layer { id: String::from("base"), kind: String::from("video"), ..Layer::default() };
        let project = project(vec![layer]);
        let mut shots = Shots::new();
        let said = notes(&project, Path::new("/tmp"), &mut shots);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("names no file"), "{}", said[0]);
    }

    // A colour is read as a name or as a hex, and the pixels of it are the parts of it.
    #[test]
    fn a_colour_of_a_word_becomes_pixels() {
        assert_eq!(colour_parts("#c2410c"), Some([0xc2 as f64 / 255.0, 0x41 as f64 / 255.0, 0x0c as f64 / 255.0]));
        assert_eq!(colour_parts("nothing"), None);
        assert!(colour_of("white").is_some());
    }
}
