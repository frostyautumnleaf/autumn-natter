// The frames of a composition, made into one file.
//
// Three ways out, and the machine decides between two of them:
//
// * MP4, when the machine has `ffmpeg`. The frames are pushed into it as raw pixels down a pipe
//   as they are drawn, so nothing is written to the disk in between and a film of a minute costs
//   the memory of one picture.
// * An animated GIF, always. The encoder of GIFs is part of the program, so a bare machine still
//   gives a moving image. A GIF carries no sound, and its frames are thinned to fifteen a second.
// * One PNG of one moment, which needs nothing but the drawing itself.
//
// A writer is opened once, given every frame in the order of the film, and closed. The frames are
// drawn by `stage`, which knows nothing about files; this knows nothing about layers. The one
// thing this knows about layers is the sound that goes under them: a list of files, each with the
// moment of the film it starts at, which is turned into the words of a filter graph.
//
// The pixels come from the drawing premultiplied, as every drawing of SVG is. The writers of MP4
// and GIF want the colour on its own, so the multiply is undone on the way out.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use resvg::tiny_skia::Pixmap;

use crate::infographic::fonts::Fonts;
use crate::infographic::media;
use crate::infographic::story;

// The sound that goes under a film. Every entry is one input of the mixing.
#[derive(Clone, Debug)]
pub struct Sound {
    // The file of it, which is a sound, or a film whose own sound is kept.
    pub path: PathBuf,
    // The second of the film the sound starts being heard at.
    pub from: f64,
    // How much of the file is used. 0.0 is the rest of it.
    pub length: f64,
    // Where in the file the sound starts.
    pub in_at: f64,
    // The pace it is played at: 1.0 is the speed of the file. Only a range of 0.5 to 2.0 is
    // honoured, which is the range the tempo filter of ffmpeg accepts in one stage.
    pub speed: f64,
    // 1.0 is full.
    pub volume: f64,
    pub fade_in: f64,
    pub fade_out: f64,
}

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
    media::has_ffmpeg()
}

// The name of the file a render writes. The words of a name a model wrote are made safe for the
// disk: only letters, figures, the dash and the underline are let through.
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
    let head = if clean.is_empty() { "film" } else { clean };
    let tail = match form {
        Form::Mp4 => "mp4",
        Form::Gif => "gif",
        Form::Png => "png",
    };
    format!("{head}.{tail}")
}

// A document of SVG made into pixels. The document is written at the measure it is asked for, so
// a layer drawn for a box of 640 by 360 becomes 640 by 360 pixels and not a scaled form of them.
pub fn paint(svg: &str, width: u32, height: u32, fonts: &Fonts) -> Result<Pixmap, String> {
    let opt = resvg::usvg::Options {
        fontdb: fonts.database(),
        ..resvg::usvg::Options::default()
    };
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &opt)
        .map_err(|why| format!("a drawing could not be read: {why}"))?;
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| "the image is too big for this computer".to_string())?;
    let size = tree.size();
    let scale = if size.width() > 0.0 && size.height() > 0.0 {
        ((width as f32) / size.width()).min((height as f32) / size.height())
    } else {
        1.0
    };
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

// ---------------------------------------------------------------- writing

// A file being written. One frame at a time is handed to it, and it is closed at the end.
pub struct Writer {
    form: Form,
    path: PathBuf,
    fps: u32,
    width: u32,
    height: u32,
    inner: Inner,
    // How many frames have been offered, whatever became of them.
    frames: usize,
    // The pixels of one frame, made straight, kept between the frames so nothing is readied
    // again for every frame of a film.
    held: Vec<u8>,
}

enum Inner {
    // An MP4: the process, and the pipe its frames go down.
    Film { child: Child, feed: BufWriter<ChildStdin> },
    // A GIF: the encoder, how many of the frames of the film are let through, and how long each
    // of them stands.
    Moving {
        enc: gif::Encoder<BufWriter<std::fs::File>>,
        each: usize,
        delay: u16,
    },
    // A PNG: the first frame is the file, and there is no second.
    Still,
}

