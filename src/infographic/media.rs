// The files a composition reads, and the pixels that come out of them.
//
// Pictures, films and sounds are all read through `ffmpeg` when the machine has it: one process
// per film, started at the point of the film the layer asks for, sending raw pixels down a pipe as
// they are decoded. A film is therefore never held in the memory of the program - one frame is in
// hand at a time - and a layer that reads from the twelfth minute of an hour does not read the
// first eleven.
//
// Without `ffmpeg` the program still draws. A still picture is decoded by the same reader that
// draws a frame, which knows PNG, JPEG and WebP, and a film is answered with a fault that says
// what is needed instead of a black box. The file of a finished film is the same story: MP4 needs
// `ffmpeg`, an animated GIF does not.
//
// The pixel work of a cut-out is done here, and not by a filter of another program. A colour is
// taken out of a picture - a green screen by default, or any colour the model names - the colour
// that spilled onto the edges of the subject is pushed back, and the edge of the cut is softened.
// The same function runs on one frame of an image and on every frame of a film, which is why a
// subject cut out of a moving film is keyed the same way as one cut out of a photograph.

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::Value;

use resvg::tiny_skia::Pixmap;

use crate::infographic::anim::Edge;

// A frame of a picture, a film or a matte: straight (not premultiplied) RGBA bytes.
#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame {
    // A frame of nought, which is transparent black.
    pub fn blank(width: u32, height: u32) -> Self {
        Frame { width, height, rgba: vec![0u8; (width as usize) * (height as usize) * 4] }
    }

    // A frame out of the pixels of a drawn layer, which are premultiplied.
    pub fn from_pixmap(one: &Pixmap) -> Self {
        let mut rgba = one.data().to_vec();
        demultiply(&mut rgba);
        Frame { width: one.width(), height: one.height(), rgba }
    }

    // The frame as a pixmap, which is what the compositor draws with.
    pub fn to_pixmap(&self) -> Option<Pixmap> {
        let mut rgba = self.rgba.clone();
        premultiply(&mut rgba);
        Pixmap::from_vec(rgba, resvg::tiny_skia::IntSize::from_wh(self.width, self.height)?)
    }

    // One pixel, as four parts from 0 to 255.
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let at = (y * self.width as usize + x) * 4;
        if at + 3 >= self.rgba.len() {
            return [0, 0, 0, 0];
        }
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }

    // The bytes of a PNG, for a file that is written to the disk.
    pub fn png(&self) -> Result<Vec<u8>, String> {
        self.to_pixmap()
            .ok_or_else(|| "the image is too big for this computer".to_string())?
            .encode_png()
            .map_err(|why| format!("the image could not be written as a PNG: {why}"))
    }

    // How many pixels are kept, as a part of the whole. A cut-out that kept all of a picture or
    // none of it is reported, because that is what a wrong key looks like.
    pub fn kept(&self) -> f64 {
        let mut sum = 0.0;
        for share in self.rgba.chunks_exact(4) {
            sum += share[3] as f64 / 255.0;
        }
        let count = (self.width as usize * self.height as usize).max(1);
        sum / count as f64
    }

    // The box of the pixels that are kept, which says where the subject of a cut-out stands.
    pub fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let mut left = self.width;
        let mut top = self.height;
        let mut right = 0u32;
        let mut bottom = 0u32;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.pixel(x as usize, y as usize)[3] > 8 {
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x + 1);
                    bottom = bottom.max(y + 1);
                }
            }
        }
        if right <= left || bottom <= top {
            return None;
        }
        Some((left, top, right - left, bottom - top))
    }
}

// Whether the machine has `ffmpeg`. Asked once for a run of the program.
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

// The reason a film is not read, when it cannot be.
pub fn need_ffmpeg() -> String {
    String::from(
        "this machine has no ffmpeg, so a film or a sound cannot be read. Put ffmpeg on the \
         path of the program, or make the film out of still pictures, which are drawn without it.",
    )
}

// What kind of file a media is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Picture,
    Film,
    Sound,
}

// What a file is: its size, its length, its frames of the second.
#[derive(Clone, Debug)]
pub struct Media {
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    pub has_sound: bool,
}

// What a file is. `ffprobe` is asked when the machine has it; the words of `ffmpeg` are read
// when it has not, because the wheel of a program that needs two tools to say what a file is
// needs too many.
pub fn probe(path: &Path) -> Result<Media, String> {
    if !path.exists() {
        return Err(format!("there is no file at {}", path.display()));
    }
    if let Some(made) = probe_with_ffprobe(path) {
        return Ok(made);
    }
    probe_with_ffmpeg(path)
}

fn probe_with_ffprobe(path: &Path) -> Option<Media> {
    let made = Command::new("ffprobe")
        .args(["-hide_banner", "-loglevel", "quiet", "-print_format", "json", "-show_streams", "-show_format"])
        .arg(path)
        .output()
        .ok()?;
    if !made.status.success() {
        return None;
    }
    let read: serde_json::Value = serde_json::from_slice(&made.stdout).ok()?;
    let streams = read.get("streams")?.as_array()?;
    let video = streams
        .iter()
        .find(|one| one.get("codec_type").and_then(Value::as_str) == Some("video"))?;
    let sound = streams
        .iter()
        .any(|one| one.get("codec_type").and_then(Value::as_str) == Some("audio"));
    let width = video.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
    let height = video.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
    let fps = video
        .get("avg_frame_rate")
        .and_then(Value::as_str)
        .and_then(ratio)
        .filter(|one| *one > 0.0)
        .unwrap_or(25.0);
    let duration = read
        .get("format")
        .and_then(|one| one.get("duration"))
        .and_then(Value::as_str)
        .and_then(|one| one.parse::<f64>().ok())
        .or_else(|| video.get("duration").and_then(Value::as_str).and_then(|one| one.parse::<f64>().ok()))
        .unwrap_or(0.0);
    let kind = kind_of(path, duration);
    Some(Media { kind, width, height, fps, duration: if kind == Kind::Film { duration } else { 0.0 }, has_sound: sound })
}

// "60000/1001" and the like.
fn ratio(words: &str) -> Option<f64> {
    let parts: Vec<&str> = words.split('/').collect();
    if parts.len() == 2 {
        let one = parts[0].parse::<f64>().ok()?;
        let next = parts[1].parse::<f64>().ok()?;
        if next.abs() < 1e-6 {
            return None;
        }
        return Some(one / next);
    }
    words.parse::<f64>().ok()
}

