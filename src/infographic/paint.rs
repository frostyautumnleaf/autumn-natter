// The look of one layer, as a drawing.
//
// A layer is drawn into a board of its own: a document of SVG whose width and height are the
// pixels of the box the layer stands at on the screen. The compositor takes that document, makes
// it into pixels, and lays it where the pose of the layer says, with the turn, the scale, the
// fading and the blending of it. Nothing here knows what stands under a layer, and nothing in the
// compositor knows what a bar chart is.
//
// The same reading of the words serves the check and the frame: `draw` measures the words with the
// fonts of the computer and says what does not fit, so a caption that runs over its box is
// reported before a minute of frames is drawn. A fault is not a refusal. The drawing carries what
// it can, and the words of the fault say what was left out and what to change.
//
// Every length of a style is a part of the canvas, not a pixel, so the same words draw the same
// picture at 720p and at 4k. `Rule` holds the two measures those parts are taken against, and
// `part` turns one into pixels.

use crate::infographic::anim::{self, Pose};
use crate::infographic::fonts::{self, Fonts};
use crate::infographic::story::{self, Layer, Palette};
use crate::infographic::text;

// A layer drawn, with what the drawing found wrong with it.
pub struct Sheet {
    // The whole of a document of SVG, `width` by `height` pixels.
    pub svg: String,
    pub faults: Vec<String>,
}

// The measure the parts of a style are taken against. Every length in a composition is a fraction
// of the frame height, so one style reads the same at 720p and at 4K.
#[derive(Clone, Copy)]
struct Rule {
    height: f64,
}

// A box within the board, in pixels.
#[derive(Clone, Copy)]
struct Box_ {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Box_ {
    fn right(&self) -> f64 {
        self.x + self.w
    }
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
    fn centre(&self) -> f64 {
        self.x + self.w / 2.0
    }
    fn middle(&self) -> f64 {
        self.y + self.h / 2.0
    }
    fn inset(&self, by: f64) -> Box_ {
        Box_ {
            x: self.x + by,
            y: self.y + by,
            w: (self.w - by * 2.0).max(1.0),
            h: (self.h - by * 2.0).max(1.0),
        }
    }
}

// A layer drawn at the size of its box. `canvas` is the whole screen, which the parts of the
// style are taken against, and `pose` is where the layer stands at the moment being drawn.
pub fn draw(
    layer: &Layer,
    pose: &Pose,
    width: u32,
    height: u32,
    canvas: (u32, u32),
    fonts: &Fonts,
    palette: &Palette,
) -> Sheet {
    let rule = Rule { height: canvas.1 as f64 };
    let board = Box_ { x: 0.0, y: 0.0, w: width as f64, h: height as f64 };
    let mut faults: Vec<String> = Vec::new();
    let body = match layer.kind.as_str() {
        "text" => words(layer, pose, board, rule, fonts, palette, &mut faults),
        "table" => table(layer, pose, board, rule, fonts, palette, &mut faults),
        "chart" => chart(layer, pose, board, rule, fonts, palette, &mut faults),
        "shape" => shapes(layer, pose, board, rule, &mut faults),
        // A film or a photograph brings its own pixels, which the compositor reads.
        "image" | "video" | "audio" => String::new(),
        other => {
            faults.push(format!("\"{other}\" is not a kind of layer"));
            String::new()
        }
    };
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">{body}</svg>"
    );
    Sheet { svg, faults }
}

// ---------------------------------------------------------------- words

// The words of a layer: the panel when one is asked for, and the lines of the text over it.
fn words(
    layer: &Layer,
    pose: &Pose,
    board: Box_,
    rule: Rule,
    fonts: &Fonts,
    palette: &Palette,
    faults: &mut Vec<String>,
) -> String {
    let style = &layer.style;
    let size = part(pose.size, 0.045, rule.height);
    let family = family_of(&style.family, fonts);
    let weight = fonts::weight_of(&style.weight);
    let bold = if weight < 700 { 700u16 } else { weight };
    let colour = ink(layer, pose, palette);
    let pad = part(style.pad, 0.012, rule.height);
    let outer = panel(layer, board, rule, palette);
    let inner = board.inset(pad);

    let para = text::paragraph_of(&style.text, Some(&colour), weight, bold);
    let line_height = if style.line_height > 0.0 { style.line_height } else { 1.25 };
    let cut = text::layout(
        fonts,
        &family,
        &para,
        size,
        inner.w,
        line_height,
        style.letter_space * rule.height,
        style.max_lines,
    );
    let step = cut
        .lines
        .first()
        .map(|one| one.height)
        .unwrap_or_else(|| fonts.line_em(&family, weight) * size * line_height);
    if cut.cut {
        faults.push(format!(
            "the words are cut at {} lines: they would need {:.2} of the height of the box, and \
             {} words went away. Make the box taller or wider, or write fewer words.",
            style.max_lines,
            ratio(cut.needed, inner.h),
            cut.lost
        ));
    } else if cut.needed > inner.h + 0.5 {
        faults.push(format!(
            "the words stand {:.0} px over the height of the box, so the last lines fall off its \
             bottom. Make the box taller by about {:.0} px, or the letters smaller.",
            cut.needed - inner.h,
            cut.needed - inner.h
        ));
    }

    // The reveal lets the words out one at a time, in the order they are read.
    let total: usize = cut.lines.iter().map(|one| one.words.len()).sum();
    let up_to = ((total as f64) * pose.reveal).floor() as usize;
    let mark = mark_of(&style.bullet);
    let shadow = style.shadow * rule.height;
    let shadow_colour = pick(&style.shadow_colour, "#000000").to_string();
    let top = board.middle() - (cut.lines.len() as f64 * step) / 2.0;
    let mut body = String::new();
    let mut shown = 0usize;
    for (place, line) in cut.lines.iter().enumerate() {
        let taken = up_to.saturating_sub(shown);
        shown += line.words.len();
        if line.words.is_empty() || taken == 0 {
            continue;
        }
        let y = top + place as f64 * step + size * 0.86;
        let x = match style.align.as_str() {
            "center" | "centre" | "middle" => board.centre() - line.width / 2.0,
            "right" => board.right() - pad - line.width,
            _ => inner.x,
        };
        if let Some(one) = mark {
            body.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                 fill=\"{colour}\">{one}</text>",
                x - size * 0.8,
                y,
                size * 1.1,
            ));
        }
        // Each word stands on its own, so a caption may be typed a word at a time.
        for word in line.words.iter().take(taken) {
            let run = &para.runs[word.run.min(para.runs.len() - 1)];
            let said = run.colour.clone().unwrap_or_else(|| colour.clone());
            body.push_str(&word_marks(
                x + word.x,
                y,
                &word.text,
                &family,
                size,
                run.weight,
                run.italic,
                &said,
                layer,
                rule,
                shadow,
                &shadow_colour,
            ));
        }
    }
    format!("{outer}{body}")
}

// The marks of one word, with the outline a caption over a bright film needs and the copy under it
// that reads as its shadow.
fn word_marks(
    x: f64,
    y: f64,
    said: &str,
    family: &str,
    size: f64,
    weight: u16,
    italic: bool,
    colour: &str,
    layer: &Layer,
    rule: Rule,
    shadow: f64,
    shadow_colour: &str,
) -> String {
    let stroke = if layer.style.stroke_width > 0.0 {
        format!(
            " stroke=\"{}\" stroke-width=\"{:.1}\" stroke-linejoin=\"round\" paint-order=\"stroke\"",
            pick(&layer.style.stroke_colour, "#000000"),
            layer.style.stroke_width * rule.height * 2.0
        )
    } else {
        String::new()
    };
    let slant = if italic { " font-style=\"italic\"" } else { "" };
    let held = format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
         font-weight=\"{weight}\"{slant} fill=\"{colour}\"{stroke}>{}</text>",
        x,
        y,
        size,
        escaped(said)
    );
    if shadow <= 0.0 {
        return held;
    }
    format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
         font-weight=\"{weight}\"{slant} fill=\"{shadow_colour}\" opacity=\"0.45\">{}</text>{held}",
        x + shadow * 0.35,
        y + shadow * 0.6,
        size,
        escaped(said),
    )
}

