// The drawing of one frame of an infographic.
//
// A frame is a picture of lines: a document of SVG, written out for one moment
// of the video. Lines are the right shape for this work. They are exact at every
// size, they cost nothing to state, and the whole of a scene is readable by a
// person and by a model alike -- a frame may be looked at as words when
// something looks wrong.
//
// The moment decides everything else. `at` finds the scene that stands at the
// moment, the way it comes in from the one before it, and how far each block has
// come in. A block that has not started is left out of the words altogether, so
// the picture carries no weight for a thing that is not to be seen.
//
// Nothing here measures words or places a block: `layout.rs` did that, and the
// same measure is what `check` reports on. Here the places are only drawn.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

use crate::infographic::fonts::Fonts;
use crate::infographic::layout::{self, Line, Placed, Rect, Row, R_AXIS, R_HEAD, R_ITEM, R_LEGEND, R_POINT, R_PILL, R_ROW, R_STEP};
use crate::infographic::marks;
use crate::infographic::script::{self, Background, Palette, Project};

// How far a block has come in, at one moment.
#[derive(Clone, Copy, Debug)]
struct Come {
    // The share of the block that is to be seen, from 0 to 1, eased.
    part: f64,
    // The share of the words of the block that are to be seen, for the typing
    // shape of coming in.
    typed: f64,
}

// The drawing of a frame. The fonts are borrowed for the time the frame is
// built, and no longer.
struct Board<'a> {
    out: String,
    defs: String,
    // The next number for the names of the pieces of a frame.
    next: usize,
    palette: Palette,
    fonts: &'a Fonts,
    board: Rect,
}

impl<'a> Board<'a> {
    fn id(&mut self, head: &str) -> String {
        self.next += 1;
        format!("{head}{}", self.next)
    }

    fn put(&mut self, words: &str) {
        self.out.push_str(words);
    }

    fn putf(&mut self, words: String) {
        self.out.push_str(&words);
    }
}

// One frame of the video, as a document of SVG.
pub fn at(project: &Project, fonts: &Fonts, ms: f64, room: Option<&std::path::Path>) -> String {
    let size = script::canvas_of(&project.canvas).unwrap_or(script::Canvas {
        word: "1080p",
        width: 1920,
        height: 1080,
    });
    let (width, height) = (size.width, size.height);
    let palette = script::colours_of(project);
    let board = layout::stage(width, height);
    let place = standing(project, ms);

    let mut board_state = Board {
        out: String::new(),
        defs: String::new(),
        next: 0,
        palette: palette.clone(),
        fonts,
        board,
    };

    match place {
        Standing::One(index, local) => {
            let words = scene(&mut board_state, project, index, local, room);
            let mut frame = head(width, height);
            frame.push_str(&defsheet(&board_state.defs));
            frame.push_str(&words);
            frame.push_str("</svg>");
            frame
        }
        Standing::Between(before, after, part, local) => {
            // The change from one scene to the next: both are drawn, and the way
            // the new one comes decides how.
            let transition = project
                .scenes
                .get(after)
                .map(|one| one.transition.clone())
                .unwrap_or_default();
            let old = scene(&mut board_state, project, before, f64::MAX, room);
            let new = scene(&mut board_state, project, after, local, room);
            let made = change(&mut board_state, &transition, old, new, part, width, height);
            let mut frame = head(width, height);
            frame.push_str(&defsheet(&board_state.defs));
            frame.push_str(&made);
            frame.push_str("</svg>");
            frame
        }
        Standing::Nothing => {
            let mut frame = head(width, height);
            frame.push_str(&defsheet(&board_state.defs));
            frame.push_str(&solid(
                &palette.bg,
                f64::from(width),
                f64::from(height),
            ));
            frame.push_str("</svg>");
            frame
        }
    }
}

// Which scene stands at a moment, and how far the change from the one before it
// has come.
enum Standing {
    One(usize, f64),
    Between(usize, usize, f64, f64),
    Nothing,
}

fn standing(project: &Project, ms: f64) -> Standing {
    if project.scenes.is_empty() {
        return Standing::Nothing;
    }
    let mut at = 0.0;
    for (index, scene) in project.scenes.iter().enumerate() {
        let length = scene.seconds * 1000.0;
        if ms < at + length || index + 1 == project.scenes.len() {
            let local = (ms - at).max(0.0);
            let change = scene.transition.ms.max(0.0);
            if index > 0 && change > 0.0 && local < change {
                return Standing::Between(
                    index - 1,
                    index,
                    ease(local / change),
                    local,
                );
            }
            return Standing::One(index, local);
        }
        at += length;
    }
    Standing::Nothing
}

// The head of a document of SVG.
fn head(width: u32, height: u32) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">"
    )
}

// The pieces a frame borrows, put in the box the renderer reads them from. An
// empty box is left out, for a frame with nothing in it is read faster.
fn defsheet(defs: &str) -> String {
    if defs.is_empty() {
        return String::new();
    }
    format!("<defs>{defs}</defs>")
}

// The way one scene comes over the one before it.
fn change(
    board: &mut Board,
    transition: &script::Transition,
    old: String,
    new: String,
    part: f64,
    width: u32,
    height: u32,
) -> String {
    let w = f64::from(width);
    let h = f64::from(height);
    match transition.kind.as_str() {
        "cut" => new,
        "zoom" => {
            // The new scene grows into place while the old one goes away.
            let scale = 0.94 + 0.06 * part;
            let fade = part;
            format!(
                "<g opacity=\"{:.3}\">{old}</g><g opacity=\"{:.3}\" transform=\"translate({:.1} {:.1}) scale({:.4}) translate({:.1} {:.1})\">{new}</g>",
                1.0 - part * 0.6,
                fade,
                w / 2.0,
                h / 2.0,
                scale,
                w / 2.0,
                h / 2.0
            )
        }
        "wipe" => {
            let id = board.id("clip");
            let (rect, _) = match transition.dir.as_str() {
                "right" => (
                    format!("<rect x=\"0\" y=\"0\" width=\"{:.1}\" height=\"{h}\"/>", w * part),
                    0.0,
                ),
                "up" => (
                    format!("<rect x=\"0\" y=\"{:.1}\" width=\"{w}\" height=\"{:.1}\"/>", h - h * part, h * part),
                    0.0,
                ),
                "down" => (
                    format!("<rect x=\"0\" y=\"0\" width=\"{w}\" height=\"{:.1}\"/>", h * part),
                    0.0,
                ),
                _ => (
                    format!("<rect x=\"{:.1}\" y=\"0\" width=\"{:.1}\" height=\"{h}\"/>", w - w * part, w * part),
                    0.0,
                ),
            };
            board.defs.push_str(&format!(
                "<clipPath id=\"{id}\">{rect}</clipPath>"
            ));
            format!(
                "<g>{old}</g><g clip-path=\"url(#{id})\">{new}</g>",
            )
        }
        "slide" => {
            let (dx, dy) = match transition.dir.as_str() {
                "right" => (-(w * (1.0 - part)), 0.0),
                "up" => (0.0, h * (1.0 - part)),
                "down" => (0.0, -(h * (1.0 - part))),
                _ => (w * (1.0 - part), 0.0),
            };
            format!(
                "<g opacity=\"{:.3}\">{old}</g><g transform=\"translate({:.1} {:.1})\">{new}</g>",
                1.0 - part,
                w - dx,
                dy
            )
        }
        _ => {
            // A fade: the old scene is left where it is and the new one comes up
            // over it.
            format!(
                "<g>{old}</g><g opacity=\"{:.3}\">{new}</g>",
                part
            )
        }
    }
}

