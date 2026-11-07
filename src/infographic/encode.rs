// The frames of an infographic, made into a file.
//
// Two ways out, and the machine decides between them:
//
//   * MP4, when the machine has `ffmpeg`. The frames are pushed into it as raw
//     pixels over a pipe, so nothing is written to the disk in between.
//   * An animated GIF, always. The GIF encoder is part of the program, so a
//     bare machine still gives a moving picture.
//
// A still picture is also asked for: one frame of a scene, so a person may look
// at the layout before the whole video is made. The same drawing code answers
// both, so what is seen in the still is what stands in the film.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::infographic::draw;
use crate::infographic::fonts::Fonts;
use crate::infographic::script::{self, Project};

// What the caller asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Mp4,
    Gif,
    Png,
}

pub fn form_of(word: &str) -> Option<Form> {
    match word.trim().to_ascii_lowercase().as_str() {
        "" | "mp4" | "video" | "movie" => Some(Form::Mp4),
        "gif" => Some(Form::Gif),
        "png" | "still" | "frame" | "jpg" => Some(Form::Png),
        _ => None,
    }
}

// Whether the machine has `ffmpeg`. The answer is asked once per run and kept.
pub fn has_ffmpeg() -> bool {
    static SEEN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SEEN.get_or_init(|| {
        Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|one| one.success())
            .unwrap_or(false)
    })
}

// The name of the file a render writes.
pub fn out_name(name: &str, form: Form) -> String {
    let clean: String = name
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
    let head = if clean.is_empty() { "video" } else { clean };
    let tail = match form {
        Form::Mp4 => "mp4",
        Form::Gif => "gif",
        Form::Png => "png",
    };
    format!("{head}.{tail}")
}

// One frame, as the bytes of a PNG.
pub fn png(project: &Project, fonts: &Fonts, ms: f64, room: Option<&Path>) -> Result<Vec<u8>, String> {
    let svg = draw::at(project, fonts, ms, room);
    let size = canvas(project);
    let pixmap = paint(&svg, size.0, size.1, fonts)?;
    pixmap
        .encode_png()
        .map_err(|why| format!("the picture could not be written as a PNG: {why}"))
}

// The whole video. The frames are made one after the other, and each is handed
// on before the next is drawn, so the run takes the memory of one picture.
pub fn video(
    project: &Project,
    fonts: &Fonts,
    room: &Path,
    form: Form,
    fps: u32,
    mut told: impl FnMut(&str),
) -> Result<PathBuf, String> {
    let fps = fps.clamp(1, script::GREATEST_FPS);
    let total = script::seconds(project);
    if project.scenes.is_empty() {
        return Err("the script has no scene to draw".to_string());
    }
    let frames = ((total * fps as f64).ceil() as usize).max(1);
    let size = canvas(project);
    let name = if project.name.trim().is_empty() {
        "video"
    } else {
        project.name.trim()
    };
    let path = room.join(out_name(name, form));

    // MP4 needs ffmpeg. Without it the caller is told and a GIF is made instead.
    let form = if form == Form::Mp4 && !has_ffmpeg() {
        told("no ffmpeg on this machine; the video is made as an animated GIF");
        Form::Gif
    } else {
        form
    };

    match form {
        Form::Png => {
            let bytes = png(project, fonts, 0.0, Some(room))?;
            std::fs::write(&path, bytes)
                .map_err(|why| format!("the picture could not be written: {why}"))?;
            Ok(path)
        }
        Form::Mp4 => mp4(project, fonts, room, &path, fps, frames, size, &mut told),
        Form::Gif => gif(project, fonts, room, &path, fps, frames, size, &mut told),
    }
}