// The panel a layer of words stands on: a box of the box, or a bar across it.
fn panel(layer: &Layer, board: Box_, rule: Rule, palette: &Palette) -> String {
    let said = layer.style.panel.as_str();
    if said == "none" || said.is_empty() {
        return String::new();
    }
    let ground = pick(&layer.style.panel_colour, &palette.ground).to_string();
    let radius = if said == "bar" {
        0.0
    } else {
        part(layer.style.radius, 0.012, rule.height)
    };
    format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" \
         fill=\"{ground}\"/>",
        board.x,
        board.y,
        board.w,
        board.h,
        radius,
        radius
    )
}

// The mark that stands before every line of a list. Nothing stands there by default.
fn mark_of(said: &str) -> Option<&'static str> {
    match said {
        "dot" => Some("&#183;"),
        "dash" => Some("&#8211;"),
        "check" => Some("&#10003;"),
        "cross" => Some("&#10007;"),
        "arrow" => Some("&#8250;"),
        "star" => Some("&#9733;"),
        _ => None,
    }
}

// ---------------------------------------------------------------- tables

// A table: the row of heads, the rows of the thing, and the lines of the form it is set in.
fn table(
    layer: &Layer,
    pose: &Pose,
    board: Box_,
    rule: Rule,
    fonts: &Fonts,
    palette: &Palette,
    faults: &mut Vec<String>,
) -> String {
    let style = &layer.style;
    let size = part(pose.size, 0.026, rule.height);
    let family = family_of(&style.family, fonts);
    let ink = ink(layer, pose, palette);
    let pad = part(style.pad, 0.006, rule.height);
    let head_size = size * 1.06;

    let columns = style
        .rows
        .iter()
        .map(|one| one.len())
        .max()
        .unwrap_or(0)
        .max(style.heads.len());
    if style.rows.is_empty() || columns == 0 {
        faults.push(
            "the table has no cells; give \"rows\" of cells, and \"heads\" when the columns have \
             names."
                .to_string(),
        );
        return String::new();
    }
    let rows = shown_rows(style.rows.len(), pose.reveal);

    // The width of a column: what was given, or what the widest cell of it needs.
    let mut widths: Vec<f64> = (0..columns)
        .map(|place| {
            if let Some(given) = style.widths.get(place) {
                return (*given).max(0.02) * board.w;
            }
            let mut widest: f64 = 0.0;
            if let Some(head) = style.heads.get(place) {
                widest = widest.max(fonts.width(head, &family, 700, false, head_size));
            }
            for row in &style.rows {
                if let Some(cell) = row.get(place) {
                    widest = widest.max(fonts.width(cell, &family, 400, false, size));
                }
            }
            widest + pad * 2.0
        })
        .collect();
    let sum: f64 = widths.iter().sum();
    if sum > board.w {
        let share = board.w / sum;
        for one in widths.iter_mut() {
            *one *= share;
        }
    } else {
        // The spare is shared out, so a table of short words does not sit in one corner.
        let spare = (board.w - sum) / columns as f64;
        for one in widths.iter_mut() {
            *one += spare;
        }
    }

    let height = size * 1.35 + pad * 2.0;
    let head_height = if style.heads.is_empty() { 0.0 } else { height * 1.05 };
    let needed = head_height + height * style.rows.len() as f64;
    let mut out = String::new();
    if needed > board.h {
        let fits = ((board.h - head_height) / height).floor().max(0.0) as usize;
        faults.push(format!(
            "the table needs {:.0} px for its {} rows and the box holds {} of them, so {} rows \
             are not drawn. Make the box taller, or the letters smaller, or put fewer rows in it.",
            needed,
            style.rows.len(),
            fits,
            style.rows.len().saturating_sub(fits)
        ));
    }
    let shown = rows.min(if style.heads.is_empty() {
        style.rows.len()
    } else {
        ((board.h - head_height) / height).floor().max(0.0) as usize
    });
    let form = pick(&style.form, "lines");
    let bottom = (board.y + head_height + height * shown as f64).min(board.bottom());

    // The grounds of the rows, under the words.
    let head_plate = if style.heads.is_empty() {
        String::new()
    } else {
        pick(&style.head_ground, &palette.ink).to_string()
    };
    if !style.heads.is_empty() {
        out.push_str(&filled(board.x, board.y, board.w, head_height, 0.0, head_plate.clone(), 1.0));
    }
    let mut y = board.y + head_height;
    for place in 0..shown {
        if form == "zebra" && place % 2 == 0 {
            let ground = pick(&style.row_ground2, &palette.ground);
            out.push_str(&filled(board.x, y, board.w, height, 0.0, ground.to_string(), 0.9));
        } else if let Some(ground) = non_empty(&style.row_ground) {
            out.push_str(&filled(board.x, y, board.w, height, 0.0, ground, 1.0));
        }
        y += height;
    }

    // The heads.
    let mut x = board.x;
    for place in 0..columns {
        let wide = widths.get(place).copied().unwrap_or(0.0);
        if let Some(head) = style.heads.get(place) {
            let align = align_of(style, place, head);
            let cx = align_x(x + pad, wide - pad * 2.0, fonts.width(head, &family, 700, false, head_size), align);
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                 font-weight=\"700\" fill=\"{}\">{}</text>",
                cx,
                board.y + head_height / 2.0 + head_size * 0.36,
                head_size,
                // The words of the plate are the colour of the set that reads against the plate,
                // which is the other of the two, whichever way the plate was set.
                pick(&style.head_colour, &against(&head_plate, palette)),
                escaped(head)
            ));
        }
        x += wide;
    }

    // The rows of the thing, and the lines of the form.
    let mut y = board.y + head_height;
    for (place, row) in style.rows.iter().enumerate().take(shown) {
        let mut x = board.x;
        for column in 0..columns {
            let wide = widths.get(column).copied().unwrap_or(0.0);
            let cell = row.get(column).map(|one| one.as_str()).unwrap_or("");
            if !cell.is_empty() {
                let align = align_of(style, column, cell);
                let cx = align_x(x + pad, wide - pad * 2.0, fonts.width(cell, &family, 400, false, size), align);
                if form == "bars" {
                    // A bar behind the number, whose length is the number against the greatest of
                    // its column.
                    if let Some(value) = value_of(cell) {
                        let greatest = column_greatest(&style.rows, column).max(1e-9);
                        let length = (wide - pad * 2.0) * (value / greatest).clamp(0.0, 1.0);
                        out.push_str(&filled(
                            x + pad,
                            y + height * 0.3,
                            length,
                            height * 0.4,
                            0.0,
                            pick(&style.bar_colour, &palette.accent).to_string(),
                            0.35,
                        ));
                    }
                }
                out.push_str(&format!(
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                     fill=\"{ink}\">{}</text>",
                    cx,
                    y + height / 2.0 + size * 0.36,
                    size,
                    escaped(cell)
                ));
            }
            x += wide;
        }
        y += height;
        if form == "grid" || form == "lines" {
            let line = pick(&style.line_colour, &palette.soft);
            out.push_str(&ruled(board.x, y, board.right(), y, 1.0, line.to_string()));
        }
        let _ = place;
    }
    if form == "grid" {
        let line = pick(&style.line_colour, &palette.soft).to_string();
        out.push_str(&ruled(board.x, board.y, board.x, bottom, 1.0, line.clone()));
        out.push_str(&ruled(board.right(), board.y, board.right(), bottom, 1.0, line.clone()));
        let mut x = board.x;
        for wide in widths.iter().take(columns.saturating_sub(1)) {
            x += wide;
            out.push_str(&ruled(x, board.y, x, bottom, 1.0, line.clone()));
        }
    }
    if !style.heads.is_empty() {
        let line = pick(&style.line_colour, &palette.soft);
        out.push_str(&ruled(
            board.x,
            board.y + head_height,
            board.right(),
            board.y + head_height,
            2.0,
            line.to_string(),
        ));
    }
    out
}