// One scene: its ground, and every block that stands at the moment.
fn scene(
    board: &mut Board,
    project: &Project,
    index: usize,
    local: f64,
    room: Option<&std::path::Path>,
) -> String {
    let Some(scene) = project.scenes.get(index) else {
        return String::new();
    };
    let held = layout::ground(&scene.background, &project.background);
    // The palette is taken aside, so the board may be drawn on while the colours
    // of the ground are read.
    let colours = board.palette.clone();
    let mut out = ground(board, &held, &colours);
    let (placed, _) = layout::scene(
        project,
        &board.palette,
        board.fonts,
        &scene.blocks,
        board.board,
        room,
    );
    for (at, block) in placed.iter().enumerate() {
        let delay = block.delay + block.stagger * 0.0;
        let part = if local == f64::MAX {
            1.0
        } else {
            come(local, delay, block.reveal_ms)
        };
        if part <= 0.0 {
            continue;
        }
        match block.kind.as_str() {
            "head" | "text" | "quote" => words(board, block, part, local),
            "bullets" => bullets(board, block, part, local),
            "stat" => stat(board, block, part),
            "bars" => bars(board, block, part, local),
            "line" => line_chart(board, block, part, local),
            "pie" => pie(board, block, part, local),
            "compare" => compare(board, block, part, local),
            "steps" => steps(board, block, part, local),
            "timeline" => timeline(board, block, part, local),
            "table" => table(board, block, part, local),
            "code" => code(board, block, part, local),
            "pill" => pills(board, block, part, local),
            "progress" => progress(board, block, part, local),
            "image" => image(board, block, part),
            "icon" => icon(board, block, part),
            "shape" => shape(board, block, part),
            "rule" => rule(board, block, part),
            "svg" => raw(board, block, part),
            _ => {}
        }
    }
    out.push_str(&board.out);
    board.out.clear();
    out
}

// The ground of a scene.
fn ground(board: &mut Board, held: &Background, palette: &Palette) -> String {
    let width = board.board.w + board.board.x * 2.0;
    let height = board.board.h + board.board.y * 2.0;
    let kind = if held.kind.is_empty() { "solid" } else { held.kind.as_str() };
    let mut out = String::new();
    match kind {
        "none" => {}
        "gradient" => {
            let from = if held.from.is_empty() {
                palette.bg.clone()
            } else {
                held.from.clone()
            };
            let to = if held.to.is_empty() {
                palette.panel.clone()
            } else {
                held.to.clone()
            };
            let id = board.id("g");
            let (x1, y1, x2, y2) = along(held.angle, width, height);
            board.defs.push_str(&format!(
                "<linearGradient id=\"{id}\" x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\" gradientUnits=\"userSpaceOnUse\"><stop offset=\"0\" stop-color=\"{}\"/><stop offset=\"1\" stop-color=\"{}\"/></linearGradient>",
                colour(&from),
                colour(&to)
            ));
            out.push_str(&format!(
                "<rect x=\"0\" y=\"0\" width=\"{width:.1}\" height=\"{height:.1}\" fill=\"url(#{id})\"/>"
            ));
        }
        "grid" | "dots" => {
            out.push_str(&solid(
                &if held.from.is_empty() {
                    palette.bg.clone()
                } else {
                    held.from.clone()
                },
                width,
                height,
            ));
            let step = if held.spacing > 0.0 {
                held.spacing * width
            } else {
                width / 24.0
            };
            let line = if held.to.is_empty() {
                palette.line.clone()
            } else {
                held.to.clone()
            };
            if kind == "grid" {
                let mut x = step;
                while x < width {
                    out.push_str(&format!(
                        "<line x1=\"{x:.1}\" y1=\"0\" x2=\"{x:.1}\" y2=\"{height:.1}\" stroke=\"{}\" stroke-width=\"1\" opacity=\"0.5\"/>",
                        colour(&line)
                    ));
                    x += step;
                }
                let mut y = step;
                while y < height {
                    out.push_str(&format!(
                        "<line x1=\"0\" y1=\"{y:.1}\" x2=\"{width:.1}\" y2=\"{y:.1}\" stroke=\"{}\" stroke-width=\"1\" opacity=\"0.5\"/>",
                        colour(&line)
                    ));
                    y += step;
                }
            } else {
                let mut y = step;
                while y < height {
                    let mut x = step;
                    while x < width {
                        out.push_str(&format!(
                            "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"1.6\" fill=\"{}\" opacity=\"0.55\"/>",
                            colour(&line)
                        ));
                        x += step;
                    }
                    y += step;
                }
            }
        }
        _ => {
            out.push_str(&solid(
                &if held.from.is_empty() {
                    palette.bg.clone()
                } else {
                    held.from.clone()
                },
                width,
                height,
            ));
        }
    }
    if held.vignette > 0.0 {
        let id = board.id("v");
        let shade = held.vignette.clamp(0.0, 1.0) * 0.85;
        board.defs.push_str(&format!(
            "<radialGradient id=\"{id}\" cx=\"0.5\" cy=\"0.5\" r=\"0.75\"><stop offset=\"0.5\" stop-color=\"#000000\" stop-opacity=\"0\"/><stop offset=\"1\" stop-color=\"#000000\" stop-opacity=\"{shade:.3}\"/></radialGradient>"
        ));
        out.push_str(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{width:.1}\" height=\"{height:.1}\" fill=\"url(#{id})\"/>"
        ));
    }
    out
}

fn solid(colour_of: &str, width: f64, height: f64) -> String {
    format!(
        "<rect x=\"0\" y=\"0\" width=\"{width:.1}\" height=\"{height:.1}\" fill=\"{}\"/>",
        colour(colour_of)
    )
}

// The two ends of a gradient, from a way in degrees.
fn along(angle: f64, width: f64, height: f64) -> (f64, f64, f64, f64) {
    let radians = angle.to_radians();
    let (dx, dy) = (radians.cos(), radians.sin());
    let x1 = width / 2.0 - dx * width / 2.0;
    let y1 = height / 2.0 - dy * height / 2.0;
    let x2 = width / 2.0 + dx * width / 2.0;
    let y2 = height / 2.0 + dy * height / 2.0;
    (x1, y1, x2, y2)
}

// ---------------------------------------------------------------- the blocks

// A block of words: a head, a piece of text, or a quote.
fn words(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let typed = if block.reveal == "type" {
        come(local, block.delay, block.reveal_ms.max(900.0))
    } else {
        1.0
    };
    let stagger = block.stagger > 0.0;
    let mut used: Vec<String> = Vec::new();
    let mut shown = 0usize;
    let total: usize = block.lines.iter().map(|one| word_count(one)).sum();
    let up_to = (total as f64 * typed).ceil() as usize;
    for (at, line) in block.lines.iter().enumerate() {
        if line.runs.is_empty() {
            used.push(String::new());
            continue;
        }
        let line_part = if stagger {
            come(local, block.delay + block.stagger * at as f64, block.reveal_ms)
        } else {
            part
        };
        if line_part <= 0.0 {
            continue;
        }
        let taken = up_to.saturating_sub(shown);
        shown += word_count(line);
        let held = clip_words(line, taken);
        if held.runs.is_empty() {
            continue;
        }
        used.push(line_words(
            board,
            &held,
            block,
            at,
            line_part,
        ));
    }
    let body = used.join("");
    let quote = if block.kind == "quote" {
        let size = block.size * 2.2;
        format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{size:.1}\" fill=\"{}\" opacity=\"0.35\">&#8220;</text>",
            block.bx.x + block.pad,
            block.bx.y + block.pad + size * 0.8,
            family(board),
            colour(&block.colour)
        )
    } else {
        String::new()
    };
    wrap_block(board, block, part, format!("{panel}{quote}{body}"));
}