fn probe_with_ffmpeg(path: &Path) -> Result<Media, String> {
    let made = Command::new("ffmpeg")
        .args(["-hide_banner", "-i"])
        .arg(path)
        .output();
    let made = match made {
        Ok(one) => one,
        Err(why) => {
            return Err(format!(
                "ffmpeg could not be asked what the file is: {why}. A still picture is still drawn."
            ))
        }
    };
    // ffmpeg writes what it found to its error stream, and stops when there is no output file.
    let said = String::from_utf8_lossy(&made.stderr).to_string();
    let mut video: Option<Media> = None;
    let mut sound = false;
    let mut sound_length = 0.0;
    for line in said.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Duration:") {
            if let Some(found) = rest
                .split(',')
                .next()
                .and_then(|one| parse_clock(one.trim()))
            {
                if let Some(held) = video.as_mut() {
                    held.duration = found;
                }
                sound_length = found;
            }
        }
        if line.starts_with("Stream #") {
            if line.contains(": Video:") {
                let (width, height) = measure(line).unwrap_or((0, 0));
                let fps = line
                    .split(", ")
                    .find(|one| one.contains(" fps"))
                    .and_then(|one| {
                        one.split(' ')
                            .find(|word| word.chars().next().is_some_and(|c| c.is_ascii_digit()))
                    })
                    .and_then(|one| one.parse::<f64>().ok())
                    .unwrap_or(25.0);
                let kind = Kind::Picture;
                let held = Media {
                    kind,
                    width,
                    height,
                    fps,
                    duration: video.as_ref().map(|one| one.duration).unwrap_or(0.0),
                    has_sound: false,
                };
                video = Some(held);
            } else if line.contains(": Audio:") {
                sound = true;
            }
        }
    }
    // A file of a voice or of a song carries no picture at all, and it is a sound that the film
    // may mix under itself.
    let Some(mut made) = video else {
        if sound {
            return Ok(Media {
                kind: Kind::Sound,
                width: 0,
                height: 0,
                fps: 0.0,
                duration: sound_length,
                has_sound: true,
            });
        }
        return Err(format!("nothing in {} is a picture, a film or a sound", path.display()));
    };
    made.has_sound = sound;
    made.kind = kind_of(path, made.duration);
    if made.kind == Kind::Picture {
        made.duration = 0.0;
    }
    Ok(made)
}

// Whether a file is a film or a still. The length of the file answers first, and the tail of its
// name answers when the length is not given, as it is not for a single frame of a film.
fn kind_of(path: &Path, duration: f64) -> Kind {
    if duration > 0.3 {
        return Kind::Film;
    }
    match path.extension().and_then(|one| one.to_str()) {
        Some(one) => match one.to_ascii_lowercase().as_str() {
            "mp4" | "mov" | "mkv" | "webm" | "avi" | "mxf" | "ts" | "m4v" | "mpg" | "mpeg" | "wmv"
            | "ogv" | "3gp" | "flv" => Kind::Film,
            _ => Kind::Picture,
        },
        None => Kind::Picture,
    }
}

// The measure of a stream, from the `1920x1080` of its line. A token of digits, an x, and digits
// again, and nothing else.
fn measure(line: &str) -> Option<(u32, u32)> {
    for word in line.split([' ', ',', '(', ')']) {
        let said = word.trim();
        let Some(at) = said.find('x') else { continue };
        let (one, next) = (&said[..at], &said[at + 1..]);
        if one.len() < 2 || one.len() > 5 || next.len() < 2 || next.len() > 5 {
            continue;
        }
        if !one.chars().all(|c| c.is_ascii_digit()) || !next.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let width = one.parse::<u32>().ok()?;
        let height = next.parse::<u32>().ok()?;
        if width > 8 && height > 8 {
            return Some((width, height));
        }
    }
    None
}

// `00:10:34.62` as seconds.
fn parse_clock(words: &str) -> Option<f64> {
    let parts: Vec<&str> = words.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let hours = parts[0].parse::<f64>().ok()?;
    let minutes = parts[1].parse::<f64>().ok()?;
    let seconds = parts[2].parse::<f64>().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

// ---------------------------------------------------------------- reading

// One still picture, with its sides no longer than the cap. The pixels are straight RGBA, so the
// effects of this file may work on them, and so they may be drawn into a frame.
pub fn read_still(path: &Path, cap: u32) -> Result<Frame, String> {
    if has_ffmpeg() {
        return read_with_ffmpeg(path, cap);
    }
    read_with_resvg(path, cap)
}

// One frame of a picture, taken through ffmpeg at the size it stands at.
fn read_with_ffmpeg(path: &Path, cap: u32) -> Result<Frame, String> {
    let (width, height, fps, duration) = ask_size(path)?;
    let (width, height) = fit_cap(width, height, cap);
    let child = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .arg("-i")
        .arg(path)
        .args(["-frames:v", "1"])
        .args(["-vf", &scale_filter(width, height)])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|why| format!("ffmpeg could not be started: {why}"))?;
    let out = child
        .wait_with_output()
        .map_err(|why| format!("ffmpeg could not be waited for: {why}"))?;
    if !out.status.success() {
        return Err(format!(
            "the picture could not be read: {}",
            tail_of(&out.stderr)
        ));
    }
    let rgba = out.stdout;
    let want = (width as usize) * (height as usize) * 4;
    if rgba.len() < want {
        return Err("the picture came back smaller than it was asked for".to_string());
    }
    let made = Frame { width, height, rgba: rgba[..want].to_vec() };
    let _ = (fps, duration);
    Ok(made)
}

// The measure of a file, from the same reading the probe takes.
fn ask_size(path: &Path) -> Result<(u32, u32, f64, f64), String> {
    let found = probe(path)?;
    Ok((found.width, found.height, found.fps, found.duration))
}

// The decoder of a film. One process per film, walking forward through the frames.
pub struct Clip {
    pub info: Media,
    path: PathBuf,
    // The sides the frames are decoded at, which is the film made smaller when the film is big.
    width: u32,
    height: u32,
    // The frames of the media that are read for one second of the media.
    rate: f64,
    // The process, and the pipe it sends frames down.
    child: Option<Child>,
    pipe: Option<BufReader<std::process::ChildStdout>>,
    // The second of the media the next frame in hand stands at, and how many frames have been
    // read since the process was started.
    start: f64,
    frames: usize,
    // The last frame handed out, which is what the screen holds when the media has run out or
    // when the layer asks for a hair behind the reading.
    last: Option<Frame>,
    ended: bool,
    // The bytes of one frame, kept between the frames so nothing is made ready again.
    held: Vec<u8>,
    error: Option<String>,
}

impl Clip {
    // Open a film. `rate` is how many frames of the media are wanted for one second of the
    // media: the frames of the second of the film, times the speed the film is played at.
    pub fn open(path: &Path, cap: u32, rate: f64) -> Result<Self, String> {
        if !has_ffmpeg() {
            return Err(need_ffmpeg());
        }
        let info = probe(path)?;
        if info.kind != Kind::Film {
            return Err(format!(
                "{} is a {}, not a film",
                path.display(),
                match info.kind {
                    Kind::Picture => "still picture",
                    Kind::Film => "film",
                    Kind::Sound => "sound",
                }
            ));
        }
        let (width, height) = fit_cap(info.width, info.height, cap);
        Ok(Clip {
            info,
            path: path.to_path_buf(),
            width,
            height,
            rate: rate.clamp(1.0, 240.0),
            child: None,
            pipe: None,
            start: 0.0,
            frames: 0,
            last: None,
            ended: false,
            held: Vec::new(),
            error: None,
        })
    }