// The rows a reveal has let out. A row stands only when all of it is let out, so a reveal of a
// half of three rows has one row on the stage.
fn shown_rows(total: usize, reveal: f64) -> usize {
    if reveal >= 1.0 {
        return total;
    }
    ((total as f64) * reveal.clamp(0.0, 1.0)).floor() as usize
}

// How a column stands: what the style said for it, or what the cell is. A number stands to the
// right, as it does in every table of numbers.
fn align_of(style: &story::Style, place: usize, cell: &str) -> &'static str {
    let given = if style.aligns.is_empty() {
        ""
    } else {
        style
            .aligns
            .get(place % style.aligns.len())
            .map(|one| one.as_str())
            .unwrap_or("")
    };
    match given {
        "left" => "left",
        "right" => "right",
        "center" | "centre" | "middle" => "center",
        _ => {
            if value_of(cell).is_some() {
                "right"
            } else {
                "left"
            }
        }
    }
}

// Where a cell of a given width begins, when it stands left, right or in the middle.
fn align_x(x: f64, wide: f64, text: f64, align: &str) -> f64 {
    match align {
        "right" => (x + wide - text).max(x),
        "center" => x + (wide - text) / 2.0,
        _ => x,
    }
}

// The greatest number of a column, for the bar behind a number.
fn column_greatest(rows: &[Vec<String>], column: usize) -> f64 {
    rows.iter()
        .filter_map(|one| one.get(column).and_then(|one| value_of(one)))
        .fold(0.0f64, |one, next| one.max(next.abs()))
}

// A number out of a cell, with its commas, its unit and its sign of it.
pub fn value_of(said: &str) -> Option<f64> {
    let held: String = said
        .chars()
        .filter(|one| one.is_ascii_digit() || *one == '.' || *one == '-' || *one == '+')
        .collect();
    if held.is_empty() || held == "." || held == "-" || held == "+" {
        return None;
    }
    held.parse::<f64>().ok()
}

// ---------------------------------------------------------------- charts

// A chart. The plot is the box with the room for the title, the note and the labels taken out, and
// the values are drawn to `reveal` of their length, so a chart grows up as it arrives.
fn chart(
    layer: &Layer,
    pose: &Pose,
    board: Box_,
    rule: Rule,
    fonts: &Fonts,
    palette: &Palette,
    faults: &mut Vec<String>,
) -> String {
    let style = &layer.style;
    let kind = pick(&style.chart, "bars");
    let ink = ink(layer, pose, palette);
    let family = family_of(&style.family, fonts);
    let size = part(if pose.size > 0.0 { pose.size } else { 0.022 }, 0.022, rule.height);
    let pad = part(style.pad, 0.01, rule.height);
    let mut out = String::new();

    // The title above the plot, and the note under it.
    let mut plot = board.inset(pad);
    if !style.title.is_empty() {
        let head = size * 1.35;
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
             font-weight=\"700\" fill=\"{ink}\">{}</text>",
            plot.x,
            plot.y + head * 0.85,
            head,
            escaped(&style.title)
        ));
        plot.y += head * 1.5;
        plot.h = (plot.h - head * 1.5).max(1.0);
    }
    if !style.note.is_empty() {
        let small = size * 0.9;
        plot.h = (plot.h - small * 2.2).max(1.0);
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
             fill=\"{}\">{}</text>",
            plot.x,
            plot.bottom() + small * 0.9,
            small,
            palette.soft,
            escaped(&style.note)
        ));
    }

    let (labels, lines) = parts(style);
    if lines.is_empty() || lines.iter().all(|one| one.is_empty()) {
        faults.push(
            "the chart has no numbers; give \"items\" of labels and values, or \"series\" of \
             values of their own."
                .to_string(),
        );
        return out;
    }
    let reveal = pose.reveal.clamp(0.0, 1.0);
    match kind {
        "bars" | "hbars" => bars(
            style, palette, fonts, &family, &ink, &labels, &lines, plot, rule, size, reveal,
            kind == "hbars",
            &mut out,
            faults,
        ),
        "line" | "area" => {
            let greatest = greatest_of(&lines, style);
            let count = labels.len().max(lines.first().map(|one| one.len()).unwrap_or(1));
            let step = plot.w / (count - 1).max(1) as f64;
            for (line, values) in lines.iter().enumerate() {
                let colour = colour_of_line(style, palette, line);
                let points: Vec<(f64, f64)> = values
                    .iter()
                    .enumerate()
                    .map(|(place, value)| {
                        (
                            plot.x + place as f64 * step,
                            plot.bottom() - plot.h * (value / greatest).clamp(0.0, 1.0) * reveal,
                        )
                    })
                    .collect();
                if points.len() < 2 {
                    faults.push(format!(
                        "a line needs at least two numbers; series {} has {}.",
                        line + 1,
                        points.len()
                    ));
                    continue;
                }
                let path: String = points
                    .iter()
                    .enumerate()
                    .map(|(at, (x, y))| format!("{}{:.1},{:.1}", if at == 0 { "M" } else { "L" }, x, y))
                    .collect();
                if kind == "area" {
                    out.push_str(&format!(
                        "<path d=\"{path} L{:.1},{:.1} L{:.1},{:.1} Z\" fill=\"{colour}\" \
                         opacity=\"0.25\"/>",
                        points.last().map(|one| one.0).unwrap_or(0.0),
                        plot.bottom(),
                        points.first().map(|one| one.0).unwrap_or(0.0),
                        plot.bottom(),
                    ));
                }
                out.push_str(&format!(
                    "<path d=\"{path}\" fill=\"none\" stroke=\"{colour}\" \
                     stroke-width=\"{:.1}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
                    0.004 * rule.height,
                ));
                for (place, (x, y)) in points.iter().enumerate() {
                    if points.len() < 8 || place + 1 == points.len() {
                        out.push_str(&format!(
                            "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{colour}\"/>",
                            x,
                            y,
                            0.005 * rule.height,
                        ));
                    }
                }
            }
            out.push_str(&ruled(plot.x, plot.bottom(), plot.right(), plot.bottom(), 2.0, palette.soft.clone()));
            for (place, label) in labels.iter().enumerate() {
                if label.is_empty() {
                    continue;
                }
                out.push_str(&format!(
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                     fill=\"{}\">{}</text>",
                    plot.x + place as f64 * step
                        - fonts.width(label, &family, 400, false, size * 0.95) / 2.0,
                    plot.bottom() + size * 1.3,
                    size * 0.95,
                    palette.soft,
                    escaped(label)
                ));
            }
        }
        "pie" | "donut" => {
            let values = lines.first().cloned().unwrap_or_default();
            let total: f64 = values.iter().map(|one| one.abs()).sum();
            if total <= 0.0 {
                faults.push("every part of the pie is nought, so it draws nothing.".to_string());
                return out;
            }
            let radius = plot.w.min(plot.h) / 2.0 * 0.94;
            let (centre_x, centre_y) = (plot.centre(), plot.middle());
            let hole = if kind == "donut" {
                radius * pick_part(style.hole, 0.55)
            } else {
                0.0
            };
            let mut from = -90.0f64;
            for (place, value) in values.iter().enumerate() {
                let sweep = 360.0 * (value.abs() / total) * reveal;
                out.push_str(&wedge(
                    centre_x,
                    centre_y,
                    radius,
                    hole,
                    from,
                    from + sweep,
                    colour_at(style, palette, place, &values, &labels),
                ));
                from += sweep;
            }
            if style.show_values {
                for (place, label) in labels.iter().enumerate() {
                    let value = values.get(place).copied().unwrap_or(0.0);
                    out.push_str(&format!(
                        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" \
                         font-size=\"{:.1}\" fill=\"{ink}\">{} - {}</text>",
                        plot.x,
                        plot.y + (place as f64 + 1.0) * size * 1.4,
                        size,
                        escaped(label),
                        words_of(value, style)
                    ));
                }
            }
        }
        "progress" => {
            let value = lines.first().and_then(|one| one.first()).copied().unwrap_or(0.0);
            let greatest = if style.max > 0.0 { style.max } else { 100.0 };
            let held = (value / greatest).clamp(0.0, 1.0) * reveal;
            let band = (style.radius * rule.height * 2.0).min(rule.height * 0.05).max(size * 0.7);
            let y = plot.middle() - band / 2.0;
            out.push_str(&filled(
                plot.x,
                y,
                plot.w,
                band,
                band / 2.0,
                palette.soft.clone(),
                0.28,
            ));
            out.push_str(&filled(
                plot.x,
                y,
                plot.w * held,
                band,
                band / 2.0,
                ink.clone(),
                1.0,
            ));
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                 font-weight=\"700\" fill=\"{ink}\" text-anchor=\"middle\">{}</text>",
                plot.centre(),
                y - band * 0.5,
                size * 1.4,
                words_of(value * reveal, style)
            ));
        }
        "stat" => {
            let value = lines.first().and_then(|one| one.first()).copied().unwrap_or(0.0) * reveal;
            let head = part(pose.size, 0.12, rule.height);
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                 font-weight=\"700\" fill=\"{ink}\" text-anchor=\"middle\">{}</text>",
                plot.centre(),
                plot.middle() + head * 0.35,
                head,
                escaped(&words_of(value, style))
            ));
        }
        "steps" | "timeline" => steps(
            &labels,
            plot,
            &family,
            size,
            rule,
            palette,
            &ink,
            reveal,
            kind == "timeline",
            &mut out,
        ),
        "scatter" => {
            let greatest = greatest_of(&lines, style);
            for (line, values) in lines.iter().enumerate() {
                let colour = colour_of_line(style, palette, line);
                let most = values.len().max(2);
                for (place, value) in values.iter().enumerate() {
                    out.push_str(&format!(
                        "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{colour}\" \
                         opacity=\"0.85\"/>",
                        plot.x + plot.w * (place as f64 / (most - 1) as f64),
                        plot.bottom()
                            - plot.h * (value / greatest).clamp(0.0, 1.0) * reveal,
                        0.006 * rule.height,
                    ));
                }
            }
        }
        other => faults.push(format!(
            "\"{other}\" is not a kind of chart. The kinds are bars, hbars, line, area, pie, \
             donut, progress, stat, steps, timeline and scatter."
        )),
    }
    if style.show_key && lines.len() > 1 {
        let each = size * 1.8;
        for (place, line) in style.series.iter().enumerate() {
            if line.name.is_empty() {
                continue;
            }
            let y = board.y + pad + place as f64 * each;
            out.push_str(&filled(
                board.right() - each * 7.0,
                y,
                each,
                each * 0.5,
                0.0,
                colour_of_line(style, palette, place),
                1.0,
            ));
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                 fill=\"{ink}\">{}</text>",
                board.right() - each * 5.6,
                y + each * 0.45,
                size * 0.95,
                escaped(&line.name)
            ));
        }
    }
    out
}