// One line of words, placed in the box.
fn line_words(
    board: &mut Board,
    line: &Line,
    block: &Placed,
    index: usize,
    part: f64,
) -> String {
    let inner = block.bx.inset(block.pad);
    let first = first_line_y(block, inner);
    let y = first + index as f64 * block.step;
    let x = match block.align.as_str() {
        "center" => inner.centre() - line.width / 2.0,
        "right" => inner.right() - line.width,
        _ => inner.x,
    };
    let slide = slide_of(block, part);
    let mut runs = String::new();
    for run in &line.runs {
        runs.push_str(&format!(
            "<tspan fill=\"{}\" font-weight=\"{}\"{}>{}</tspan>",
            colour(&run.colour),
            run.weight,
            if run.italic { " font-style=\"italic\"" } else { "" },
            said(&run.text)
        ));
    }
    format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" text-anchor=\"start\" opacity=\"{:.3}\" transform=\"translate({:.1} {:.1})\">{runs}</text>",
        x + slide.0,
        y + slide.1,
        family(board),
        line.size,
        block.opacity * part,
        0.0,
        0.0
    )
}

// The foot of the first line of a block, with the vertical place taken in.
fn first_line_y(block: &Placed, inner: Rect) -> f64 {
    let lines = block.lines.len().max(1) as f64;
    let taken = lines * block.step;
    let top = match block.valign.as_str() {
        "middle" => inner.middle() - taken / 2.0,
        "bottom" => inner.bottom() - taken,
        _ => inner.y,
    };
    top + block.size * 0.94
}

// A list of lines with marks beside them.
fn bullets(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let mut body = String::new();
    for (at, row) in block.rows.iter().enumerate() {
        let held = if block.stagger > 0.0 {
            come(local, block.delay + block.stagger * at as f64, block.reveal_ms)
        } else {
            part
        };
        if held <= 0.0 {
            continue;
        }
        body.push_str(&one_row(board, block, row, held, at + 1));
    }
    wrap_block(board, block, part, format!("{panel}{body}"));
}

// One row of a list, one side of a compare.
fn one_row(board: &mut Board, block: &Placed, row: &Row, part: f64, number: usize) -> String {
    let mut out = String::new();
    let size = row.lines.first().map(|one| one.size).unwrap_or(block.size);
    let mark_room = if row.mark == "none" {
        0.0
    } else {
        size * 1.15 * 1.6
    };
    if row.mark != "none" {
        let cx = row.bx.x + mark_room / 2.0;
        let cy = row.bx.y + size * 0.62;
        out.push_str(&mark_at(
            board,
            &row.mark,
            cx,
            cy,
            size * 1.15,
            &if row.colour.is_empty() {
                block.colour.clone()
            } else {
                row.colour.clone()
            },
            number,
            part,
        ));
    }
    for (at, line) in row.lines.iter().enumerate() {
        let x = row.bx.x + mark_room;
        let y = row.bx.y + at as f64 * size * block_leading(block, size) + size * 0.94;
        let mut runs = String::new();
        for run in &line.runs {
            runs.push_str(&format!(
                "<tspan fill=\"{}\" font-weight=\"{}\">{}</tspan>",
                colour(&run.colour),
                run.weight,
                said(&run.text)
            ));
        }
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" opacity=\"{:.3}\">{runs}</text>",
            x,
            y,
            family(board),
            size,
            block.opacity * part
        ));
    }
    let mut sub_y = row.bx.y + row.lines.len() as f64 * size * block_leading(block, size);
    for (at, line) in row.sub.iter().enumerate() {
        let sub_size = line.size;
        let y = sub_y + at as f64 * sub_size * 1.25 + sub_size * 0.94;
        let mut runs = String::new();
        for run in &line.runs {
            runs.push_str(&format!(
                "<tspan fill=\"{}\">{}</tspan>",
                colour(&run.colour),
                said(&run.text)
            ));
        }
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" opacity=\"{:.3}\">{runs}</text>",
            row.bx.x + mark_room,
            y,
            family(board),
            sub_size,
            block.opacity * part * 0.95
        ));
        sub_y += sub_size * 1.25;
    }
    out
}

fn block_leading(block: &Placed, size: f64) -> f64 {
    if size <= 0.0 {
        return 1.2;
    }
    (block.step / size).clamp(1.0, 2.0)
}