    // The frame of the media at a second of the media. When the media has run out, the last
    // frame of it is held, which is what a film that is shorter than the layer that reads it
    // should do; a layer that wants the screen empty after the film takes care of that with its
    // own `to`.
    pub fn frame(&mut self, at: f64) -> Result<Option<Frame>, String> {
        if let Some(said) = &self.error {
            return Err(said.clone());
        }
        let wanted = at.max(0.0);
        if self.ended {
            return Ok(self.last.clone());
        }
        // The film is read forward. A frame behind the reading, or one a long way ahead of it,
        // is answered by starting the reading again where the layer wants: that is what a film
        // that runs round again does, and what a cut to a later part of a film does.
        if self.pipe.is_none() {
            self.start_at(wanted)?;
        } else {
            let ahead = (wanted - self.start) * self.rate;
            if ahead < -1.0 || ahead > self.frames as f64 + self.rate * 5.0 {
                self.shut();
                self.start_at(wanted)?;
            }
        }
        let goal = ((wanted - self.start) * self.rate).round().max(0.0) as usize;
        if goal < self.frames {
            return Ok(self.last.clone());
        }
        while self.frames <= goal {
            match self.next()? {
                Some(one) => self.last = Some(one),
                None => {
                    self.ended = true;
                    break;
                }
            }
        }
        Ok(self.last.clone())
    }

    fn start_at(&mut self, at: f64) -> Result<(), String> {
        let seek = at.max(0.0);
        let filter = format!(
            "fps={:.6},{}",
            self.rate,
            scale_filter(self.width, self.height)
        );
        let mut child = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .arg("-ss")
            .arg(format!("{seek:.3}"))
            .arg("-i")
            .arg(&self.path)
            .args(["-vf", &filter])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|why| format!("ffmpeg could not be started: {why}"))?;
        let out = child.stdout.take().ok_or_else(|| {
            "ffmpeg gave no pipe of frames".to_string()
        })?;
        self.child = Some(child);
        self.pipe = Some(BufReader::new(out));
        self.start = seek;
        self.frames = 0;
        self.held = vec![0u8; (self.width as usize) * (self.height as usize) * 4];
        Ok(())
    }

    // The next frame of the pipe.
    fn next(&mut self) -> Result<Option<Frame>, String> {
        let pipe = match self.pipe.as_mut() {
            Some(one) => one,
            None => return Ok(None),
        };
        let mut read = 0usize;
        let want = self.held.len();
        while read < want {
            match pipe.read(&mut self.held[read..]) {
                Ok(0) => {
                    self.shut();
                    return Ok(None);
                }
                Ok(got) => read += got,
                Err(why) => {
                    let said = format!("the frames of the film could not be read: {why}");
                    self.error = Some(said.clone());
                    self.shut();
                    return Err(said);
                }
            }
        }
        self.frames += 1;
        Ok(Some(Frame {
            width: self.width,
            height: self.height,
            rgba: self.held.clone(),
        }))
    }

    fn shut(&mut self) {
        self.pipe = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Clip {
    fn drop(&mut self) {
        self.shut();
    }
}

// A sound, or the sound of a film, sent into ffmpeg to be mixed under the film. The work of the
// mixing is done by `encode`, which knows the whole of the list; this answers what a sound is.
pub fn is_sound(path: &Path) -> bool {
    match path.extension().and_then(|one| one.to_str()) {
        Some(one) => matches!(
            one.to_ascii_lowercase().as_str(),
            "mp3" | "aac" | "m4a" | "wav" | "ogg" | "flac" | "opus" | "aiff" | "wma"
        ),
        None => false,
    }
}

// The sides of a frame, with its longest side no more than the cap, and its sides even for the
// codecs of ffmpeg.
fn fit_cap(width: u32, height: u32, cap: u32) -> (u32, u32) {
    if cap == 0 || width == 0 || height == 0 {
        return (width.max(1), height.max(1));
    }
    let long = width.max(height);
    if long <= cap {
        return (even(width), even(height));
    }
    let part = cap as f64 / long as f64;
    (even(((width as f64) * part).round() as u32), even(((height as f64) * part).round() as u32))
}

fn even(value: u32) -> u32 {
    if value.is_multiple_of(2) {
        value.max(2)
    } else {
        value.saturating_sub(1).max(1)
    }
}

// A picture, without ffmpeg: the same reader that draws a frame, asked for the picture at the
// size of its own file.
fn read_with_resvg(path: &Path, cap: u32) -> Result<Frame, String> {
    let bytes = std::fs::read(path).map_err(|why| format!("the picture could not be read: {why}"))?;
    let (width, height) = still_measure(&bytes).ok_or_else(|| {
        format!(
            "{} is a kind of picture this program cannot read without ffmpeg",
            path.display()
        )
    })?;
    let (width, height) = fit_cap(width, height, cap);
    let head = "data:image/png;base64,";
    let body = base6_of(&bytes);
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\
         <image x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" \
         href=\"{head}{body}\" preserveAspectRatio=\"none\"/></svg>"
    );
    let made = crate::infographic::encode::paint(&svg, width, height, &crate::infographic::fonts::Fonts::new())?;
    Ok(Frame::from_pixmap(&made))
}

fn base6_of(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// The sides of a PNG, a JPEG or a WebP, from the words of its header.
pub fn still_measure(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return Some((width, height));
    }
    if bytes.starts_with(b"RIFF") && bytes.len() > 30 && &bytes[8..12] == b"WEBP" {
        // VP8 (lossy), VP8L (lossless) and VP8X (extended) all say their measure within the
        // first thirty bytes or so.
        if &bytes[12..16] == b"VP8X" {
            let width = 1 + (bytes[24] as u32 | (bytes[25] as u32) << 8 | (bytes[26] as u32) << 16);
            let height = 1 + (bytes[27] as u32 | (bytes[28] as u32) << 8 | (bytes[29] as u32) << 16);
            return Some((width, height));
        }
        if &bytes[12..16] == b"VP8 " {
            let width = (bytes[26] as u32 | (bytes[27] as u32) << 8) & 0x3fff;
            let height = (bytes[28] as u32 | (bytes[29] as u32) << 8) & 0x3fff;
            return Some((width, height));
        }
    }
    if bytes.starts_with(b"\xff\xd8") {
        // A JPEG of markers: the start of a frame (SOF0..SOF15, but not the ones of a table)
        // carries the measure.
        let mut at = 2usize;
        while at + 9 < bytes.len() {
            if bytes[at] != 0xff {
                at += 1;
                continue;
            }
            let mark = bytes[at + 1];
            if (0xc0..=0xcf).contains(&mark) && !matches!(mark, 0xc4 | 0xc8 | 0xcc) {
                let height = u16::from_be_bytes([bytes[at + 5], bytes[at + 6]]) as u32;
                let width = u16::from_be_bytes([bytes[at + 7], bytes[at + 8]]) as u32;
                return Some((width, height));
            }
            if matches!(mark, 0xd8 | 0x01) || (0xd0..=0xd7).contains(&mark) {
                at += 2;
                continue;
            }
            let long = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
            at += 2 + long.max(2);
        }
    }
    None
}