// The bars of a chart, standing up or lying down, one band per label.
fn bars(
    style: &story::Style,
    palette: &Palette,
    fonts: &Fonts,
    family: &str,
    ink: &str,
    labels: &[String],
    lines: &[Vec<f64>],
    plot: Box_,
    _rule: Rule,
    size: f64,
    reveal: f64,
    horizontal: bool,
    out: &mut String,
    faults: &mut Vec<String>,
) {
    let greatest = greatest_of(lines, style);
    if greatest == 0.0 {
        faults.push("every value of the chart is nought, so nothing stands.".to_string());
        return;
    }
    let groups = lines.len().max(1);
    let labels = if labels.is_empty() {
        (0..lines.first().map(|one| one.len()).unwrap_or(0))
            .map(|_| String::new())
            .collect::<Vec<String>>()
    } else {
        labels.to_vec()
    };
    if horizontal {
        let row = plot.h / (labels.len().max(1) as f64 * groups as f64).max(1.0);
        let label_room = (plot.w * 0.26).min(
            labels
                .iter()
                .map(|one| fonts.width(one, family, 400, false, size))
                .fold(0.0f64, |one, next| one.max(next))
                + size * 0.5,
        );
        let field = (plot.w - label_room).max(1.0);
        for (place, label) in labels.iter().enumerate() {
            let top = plot.y + place as f64 * row * groups as f64;
            if !label.is_empty() {
                out.push_str(&format!(
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                     fill=\"{ink}\">{}</text>",
                    plot.x,
                    top + row * groups as f64 / 2.0 + size * 0.34,
                    size,
                    escaped(label)
                ));
            }
            for (line, values) in lines.iter().enumerate() {
                let value = values.get(place).copied().unwrap_or(0.0);
                let length = field * (value / greatest).clamp(0.0, 1.0) * reveal;
                out.push_str(&filled(
                    plot.x + label_room,
                    top + line as f64 * row + row * 0.12,
                    length,
                    (row * 0.76).max(1.0),
                    0.0,
                    colour_of_line(style, palette, line),
                    1.0,
                ));
                if style.show_values && length > size * 1.5 {
                    out.push_str(&format!(
                        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" \
                         font-size=\"{:.1}\" fill=\"{ink}\">{}</text>",
                        plot.x + label_room + length + size * 0.3,
                        top + line as f64 * row + row * 0.62,
                        size * 0.95,
                        words_of(value, style)
                    ));
                }
            }
        }
        out.push_str(&ruled(
            plot.x + label_room,
            plot.bottom(),
            plot.right(),
            plot.bottom(),
            2.0,
            palette.soft.clone(),
        ));
        return;
    }
    let column = plot.w / (labels.len().max(1) as f64 * groups as f64).max(1.0);
    let baseline = plot.bottom();
    for (place, label) in labels.iter().enumerate() {
        let left = plot.x + place as f64 * column * groups as f64;
        for (line, values) in lines.iter().enumerate() {
            let value = values.get(place).copied().unwrap_or(0.0);
            let length = plot.h * (value / greatest).clamp(0.0, 1.0) * reveal;
            out.push_str(&filled(
                left + line as f64 * column + column * 0.12,
                baseline - length,
                (column * 0.76).max(1.0),
                length,
                0.0,
                colour_of_line(style, palette, line),
                1.0,
            ));
            if style.show_values && length > 0.0 {
                out.push_str(&format!(
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                     fill=\"{ink}\">{}</text>",
                    left + line as f64 * column + column * 0.1,
                    baseline - length - size * 0.35,
                    size * 0.95,
                    words_of(value, style)
                ));
            }
        }
        if !label.is_empty() {
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                 fill=\"{}\">{}</text>",
                left + column * groups as f64 / 2.0
                    - fonts.width(label, family, 400, false, size * 0.95) / 2.0,
                baseline + size * 1.3,
                size * 0.95,
                palette.soft,
                escaped(label)
            ));
        }
    }
    out.push_str(&ruled(plot.x, baseline, plot.right(), baseline, 2.0, palette.soft.clone()));
}