// A number, big, with a line under it.
fn stat(board: &mut Board, block: &Placed, part: f64) {
    let panel = panel(board, block, part);
    let inner = block.bx.inset(block.pad);
    let wide = board
        .fonts
        .width(&block.number, &board.fonts.body, 700, false, block.number_size);
    let x = match block.align.as_str() {
        "center" => inner.centre() - wide / 2.0,
        "right" => inner.right() - wide,
        _ => inner.x,
    };
    let taken = block.number_size + if block.lines.is_empty() {
        0.0
    } else {
        block.lines.len() as f64 * block.number_size * 0.26 * 1.2 + block.number_size * 0.14
    };
    let top = match block.valign.as_str() {
        "middle" => inner.middle() - taken / 2.0,
        "bottom" => inner.bottom() - taken,
        _ => inner.y,
    };
    let slide = slide_of(block, part);
    let mut out = format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"{}\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
        x + slide.0,
        top + block.number_size * 0.92 + slide.1,
        family(board),
        block.number_size,
        700,
        colour(&block.colour),
        block.opacity * part,
        said(&count_to(&block.number, part))
    );
    let mut y = top + block.number_size * 1.12;
    for line in &block.lines {
        let label_size = line.size;
        let label_wide = board
            .fonts
            .width(
                &line.runs.iter().map(|one| one.text.as_str()).collect::<String>(),
                &board.fonts.body,
                400,
                false,
                label_size,
            );
        let lx = match block.align.as_str() {
            "center" => inner.centre() - label_wide / 2.0,
            "right" => inner.right() - label_wide,
            _ => inner.x,
        };
        y += label_size * 1.15;
        let mut runs = String::new();
        for run in &line.runs {
            runs.push_str(&format!(
                "<tspan fill=\"{}\">{}</tspan>",
                colour(&run.colour),
                said(&run.text)
            ));
        }
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" opacity=\"{:.3}\">{runs}</text>",
            lx + slide.0,
            y + slide.1,
            family(board),
            label_size,
            block.opacity * part
        ));
    }
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// A bar chart: the axis, the grid, the bars, and the words under them.
fn bars(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let mut out = axis(board, block, part);
    let horizontal = block.rows.iter().any(|one| one.role == R_AXIS)
        && block.chart.bars.iter().any(|one| one.bx.w < one.bx.h);
    let grow = grow_to(block, part, local);
    for one in &block.chart.bars {
        let bx = if horizontal {
            Rect::new(one.bx.x, one.bx.y, one.bx.w * grow, one.bx.h)
        } else {
            Rect::new(one.bx.x, one.bx.bottom() - one.bx.h * grow, one.bx.w, one.bx.h * grow)
        };
        if bx.w < 0.4 || bx.h < 0.4 {
            continue;
        }
        let round = (bx.w.min(bx.h) * 0.16).min(block.size * 0.22);
        out.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
            bx.x,
            bx.y,
            bx.w,
            bx.h,
            round,
            colour(&one.colour),
            block.opacity
        ));
        if shows_values(block) {
            let words = layout::number_words(
                one.value,
                "",
                "",
                if block.kind == "bars" { -1 } else { -1 },
            );
            let size = block.size * 0.82;
            let wide = board.fonts.width(&words, &board.fonts.body, 700, false, size);
            let (x, y) = if horizontal {
                (bx.right() + size * 0.4, bx.middle() + size * 0.34)
            } else {
                (bx.centre() - wide / 2.0, bx.y - size * 0.4)
            };
            out.push_str(&format!(
                "<text x=\"{x:.1}\" y=\"{y:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"700\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
                family(board),
                size,
                colour(&board.palette.ink),
                block.opacity,
                said(&words)
            ));
        }
    }
    out.push_str(&axis_labels(board, block, part));
    out.push_str(&key(board, block, part));
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// A line chart: the axis, the lines, the places of the numbers, and the key.
fn line_chart(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let mut out = axis(board, block, part);
    let grow = grow_to(block, part, local);
    let id = board.id("clip");
    let plot = block.chart.plot;
    board.defs.push_str(&format!(
        "<clipPath id=\"{id}\"><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\"/></clipPath>",
        plot.x - 2.0,
        plot.y - plot.h * 0.05 - 4.0,
        (plot.w + 4.0) * grow,
        plot.h * 1.1 + 8.0
    ));
    out.push_str(&format!("<g clip-path=\"url(#{id})\">"));
    for one in &block.chart.lines {
        if one.points.len() < 2 {
            if let Some((x, y)) = one.points.first() {
                out.push_str(&format!(
                    "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
                    one.width * 1.4,
                    colour(&one.colour),
                    block.opacity
                ));
            }
            continue;
        }
        let path = one
            .points
            .iter()
            .enumerate()
            .map(|(at, (x, y))| format!("{}{:.1} {:.1}", if at == 0 { "M" } else { " L" }, x, y))
            .collect::<String>();
        if one.fill {
            let last = one.points.len() - 1;
            let area = format!(
                "{path} L{:.1} {:.1} L{:.1} {:.1} Z",
                one.points[last].0,
                block.chart.zero,
                one.points[0].0,
                block.chart.zero
            );
            out.push_str(&format!(
                "<path d=\"{area}\" fill=\"{}\" opacity=\"{:.3}\"/>",
                colour(&one.colour),
                block.opacity * 0.2
            ));
        }
        out.push_str(&format!(
            "<path d=\"{path}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.1}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"{} opacity=\"{:.3}\"/>",
            colour(&one.colour),
            one.width,
            if one.dashed {
                " stroke-dasharray=\"6 5\""
            } else {
                ""
            },
            block.opacity
        ));
        for (x, y) in &one.points {
            out.push_str(&format!(
                "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
                one.width * 1.15,
                colour(&one.colour),
                block.opacity
            ));
        }
    }
    out.push_str("</g>");
    out.push_str(&axis_labels(board, block, part));
    out.push_str(&key(board, block, part));
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// A pie or a ring: the wedges, and the key beside them.
fn pie(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let grow = grow_to(block, part, local);
    let mut out = String::new();
    let cx = block.chart.cx;
    let cy = block.chart.cy;
    let radius = block.chart.radius;
    let inner = radius * block.hole;
    let mut first = true;
    for one in &block.chart.wedges {
        let to = one.from + (one.to - one.from) * grow;
        if to - one.from <= 0.02 {
            continue;
        }
        out.push_str(&format!(
            "<path d=\"{}\" fill=\"{}\" opacity=\"{:.3}\"/>",
            wedge(cx, cy, radius, inner, one.from, to),
            colour(&one.colour),
            block.opacity
        ));
        first = false;
    }
    if first && block.chart.wedges.is_empty() {
        out.push_str(&format!(
            "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{radius:.1}\" fill=\"{}\" opacity=\"0.25\"/>",
            colour(&board.palette.line)
        ));
    }
    if block.hole > 0.0 {
        // The middle of a ring is a place for one number.
        let words = if block.chart.wedges.first().is_some() {
            let total: f64 = block.chart.wedges.iter().map(|one| one.value).sum();
            layout::number_words(total, "", "", -1)
        } else {
            String::new()
        };
        let size = radius * 0.34;
        let wide = board.fonts.width(&words, &board.fonts.body, 700, false, size);
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"700\" fill=\"{}\" text-anchor=\"middle\" opacity=\"{:.3}\">{}</text>",
            cx,
            cy + size * 0.34,
            family(board),
            size,
            colour(&block.colour),
            block.opacity,
            said(&words)
        ));
    }
    out.push_str(&key(board, block, part));
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// The two sides of a compare.
fn compare(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let mut out = String::new();
    for (at, row) in block.rows.iter().enumerate() {
        let held = if block.stagger > 0.0 {
            come(local, block.delay + block.stagger * at as f64, block.reveal_ms)
        } else {
            part
        };
        if held <= 0.0 {
            continue;
        }
        if row.role == R_HEAD {
            for (line_at, line) in row.title.iter().enumerate() {
                let size = line.size;
                out.push_str(&format!(
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"{}\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
                    row.bx.x,
                    row.bx.y + size * 0.94 + line_at as f64 * size * 1.15,
                    family(board),
                    size,
                    700,
                    colour(&row.colour),
                    block.opacity * held,
                    said(&line_text(line))
                ));
            }
            continue;
        }
        out.push_str(&one_row(board, block, row, held, at + 1));
    }
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// The steps of a process, with the way between them.
fn steps(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let mut out = String::new();
    let row_wise = block
        .rows
        .first()
        .map(|one| one.bx.w < block.bx.w * 0.9)
        .unwrap_or(true);
    for (at, row) in block.rows.iter().enumerate() {
        let held = if block.stagger > 0.0 {
            come(local, block.delay + block.stagger * at as f64, block.reveal_ms)
        } else {
            part
        };
        if held <= 0.0 {
            continue;
        }
        let bx = row.bx;
        out.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
            bx.x,
            bx.y,
            bx.w,
            bx.h,
            (bx.w.min(bx.h) * 0.09).min(block.size),
            colour(&board.palette.panel),
            colour(&board.palette.line),
            block.stroke,
            block.opacity * held
        ));
        let number_size = block.size * 0.85;
        out.push_str(&format!(
            "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
            bx.x + number_size * 0.95,
            bx.y + number_size * 0.95,
            number_size * 0.95,
            colour(&row.colour),
            block.opacity * held
        ));
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"700\" fill=\"{}\" text-anchor=\"middle\" opacity=\"{:.3}\">{}</text>",
            bx.x + number_size * 0.95,
            bx.y + number_size * 1.3,
            family(board),
            number_size,
            colour(&board.palette.bg),
            block.opacity * held,
            row.value as i64
        ));
        let mut y = bx.y + number_size * 2.3;
        for line in &row.title {
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"700\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
                bx.x,
                y,
                family(board),
                line.size,
                colour(&row.colour),
                block.opacity * held,
                said(&line_text(line))
            ));
            y += line.size * 1.15;
        }
        y += block.size * 0.3;
        for line in &row.lines {
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
                bx.x,
                y,
                family(board),
                line.size,
                colour(&board.palette.ink),
                block.opacity * held,
                said(&line_text(line))
            ));
            y += line.size * 1.25;
        }
        // The way on to the next step.
        if at + 1 < block.rows.len() {
            let next = &block.rows[at + 1];
            let (x1, y1, x2, y2) = if row_wise {
                (bx.right() + 2.0, bx.middle(), next.bx.x - 2.0, next.bx.middle())
            } else {
                (bx.centre(), bx.bottom() + 2.0, next.bx.centre(), next.bx.y - 2.0)
            };
            out.push_str(&format!(
                "<path d=\"M{x1:.1} {y1:.1} L{x2:.1} {y2:.1}\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
                colour(&board.palette.line),
                block.stroke * 1.4,
                block.opacity * held
            ));
        }
    }
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// A line of time with the points on it.
fn timeline(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let mut out = String::new();
    let plot = block.chart.plot;
    let grow = grow_to(block, part, local);
    out.push_str(&format!(
        "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
        plot.x,
        plot.y,
        plot.right(),
        plot.y,
        colour(&board.palette.line),
        block.stroke * 1.6,
        block.opacity
    ));
    out.push_str(&format!(
        "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
        plot.x,
        plot.y,
        plot.x + plot.w * grow,
        plot.y,
        colour(&block.colour),
        block.stroke * 1.6,
        block.opacity
    ));
    for (at, row) in block.rows.iter().enumerate() {
        let held = if block.stagger > 0.0 {
            come(local, block.delay + block.stagger * at as f64, block.reveal_ms)
        } else {
            come(local, block.delay + 240.0 * at as f64, block.reveal_ms)
        };
        if held <= 0.0 {
            continue;
        }
        let cx = plot.x + plot.w * row.at;
        let r = block.size * 0.34;
        out.push_str(&format!(
            "<circle cx=\"{cx:.1}\" cy=\"{:.1}\" r=\"{r:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
            plot.y,
            colour(&block.colour),
            colour(&board.palette.bg),
            block.stroke,
            block.opacity * held
        ));
        let mut y = plot.y + block.size * 1.1;
        for (line, words) in [
            (&row.when, 700),
            (&row.title, 700),
            (&row.lines, 400),
        ] {
            for one in line {
                out.push_str(&format!(
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"{words}\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
                    row.bx.x,
                    y,
                    family(board),
                    one.size,
                    colour(&if one.runs.first().map(|r| r.colour.clone()).unwrap_or_default().is_empty() {
                        board.palette.ink.clone()
                    } else {
                        one.runs.first().map(|r| r.colour.clone()).unwrap_or_default()
                    }),
                    block.opacity * held,
                    said(&line_text(one))
                ));
                y += one.size * 1.18;
            }
        }
    }
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// A table: the head row, the rows, and the hairlines between them.
fn table(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let mut out = String::new();
    for (at, row) in block.rows.iter().enumerate() {
        let held = if block.stagger > 0.0 {
            come(local, block.delay + block.stagger * at as f64, block.reveal_ms)
        } else {
            part
        };
        if held <= 0.0 {
            continue;
        }
        let head = at == 0 && row.role == "head";
        let bx = row.bx;
        if head {
            out.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
                bx.x,
                bx.y,
                bx.w,
                bx.h,
                block.radius * 0.5,
                colour(&board.palette.panel),
                block.opacity * held
            ));
        } else if at % 2 == 0 {
            out.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
                bx.x,
                bx.y,
                bx.w,
                bx.h,
                colour(&board.palette.panel),
                block.opacity * held * 0.45
            ));
        }
        let size = block.size;
        let pad = size * 0.55;
        let mut x = bx.x + pad;
        for (place, cell) in row.words.iter().enumerate() {
            let lines: Vec<&str> = cell.split('\n').collect();
            for (line_at, line) in lines.iter().enumerate() {
                out.push_str(&format!(
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"{}\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
                    x,
                    bx.y + size * 0.94 + line_at as f64 * size * 1.2,
                    family(board),
                    size,
                    if head { 700 } else { 400 },
                    colour(&if head {
                        board.palette.accent.clone()
                    } else {
                        board.palette.ink.clone()
                    }),
                    block.opacity * held,
                    said(line)
                ));
            }
            // The room of the column is the width of the widest cell of it.
            let widest = row
                .words
                .iter()
                .map(|one| {
                    one.split('\n')
                        .map(|line| board.fonts.width(line, &board.fonts.body, 400, false, size))
                        .fold(0.0_f64, f64::max)
                })
                .nth(place)
                .unwrap_or(40.0);
            x += widest + pad * 2.0;
        }
        if !head {
            out.push_str(&format!(
                "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"1\" opacity=\"{:.3}\"/>",
                bx.x,
                bx.bottom(),
                bx.right(),
                bx.bottom(),
                colour(&board.palette.line),
                block.opacity * held * 0.6
            ));
        }
    }
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// Words of a program, on a panel, in the shape of a code block.
fn code(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let inner = block.bx.inset(block.pad);
    let typed = if block.reveal == "none" {
        1.0
    } else {
        come(local, block.delay, block.reveal_ms.max(1200.0))
    };
    let mut out = String::new();
    let total: usize = block.lines.iter().map(|one| one.runs.iter().map(|r| r.text.chars().count()).sum::<usize>()).sum();
    let mut shown = 0usize;
    let up_to = (total as f64 * typed).ceil() as usize;
    for (at, line) in block.lines.iter().enumerate() {
        let take = up_to.saturating_sub(shown);
        let text: String = line
            .runs
            .first()
            .map(|one| one.text.chars().take(take).collect())
            .unwrap_or_default();
        shown += line
            .runs
            .first()
            .map(|one| one.text.chars().count())
            .unwrap_or(0);
        if text.is_empty() && take == 0 {
            continue;
        }
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\" xml:space=\"preserve\">{}</text>",
            inner.x,
            inner.y + at as f64 * block.step + block.size * 0.94,
            family_code(board),
            line.size,
            colour(&block.colour),
            block.opacity,
            said(&text)
        ));
    }
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// The pills: small rounded labels of one word each.
fn pills(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let mut out = String::new();
    for (at, row) in block.rows.iter().enumerate() {
        let held = if block.stagger > 0.0 {
            come(local, block.delay + block.stagger * at as f64, block.reveal_ms)
        } else {
            part
        };
        if held <= 0.0 {
            continue;
        }
        let bx = row.bx;
        out.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
            bx.x,
            bx.y,
            bx.w,
            bx.h,
            bx.h / 2.0,
            colour(&if block.bg.is_empty() {
                board.palette.panel.clone()
            } else {
                block.bg.clone()
            }),
            colour(&block.colour),
            block.stroke,
            block.opacity * held
        ));
        let words = row.words.first().cloned().unwrap_or_default();
        let size = block.size;
        let wide = board.fonts.width(&words, &board.fonts.body, 700, false, size);
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"700\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
            bx.centre() - wide / 2.0,
            bx.middle() + size * 0.34,
            family(board),
            size,
            colour(&block.colour),
            block.opacity * held,
            said(&words)
        ));
    }
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// A bar that fills to a value.
fn progress(board: &mut Board, block: &Placed, part: f64, local: f64) {
    let panel = panel(board, block, part);
    let plot = block.chart.plot;
    let grow = grow_to(block, part, local);
    let mut out = String::new();
    let height = plot.h;
    out.push_str(&format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{height:.1}\" rx=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
        plot.x,
        plot.y,
        plot.w,
        height / 2.0,
        colour(&if block.bg.is_empty() {
            board.palette.panel.clone()
        } else {
            block.bg.clone()
        }),
        block.opacity
    ));
    out.push_str(&format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{height:.1}\" rx=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
        plot.x,
        plot.y,
        (plot.w * grow).max(height * 0.9),
        height / 2.0,
        colour(&block.colour),
        block.opacity
    ));
    let label_size = block.size;
    if !block.label.is_empty() {
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
            plot.x,
            plot.y - label_size * 0.4,
            family(board),
            label_size,
            colour(&board.palette.ink),
            block.opacity * part,
            said(&block.label)
        ));
    }
    let wide = board.fonts.width(&block.number, &board.fonts.body, 700, false, label_size);
    out.push_str(&format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"700\" fill=\"{}\" text-anchor=\"end\" opacity=\"{:.3}\">{}</text>",
        plot.right(),
        plot.y - label_size * 0.4,
        family(board),
        label_size,
        colour(&block.colour),
        block.opacity * part,
        said(&count_to(&block.number, grow))
    ));
    wrap_block(board, block, part, format!("{panel}{out}"));
}