// ---------------------------------------------------------------- pixels

// The colour of the border of a picture: the colour a green screen is painted in. The four edges
// are read and the colour nearest the middle of them wins, so a shadow along one edge does not
// choose the key.
pub fn border_colour(frame: &Frame) -> Option<[f64; 3]> {
    let (width, height) = (frame.width as usize, frame.height as usize);
    if width < 4 || height < 4 {
        return None;
    }
    let band = ((width.min(height) as f64 * 0.03).round() as usize).clamp(1, 12);
    let mut sums = [[0.0f64; 3]; 4];
    let mut counts = [0u32; 4];
    for x in 0..width {
        for share in 0..band {
            let at = (share * width + x) * 4;
            push_colour(&mut sums[0], &mut counts[0], &frame.rgba, at);
            let at = ((height - 1 - share) * width + x) * 4;
            push_colour(&mut sums[1], &mut counts[1], &frame.rgba, at);
        }
    }
    for y in 0..height {
        for share in 0..band {
            let at = (y * width + share) * 4;
            push_colour(&mut sums[2], &mut counts[2], &frame.rgba, at);
            let at = (y * width + (width - 1 - share)) * 4;
            push_colour(&mut sums[3], &mut counts[3], &frame.rgba, at);
        }
    }
    let mut best: Option<([f64; 3], u32)> = None;
    for place in 0..4 {
        if counts[place] == 0 {
            continue;
        }
        let colour = [
            sums[place][0] / counts[place] as f64,
            sums[place][1] / counts[place] as f64,
            sums[place][2] / counts[place] as f64,
        ];
        if best.as_ref().map(|(_, count)| counts[place] > *count).unwrap_or(true) {
            best = Some((colour, counts[place]));
        }
    }
    best.map(|(one, _)| one)
}

fn push_colour(sum: &mut [f64; 3], count: &mut u32, bytes: &[u8], at: usize) {
    if at + 3 >= bytes.len() {
        return;
    }
    // The parts are added as they are compared elsewhere: from 0.0 to 1.0.
    sum[0] += bytes[at] as f64 / 255.0;
    sum[1] += bytes[at + 1] as f64 / 255.0;
    sum[2] += bytes[at + 2] as f64 / 255.0;
    *count += 1;
}

// The part of the border that is one colour, as a part from 0.0 to 1.0. A green screen reads
// above 0.9; a photograph of a street reads near nought, which is the answer that tells a model
// the key has to be named or a mask drawn instead.
pub fn border_agreement(frame: &Frame, colour: [f64; 3]) -> f64 {
    let (width, height) = (frame.width as usize, frame.height as usize);
    if width < 4 || height < 4 {
        return 0.0;
    }
    let band = ((width.min(height) as f64 * 0.03).round() as usize).clamp(1, 12);
    let mut same = 0u32;
    let mut count = 0u32;
    let mut one_edge = |x: usize, y: usize| {
        let at = (y * width + x) * 4;
        if at + 3 >= frame.rgba.len() {
            return;
        }
        count += 1;
        let pixel = [
            frame.rgba[at] as f64 / 255.0,
            frame.rgba[at + 1] as f64 / 255.0,
            frame.rgba[at + 2] as f64 / 255.0,
        ];
        if distance(pixel, colour) < 0.22 {
            same += 1;
        }
    };
    for x in 0..width {
        for share in 0..band {
            one_edge(x, share);
            one_edge(x, height - 1 - share);
        }
    }
    for y in 0..height {
        for share in 0..band {
            one_edge(share, y);
            one_edge(width - 1 - share, y);
        }
    }
    if count == 0 {
        return 0.0;
    }
    same as f64 / count as f64
}

// How far two colours are, from 0.0 to 1.0.
pub fn distance(one: [f64; 3], next: [f64; 3]) -> f64 {
    let mut sum = 0.0;
    for place in 0..3 {
        let part = one[place] - next[place];
        sum += part * part;
    }
    sum.sqrt() / 3.0f64.sqrt()
}

// The bounds a key of this picture asks for, measured from the picture itself, as the pair
// (`similarity`, `blend`) that `key_out` is taken at.
//
// The border of the picture is the screen, so the farthest pixel of the border from the key colour
// is as far as a screen on this set goes: `similarity`. Beyond that lies the fringe, where the
// screen and the subject are mixed at an edge, and beyond the fringe lies the body of the subject,
// which is a crowd of pixels and stands as a hill in the histogram of distances. The ramp of the
// key therefore runs from the screen to the foot of that hill, so that the mixed pixels of an edge
// are seen through by the part of the mix they are, while the body of the subject stands wholly.
// Numbers guessed away from the picture cannot do this, because the colours of a screen and of a
// subject are known only from the picture.
pub fn key_bounds(frame: &Frame, colour: [f64; 3], band: usize) -> Option<(f64, f64)> {
    // The distance is measured in steps of a hundredth, out to eight tenths, which is further than
    // any key reaches: a subject nearer to the screen colour than that is not to be keyed apart.
    const STEP: f64 = 0.02;
    const BINS: usize = 40;
    let width = frame.width as usize;
    let height = frame.height as usize;
    if width < 8 || height < 8 {
        return None;
    }
    let band = band.max(1).min(width.min(height) / 4);
    let edge = |x: usize, y: usize| {
        x < band || y < band || x + 1 + band > width || y + 1 + band > height
    };
    let mut screen: Vec<f64> = Vec::new();
    let mut bins = vec![0u64; BINS];
    let mut count = 0u64;
    for y in 0..height {
        for x in 0..width {
            // One pixel of nine, which keeps a picture of 1080 lines to two hundred thousand
            // samples, enough for a histogram of a key.
            if x % 3 != 0 || y % 3 != 0 {
                continue;
            }
            let at = (y * width + x) * 4;
            if at + 2 >= frame.rgba.len() {
                break;
            }
            let pixel = [
                frame.rgba[at] as f64 / 255.0,
                frame.rgba[at + 1] as f64 / 255.0,
                frame.rgba[at + 2] as f64 / 255.0,
            ];
            let far = distance(pixel, colour);
            bins[((far / STEP) as usize).min(BINS - 1)] += 1;
            count += 1;
            if edge(x, y) {
                screen.push(far);
            }
        }
    }
    if screen.len() < 16 || count < 64 {
        return None;
    }
    screen.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // The colour is a screen only if the middle of its own border is that colour.
    if half_of(&screen, 0.5) > 0.15 {
        return None;
    }
    // A body of pixels is fifteen steps in the thousand of the picture: enough for a colour to be
    // something in the scene rather than the fringe of an edge.
    let crowd = ((count as f64) * 0.015).max(30.0) as u64;
    // The screen is the run of steps at the foot of the histogram that each hold a body of pixels,
    // starting at the key colour itself. As far as that run goes is `similarity`.
    let mut place = 0usize;
    let mut similarity = 0.0;
    while place < BINS && bins[place] > crowd {
        similarity = (place as f64 + 1.0) * STEP;
        place += 1;
    }
    // A colour whose own body reaches further than three tenths is not a screen that may be keyed
    // apart from a subject, and a picture of nothing but that colour is not a key at all.
    if similarity > 0.3 {
        return None;
    }
    // The body of the subject begins at the next step that holds a body of pixels. Between the two
    // lie the mixed pixels of an edge, few in a picture, and each of them is seen through by the
    // part of the gap it stands on.
    let mut top = similarity;
    while place < BINS && bins[place] <= crowd {
        top = (place as f64 + 1.0) * STEP;
        place += 1;
    }
    let blend = (top - similarity).clamp(0.04, 0.35);
    Some((similarity, blend))
}