impl Writer {
    // Open the file. `sounds` is what goes under the film, which only an MP4 carries.
    pub fn open(
        room: &Path,
        name: &str,
        form: Form,
        width: u32,
        height: u32,
        fps: u32,
        sounds: &[Sound],
    ) -> Result<Self, String> {
        let fps = fps.clamp(story::LEAST_FPS, story::GREATEST_FPS);
        let path = room.join(out_name(name, form));
        let inner = match form {
            Form::Mp4 => {
                if !has_ffmpeg() {
                    return Err(media::need_ffmpeg());
                }
                create_room(room)?;
                let mut child = start_film(&path, width, height, fps, sounds)?;
                let feed = child
                    .stdin
                    .take()
                    .ok_or_else(|| "ffmpeg gave no pipe for the frames".to_string())?;
                Inner::Film { child, feed: BufWriter::new(feed) }
            }
            Form::Gif => {
                create_room(room)?;
                let file = std::fs::File::create(&path)
                    .map_err(|why| format!("the file could not be made: {why}"))?;
                // The longest delay a GIF may carry is a little over a second, and the shortest
                // is two hundredths of one, so twenty five a second is the most that is of any
                // use. Fifteen keeps the file of a minute of film sane.
                let kept = fps.min(15);
                let each = ((fps as f64 / kept as f64).round() as usize).max(1);
                let delay = ((100.0 / fps as f64) * each as f64).round().max(2.0) as u16;
                let enc = gif::Encoder::new(BufWriter::new(file), width as u16, height as u16, &[])
                    .map_err(|why| format!("the GIF could not be started: {why}"))?;
                Inner::Moving { enc, each, delay }
            }
            Form::Png => {
                create_room(room)?;
                Inner::Still
            }
        };
        Ok(Writer {
            form,
            path,
            fps,
            width,
            height,
            inner,
            frames: 0,
            held: Vec::new(),
        })
    }

    // The file the frames are going into.
    pub fn path(&self) -> &Path {
        &self.path
    }

    // The frames of the second the file is written at.
    pub fn fps(&self) -> u32 {
        self.fps
    }

    // The form of the file.
    pub fn form(&self) -> Form {
        self.form
    }

    // How many frames were offered.
    pub fn frames(&self) -> usize {
        self.frames
    }

    // One frame, at the moment of the film it stands at. The frames are offered in order.
    pub fn push(&mut self, pixmap: &Pixmap, at: f64) -> Result<(), String> {
        if pixmap.width() != self.width || pixmap.height() != self.height {
            return Err(format!(
                "a frame of {} by {} was offered to a file of {} by {}",
                pixmap.width(),
                pixmap.height(),
                self.width,
                self.height
            ));
        }
        let number = self.frames;
        self.frames += 1;
        match &mut self.inner {
            Inner::Film { feed, .. } => {
                straight(pixmap, &mut self.held);
                feed.write_all(&self.held)
                    .map_err(|why| format!("a frame could not be sent to ffmpeg: {why}"))?;
                let _ = at;
                Ok(())
            }
            Inner::Moving { enc, each, delay } => {
                if !number.is_multiple_of(*each) {
                    return Ok(());
                }
                straight(pixmap, &mut self.held);
                let mut frame =
                    gif::Frame::from_rgba_speed(self.width as u16, self.height as u16, &mut self.held, 10);
                frame.delay = *delay;
                frame.dispose = gif::DisposalMethod::Background;
                enc.write_frame(&frame)
                    .map_err(|why| format!("a frame could not be written into the GIF: {why}"))
            }
            Inner::Still => {
                let bytes = pixmap
                    .encode_png()
                    .map_err(|why| format!("the picture could not be written: {why}"))?;
                std::fs::write(&self.path, bytes)
                    .map_err(|why| format!("{} could not be written: {why}", self.path.display()))
            }
        }
    }

    // End the file. An MP4 is not finished until its pipe is closed and the process has stopped,
    // which is where a fault of the mixing shows itself.
    pub fn close(self) -> Result<PathBuf, String> {
        match self.inner {
            Inner::Film { child, feed } => {
                // Dropping the pipe is how ffmpeg is told the frames are all in.
                drop(feed);
                let made = child
                    .wait_with_output()
                    .map_err(|why| format!("ffmpeg could not be waited for: {why}"))?;
                if !made.status.success() {
                    let said = media::tail_of(&made.stderr);
                    // A file of nothing at all is worse than no file, so it is taken away.
                    let _ = std::fs::remove_file(&self.path);
                    return Err(format!("the film could not be written: {said}"));
                }
                Ok(self.path)
            }
            Inner::Moving { enc, .. } => {
                enc.into_inner()
                    .map_err(|why| format!("the GIF could not be closed: {why}"))?;
                Ok(self.path)
            }
            Inner::Still => {
                if self.frames == 0 {
                    return Err("no frame was offered to the picture".to_string());
                }
                Ok(self.path)
            }
        }
    }