// A picture of the room of the chat.
fn image(board: &mut Board, block: &Placed, part: f64) {
    let Some(held) = &block.image else {
        return;
    };
    let plot = if block.chart.plot.h > 1.0 {
        block.chart.plot
    } else {
        block.bx
    };
    let id = board.id("clip");
    board.defs.push_str(&format!(
        "<clipPath id=\"{id}\"><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\"/></clipPath>",
        plot.x,
        plot.y,
        plot.w,
        plot.h,
        block.radius
    ));
    let encoded = B64.encode(&held.data);
    let fit = if held.mime == "image/svg+xml" {
        String::from("preserveAspectRatio=\"xMidYMid meet\"")
    } else {
        String::from("preserveAspectRatio=\"xMidYMid slice\"")
    };
    let mut out = format!(
        "<g clip-path=\"url(#{id})\" opacity=\"{:.3}\"><image x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" href=\"data:{};base64,{encoded}\" {fit}/></g>",
        block.opacity * part,
        plot.x,
        plot.y,
        plot.w,
        plot.h,
        held.mime
    );
    if !block.border.is_empty() {
        out.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
            plot.x,
            plot.y,
            plot.w,
            plot.h,
            block.radius,
            colour(&block.border),
            block.stroke,
            block.opacity * part
        ));
    }
    if !block.label.is_empty() {
        let size = block.size;
        let lines = board
            .fonts
            .wrap(&block.label, &board.fonts.body, 400, false, size, block.bx.w);
        let mut y = plot.bottom() + size * 1.3;
        for line in lines {
            let wide = board.fonts.width(&line, &board.fonts.body, 400, false, size);
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
                plot.centre() - wide / 2.0,
                y,
                family(board),
                size,
                colour(&block.colour),
                block.opacity * part,
                said(&line)
            ));
            y += size * 1.25;
        }
    }
    wrap_block(board, block, part, out);
}