// The steps of a list, or of a timeline that runs from the left of the screen to the right.
fn steps(
    labels: &[String],
    plot: Box_,
    family: &str,
    size: f64,
    rule: Rule,
    palette: &Palette,
    ink: &str,
    reveal: f64,
    horizontal: bool,
    out: &mut String,
) {
    let shown = shown_rows(labels.len(), reveal);
    if horizontal {
        let step = plot.w / (labels.len().max(2) - 1) as f64;
        let y = plot.middle();
        out.push_str(&ruled(plot.x, y, plot.right(), y, 2.0, palette.soft.clone()));
        for place in 0..shown {
            let x = plot.x + place as f64 * step;
            out.push_str(&format!(
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{}\"/>",
                x,
                y,
                0.008 * rule.height,
                story::series_colour(palette, place)
            ));
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
                 fill=\"{ink}\" text-anchor=\"middle\">{}</text>",
                x,
                if place % 2 == 0 { y - size * 1.2 } else { y + size * 1.7 },
                size,
                escaped(labels.get(place).map(|one| one.as_str()).unwrap_or(""))
            ));
        }
        return;
    }
    let each = plot.h / labels.len().max(1) as f64;
    for place in 0..shown {
        let y = plot.y + place as f64 * each;
        out.push_str(&format!(
            "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{}\"/>",
            plot.x + size * 0.55,
            y + each / 2.0,
            size * 0.45,
            story::series_colour(palette, place)
        ));
        if place + 1 < shown {
            out.push_str(&ruled(
                plot.x + size * 0.55,
                y + each,
                plot.x + size * 0.55,
                y + each * 1.5,
                1.5,
                palette.soft.clone(),
            ));
        }
        out.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{family}\" font-size=\"{:.1}\" \
             fill=\"{ink}\">{}</text>",
            plot.x + size * 1.4,
            y + each / 2.0 + size * 0.34,
            size,
            escaped(labels.get(place).map(|one| one.as_str()).unwrap_or(""))
        ));
    }
}

// The labels and the values of a chart, from either of the two ways they are given.
fn parts(style: &story::Style) -> (Vec<String>, Vec<Vec<f64>>) {
    if !style.series.is_empty() {
        let labels = style
            .series
            .first()
            .map(|one| one.labels.clone())
            .unwrap_or_default();
        let lines = style.series.iter().map(|one| one.values.clone()).collect();
        return (labels, lines);
    }
    let labels = style.items.iter().map(|one| one.label.clone()).collect();
    let lines = vec![style.items.iter().map(|one| one.value).collect()];
    (labels, lines)
}

// The colour of one series: what it was given, or the set of the frame.
fn colour_of_line(style: &story::Style, palette: &Palette, place: usize) -> String {
    if let Some(one) = style.series.get(place) {
        if !one.colour.is_empty() {
            return one.colour.clone();
        }
    }
    if let Some(item) = style.items.get(place) {
        if !item.colour.is_empty() {
            return item.colour.clone();
        }
    }
    story::series_colour(palette, place)
}

// The colour of one part of a pie: its own, or the set of the frame.
fn colour_at(
    style: &story::Style,
    palette: &Palette,
    place: usize,
    _values: &[f64],
    _labels: &[String],
) -> String {
    colour_of_line(style, palette, place)
}

// The greatest value of a chart, which the lengths are taken against.
fn greatest_of(lines: &[Vec<f64>], style: &story::Style) -> f64 {
    if style.max > 0.0 {
        return style.max;
    }
    let found = lines
        .iter()
        .flat_map(|one| one.iter())
        .fold(0.0f64, |one, next| one.max(next.abs()));
    if found <= 0.0 {
        1.0
    } else {
        found
    }
}

// A number of a chart, with its unit and its decimals.
fn words_of(value: f64, style: &story::Style) -> String {
    let decimals = if style.decimals >= 0 { style.decimals } else { 0 };
    text::number(value, decimals, &style.unit_pre, &style.unit_post)
}

// A wedge of a pie, or of a donut when the hole is more than nought.
fn wedge(
    centre_x: f64,
    centre_y: f64,
    radius: f64,
    hole: f64,
    from: f64,
    to: f64,
    colour: String,
) -> String {
    let (x1, y1) = on_circle(centre_x, centre_y, radius, from);
    let (x2, y2) = on_circle(centre_x, centre_y, radius, to);
    let large = if to - from > 180.0 { 1 } else { 0 };
    if hole > 0.0 {
        let (i1, iy1) = on_circle(centre_x, centre_y, hole, from);
        let (i2, iy2) = on_circle(centre_x, centre_y, hole, to);
        return format!(
            "<path d=\"M{x1:.1},{y1:.1} A{radius:.1},{radius:.1} 0 {large} 1 {x2:.1},{y2:.1} \
             L{i2:.1},{iy2:.1} A{hole:.1},{hole:.1} 0 {large} 0 {i1:.1},{iy1:.1} Z\" \
             fill=\"{colour}\"/>"
        );
    }
    format!(
        "<path d=\"M{centre_x:.1},{centre_y:.1} L{x1:.1},{y1:.1} A{radius:.1},{radius:.1} 0 \
         {large} 1 {x2:.1},{y2:.1} Z\" fill=\"{colour}\"/>"
    )
}

// A point on the circle of a pie, where nought degrees is the top of it.
fn on_circle(centre_x: f64, centre_y: f64, radius: f64, degrees: f64) -> (f64, f64) {
    let angle = degrees.to_radians();
    (
        centre_x + angle.cos() * radius,
        centre_y + angle.sin() * radius,
    )
}

// ---------------------------------------------------------------- shapes

// A shape: a figure of the box, filled, outlined, or drawn as a line. A shape that arrives grows
// from the middle of its box, and a shape drawn as a stroke is drawn on from one end.
fn shapes(
    layer: &Layer,
    pose: &Pose,
    board: Box_,
    rule: Rule,
    faults: &mut Vec<String>,
) -> String {
    let style = &layer.style;
    let said = pick(&style.shape, "rect");
    let fill = match &pose.fill {
        Some(one) => Some(one.clone()),
        None if !style.fill.is_empty() => Some(style.fill.clone()),
        None => None,
    };
    let stroke = if style.stroke.is_empty() {
        None
    } else {
        Some(style.stroke.clone())
    };
    if fill.is_none() && stroke.is_none() {
        faults.push(String::from(
            "the shape has neither a fill nor a stroke, so nothing of it can be seen. Give it a \"fill\" or a \"stroke\".",
        ));
        return String::new();
    }
    let weight = part(style.stroke_size, 0.006, rule.height);
    let paint = format!(
        "{}{}",
        match &fill {
            Some(one) => format!(" fill=\"{one}\""),
            None => String::from(" fill=\"none\""),
        },
        match &stroke {
            Some(one) => format!(
                " stroke=\"{one}\" stroke-width=\"{:.1}\" stroke-linejoin=\"round\"",
                weight
            ),
            None => String::new(),
        }
    );
    let reveal = pose.reveal.clamp(0.0, 1.0);
    let scale = if reveal < 1.0 && stroke.is_none() {
        0.82 + 0.18 * reveal
    } else {
        1.0
    };
    let inner = Box_ {
        x: board.centre() - board.w * scale / 2.0,
        y: board.middle() - board.h * scale / 2.0,
        w: (board.w * scale).max(1.0),
        h: (board.h * scale).max(1.0),
    };
    let element = match said {
        "rect" => format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\"",
            inner.x, inner.y, inner.w, inner.h
        ),
        "round" => {
            let radius = part(style.radius, 0.02, rule.height)
                .min(inner.w / 2.0)
                .min(inner.h / 2.0);
            format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" \
                 ry=\"{:.1}\"",
                inner.x, inner.y, inner.w, inner.h, radius, radius
            )
        }
        "circle" => format!(
            "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\"",
            inner.centre(),
            inner.middle(),
            inner.w.min(inner.h) / 2.0
        ),
        "ellipse" => format!(
            "<ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\"",
            inner.centre(),
            inner.middle(),
            inner.w / 2.0,
            inner.h / 2.0
        ),
        "triangle" => polygon(&inner, 3, -90.0),
        "star" => star(&inner, 5, 0.45),
        "plus" => plus(&inner),
        "arrow" => arrow(&inner, style.arc),
        "blob" => blob(&inner),
        "line" => format!(
            "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\"",
            inner.x,
            inner.middle(),
            inner.right(),
            inner.middle()
        ),
        "path" => {
            if style.path.trim().is_empty() {
                faults.push("the shape has no \"path\" to draw.".to_string());
                return String::new();
            }
            let view = if style.view > 0.0 { style.view } else { 100.0 };
            let (sx, sy) = (inner.w / view, inner.h / view);
            let dash = dash_of(&stroke, reveal, (inner.w + inner.h) * view * 2.0);
            return format!(
                "<g transform=\"translate({:.1} {:.1}) scale({:.4} {:.4})\"><path d=\"{}\"{}{}/></g>",
                inner.x,
                inner.y,
                sx,
                sy,
                escaped_path(&style.path),
                paint,
                dash
            );
        }
        other => {
            faults.push(format!(
                "\"{other}\" is not a kind of shape. The kinds are rect, round, circle, ellipse, \
                 triangle, arrow, line, star, plus, blob and path."
            ));
            return String::new();
        }
    };
    format!("{element}{paint}{}/>", dash_of(&stroke, reveal, (inner.w + inner.h) * 2.0))
}