// The frames pushed into ffmpeg as raw pixels.
fn mp4(
    project: &Project,
    fonts: &Fonts,
    room: &Path,
    path: &Path,
    fps: u32,
    frames: usize,
    size: (u32, u32),
    told: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    let mut child = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", size.0, size.1)])
        .args(["-r", &fps.to_string()])
        .args(["-i", "-"])
        .args(["-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|why| format!("ffmpeg could not be started: {why}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "ffmpeg gave no pipe to write into".to_string())?;
    let mut pipe = BufWriter::new(stdin);

    let mut written = 0usize;
    for step in 0..frames {
        let ms = step as f64 * 1000.0 / fps as f64;
        let bytes = pixels(project, fonts, ms, room, size)?;
        pipe.write_all(&bytes)
            .map_err(|why| format!("a frame could not be sent to ffmpeg: {why}"))?;
        written += 1;
        if step % (fps.max(1) as usize * 2).max(1) == 0 {
            told(&format!(
                "frame {written} of {frames}, at {}",
                script::trim_time(ms / 1000.0)
            ));
        }
    }
    pipe.flush().map_err(|why| why.to_string())?;
    drop(pipe);
    let status = child
        .wait_with_output()
        .map(|one| one.status)
        .map_err(|why| format!("ffmpeg could not be waited for: {why}"))?;
    if !status.success() {
        return Err("ffmpeg stopped before the video was made".to_string());
    }
    Ok(path.to_path_buf())
}

// The frames written into an animated GIF.
fn gif(
    project: &Project,
    fonts: &Fonts,
    room: &Path,
    path: &Path,
    fps: u32,
    frames_total: usize,
    size: (u32, u32),
    told: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    // A GIF of every frame of a 25 per second video is a great file. The frames
    // are thinned to ten a second at most, and each is held for the time of the
    // ones that were dropped.
    let kept = fps.min(10);
    let each = fps as f64 / kept as f64;
    let frames = ((frames_total as f64 / each).ceil() as usize).max(1);
    let delay = (100.0 / kept as f64).round() as u16;

    let file = std::fs::File::create(path)
        .map_err(|why| format!("the GIF could not be made: {why}"))?;
    let mut enc = gif::Encoder::new(BufWriter::new(file), size.0 as u16, size.1 as u16, &[])
        .map_err(|why| format!("the GIF could not be started: {why}"))?;

    for step in 0..frames {
        let ms = step as f64 * each * 1000.0 / fps as f64;
        let mut bytes = pixels(project, fonts, ms, room, size)?;
        for share in bytes.chunks_exact_mut(4) {
            unpremultiply(share);
        }
        let mut frame = gif::Frame::from_rgba_speed(size.0 as u16, size.1 as u16, &mut bytes, 10);
        frame.delay = delay;
        frame.dispose = gif::DisposalMethod::Background;
        enc.write_frame(&frame)
            .map_err(|why| format!("a frame could not be written into the GIF: {why}"))?;
        if step % 10 == 0 {
            told(&format!(
                "frame {} of {frames}, at {}",
                step + 1,
                script::trim_time(ms / 1000.0)
            ));
        }
    }
    enc.into_inner()
        .map_err(|why| format!("the GIF could not be closed: {why}"))?;
    Ok(path.to_path_buf())
}

// One frame as raw pixels, straight, not premultiplied.
fn pixels(
    project: &Project,
    fonts: &Fonts,
    ms: f64,
    room: &Path,
    size: (u32, u32),
) -> Result<Vec<u8>, String> {
    let svg = draw::at(project, fonts, ms, Some(room));
    let pixmap = paint(&svg, size.0, size.1, fonts)?;
    Ok(pixmap.data().to_vec())
}

// A document of SVG made into pixels.
fn paint(svg: &str, width: u32, height: u32, fonts: &Fonts) -> Result<resvg::tiny_skia::Pixmap, String> {
    let opt = resvg::usvg::Options {
        fontdb: fonts.database(),
        ..resvg::usvg::Options::default()
    };
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &opt)
        .map_err(|why| format!("the frame could not be read: {why}"))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| "the picture is too big for this machine".to_string())?;
    let scale = ((width as f32) / tree.size().width()).min((height as f32) / tree.size().height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

// Pixels come from the renderer with the colour already multiplied by the
// alpha. The writers of the files want the colour on its own.
fn unpremultiply(share: &mut [u8]) {
    let alpha = share[3] as u32;
    if alpha == 0 {
        share[0] = 0;
        share[1] = 0;
        share[2] = 0;
        return;
    }
    if alpha == 255 {
        return;
    }
    for part in share.iter_mut().take(3) {
        *part = (((*part as u32) * 255 + alpha / 2) / alpha).min(255) as u8;
    }
}

// The size of the canvas of a project.
fn canvas(project: &Project) -> (u32, u32) {
    let size = script::canvas_of(&project.canvas).unwrap_or(script::Canvas {
        word: "1080p",
        width: 1920,
        height: 1080,
    });
    (size.width, size.height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // One frame of a small script is drawn, and the bytes are a PNG. This is the
    // whole of the way through: the words, the layout, the letters, the picture.
    #[test]
    fn one_frame_is_drawn_as_a_png() {
        let head = json!({
            "canvas": "square",
            "fps": 12,
            "palette": "ink",
            "scenes": [{ "title": "One", "seconds": 1.0 }]
        });
        let had = Project {
            name: String::from("smoke"),
            canvas: String::from("square"),
            fps: 12,
            palette: String::from("ink"),
            scenes: Vec::new(),
            ..Default::default()
        };
        let made = script::merge(&had, &head).expect("a project");
        let fonts = Fonts::new();
        let bytes = png(&made, &fonts, 500.0, None).expect("a frame");
        assert_eq!(&bytes[0..4], &[0x89, b'P', b'N', b'G'], "the bytes are a PNG");
        assert!(bytes.len() > 500, "the picture carries something: {} bytes", bytes.len());
    }
}