// One mark of the set the program draws.
fn icon(board: &mut Board, block: &Placed, part: f64) {
    let Some(one) = marks::of(&block.mark) else {
        return;
    };
    let size = block.size;
    let x = match block.align.as_str() {
        "center" => block.bx.centre() - size / 2.0,
        "right" => block.bx.right() - size,
        _ => block.bx.x,
    };
    let y = match block.valign.as_str() {
        "middle" => block.bx.middle() - size / 2.0,
        "bottom" => block.bx.bottom() - size,
        _ => block.bx.y,
    };
    let mut out = String::new();
    if !block.bg.is_empty() {
        out.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
            x - size * 0.3,
            y - size * 0.3,
            size * 1.6,
            size * 1.6,
            size * 0.45,
            colour(&block.bg),
            block.opacity * part
        ));
    }
    out.push_str(&mark_words(
        board,
        one,
        x,
        y,
        size,
        &block.colour,
        block.stroke * 2.2,
        block.opacity * part,
    ));
    wrap_block(board, block, part, out);
}

// The words of a mark, laid in a field of 24 by 24.
fn mark_words(
    board: &mut Board,
    one: &'static marks::Mark,
    x: f64,
    y: f64,
    size: f64,
    colour_of: &str,
    weight: f64,
    opacity: f64,
) -> String {
    let scale = size / 24.0;
    let mut parts = String::new();
    for (at, part) in one.parts.iter().enumerate() {
        let filled = one.filled.get(at).copied().unwrap_or(false);
        parts.push_str(&format!(
            "<path d=\"{part}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{:.2}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
            if filled {
                colour(colour_of)
            } else {
                String::from("none")
            },
            colour(colour_of),
            weight / scale.max(0.001)
        ));
    }
    let _ = board;
    format!(
        "<g transform=\"translate({x:.1} {y:.1}) scale({scale:.4})\" opacity=\"{opacity:.3}\">{parts}</g>"
    )
}

// A mark beside a line of a list. A number is drawn rather than a path.
fn mark_at(
    board: &mut Board,
    name: &str,
    cx: f64,
    cy: f64,
    size: f64,
    colour_of: &str,
    number: usize,
    opacity: f64,
) -> String {
    match name {
        "number" => {
            let r = size * 0.62;
            format!(
                "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{r:.1}\" fill=\"{fill}\" opacity=\"{opacity:.3}\"/><text x=\"{cx:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"700\" fill=\"{}\" text-anchor=\"middle\" opacity=\"{opacity:.3}\">{number}</text>",
                cy + size * 0.24,
                family(board),
                size * 0.72,
                colour(&board.palette.bg),
                fill = colour(colour_of)
            )
        }
        "dot" => {
            format!(
                "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
                size * 0.19,
                colour(colour_of),
                opacity
            )
        }
        other => {
            let made = marks::of(other)
                .or_else(|| marks::of("dot"))
                .expect("the set holds a dot");
            mark_words(
                board,
                made,
                cx - size / 2.0,
                cy - size / 2.0,
                size,
                colour_of,
                size * 0.16,
                opacity,
            )
        }
    }
}

// A shape: a panel, a ring, a circle, a bar.
fn shape(board: &mut Board, block: &Placed, part: f64) {
    let bx = block.bx;
    let fill = if block.bg.is_empty() {
        block.colour.clone()
    } else {
        block.bg.clone()
    };
    let mut out = String::new();
    match block.shape.as_str() {
        "circle" => {
            out.push_str(&format!(
                "<ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
                bx.centre(),
                bx.middle(),
                bx.w / 2.0,
                bx.h / 2.0,
                colour(&fill),
                colour(&block.border),
                block.stroke,
                block.opacity * part
            ));
        }
        "ring" => {
            let r = bx.w.min(bx.h) / 2.0;
            out.push_str(&format!(
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
                bx.centre(),
                bx.middle(),
                r * (1.0 - block.hole * 0.5),
                colour(&fill),
                (r * block.hole).max(block.stroke),
                block.opacity * part
            ));
        }
        _ => {
            out.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
                bx.x,
                bx.y,
                bx.w,
                bx.h,
                block.radius,
                colour(&fill),
                colour(&block.border),
                block.stroke,
                block.opacity * part
            ));
        }
    }
    wrap_block(board, block, part, out);
}