// The marks that draw a stroke on from one end, when a shape is arriving: the line is hidden and
// let out as the reveal grows.
fn dash_of(stroke: &Option<String>, reveal: f64, length: f64) -> String {
    if stroke.is_none() || reveal >= 1.0 {
        return String::new();
    }
    format!(
        " stroke-dasharray=\"{:.1}\" stroke-dashoffset=\"{:.1}\"",
        length,
        length * (1.0 - reveal)
    )
}

// The marks of a regular figure of `sides`, turned so that the first stands at `from` degrees.
fn polygon(board: &Box_, sides: usize, from: f64) -> String {
    let points: Vec<String> = (0..sides)
        .map(|place| {
            let angle = (from + place as f64 * (360.0 / sides as f64)).to_radians();
            format!(
                "{:.1},{:.1}",
                board.centre() + angle.cos() * board.w / 2.0,
                board.middle() + angle.sin() * board.h / 2.0
            )
        })
        .collect();
    format!("<polygon points=\"{}\"", points.join(" "))
}

// A star of `points`, with the inner ring at `part` of the outer.
fn star(board: &Box_, points: usize, part: f64) -> String {
    let mut held: Vec<String> = Vec::new();
    for place in 0..points * 2 {
        let angle = (-90.0 + place as f64 * (180.0 / points as f64)).to_radians();
        let (radius_x, radius_y) = if place % 2 == 0 {
            (board.w / 2.0, board.h / 2.0)
        } else {
            (board.w / 2.0 * part, board.h / 2.0 * part)
        };
        held.push(format!(
            "{:.1},{:.1}",
            board.centre() + angle.cos() * radius_x,
            board.middle() + angle.sin() * radius_y
        ));
    }
    format!("<polygon points=\"{}\"", held.join(" "))
}

// A cross of two bars, which reads as an adding, or as a marker of a place.
fn plus(board: &Box_) -> String {
    let thick = board.w.min(board.h) * 0.32;
    let corners = [
        (board.centre() - thick / 2.0, board.y),
        (board.centre() + thick / 2.0, board.y),
        (board.centre() + thick / 2.0, board.middle() - thick / 2.0),
        (board.right(), board.middle() - thick / 2.0),
        (board.right(), board.middle() + thick / 2.0),
        (board.centre() + thick / 2.0, board.middle() + thick / 2.0),
        (board.centre() + thick / 2.0, board.bottom()),
        (board.centre() - thick / 2.0, board.bottom()),
        (board.centre() - thick / 2.0, board.middle() + thick / 2.0),
        (board.x, board.middle() + thick / 2.0),
        (board.x, board.middle() - thick / 2.0),
        (board.centre() - thick / 2.0, board.middle() - thick / 2.0),
    ];
    let points: Vec<String> = corners
        .iter()
        .map(|(x, y)| format!("{:.1},{:.1}", x, y))
        .collect();
    format!("<polygon points=\"{}\"", points.join(" "))
}

// An arrow: a shaft and a head, pointing right by default and down when `arc` says so.
fn arrow(board: &Box_, arc: f64) -> String {
    let down = (arc - 90.0).abs() < 45.0 || (arc - 270.0).abs() < 45.0;
    if down {
        let shaft = board.w * 0.3;
        let head = board.h * 0.34;
        let points = vec![
            (board.centre() - shaft / 2.0, board.y),
            (board.centre() + shaft / 2.0, board.y),
            (board.centre() + shaft / 2.0, board.bottom() - head),
            (board.right(), board.bottom() - head),
            (board.centre(), board.bottom()),
            (board.x, board.bottom() - head),
            (board.centre() - shaft / 2.0, board.bottom() - head),
        ];
        return polygon_of(&points);
    }
    let shaft = board.h * 0.3;
    let head = board.w * 0.34;
    let points = vec![
        (board.x, board.middle() - shaft / 2.0),
        (board.right() - head, board.middle() - shaft / 2.0),
        (board.right() - head, board.y),
        (board.right(), board.middle()),
        (board.right() - head, board.bottom()),
        (board.right() - head, board.middle() + shaft / 2.0),
        (board.x, board.middle() + shaft / 2.0),
    ];
    polygon_of(&points)
}

// The marks of a list of points, as a figure.
fn polygon_of(points: &[(f64, f64)]) -> String {
    let held: Vec<String> = points
        .iter()
        .map(|(x, y)| format!("{:.1},{:.1}", x, y))
        .collect();
    format!("<polygon points=\"{}\"", held.join(" "))
}

// A figure with soft lobes, which is what a shape behind a word wants when a rectangle is blunt.
fn blob(board: &Box_) -> String {
    let lobes = 6usize;
    let held: Vec<(f64, f64)> = (0..lobes)
        .map(|place| {
            let angle = place as f64 * (360.0 / lobes as f64);
            let wobble = 0.8 + 0.2 * (((place as f64) * 2.4).sin().abs());
            (
                board.centre() + angle.to_radians().cos() * board.w / 2.0 * wobble,
                board.middle() + angle.to_radians().sin() * board.h / 2.0 * wobble,
            )
        })
        .collect();
    let mut path = String::new();
    for place in 0..held.len() {
        let here = held[place];
        let next = held[(place + 1) % held.len()];
        if place == 0 {
            path.push_str(&format!("M{:.1},{:.1}", here.0, here.1));
        }
        let middle = ((here.0 + next.0) / 2.0, (here.1 + next.1) / 2.0);
        path.push_str(&format!(
            " Q{:.1},{:.1} {:.1},{:.1}",
            here.0, here.1, middle.0, middle.1
        ));
    }
    path.push('Z');
    format!("<path d=\"{path}\"")
}