// The value a part of a sorted list of numbers stands under: `half_of(&held, 0.95)` is the value
// that 95 per cent of the list is at or below.
fn half_of(sorted: &[f64], part: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let place = ((sorted.len() as f64 - 1.0) * part.clamp(0.0, 1.0)).round() as usize;
    sorted[place.min(sorted.len() - 1)]
}

// The colour of the key taken out of a picture, and its edge softened.
//
// `similarity` is how far a colour may be from the key and still be taken out; `blend` is how
// wide the ramp of the edge is. A pixel of the key colour becomes wholly away, a pixel far from
// it wholly stays, and the pixels between the two are seen through by the part of the ramp they
// fall on. What is left of the key colour on the edge of the subject - the green that lights
// bounce off a screen - is pushed back towards the colour of the subject.
pub fn key_out(frame: &mut Frame, colour: [f64; 3], similarity: f64, blend: f64, spill: f64) {
    let similarity = similarity.clamp(0.0, 1.0);
    let blend = blend.max(0.01);
    let strongest = strongest_part(colour);
    for share in frame.rgba.chunks_exact_mut(4) {
        let pixel = [
            share[0] as f64 / 255.0,
            share[1] as f64 / 255.0,
            share[2] as f64 / 255.0,
        ];
        let far = distance(pixel, colour);
        let part = ((far - similarity) / blend).clamp(0.0, 1.0);
        let alpha = share[3] as f64 / 255.0 * part;
        if part > 0.01 {
            // The green of a screen lying in the hair of a subject is taken out of that channel,
            // towards the lesser of the other two, which is the colour the hair really is.
            let keep = suppress(pixel, strongest, spill * part);
            share[0] = (keep[0] * 255.0).round() as u8;
            share[1] = (keep[1] * 255.0).round() as u8;
            share[2] = (keep[2] * 255.0).round() as u8;
        }
        share[3] = (alpha * 255.0).round() as u8;
    }
}

// The key colour left on the subject pushed back, without touching what was cut away.
//
// A key takes the screen out of a picture; what it leaves on the edges of the subject is the
// colour of the screen, bounced off it onto the hair and the shoulders. This is the same push the
// edge of a key gets, run over the whole of the picture, for a model that keyed a picture and
// finds the subject still green at the edges. The alpha is left alone, so a despill does not undo
// a cut.
pub fn despill(frame: &mut Frame, colour: [f64; 3], strength: f64) {
    let amount = strength.clamp(0.0, 1.0);
    if amount <= 0.0 {
        return;
    }
    let strongest = strongest_part(colour);
    for share in frame.rgba.chunks_exact_mut(4) {
        let pixel = [
            share[0] as f64 / 255.0,
            share[1] as f64 / 255.0,
            share[2] as f64 / 255.0,
        ];
        let keep = suppress(pixel, strongest, amount);
        share[0] = (keep[0] * 255.0).round() as u8;
        share[1] = (keep[1] * 255.0).round() as u8;
        share[2] = (keep[2] * 255.0).round() as u8;
    }
}

// Which part of a colour is the key of a screen: the green of a green screen, the blue of a blue.
fn strongest_part(colour: [f64; 3]) -> usize {
    let mut best = 0usize;
    for place in 1..3 {
        if colour[place] > colour[best] {
            best = place;
        }
    }
    best
}

// The spill of the key colour pushed back into the colour of the subject.
fn suppress(pixel: [f64; 3], part: usize, amount: f64) -> [f64; 3] {
    if amount <= 0.0 {
        return pixel;
    }
    let others = (0..3).filter(|one| *one != part).collect::<Vec<usize>>();
    let least = pixel[others[0]].min(pixel[others[1]]);
    let mut made = pixel;
    let gap = pixel[part] - least;
    if gap > 0.0 {
        made[part] = pixel[part] - gap * amount.clamp(0.0, 1.0);
    }
    made
}

// The alpha only, blurred: the edge of a cut softened, so the subject does not carry a hard line.
pub fn feather(frame: &mut Frame, radius: u32) {
    if radius == 0 {
        return;
    }
    let width = frame.width as usize;
    let height = frame.height as usize;
    let mut alpha: Vec<u8> = frame.rgba.iter().skip(3).step_by(4).copied().collect();
    blur_plane(&mut alpha, width, height, radius as i32);
    for (at, value) in alpha.iter().enumerate() {
        frame.rgba[at * 4 + 3] = *value;
    }
}

// The alpha only, taken inwards from the edge of the cut, so a fringe of the key colour goes
// away with the edge.
pub fn choke(frame: &mut Frame, amount: f64) {
    if amount.abs() < 0.01 {
        return;
    }
    for share in frame.rgba.chunks_exact_mut(4) {
        let value = share[3] as f64 / 255.0;
        let made = if amount > 0.0 {
            ((value - amount) / (1.0 - amount)).clamp(0.0, 1.0)
        } else {
            (value * (1.0 - amount)).clamp(0.0, 1.0)
        };
        share[3] = (made * 255.0).round() as u8;
    }
}

// A blur of the whole picture, colour and alpha together.
pub fn blur(frame: &mut Frame, radius: u32) {
    if radius == 0 {
        return;
    }
    let width = frame.width as usize;
    let height = frame.height as usize;
    let long = radius as i32;
    for place in 0..4 {
        let mut plane: Vec<u8> = frame.rgba.iter().skip(place).step_by(4).copied().collect();
        blur_plane(&mut plane, width, height, long);
        blur_plane(&mut plane, width, height, long);
        for (at, value) in plane.iter().enumerate() {
            frame.rgba[at * 4 + place] = *value;
        }
    }
}