// A hairline, the way across or the way down.
fn rule(board: &mut Board, block: &Placed, part: f64) {
    let bx = block.bx;
    // A rule is drawn the way down when its box is taller than it is wide.
    let vertical = bx.h > bx.w;
    let (x1, y1, x2, y2) = if vertical {
        (bx.centre(), bx.y, bx.centre(), bx.y + bx.h * part)
    } else {
        (bx.x, bx.middle(), bx.x + bx.w * part, bx.middle())
    };
    wrap_block(
        board,
        block,
        part,
        format!(
            "<line x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\" stroke=\"{}\" stroke-width=\"{:.1}\" stroke-linecap=\"round\" opacity=\"{:.3}\"/>",
            colour(&block.colour),
            block.stroke,
            block.opacity
        ),
    );
}

// A piece of the model's own drawing, laid in its box.
fn raw(board: &mut Board, block: &Placed, part: f64) {
    let bx = block.bx;
    let words = block.svg.trim();
    // The words are put in a group of their own, moved and scaled into the box.
    // A piece of drawing of the model's own is taken as it stands: it is the one
    // way to have a thing the kinds do not hold.
    let held = if words.contains("xmlns=") {
        String::from(words)
    } else {
        format!("<g>{words}</g>")
    };
    wrap_block(
        board,
        block,
        part,
        format!(
            "<g transform=\"translate({:.1} {:.1})\" opacity=\"{:.3}\">{held}</g>",
            bx.x,
            bx.y,
            block.opacity * part
        ),
    );
}

// ---------------------------------------------------------------- the parts

// The panel behind a block, when the block names a ground or a border.
fn panel(board: &mut Board, block: &Placed, part: f64) -> String {
    if block.bg.is_empty() && block.border.is_empty() {
        return String::new();
    }
    let bx = block.bx;
    let _ = board;
    format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
        bx.x,
        bx.y,
        bx.w,
        bx.h,
        block.radius,
        if block.bg.is_empty() {
            String::from("none")
        } else {
            colour(&block.bg)
        },
        if block.border.is_empty() {
            String::from("none")
        } else {
            colour(&block.border)
        },
        block.stroke,
        block.opacity * part
    )
}

// The group a block is drawn in, with the way it comes in laid over it.
fn wrap_block(board: &mut Board, block: &Placed, part: f64, words: String) {
    if words.is_empty() {
        return;
    }
    let (dx, dy) = slide_of(block, part);
    match block.reveal.as_str() {
        "wipe" => {
            let bx = block.bx;
            let id = board.id("clip");
            board.defs.push_str(&format!(
                "<clipPath id=\"{id}\"><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\"/></clipPath>",
                bx.x - 4.0,
                bx.y - 4.0,
                (bx.w + 8.0) * part,
                bx.h + 8.0
            ));
            board.put(&format!(
                "<g clip-path=\"url(#{id})\" opacity=\"{:.3}\">{words}</g>",
                block.opacity
            ));
        }
        "pop" => {
            let bx = block.bx;
            let scale = 0.9 + 0.1 * part;
            board.put(&format!(
                "<g opacity=\"{:.3}\" transform=\"translate({:.1} {:.1}) scale({:.4}) translate({:.1} {:.1})\">{words}</g>",
                block.opacity * part,
                bx.centre(),
                bx.middle(),
                scale,
                bx.centre(),
                bx.middle()
            ));
        }
        _ => {
            if dx == 0.0 && dy == 0.0 && (block.opacity * part) >= 0.999 {
                board.put(&words);
            } else {
                board.put(&format!(
                    "<g opacity=\"{:.3}\" transform=\"translate({:.1} {:.1})\">{words}</g>",
                    block.opacity * part,
                    dx,
                    dy
                ));
            }
        }
    }
}

// The shift a block carries while it comes in.
fn slide_of(block: &Placed, part: f64) -> (f64, f64) {
    if part >= 1.0 {
        return (0.0, 0.0);
    }
    let held = 1.0 - part;
    match block.reveal.as_str() {
        "rise" => (0.0, block.size * 0.7 * held),
        "left" => (-block.size * 1.2 * held, 0.0),
        "right" => (block.size * 1.2 * held, 0.0),
        _ => (0.0, 0.0),
    }
}

// The value axis of a chart: the steps and the hairlines.
fn axis(board: &mut Board, block: &Placed, part: f64) -> String {
    let mut out = String::new();
    let plot = block.chart.plot;
    for one in &block.chart.ticks {
        out.push_str(&format!(
            "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"1\" opacity=\"{:.3}\"/>",
            plot.x,
            one.y,
            plot.right(),
            one.y,
            colour(&board.palette.line),
            board.palette.ink.is_empty().then_some(0.0).unwrap_or(0.55) * block.opacity
        ));
        let size = block.size * 0.78;
        let wide = board.fonts.width(&one.words, &board.fonts.body, 400, false, size);
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" fill=\"{}\" text-anchor=\"end\" opacity=\"{:.3}\">{}</text>",
            plot.x - size * 0.4,
            one.y + size * 0.34,
            family(board),
            size,
            colour(&board.palette.muted),
            block.opacity * part,
            said(&one.words)
        ));
    }
    // The line of the zero, when the chart holds numbers under it.
    if block.chart.floor < 0.0 {
        out.push_str(&format!(
            "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"{:.1}\" opacity=\"{:.3}\"/>",
            plot.x,
            block.chart.zero,
            plot.right(),
            block.chart.zero,
            colour(&board.palette.muted),
            block.stroke,
            block.opacity
        ));
    }
    out
}

// The words under the bars or under the points.
fn axis_labels(board: &mut Board, block: &Placed, part: f64) -> String {
    let mut out = String::new();
    for row in &block.rows {
        if row.role != R_AXIS {
            continue;
        }
        let words = row.words.first().cloned().unwrap_or_default();
        if words.is_empty() {
            continue;
        }
        let size = row.lines.first().map(|one| one.size).unwrap_or(block.size * 0.82);
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" fill=\"{}\" text-anchor=\"middle\" opacity=\"{:.3}\">{}</text>",
            row.bx.x + if row.bx.w > 1.0 { 0.0 } else { 0.0 },
            row.bx.y + size * 0.9,
            family(board),
            size,
            colour(&board.palette.muted),
            block.opacity * part,
            said(&words)
        ));
    }
    out
}

// The key of a chart.
fn key(board: &mut Board, block: &Placed, part: f64) -> String {
    let mut out = String::new();
    for row in &block.rows {
        if row.role != R_LEGEND {
            continue;
        }
        let size = block.size * 0.85;
        let box_of = size * 0.85;
        out.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{box_of:.1}\" height=\"{box_of:.1}\" rx=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\"/>",
            row.bx.x,
            row.bx.y + size * 0.3,
            box_of * 0.28,
            colour(&row.colour),
            block.opacity * part
        ));
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" fill=\"{}\" opacity=\"{:.3}\">{}</text>",
            row.bx.x + box_of * 1.6,
            row.bx.y + size * 1.0,
            family(board),
            size,
            colour(&board.palette.ink),
            block.opacity * part,
            said(row.words.first().map(|one| one.as_str()).unwrap_or(""))
        ));
    }
    out
}