// The words of a path, with the marks of it kept and the quotes of it turned, for the words of the
// path stand between quotes of their own.
fn escaped_path(said: &str) -> String {
    let mut out = String::with_capacity(said.len());
    for c in said.chars() {
        match c {
            '"' => out.push('\''),
            '<' | '>' | '&' => {}
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------- helps

// One part of a measure, in pixels, with the default a kind keeps.
fn part(given: f64, fallback: f64, measure: f64) -> f64 {
    if given > 0.0 {
        given * measure
    } else {
        fallback * measure
    }
}

// A part of one thing against another, as it is reported to a model.
fn pick_part(given: f64, fallback: f64) -> f64 {
    if given > 0.0 {
        given
    } else {
        fallback
    }
}

// How much of a length was needed against what there was.
fn ratio(needed: f64, held: f64) -> f64 {
    if held <= 0.0 {
        return 0.0;
    }
    needed / held
}

// A word of a style, or the word a kind keeps when nothing was given.
fn pick<'a>(given: &'a str, fallback: &'a str) -> &'a str {
    if given.is_empty() {
        fallback
    } else {
        given
    }
}

// A word of a style, when it is a word at all.
fn non_empty(given: &str) -> Option<String> {
    if given.is_empty() {
        None
    } else {
        Some(given.to_string())
    }
}

// The colour a ground is read in: of the ink of the set and the ground of the set, the one further
// from the ground in brightness. A plate and the words upon it must not be the same colour.
fn against(ground: &str, palette: &Palette) -> String {
    let bright = |said: &str| -> f64 {
        let held = anim::colour_of(said).unwrap_or([0.5, 0.5, 0.5]);
        0.2126 * held[0] + 0.7152 * held[1] + 0.0722 * held[2]
    };
    if ground.is_empty() {
        return palette.ink.clone();
    }
    let held = bright(ground);
    let ink = bright(&palette.ink);
    let next = bright(&palette.ground);
    if (ink - held).abs() >= (next - held).abs() {
        palette.ink.clone()
    } else {
        palette.ground.clone()
    }
}

// The colour of the content of a layer: a keyframe first, then the style, then the ink of the set.
fn ink(layer: &Layer, pose: &Pose, palette: &Palette) -> String {
    if let Some(one) = &pose.colour {
        return one.clone();
    }
    if !layer.style.colour.is_empty() {
        return layer.style.colour.clone();
    }
    palette.ink.clone()
}

// The family a layer asks for, as the computer named it. The names are quoted with single quotes,
// for the whole list stands inside a double-quoted attribute of the document, and a double quote
// there stops the document being read at all.
fn family_of(said: &str, fonts: &Fonts) -> String {
    let clear = |one: &str| one.replace(['\'', '"'], "").trim().to_string();
    match said.trim().to_ascii_lowercase().as_str() {
        "" | "sans" | "sans-serif" => format!("'{}', sans-serif", clear(&fonts.body)),
        "mono" | "monospace" | "code" => format!("'{}', monospace", clear(&fonts.mono)),
        "serif" => String::from("serif"),
        other => format!("'{}', sans-serif", clear(other)),
    }
}

// One ruled line of a table or of an axis.
fn ruled(x: f64, y: f64, next_x: f64, next_y: f64, weight: f64, colour: String) -> String {
    format!(
        "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{colour}\" \
         stroke-width=\"{:.1}\"/>",
        x, y, next_x, next_y, weight
    )
}

// One filled box, with a corner of its own.
fn filled(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
    colour: String,
    opacity: f64,
) -> String {
    format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" \
         fill=\"{colour}\" opacity=\"{opacity:.3}\"/>",
        x,
        y,
        width.max(0.0),
        height.max(0.0),
        radius,
        radius
    )
}

// Text made ready to stand in the words of a document.
fn escaped(text: &str) -> String {
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

#[cfg(test)]
mod checks {
    use crate::infographic::anim;
    use crate::infographic::story::{Datum, Layer, Palette, Style};

    use super::*;

    fn pose_of(layer: &Layer) -> Pose {
        anim::pose(layer, layer.from, layer.to)
    }

    fn sheet(layer: &Layer, width: u32, height: u32) -> Sheet {
        draw(layer, &pose_of(layer), width, height, (1920, 1080), &Fonts::new(), &Palette::default())
    }

    // The words of a layer are drawn, and nothing is found wrong with a box of room.
    #[test]
    fn the_words_of_a_layer_are_drawn() {
        let layer = Layer {
            kind: String::from("text"),
            style: Style {
                text: String::from("**Autumn** falls, {#c2410c}slowly{}"),
                size: 0.06,
                ..Style::default()
            },
            ..Layer::default()
        };
        let made = sheet(&layer, 900, 200);
        assert!(made.faults.is_empty(), "{:?}", made.faults);
        assert!(made.svg.starts_with("<svg"), "{}", &made.svg[..40]);
        assert!(made.svg.contains("Autumn"), "the words stand");
        assert!(made.svg.contains("#c2410c"), "the colour of the run stands");
        assert!(made.svg.contains("font-weight=\"700\""), "the bold weight stands");
    }

    // A layer of words is painted into pixels, which is the check that the document the drawing
    // makes is one that can be read at all: an attribute quoted wrongly stops the reading, and
    // the words then stand nowhere.
    #[test]
    fn a_layer_of_words_is_painted_into_ink() {
        use crate::infographic::encode;
        let layer = Layer {
            kind: String::from("text"),
            style: Style { text: String::from("**Autumn**"), size: 0.06, ..Style::default() },
            from: 0.0,
            to: 1.0,
            ..Layer::default()
        };
        let made = sheet(&layer, 600, 120);
        assert!(!made.svg.contains("\"\""), "no attribute holds a bare quote: {}", &made.svg[..120.min(made.svg.len())]);
        let pixels = encode::paint(&made.svg, 600, 120, &Fonts::new()).expect("the words are painted");
        let mut ink = 0usize;
        for one in pixels.pixels() {
            if one.red() < 128 && one.green() < 128 && one.alpha() > 0 {
                ink += 1;
            }
        }
        assert!(ink > 200, "the words stand as ink in the pixels: {ink}");
    }

    // Words that do not fit their box are told, and how much went away.
    #[test]
    fn words_that_do_not_fit_are_told() {
        let layer = Layer {
            kind: String::from("text"),
            style: Style {
                text: String::from("one two three four five six seven eight nine ten"),
                size: 0.08,
                max_lines: 1,
                ..Style::default()
            },
            ..Layer::default()
        };
        let made = sheet(&layer, 400, 90);
        assert_eq!(made.faults.len(), 1, "{:?}", made.faults);
        assert!(made.faults[0].contains("words"), "{}", made.faults[0]);
    }

    // A reveal of a third lets one word of three out, and a reveal of one lets all of them out.
    #[test]
    fn a_let_out_of_words_keeps_the_words_it_keeps() {
        let mut layer = Layer {
            kind: String::from("text"),
            style: Style { text: String::from("one two three"), size: 0.05, ..Style::default() },
            from: 0.0,
            to: 1.0,
            ..Layer::default()
        };
        layer.style.reveal = 0.34;
        let part = sheet(&layer, 600, 120);
        layer.style.reveal = 1.0;
        let whole = sheet(&layer, 600, 120);
        assert_eq!(count(&part.svg, "<text"), 1, "one word of three stands");
        assert_eq!(count(&whole.svg, "<text"), 3, "all three stand");
    }

    fn count(said: &str, needle: &str) -> usize {
        said.matches(needle).count()
    }

    // A panel stands behind words that ask for one.
    #[test]
    fn a_panel_stands_behind_the_words() {
        let layer = Layer {
            kind: String::from("text"),
            style: Style {
                text: String::from("lower third"),
                panel: String::from("bar"),
                panel_colour: String::from("#0d1117"),
                colour: String::from("#f2f4f8"),
                ..Style::default()
            },
            ..Layer::default()
        };
        let made = sheet(&layer, 800, 120);
        assert!(made.svg.contains("<rect") && made.svg.contains("#0d1117"), "{}", made.svg);
    }

    // A table of numbers stands with its heads, and its numbers stand to the right.
    #[test]
    fn a_table_of_numbers_is_drawn() {
        let layer = Layer {
            kind: String::from("table"),
            style: Style {
                heads: vec![String::from("Month"), String::from("Rain")],
                rows: vec![
                    vec![String::from("April"), String::from("31")],
                    vec![String::from("May"), String::from("28")],
                ],
                form: String::from("grid"),
                size: 0.03,
                ..Style::default()
            },
            ..Layer::default()
        };
        let made = sheet(&layer, 900, 220);
        assert!(made.faults.is_empty(), "{:?}", made.faults);
        assert!(made.svg.contains("Month") && made.svg.contains("April"), "the cells stand");
        assert!(made.svg.contains("<line"), "the grid stands");
        assert_eq!(count(&made.svg, "<text"), 6, "two heads and four cells");
    }

    // The words of the plate of a table are not of the colour of the plate, whichever way the set
    // is turned: a head nobody can read is a fault nobody reports.
    #[test]
    fn the_words_of_a_plate_read_against_it() {
        let layer = Layer {
            kind: String::from("table"),
            style: Style {
                heads: vec![String::from("Part"), String::from("Share")],
                rows: vec![
                    vec![String::from("Ground"), String::from("0.42")],
                    vec![String::from("Water"), String::from("0.31")],
                    vec![String::from("Air"), String::from("0.27")],
                ],
                form: String::from("zebra"),
                ..Style::default()
            },
            from: 0.0,
            to: 1.0,
            ..Layer::default()
        };
        let palette = story::palette_of("night").expect("the set is known");
        let made =
            draw(&layer, &pose_of(&layer), 600, 260, (1920, 1080), &Fonts::new(), &palette);
        assert!(made.faults.is_empty(), "{:?}", made.faults);
        assert_eq!(count(&made.svg, "font-weight=\"700\""), 2, "two heads stand");
        // The plate is the ink of the set, and the words upon it are the ground of it.
        assert!(made.svg.contains(&format!("fill=\"{}\"", palette.ink)), "{}", made.svg);
        for one in made.svg.split("font-weight=\"700\"").skip(1) {
            let held = one
                .split("fill=\"")
                .nth(1)
                .unwrap_or("")
                .split('"')
                .next()
                .unwrap_or("");
            assert_ne!(held, palette.ink, "a head drawn in the colour of its own plate");
            assert_eq!(held, palette.ground, "the head reads in the ground of the set");
        }
        // The first row of a zebra carries the plate, so a table of three rows is barred twice.
        let said = format!("fill=\"{}\" opacity=\"0.900\"", palette.ground);
        assert_eq!(count(&made.svg, &said), 2, "a plate under the first row and under the third");
    }

    // The colour a plate is read in is the further of the two colours of the set.
    #[test]
    fn a_colour_is_read_against_its_plate() {
        let palette = story::palette_of("ink").expect("the set is known");
        assert_eq!(against(&palette.ink, &palette), palette.ground);
        assert_eq!(against(&palette.ground, &palette), palette.ink);
        assert_eq!(against("", &palette), palette.ink, "nothing was set as a plate");
        let said = story::palette_of("mono").expect("the set is known");
        assert_eq!(against("#101010", &said), "#ffffff", "a plate of the mono set");
    }

    // A table with nothing in it is told, and a chart with nothing in it is told.
    #[test]
    fn empty_numbers_are_told() {
        let table = Layer { kind: String::from("table"), ..Layer::default() };
        let made = sheet(&table, 600, 300);
        assert!(!made.faults.is_empty(), "a table of nothing is told");
        let chart = Layer {
            kind: String::from("chart"),
            style: Style { chart: String::from("bars"), ..Style::default() },
            ..Layer::default()
        };
        let made = sheet(&chart, 600, 300);
        assert!(made.faults.iter().any(|one| one.contains("no numbers")), "{:?}", made.faults);
    }

    // A chart of bars grows to its values, so a reveal of a half draws a half of every bar.
    #[test]
    fn a_chart_grows_to_its_values() {
        let mut layer = Layer {
            kind: String::from("chart"),
            style: Style {
                chart: String::from("bars"),
                items: vec![
                    Datum { label: String::from("one"), value: 10.0, ..Datum::default() },
                    Datum { label: String::from("two"), value: 5.0, ..Datum::default() },
                ],
                size: 0.02,
                ..Style::default()
            },
            ..Layer::default()
        };
        layer.style.reveal = 1.0;
        let whole = sheet(&layer, 800, 400);
        layer.style.reveal = 0.5;
        let part = sheet(&layer, 800, 400);
        let tall = first_height(&whole.svg).expect("a bar is drawn");
        let short = first_height(&part.svg).expect("a bar is drawn");
        assert!(short > 0.0 && (short - tall / 2.0).abs() < 1.0, "{short} against {tall}");
    }

    // The height of the first box of a drawing, which is the first bar of a chart.
    fn first_height(said: &str) -> Option<f64> {
        let at = said.find("<rect")?;
        let rest = &said[at..];
        let from = rest.find("height=\"")? + "height=\"".len();
        let to = from + rest[from..].find('"')?;
        rest[from..to].parse::<f64>().ok()
    }

    // A shape with neither a fill nor a stroke is told, and a shape of a kind nothing knows is
    // told.
    #[test]
    fn a_shape_that_cannot_be_seen_is_told() {
        let shape = Layer {
            kind: String::from("shape"),
            style: Style { shape: String::from("rect"), ..Style::default() },
            ..Layer::default()
        };
        let made = sheet(&shape, 400, 200);
        assert!(made.faults.iter().any(|one| one.contains("neither a fill")), "{:?}", made.faults);
        let other = Layer {
            kind: String::from("shape"),
            style: Style {
                shape: String::from("waffle"),
                fill: String::from("#ffffff"),
                ..Style::default()
            },
            ..Layer::default()
        };
        let made = sheet(&other, 400, 200);
        assert!(made.faults.iter().any(|one| one.contains("waffle")), "{:?}", made.faults);
    }

    // A filled shape is drawn, and a path of the words of SVG is drawn within its box.
    #[test]
    fn a_shape_and_a_path_are_drawn() {
        let shape = Layer {
            kind: String::from("shape"),
            style: Style { shape: String::from("circle"), fill: String::from("#0f766e"), ..Style::default() },
            ..Layer::default()
        };
        let made = sheet(&shape, 400, 200);
        assert!(made.svg.contains("<circle") && made.svg.contains("#0f766e"), "{}", made.svg);
        assert!(!made.svg.contains("*/"), "the drawing is shut: {}", made.svg);
        let path = Layer {
            kind: String::from("shape"),
            style: Style {
                shape: String::from("path"),
                path: String::from("M10 10 L90 10 L90 90 Z"),
                stroke: String::from("#c2410c"),
                stroke_size: 0.004,
                view: 100.0,
                ..Style::default()
            },
            ..Layer::default()
        };
        let made = sheet(&path, 400, 200);
        assert!(made.svg.contains("M10 10 L90 10 L90 90 Z"), "{}", made.svg);
        assert!(made.svg.contains("scale("), "the path is scaled to the box");
        assert!(!made.svg.contains("*/"), "the drawing is shut: {}", made.svg);
    }

    // Every kind of shape draws something, and none of them leaves a mark of the helper out.
    #[test]
    fn every_kind_of_shape_is_drawn() {
        for kind in ["rect", "round", "circle", "ellipse", "triangle", "star", "plus", "arrow", "blob", "line"] {
            let layer = Layer {
                kind: String::from("shape"),
                style: Style {
                    shape: String::from(kind),
                    fill: String::from("#c2410c"),
                    ..Style::default()
                },
                ..Layer::default()
            };
            let made = sheet(&layer, 400, 200);
            assert!(made.faults.is_empty(), "{kind}: {:?}", made.faults);
            assert!(made.svg.contains("<"), "{kind} draws nothing");
            assert!(!made.svg.contains("*/"), "{kind} leaves a mark out: {}", made.svg);
        }
    }

    // A table of values taller than its box says how many rows stand.
    #[test]
    fn a_table_over_its_box_says_what_stands() {
        let rows: Vec<Vec<String>> = (0..20)
            .map(|place| vec![format!("row {place}"), format!("{}", place * 3)])
            .collect();
        let layer = Layer {
            kind: String::from("table"),
            style: Style { heads: vec![String::from("A"), String::from("B")], rows, size: 0.03, ..Style::default() },
            ..Layer::default()
        };
        let made = sheet(&layer, 700, 200);
        assert!(made.faults.iter().any(|one| one.contains("rows")), "{:?}", made.faults);
    }

    // The value of a cell of a table, out of a number with a comma and a unit upon it.
    #[test]
    fn a_number_of_a_cell_is_read() {
        assert_eq!(value_of("1,234"), Some(1234.0));
        assert_eq!(value_of("$42.50"), Some(42.5));
        assert_eq!(value_of("April"), None);
        assert_eq!(value_of(""), None);
    }

    // The picture of a layer of media carries no drawing of its own: the compositor brings the
    // frame.
    #[test]
    fn a_layer_of_media_draws_nothing() {
        let layer = Layer { kind: String::from("video"), ..Layer::default() };
        let made = sheet(&layer, 600, 400);
        assert!(made.faults.is_empty());
        assert!(!made.svg.contains("<rect"));
    }
}