// One plane of a picture, blurred along the row and then down the column. The edges of the
// picture are held outwards while the row is read, so a blurred picture does not gather a dark
// line at its border.
fn blur_plane(planes: &mut [u8], width: usize, height: usize, radius: i32) {
    if radius <= 0 || width == 0 || height == 0 {
        return;
    }
    let long = radius as usize;
    let window = long * 2 + 1;
    // Along the row, into a copy with the ends held out.
    let mut held = vec![0u8; (width + long * 2) * height];
    for y in 0..height {
        let row = y * width;
        let held_row = y * (width + long * 2);
        for at in 0..long {
            held[held_row + at] = planes[row];
            held[held_row + long + width + at] = planes[row + width - 1];
        }
        held[held_row + long..held_row + long + width].copy_from_slice(&planes[row..row + width]);
    }
    for y in 0..height {
        let held_row = y * (width + long * 2);
        let mut total: u32 = 0;
        for at in 0..window {
            total += held[held_row + at] as u32;
        }
        for x in 0..width {
            planes[y * width + x] = (total / window as u32) as u8;
            // The window of the next pixel drops the one it started on and takes the one after
            // its end, which is only asked for while another pixel follows.
            if x + 1 < width {
                total = total - held[held_row + x] as u32 + held[held_row + x + window] as u32;
            }
        }
    }
    // Down the column, the same way.
    let mut held = vec![0u8; (height + long * 2) * width];
    for x in 0..width {
        for at in 0..long {
            held[at * width + x] = planes[x];
            held[(long + height + at) * width + x] = planes[(height - 1) * width + x];
        }
    }
    for y in 0..height {
        for x in 0..width {
            held[(long + y) * width + x] = planes[y * width + x];
        }
    }
    for x in 0..width {
        let mut total: u32 = 0;
        for at in 0..window {
            total += held[at * width + x] as u32;
        }
        for y in 0..height {
            planes[y * width + x] = (total / window as u32) as u8;
            if y + 1 < height {
                total = total - held[y * width + x] as u32 + held[(y + window) * width + x] as u32;
            }
        }
    }
}

// A mask: the alpha of the picture multiplied by a shape, so only the part of it inside the shape
// is seen. The shapes are the shapes of a layer of kind "shape", drawn here in white on black and
// read as the alpha of the answer.
pub fn shape_mask(
    frame: &mut Frame,
    svg: &str,
    fade: Option<(Edge, f64)>,
    soft: u32,
) -> Result<(), String> {
    if !svg.is_empty() {
        let made = crate::infographic::encode::paint(
            svg,
            frame.width,
            frame.height,
            &crate::infographic::fonts::Fonts::new(),
        )?;
        let mask = Frame::from_pixmap(&made);
        for (at, share) in frame.rgba.chunks_exact_mut(4).enumerate() {
            let held = mask.rgba[at * 4 + 3] as f64 / 255.0;
            share[3] = (share[3] as f64 * held).round() as u8;
        }
    }
    if let Some((edge, part)) = fade {
        fade_edge(frame, edge, part);
    }
    if soft > 0 {
        feather(frame, soft);
    }
    Ok(())
}

// A part of the picture kept, and the rest made away with, from one edge: how a wipe and a soft
// mask are drawn. `part` is how much of the picture is kept, and the edge of the keeping is a
// band of a few parts of the width, so a wipe is not a hard cut.
pub fn fade_edge(frame: &mut Frame, edge: Edge, part: f64) {
    let (width, height) = (frame.width as usize, frame.height as usize);
    let part = part.clamp(0.0, 1.0);
    if part >= 1.0 {
        return;
    }
    if part <= 0.0 {
        for share in frame.rgba.chunks_exact_mut(4) {
            share[3] = 0;
        }
        return;
    }
    for y in 0..height {
        for x in 0..width {
            let kept = match edge {
                // The edge travels right, so what stands to the left of it is kept.
                Edge::Right => {
                    let edge = part * width as f64;
                    ramp(edge - x as f64, width as f64 * 0.06)
                }
                Edge::Left => {
                    let edge = (1.0 - part) * width as f64;
                    ramp(x as f64 - edge, width as f64 * 0.06)
                }
                Edge::Down => {
                    let edge = part * height as f64;
                    ramp(edge - y as f64, height as f64 * 0.06)
                }
                Edge::Up => {
                    let edge = (1.0 - part) * height as f64;
                    ramp(y as f64 - edge, height as f64 * 0.06)
                }
            };
            let at = (y * width + x) * 4;
            frame.rgba[at + 3] = (frame.rgba[at + 3] as f64 * kept).round() as u8;
        }
    }
}

// How far into the soft band of an edge a pixel stands: 0.0 outside, 1.0 well within.
fn ramp(into: f64, band: f64) -> f64 {
    if band <= 0.0 {
        return if into >= 0.0 { 1.0 } else { 0.0 };
    }
    (into / band).clamp(0.0, 1.0)
}

// A shadow of the picture: a blurred copy of its alpha, under it, offset.
pub fn shadow(frame: &Frame, colour: [f64; 3], dx: i32, dy: i32, radius: u32, strength: f64) -> Frame {
    let mut made = Frame { width: frame.width, height: frame.height, rgba: vec![0u8; frame.rgba.len()] };
    let tint = [
        (colour[0] * 255.0).round() as u8,
        (colour[1] * 255.0).round() as u8,
        (colour[2] * 255.0).round() as u8,
    ];
    for y in 0..frame.height as usize {
        for x in 0..frame.width as usize {
            let source = frame.pixel(x, y);
            let value = (source[3] as f64 * strength.clamp(0.0, 1.0)).round() as u8;
            let at = (y * frame.width as usize + x) * 4;
            made.rgba[at] = tint[0];
            made.rgba[at + 1] = tint[1];
            made.rgba[at + 2] = tint[2];
            made.rgba[at + 3] = value;
        }
    }
    blur(&mut made, radius.max(1));
    shift(&mut made, dx, dy);
    made
}

// The pixels of a picture moved by a whole number of pixels.
pub fn shift(frame: &mut Frame, dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }
    let width = frame.width as usize;
    let height = frame.height as usize;
    let mut made = vec![0u8; frame.rgba.len()];
    for y in 0..height {
        let next = y as i64 + dy as i64;
        if next < 0 || next as usize >= height {
            continue;
        }
        for x in 0..width {
            let place = x as i64 + dx as i64;
            if place < 0 || place as usize >= width {
                continue;
            }
            let from = (y * width + x) * 4;
            let into = (next as usize * width + place as usize) * 4;
            made[into..into + 4].copy_from_slice(&frame.rgba[from..from + 4]);
        }
    }
    frame.rgba = made;
}