    // Stop the file and take it away, which is what a render that was interrupted or that failed
    // half way through does: a part of a video is worse than no video, because it looks finished.
    pub fn abort(mut self) {
        if let Inner::Film { child, .. } = &mut self.inner {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

// The folder a file is written into, made when it is not there.
fn create_room(room: &Path) -> Result<(), String> {
    std::fs::create_dir_all(room)
        .map_err(|why| format!("the folder of the film could not be made: {why}"))
}

// The pixels of a drawing, made straight, in the buffer they are written from.
fn straight(pixmap: &Pixmap, held: &mut Vec<u8>) {
    held.clear();
    held.extend_from_slice(pixmap.data());
    for share in held.chunks_exact_mut(4) {
        unpremultiply(share);
    }
}

// Pixels come from the renderer with the colour already multiplied by the alpha. The writers of
// the files want the colour on its own.
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

// ---------------------------------------------------------------- the film

// The process of an MP4, with the frames coming down a pipe and the sound mixed under them.
fn start_film(
    path: &Path,
    width: u32,
    height: u32,
    fps: u32,
    sounds: &[Sound],
) -> Result<Child, String> {
    let mut args: Vec<String> = vec![
        String::from("-hide_banner"),
        String::from("-loglevel"),
        String::from("error"),
        String::from("-y"),
        // The frames are the first input, and they come in as they are drawn.
        String::from("-f"),
        String::from("rawvideo"),
        String::from("-pix_fmt"),
        String::from("rgba"),
        String::from("-s"),
        format!("{width}x{height}"),
        String::from("-framerate"),
        format!("{fps}"),
        String::from("-i"),
        String::from("-"),
    ];
    for sound in sounds {
        if !sound.path.exists() {
            return Err(format!("the sound {} is not there", sound.path.display()));
        }
        args.push(String::from("-ss"));
        args.push(format!("{:.3}", sound.in_at.max(0.0)));
        if sound.length > 0.0 {
            args.push(String::from("-t"));
            args.push(format!("{:.3}", sound.length));
        }
        args.push(String::from("-i"));
        args.push(sound.path.display().to_string());
    }
    // The graph of the mixing: every sound turned to its level, held to its moment, faded at its
    // two ends, and all of them poured into one.
    let mut graph = String::new();
    let mut pads: Vec<String> = Vec::new();
    for (place, sound) in sounds.iter().enumerate() {
        let input = place + 1;
        let volume = sound.volume.max(0.0);
        let delay = (sound.from.max(0.0) * 1000.0).round() as i64;
        let mut line = format!("[{input}:a]volume={volume:.3}");
        let speed = if sound.speed.abs() < 0.06 { 1.0 } else { sound.speed.abs().clamp(0.5, 2.0) };
        if (speed - 1.0).abs() > 0.01 {
            line.push_str(&format!(",atempo={speed:.3}"));
        }
        if delay > 0 {
            line.push_str(&format!(",adelay={delay}|{delay}|{delay}"));
        }
        if sound.fade_in > 0.0 {
            line.push_str(&format!(
                ",afade=t=in:st={:.3}:d={:.3}",
                delay as f64 / 1000.0,
                sound.fade_in
            ));
        }
        if sound.fade_out > 0.0 {
            // The fade out is measured back from the end of what is heard, so the length of the
            // sound is wanted. Where it is not known the fade is left out rather than guessed at.
            let whole = if sound.length > 0.0 {
                sound.length
            } else {
                media::probe(&sound.path)
                    .map(|one| (one.duration - sound.in_at).max(0.0))
                    .unwrap_or(0.0)
            };
            if whole > sound.fade_out {
                line.push_str(&format!(
                    ",afade=t=out:st={:.3}:d={:.3}",
                    delay as f64 / 1000.0 + whole - sound.fade_out,
                    sound.fade_out
                ));
            }
        }
        let pad = format!("s{place}");
        graph.push_str(&format!("{line}[{pad}];"));
        pads.push(pad);
    }
    let mut maps: Vec<String> = vec![String::from("-map"), String::from("0:v:0")];
    if !pads.is_empty() {
        let out = if pads.len() == 1 {
            format!("[{}]", pads[0])
        } else {
            graph.push_str(&format!(
                "{}amix=inputs={}:duration=longest:normalize=0[a]",
                pads.iter().map(|one| format!("[{one}]")).collect::<String>(),
                pads.len()
            ));
            String::from("[a]")
        };
        maps.push(String::from("-map"));
        maps.push(out);
    }
    let mut command = Command::new("ffmpeg");
    for one in &args {
        command.arg(one);
    }
    if !graph.is_empty() {
        command.args(["-filter_complex", &graph]).args(&maps);
        command.args(["-c:a", "aac", "-b:a", "192k"]);
    } else {
        command.args(&maps);
    }
    command.args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "20"]);
    // The colour form of an MP4 wants an even number of pixels on each side, which a canvas of
    // an odd measure does not give.
    if width % 2 == 1 || height % 2 == 1 {
        command.args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"]);
    }
    command.args(["-pix_fmt", "yuv420p", "-movflags", "+faststart", "-shortest"]);
    command.arg(path);
    command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    command.spawn().map_err(|why| {
        format!(
            "ffmpeg could not be started: {why}. Ask for a gif instead, which this program \
             writes by itself."
        )
    })
}

#[cfg(test)]
mod checks {
    use super::*;