// Whether the numbers of a chart stand on it.
fn shows_values(block: &Placed) -> bool {
    block.show_values
}

// How far a chart has grown. A chart that does not grow is drawn whole at once.
fn grow_to(block: &Placed, _part: f64, local: f64) -> f64 {
    if !block.grows {
        return 1.0;
    }
    let length = if block.grow_ms > 0.0 {
        block.grow_ms
    } else if block.reveal_ms > 0.0 {
        block.reveal_ms
    } else {
        700.0
    };
    come(local, block.delay, length)
}

// The whole of the words of a line, for the count of a typed coming in.
fn word_count(line: &Line) -> usize {
    line.runs
        .iter()
        .map(|one| one.text.split_whitespace().count())
        .sum()
}

// A line cut to the number of words that are to be seen.
fn clip_words(line: &Line, words: usize) -> Line {
    if words >= word_count(line) {
        return line.clone();
    }
    let mut left = words;
    let mut runs: Vec<crate::infographic::layout::Run> = Vec::new();
    for run in &line.runs {
        if left == 0 {
            break;
        }
        let parts: Vec<&str> = run.text.split_whitespace().collect();
        let take = parts.len().min(left);
        let held = parts.iter().take(take).cloned().collect::<Vec<&str>>().join(" ");
        left -= take;
        if !held.is_empty() {
            runs.push(crate::infographic::layout::Run {
                text: held,
                colour: run.colour.clone(),
                weight: run.weight,
                italic: run.italic,
            });
        }
    }
    Line {
        runs,
        width: line.width * (words as f64 / (word_count(line).max(1) as f64)),
        size: line.size,
    }
}

// The words of a line, glued together.
fn line_text(line: &Line) -> String {
    line.runs
        .iter()
        .map(|one| one.text.as_str())
        .collect::<Vec<&str>>()
        .join("")
}

// The number of a stat or a progress bar, part way up.
fn count_to(words: &str, part: f64) -> String {
    if part >= 0.999 {
        return String::from(words);
    }
    // The digits of the number are taken out of the words, the rest of the words
    // kept: a prefix and a suffix stay where they stand.
    let mut head = String::new();
    let mut tail = String::new();
    let chars: Vec<char> = words.chars().collect();
    let mut at = 0;
    while at < chars.len() && !(chars[at].is_ascii_digit() || chars[at] == '-' || chars[at] == '.') {
        head.push(chars[at]);
        at += 1;
    }
    let mut end = at;
    while end < chars.len() && (chars[end].is_ascii_digit() || chars[end] == '.' || chars[end] == ',') {
        end += 1;
    }
    let middle: String = chars[at..end].iter().collect();
    for c in chars.iter().skip(end) {
        tail.push(*c);
    }
    let clean = middle.replace(',', "");
    match clean.parse::<f64>() {
        Ok(value) => {
            let held = value * part;
            let places = clean.split('.').nth(1).map(|one| one.chars().count()).unwrap_or(0);
            let written = if places == 0 {
                format!("{}", held.round() as i64)
            } else {
                format!("{held:.places$}")
            };
            format!("{head}{written}{tail}")
        }
        Err(_) => String::from(words),
    }
}

// The wedge of a pie, between two angles from the top.
fn wedge(cx: f64, cy: f64, r: f64, inner: f64, from: f64, to: f64) -> String {
    let span = to - from;
    if span >= 359.9 {
        // The whole of the ring: two arcs, for one arc of a whole circle draws
        // nothing.
        let mut out = format!(
            "M{:.1} {:.1} A{r:.1} {r:.1} 0 1 1 {:.1} {:.1} A{r:.1} {r:.1} 0 1 1 {:.1} {:.1} Z",
            cx,
            cy - r,
            cx,
            cy + r,
            cx,
            cy - r
        );
        if inner > 0.0 {
            out.push_str(&format!(
                " M{:.1} {:.1} A{inner:.1} {inner:.1} 0 1 0 {:.1} {:.1} A{inner:.1} {inner:.1} 0 1 0 {:.1} {:.1} Z",
                cx,
                cy - inner,
                cx,
                cy + inner,
                cx,
                cy - inner
            ));
        }
        return out;
    }
    let (x1, y1) = edge(cx, cy, r, from);
    let (x2, y2) = edge(cx, cy, r, to);
    let large = if span > 180.0 { 1 } else { 0 };
    if inner <= 0.0 {
        return format!("M{cx:.1} {cy:.1} L{x1:.1} {y1:.1} A{r:.1} {r:.1} 0 {large} 1 {x2:.1} {y2:.1} Z");
    }
    let (x3, y3) = edge(cx, cy, inner, to);
    let (x4, y4) = edge(cx, cy, inner, from);
    format!(
        "M{x1:.1} {y1:.1} A{r:.1} {r:.1} 0 {large} 1 {x2:.1} {y2:.1} L{x3:.1} {y3:.1} A{inner:.1} {inner:.1} 0 {large} 0 {x4:.1} {y4:.1} Z"
    )
}

// A point on a circle, at an angle from the top, going right.
fn edge(cx: f64, cy: f64, r: f64, angle: f64) -> (f64, f64) {
    let radians = (angle - 90.0).to_radians();
    (cx + r * radians.cos(), cy + r * radians.sin())
}

// How far a thing has come in at a moment, eased.
fn come(local: f64, delay: f64, length: f64) -> f64 {
    if local == f64::MAX {
        return 1.0;
    }
    let length = length.max(1.0);
    let held = (local - delay) / length;
    ease(held.clamp(0.0, 1.0))
}

// The easing of a coming in: slow at both ends, quick in the middle.
fn ease(part: f64) -> f64 {
    let held = part.clamp(0.0, 1.0);
    held * held * (3.0 - 2.0 * held)
}

// The family of the words of a frame.
fn family(board: &Board) -> String {
    quote_family(&board.fonts.body)
}

// The family of the words of a code block.
fn family_code(board: &Board) -> String {
    let mono = board.fonts.mono.clone();
    quote_family(&mono)
}

// A family name with the marks a document of SVG needs.
fn quote_family(name: &str) -> String {
    if name.is_empty() {
        return String::from("sans-serif");
    }
    format!("'{}'", name.replace('\'', ""))
}

// A colour of the program, or one the model named. A name that is not a colour
// is answered with nothing, so a fault of writing costs a fill and not a frame.
fn colour(named: &str) -> String {
    let said = named.trim();
    if said.is_empty() {
        return String::from("none");
    }
    if said.starts_with('#') {
        let held = &said[1..];
        let good = match held.len() {
            3 => held.chars().all(|c| c.is_ascii_hexdigit()),
            6 => held.chars().all(|c| c.is_ascii_hexdigit()),
            8 => held.chars().all(|c| c.is_ascii_hexdigit()),
            _ => false,
        };
        if good {
            return String::from(said);
        }
    }
    match said.to_ascii_lowercase().as_str() {
        "none" => String::from("none"),
        "white" => String::from("#ffffff"),
        "black" => String::from("#000000"),
        "red" => String::from("#e0563f"),
        "green" => String::from("#4ec26a"),
        "blue" => String::from("#5aa9e6"),
        "yellow" => String::from("#f2c14e"),
        "orange" => String::from("#ED7401"),
        "purple" => String::from("#c56bd6"),
        "grey" | "gray" => String::from("#8a8a8a"),
        _ => String::from("none"),
    }
}

// Words of the model, made safe for a document of SVG.
fn said(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