// Grey, inverted, brightened, darkened, contrasted, tinted: the small changes of a picture.
pub fn adjust(frame: &mut Frame, kind: &str, colour: [f64; 3], amount: f64) {
    for share in frame.rgba.chunks_exact_mut(4) {
        let mut pixel = [
            share[0] as f64 / 255.0,
            share[1] as f64 / 255.0,
            share[2] as f64 / 255.0,
        ];
        match kind {
            "grey" => {
                let held = pixel[0] * 0.2126 + pixel[1] * 0.7152 + pixel[2] * 0.0722;
                let made = if amount.abs() < 1.0 { mix(pixel, [held; 3], amount) } else { [held; 3] };
                pixel = made;
            }
            "invert" => {
                let made = [
                    mix1(pixel[0], 1.0 - pixel[0], amount),
                    mix1(pixel[1], 1.0 - pixel[1], amount),
                    mix1(pixel[2], 1.0 - pixel[2], amount),
                ];
                pixel = made;
            }
            "brighten" => {
                for held in pixel.iter_mut() {
                    *held = (*held + amount * 0.5).clamp(0.0, 1.0);
                }
            }
            "contrast" => {
                for held in pixel.iter_mut() {
                    *held = ((*held - 0.5) * (1.0 + amount) + 0.5).clamp(0.0, 1.0);
                }
            }
            "tint" => {
                for place in 0..3 {
                    pixel[place] = mix1(pixel[place], colour[place], amount.abs().min(1.0) * 0.6);
                }
            }
            _ => {}
        }
        share[0] = (pixel[0] * 255.0).round() as u8;
        share[1] = (pixel[1] * 255.0).round() as u8;
        share[2] = (pixel[2] * 255.0).round() as u8;
    }
}

fn mix(one: [f64; 3], next: [f64; 3], part: f64) -> [f64; 3] {
    [
        mix1(one[0], next[0], part),
        mix1(one[1], next[1], part),
        mix1(one[2], next[2], part),
    ]
}

fn mix1(one: f64, next: f64, part: f64) -> f64 {
    let part = part.clamp(0.0, 1.0);
    one + (next - one) * part
}

// Straight RGBA to premultiplied, and back.
pub fn premultiply(bytes: &mut [u8]) {
    for share in bytes.chunks_exact_mut(4) {
        let alpha = share[3] as u32;
        if alpha == 255 {
            continue;
        }
        for held in share[..3].iter_mut() {
            *held = ((*held as u32 * alpha) / 255) as u8;
        }
    }
}