    // The name of a file is made of the words of a name a model wrote, and the end of the form.
    #[test]
    fn the_name_of_a_file_is_made_from_the_words_of_a_name() {
        assert_eq!(out_name("My Film 2", Form::Mp4), "My-Film-2.mp4");
        assert_eq!(out_name("  ", Form::Gif), "film.gif");
        assert_eq!(out_name("plain", Form::Png), "plain.png");
    }

    // The forms are read from the words a model writes, and an empty word is the MP4.
    #[test]
    fn the_form_is_read_from_a_word() {
        assert_eq!(form_of(""), Some(Form::Mp4));
        assert_eq!(form_of("movie"), Some(Form::Mp4));
        assert_eq!(form_of("gif"), Some(Form::Gif));
        assert_eq!(form_of("still"), Some(Form::Png));
        assert_eq!(form_of("tiff"), None);
    }

    // A document of SVG is painted at the measure it is written at, and the pixels are the colour
    // that was asked for.
    #[test]
    fn a_drawing_is_painted_at_its_own_measure() {
        let fonts = Fonts::new();
        let svg = String::from(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"20\">",
        ) + "<rect x=\"0\" y=\"0\" width=\"40\" height=\"20\" fill=\"#c2410c\"/></svg>";
        let made = paint(&svg, 40, 20, &fonts).expect("the drawing is painted");
        assert_eq!((made.width(), made.height()), (40, 20));
        let pixel = made.pixel(4, 4).expect("a pixel");
        assert!(pixel.red() > 180 && pixel.green() < 90, "{pixel:?}");
    }

    // A GIF is written by this program, so a machine without ffmpeg still gives a moving picture.
    #[test]
    fn a_gif_is_written_by_this_program() {
        let room = temp_room("encode-gif");
        let mut made =
            Writer::open(&room, "check", Form::Gif, 8, 8, 24, &[]).expect("a GIF is opened");
        let pixmap = Pixmap::new(8, 8).expect("a frame");
        made.push(&pixmap, 0.0).expect("a frame is written");
        made.push(&pixmap, 0.042).expect("a second frame is written");
        let path = made.close().expect("the GIF is closed");
        let bytes = std::fs::read(&path).expect("the file is read");
        assert_eq!(&bytes[0..6], b"GIF89a", "the file is a GIF of the writers of this program");
        let _ = std::fs::remove_dir_all(&room);
    }

    // A frame of the wrong measure is refused, so a fault of the compositor is not written into
    // the middle of a file.
    #[test]
    fn a_frame_of_the_wrong_measure_is_refused() {
        let room = temp_room("encode-refuse");
        let mut made = Writer::open(&room, "check", Form::Gif, 8, 8, 10, &[]).expect("opened");
        let wrong = Pixmap::new(4, 4).expect("a frame");
        assert!(made.push(&wrong, 0.0).is_err(), "a frame of 4 by 4 is refused");
        let _ = std::fs::remove_dir_all(&room);
    }

    // A folder of the tests, under the name this check is known by.
    fn temp_room(name: &str) -> PathBuf {
        let room = std::env::temp_dir().join(format!("autumn-natter-{name}"));
        let _ = std::fs::remove_dir_all(&room);
        std::fs::create_dir_all(&room).expect("the folder is made");
        room
    }
}