pub fn demultiply(bytes: &mut [u8]) {
    for share in bytes.chunks_exact_mut(4) {
        let alpha = share[3] as u32;
        if alpha == 0 || alpha == 255 {
            continue;
        }
        for held in share[..3].iter_mut() {
            *held = ((*held as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
}

// A pixmap as a frame of straight RGBA.
pub fn frame_of(one: &Pixmap) -> Frame {
    let mut rgba = one.data().to_vec();
    demultiply(&mut rgba);
    Frame { width: one.width(), height: one.height(), rgba }
}

// The bytes at the end of what a program complained about, for a fault that is worth reading.
pub fn tail_of(bytes: &[u8]) -> String {
    let said = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = said.lines().filter(|one| !one.trim().is_empty()).collect();
    let held = lines.last().copied().unwrap_or("").trim();
    if held.chars().count() > 200 {
        return held.chars().take(200).collect();
    }
    if held.is_empty() {
        return String::from("the program gave no reason");
    }
    String::from(held)
}

// A scale of the sides of a picture, as ffmpeg words it.
pub fn scale_filter(width: u32, height: u32) -> String {
    format!("scale={width}:{height}:flags=lanczos", width = width, height = height)
}

// The whole of a picture as the words of a PNG, for the answer of a tool.
pub fn write_png(path: &Path, frame: &Frame) -> Result<(), String> {
    let bytes = frame.png()?;
    std::fs::write(path, bytes).map_err(|why| format!("{} could not be written: {why}", path.display()))
}

#[cfg(test)]
mod checks {
    use super::*;

    // The sides of a PNG are read from its header.
    #[test]
    fn the_sides_of_a_png_are_read_from_its_header() {
        let mut bytes = vec![0u8; 24];
        bytes[0..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes[16..20].copy_from_slice(&1280u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&720u32.to_be_bytes());
        assert_eq!(still_measure(&bytes), Some((1280, 720)));
    }

    // The colour of a border is the colour of a green screen, and the border agrees with it.
    #[test]
    fn the_colour_of_a_border_is_the_colour_of_a_screen() {
        let mut frame = Frame::blank(64, 48);
        for share in frame.rgba.chunks_exact_mut(4) {
            share[0] = 0;
            share[1] = 210;
            share[2] = 0;
            share[3] = 255;
        }
        // A subject of a plain colour in the middle of it.
        for y in 12..36 {
            for x in 20..44 {
                let at = (y * 64 + x) * 4;
                frame.rgba[at] = 200;
                frame.rgba[at + 1] = 160;
                frame.rgba[at + 2] = 140;
            }
        }
        let found = border_colour(&frame).expect("a border colour is found");
        assert!(found[1] > 0.7 && found[0] < 0.2, "{found:?}");
        assert!(border_agreement(&frame, found) > 0.9);
    }

    // The bounds of a key are measured from the picture: the screen goes wholly, the body of the
    // subject stands wholly, and the pixels of the edge between them are seen through by a part.
    #[test]
    fn the_bounds_of_a_key_are_measured_from_the_picture() {
        let mut frame = Frame::blank(96, 72);
        let screen = [110u8, 229, 27];
        for share in frame.rgba.chunks_exact_mut(4) {
            share[0..3].copy_from_slice(&screen);
            share[3] = 255;
        }
        // A subject, with one ring of pixels where the screen and the subject are mixed three
        // parts to one, which is what an edge of hair or of cloth is made of.
        let subject = [200u8, 40, 40];
        let fringe = [
            ((screen[0] as u16 * 3 + subject[0] as u16) / 4) as u8,
            ((screen[1] as u16 * 3 + subject[1] as u16) / 4) as u8,
            ((screen[2] as u16 * 3 + subject[2] as u16) / 4) as u8,
        ];
        for y in 19..53 {
            for x in 29..67 {
                let edge = y == 19 || y == 52 || x == 29 || x == 66;
                let at = (y * 96 + x) * 4;
                let held = if edge { fringe } else { subject };
                frame.rgba[at..at + 3].copy_from_slice(&held);
            }
        }
        let colour = border_colour(&frame).expect("the screen is found");
        let (similarity, blend) = key_bounds(&frame, colour, 8).expect("the bounds are measured");
        assert!(similarity <= 0.04, "the screen is close to itself: {similarity}");
        assert!(blend >= 0.2 && blend <= 0.35, "the ramp reaches over the fringe: {blend}");
        key_out(&mut frame, colour, similarity, blend, 0.0);
        let alpha_at = |x: usize, y: usize| frame.rgba[(y * 96 + x) * 4 + 3];
        assert_eq!(alpha_at(2, 2), 0, "the screen of the corner is gone");
        assert_eq!(alpha_at(48, 36), 255, "the body of the subject stands wholly");
        let edge = alpha_at(48, 19);
        assert!(edge > 0 && edge < 255, "the fringe is seen through by a part: {edge}");
    }

    // A colour nothing keys against is answered with nothing: the bounds of a key mean nothing in a
    // picture that is not a screen.
    #[test]
    fn the_bounds_of_a_key_refuse_a_picture_that_is_no_screen() {
        let mut frame = Frame::blank(64, 48);
        // Half the picture one colour, half another, with no screen anywhere.
        for y in 0..48usize {
            for x in 0..64usize {
                let at = (y * 64 + x) * 4;
                let held = if x < 32 { [20u8, 20, 20] } else { [240, 240, 240] };
                frame.rgba[at..at + 3].copy_from_slice(&held);
                frame.rgba[at + 3] = 255;
            }
        }
        assert!(key_bounds(&frame, [0.5, 0.9, 0.1], 8).is_none(), "no screen, no bounds");
    }

    // A screen taken out leaves the subject and takes the ground.
    #[test]
    fn a_screen_is_taken_out_and_the_subject_kept() {
        let mut frame = Frame::blank(64, 48);
        for share in frame.rgba.chunks_exact_mut(4) {
            share.copy_from_slice(&[0, 210, 0, 255]);
        }
        for y in 12..36 {
            for x in 20..44 {
                let at = (y * 64 + x) * 4;
                frame.rgba[at] = 200;
                frame.rgba[at + 1] = 160;
                frame.rgba[at + 2] = 140;
            }
        }
        let colour = [0.0, 210.0 / 255.0, 0.0];
        key_out(&mut frame, colour, 0.24, 0.12, 0.7);
        // The ground is gone.
        assert_eq!(frame.pixel(2, 2)[3], 0);
        // The subject stands.
        assert!(frame.pixel(32, 24)[3] > 250, "{:?}", frame.pixel(32, 24));
        let kept = frame.kept();
        let whole = (24.0 * 24.0) / (64.0 * 48.0);
        assert!((kept - whole).abs() < 0.02, "{kept} against {whole}");
        assert_eq!(frame.bounds(), Some((20, 12, 24, 24)));
    }

    // The green that lies in the edge of a subject is pushed back out of it.
    #[test]
    fn the_spill_of_a_screen_is_pushed_back() {
        let mut frame = Frame::blank(4, 1);
        frame.rgba[0..4].copy_from_slice(&[120, 200, 110, 255]);
        let colour = [0.0, 210.0 / 255.0, 0.0];
        key_out(&mut frame, colour, 0.05, 0.01, 1.0);
        let after = &frame.rgba[0..3];
        assert!(after[1] <= after[0].max(after[2]) + 20, "{after:?}");
    }

    // A blur of a plane keeps the total of a plane, and softens a line.
    #[test]
    fn a_blur_softens_a_line() {
        let mut frame = Frame::blank(16, 16);
        for y in 0..16 {
            for x in 0..8 {
                let at = (y * 16 + x) * 4;
                frame.rgba[at] = 255;
                frame.rgba[at + 1] = 255;
                frame.rgba[at + 2] = 255;
                frame.rgba[at + 3] = 255;
            }
        }
        blur(&mut frame, 3);
        assert!(frame.pixel(2, 8)[0] > 200, "the left of the line stays light");
        assert!(frame.pixel(11, 8)[0] < 60, "the right of the line is dark");
        assert!(frame.pixel(9, 8)[0] > 20 && frame.pixel(9, 8)[0] < 235, "the edge is soft");
    }

    // A feather softens the edge of a cut without taking the body of the subject away.
    #[test]
    fn a_feather_softens_an_edge() {
        let mut frame = Frame::blank(20, 4);
        for y in 0..4 {
            for x in 4..16 {
                let at = (y * 20 + x) * 4;
                frame.rgba[at + 3] = 255;
            }
        }
        feather(&mut frame, 2);
        assert!(frame.pixel(10, 2)[3] > 250, "the body of the cut stands");
        assert!(frame.pixel(1, 2)[3] == 0, "away from the cut nothing stands");
        assert!(frame.pixel(4, 2)[3] < 255 && frame.pixel(4, 2)[3] > 0, "the edge of the cut is soft");
    }

    // A choke takes the fringe of a key inwards.
    #[test]
    fn a_choke_takes_a_fringe_in() {
        let mut frame = Frame::blank(2, 1);
        frame.rgba[3] = 200;
        frame.rgba[7] = 40;
        choke(&mut frame, 0.2);
        assert!(frame.pixel(0, 0)[3] < 200 && frame.pixel(0, 0)[3] > 140);
        assert!(frame.pixel(1, 0)[3] < 40);
    }

    // A shadow is the alpha of a picture, moved and dimmed.
    #[test]
    fn a_shadow_follows_the_picture() {
        let mut frame = Frame::blank(8, 8);
        for y in 2..5 {
            for x in 2..5 {
                let at = (y * 8 + x) * 4;
                frame.rgba[at + 3] = 255;
            }
        }
        let made = shadow(&frame, [0.0, 0.0, 0.0], 2, 2, 1, 0.5);
        assert!(made.pixel(6, 6)[3] > 0, "the shadow stands where the picture was moved to");
        assert!(made.pixel(0, 0)[3] == 0, "and not where nothing was");
    }

    // Multiplying and dividing by an alpha are the reverse of one another.
    #[test]
    fn premultiply_and_demultiply_are_reversed() {
        let mut bytes = vec![200u8, 100, 50, 128];
        let was = bytes.clone();
        premultiply(&mut bytes);
        demultiply(&mut bytes);
        let near = (0..3).all(|place| (bytes[place] as i32 - was[place] as i32).abs() <= 1);
        assert!(near, "{bytes:?} against {was:?}");
    }

    // The clock of a file is read as seconds.
    #[test]
    fn a_clock_of_a_file_is_seconds() {
        assert_eq!(parse_clock("00:10:34.62"), Some(634.62));
        assert_eq!(parse_clock("00:00:05.00"), Some(5.0));
        assert_eq!(parse_clock("not a clock"), None);
    }

    // The measure of a stream line, which ffmpeg writes as `1920x1080 [SAR 1:1 DAR 16:9]`.
    #[test]
    fn the_measure_of_a_stream_is_read() {
        let line = "Stream #0:0(und): Video: h264 (High) (avc1 / 0x31637661), yuv420p, 1920x1080 [SAR 1:1 DAR 16:9], 3329 kb/s, 59.94 fps";
        assert_eq!(measure(line), Some((1920, 1080)));
        assert_eq!(measure("nothing of a measure"), None);
    }

    // A cap of the sides keeps the shape of a picture and keeps the sides even.
    #[test]
    fn a_cap_keeps_the_shape_and_even_sides() {
        assert_eq!(fit_cap(1920, 1080, 1280), (1280, 720));
        assert_eq!(fit_cap(1000, 700, 1000), (1000, 700));
        assert_eq!(fit_cap(1001, 701, 2000), (1000, 700));
    }

    // A frame that a machine without ffmpeg cannot read is still answered with a reason.
    #[test]
    fn a_sound_is_known_by_its_tail() {
        assert!(is_sound(&PathBuf::from("/tmp/one.mp3")));
        assert!(!is_sound(&PathBuf::from("/tmp/one.png")));
    }
}
