// The layout of an infographic: what a block was asked for, and what it takes
// on the screen.
//
// One pass reads a scene and answers two questions about every block: where it
// stands, and whether what it holds fits there. The same answer is used to check
// a scene and to draw it, so a scene that checks clean draws clean. A check that
// measured one thing and a renderer another would be a check nobody could trust.
//
// A block is placed on the grid of twenty-four columns and sixteen rows, inside
// the safe margin of the canvas. Words are cut into lines that fit the width of
// the box. A block whose words stand taller than its box is shrunk a step at a
// time, down to six-tenths of the size it asked for; a block that is still too
// full is told to the model. That is the point of the whole measure: the video is
// mended while it is still words, not after it is a picture.

use crate::infographic::fonts::{self, Fonts};
use crate::infographic::marks;
use crate::infographic::script::{self, Background, Block, Item, Palette, Project};

// The safe margin, as a share of the shorter side of the canvas.
pub const MARGIN: f64 = 0.045;

// How far a block may be shrunk to make its words fit, and how big a step of it
// is.
const SMALLEST: f64 = 0.6;
const STEP: f64 = 0.94;

// A place on the screen, in pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn middle(&self) -> f64 {
        self.y + self.h / 2.0
    }
    pub fn centre(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn inset(&self, by: f64) -> Rect {
        Rect::new(
            self.x + by,
            self.y + by,
            (self.w - by * 2.0).max(1.0),
            (self.h - by * 2.0).max(1.0),
        )
    }
}

// One run of words with the colour and the weight of its own.
#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub colour: String,
    pub weight: u16,
    pub italic: bool,
}

// One line of words, with the width it measured and the size of its letters.
#[derive(Clone, Debug, Default)]
pub struct Line {
    pub runs: Vec<Run>,
    pub width: f64,
    pub size: f64,
}

// What a row of a block is: one line of a list, one side-head of a compare, one
// row of a table, one step, one point of a timeline, one pill, one line of a
// chart key, one word under a bar.
pub const R_ITEM: &str = "item";
pub const R_HEAD: &str = "head";
pub const R_SUB: &str = "sub";
pub const R_ROW: &str = "row";
pub const R_STEP: &str = "step";
pub const R_POINT: &str = "point";
pub const R_PILL: &str = "pill";
pub const R_LEGEND: &str = "legend";
pub const R_AXIS: &str = "axis";

// One row of a block, placed.
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub role: String,
    pub bx: Rect,
    // The words of the row. For a table row, `words` holds the cells.
    pub lines: Vec<Line>,
    pub sub: Vec<Line>,
    pub title: Vec<Line>,
    pub when: Vec<Line>,
    pub words: Vec<String>,
    pub colour: String,
    pub value: f64,
    pub mark: String,
    pub at: f64,
}

// The geometry of a chart, worked out once.
#[derive(Clone, Debug, Default)]
pub struct Chart {
    // The plot inside the box, after the axis and the labels took their room.
    pub plot: Rect,
    pub bars: Vec<Bar>,
    pub lines: Vec<LineChart>,
    pub wedges: Vec<Wedge>,
    pub ticks: Vec<Tick>,
    // The place of the zero of the value axis, for a chart with negative bars.
    pub zero: f64,
    // The top and the bottom of the value axis.
    pub top: f64,
    pub floor: f64,
    // The centre of a pie, and its radius.
    pub cx: f64,
    pub cy: f64,
    pub radius: f64,
}

#[derive(Clone, Debug)]
pub struct Bar {
    pub label: String,
    pub value: f64,
    pub colour: String,
    pub bx: Rect,
    pub set: usize,
}

#[derive(Clone, Debug)]
pub struct LineChart {
    pub name: String,
    pub colour: String,
    pub fill: bool,
    pub dashed: bool,
    pub width: f64,
    pub points: Vec<(f64, f64)>,
}

#[derive(Clone, Debug)]
pub struct Wedge {
    pub label: String,
    pub value: f64,
    pub colour: String,
    pub from: f64,
    pub to: f64,
    pub share: f64,
}

#[derive(Clone, Debug)]
pub struct Tick {
    pub y: f64,
    pub words: String,
}

// A picture of the room, read for the drawing.
#[derive(Clone, Debug)]
pub struct Image {
    pub data: Vec<u8>,
    pub mime: String,
    pub name: String,
}

// A block that has been read, placed and measured.
#[derive(Clone, Debug)]
pub struct Placed {
    pub kind: String,
    pub bx: Rect,
    pub lines: Vec<Line>,
    // The height the content takes, so a block too full says by how much.
    pub content: f64,
    pub size: f64,
    // The height of one line of the block.
    pub step: f64,
    pub align: String,
    pub valign: String,
    pub colour: String,
    pub bg: String,
    pub border: String,
    pub radius: f64,
    pub pad: f64,
    pub opacity: f64,
    pub reveal: String,
    pub delay: f64,
    pub stagger: f64,
    pub reveal_ms: f64,
    pub chart: Chart,
    pub rows: Vec<Row>,
    pub image: Option<Image>,
    pub mark: String,
    pub shape: String,
    pub stroke: f64,
    pub hole: f64,
    pub svg: String,
    // A number ready to draw, for a stat and for a progress bar.
    pub number: String,
    pub number_size: f64,
    pub label: String,
    // Whether the numbers of a chart stand on it, and whether a chart grows.
    pub show_values: bool,
    pub grows: bool,
    pub grow_ms: f64,
    // Whether the block was shrunk to make its content fit.
    pub shrunk: bool,
}

// ---------------------------------------------------------------- the grid

// The room of the grid: the canvas with the safe margin off it.
pub fn stage(width: u32, height: u32) -> Rect {
    let side = width.min(height) as f64;
    let margin = MARGIN * side;
    Rect::new(
        margin,
        margin,
        (width as f64) - margin * 2.0,
        (height as f64) - margin * 2.0,
    )
}

// The place of a block: the pixels it names, or the grid it names.
pub fn place(block: &Block, board: Rect) -> Rect {
    if block.pw > 0.0 && block.ph > 0.0 {
        return Rect::new(block.x, block.y, block.pw, block.ph);
    }
    let cell_w = board.w / f64::from(script::COLS);
    let cell_h = board.h / f64::from(script::ROWS);
    let col = if block.col > 1 { block.col - 1 } else { 0 };
    let row = if block.row > 1 { block.row - 1 } else { 0 };
    let col = col.clamp(0, script::COLS - 1);
    let row = row.clamp(0, script::ROWS - 1);
    let w = if block.w > 0 { block.w } else { script::COLS - col };
    let h = if block.h > 0 { block.h } else { 2 };
    let w = w.clamp(1, script::COLS - col);
    let h = h.clamp(1, script::ROWS - row);
    Rect::new(
        board.x + f64::from(col) * cell_w,
        board.y + f64::from(row) * cell_h,
        f64::from(w) * cell_w,
        f64::from(h) * cell_h,
    )
}

// ---------------------------------------------------------------- measuring

// The whole of a scene: every block placed and measured, with the faults of the
// scene beside them.
pub fn scene(
    project: &Project,
    palette: &Palette,
    fonts: &Fonts,
    blocks: &[Block],
    board: Rect,
    room: Option<&std::path::Path>,
) -> (Vec<Placed>, Vec<String>) {
    let mut out: Vec<Placed> = Vec::new();
    let mut faults: Vec<String> = Vec::new();
    for (number, block) in blocks.iter().enumerate() {
        let (made, bad) = block_of(project, palette, fonts, block, board, room);
        for one in bad {
            faults.push(format!("block {} of kind \"{}\": {one}", number + 1, block.kind));
        }
        out.push(made);
    }
    faults.extend(overlaps(&out));
    (out, faults)
}

// Two blocks that stand on one another. Laid over is nearly always an accident,
// so a pair that shares more than a corner is told.
fn overlaps(placed: &[Placed]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (left, one) in placed.iter().enumerate() {
        // A shape, a rule and a piece of the model's own drawing are laid under
        // other things on purpose.
        if one.kind == "shape" || one.kind == "rule" || one.kind == "svg" {
            continue;
        }
        for (right, other) in placed.iter().enumerate().skip(left + 1) {
            if other.kind == "shape" || other.kind == "rule" || other.kind == "svg" {
                continue;
            }
            let Some(part) = shared(one.bx, other.bx) else { continue };
            if part < 0.12 {
                continue;
            }
            out.push(format!(
                "block {} of kind \"{}\" and block {} of kind \"{}\" stand on one another: {} of the smaller one is covered. Give one of them another row, or make the boxes narrower.",
                left + 1,
                one.kind,
                right + 1,
                other.kind,
                percent(part)
            ));
        }
    }
    out
}

// The share of the smaller of two boxes that both cover.
fn shared(one: Rect, other: Rect) -> Option<f64> {
    let w = one.right().min(other.right()) - one.x.max(other.x);
    let h = one.bottom().min(other.bottom()) - one.y.max(other.y);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let small = (one.w * one.h).min(other.w * other.h);
    if small <= 0.0 {
        return None;
    }
    Some(((w * h) / small).min(1.0))
}

fn percent(part: f64) -> String {
    format!("{:.0}%", part * 100.0)
}

// One block: measured, shrunk if it must be, and told on where it fails.
pub fn block_of(
    project: &Project,
    palette: &Palette,
    fonts: &Fonts,
    block: &Block,
    board: Rect,
    room: Option<&std::path::Path>,
) -> (Placed, Vec<String>) {
    let (mut made, mut faults) = measure(project, palette, fonts, block, board, room);
    if !can_shrink(&made.kind) || made.content <= made.bx.h + 1.0 {
        return (made, faults);
    }
    // The block is too full. A smaller size may break the words into fewer lines,
    // so the whole block is measured again at each step.
    let first = made.size;
    let mut size = made.size;
    for _ in 0..12 {
        size *= STEP;
        if size < first * SMALLEST {
            break;
        }
        let (smaller, _) = measure_size(project, palette, fonts, block, board, room, size);
        made = smaller;
        if made.content <= made.bx.h + 1.0 {
            made.shrunk = true;
            return (made, faults);
        }
    }
    made.shrunk = true;
    let rows_up = (made.content / (board.h / f64::from(script::ROWS))).ceil() as i32;
    faults.push(format!(
        "the content stands {} tall and the box holds {}; shrinking to {} of the size did not mend it. Shorten the words, or give the block a greater \"h\" ({} rows stands).",
        whole(made.content),
        whole(made.bx.h),
        percent(SMALLEST),
        rows_up
    ));
    (made, faults)
}

// Whether shrinking the words of a kind can make it fit.
fn can_shrink(kind: &str) -> bool {
    matches!(
        kind,
        "head" | "text" | "quote" | "bullets" | "steps" | "timeline" | "table" | "pill" | "code" | "stat"
    )
}

// A block measured at a size the caller names.
fn measure_size(
    project: &Project,
    palette: &Palette,
    fonts: &Fonts,
    block: &Block,
    board: Rect,
    room: Option<&std::path::Path>,
    size: f64,
) -> (Placed, Vec<String>) {
    let mut again = block.clone();
    // A size of a block is read in pixels when it stands at or above 0.2, so the
    // size settled on is passed on outright.
    again.size = size.max(0.2);
    measure(project, palette, fonts, &again, board, room)
}

// One pass over one block. The fault of standing too tall is left to `block_of`,
// which is the one that shrinks.
fn measure(
    project: &Project,
    palette: &Palette,
    fonts: &Fonts,
    block: &Block,
    board: Rect,
    room: Option<&std::path::Path>,
) -> (Placed, Vec<String>) {
    let kind = block.kind.as_str();
    let bx = place(block, board);
    let mut faults: Vec<String> = Vec::new();
    let body = fonts.body.clone();
    let heading_weight = fonts::weight_of(&project.heading_weight);
    let body_weight = fonts::weight_of(&block.weight);
    let size = size_of(block, kind, board.h);
    let mut made = Placed {
        kind: String::from(kind),
        bx,
        lines: Vec::new(),
        content: 0.0,
        size,
        step: size * 1.2,
        align: word(&block.align, default_align(kind), &["left", "center", "right"]),
        valign: word(&block.valign, default_valign(kind), &["top", "middle", "bottom"]),
        colour: named_or(&block.colour, default_colour(kind, palette)),
        bg: String::from(&block.bg),
        border: String::from(&block.border),
        radius: share(block.radius, 0.012, board),
        pad: share(block.pad, default_pad(kind), board),
        opacity: if block.opacity > 0.0 {
            block.opacity.min(1.0)
        } else {
            1.0
        },
        reveal: word(
            &block.reveal,
            default_reveal(kind),
            &["none", "fade", "rise", "left", "right", "pop", "wipe", "type"],
        ),
        delay: block.delay.max(0.0),
        stagger: block.stagger.max(0.0),
        reveal_ms: if block.reveal_ms > 0.0 {
            block.reveal_ms
        } else {
            420.0
        },
        chart: Chart::default(),
        rows: Vec::new(),
        image: None,
        mark: String::from(&block.mark),
        shape: String::from(&block.shape),
        stroke: share(block.stroke, default_stroke(kind), board),
        hole: block.hole.clamp(0.0, 0.9),
        svg: String::from(&block.svg),
        number: String::new(),
        number_size: 0.0,
        label: String::new(),
        show_values: block.show_values.unwrap_or(false),
        grows: block
            .grow
            .unwrap_or(matches!(kind, "bars" | "line" | "pie" | "progress" | "timeline")),
        grow_ms: if block.grow_ms > 0.0 { block.grow_ms } else { 700.0 },
        shrunk: block.size >= 0.2,
    };
    let leading = if block.leading > 0.0 {
        block.leading
    } else {
        fonts.line_em(&body, body_weight).clamp(1.08, 1.5)
    };
    let inner = Rect::new(
        bx.x + made.pad,
        bx.y + made.pad,
        (bx.w - made.pad * 2.0).max(10.0),
        (bx.h - made.pad * 2.0).max(10.0),
    );

    match kind {
        "head" | "text" | "quote" => {
            let weight = if kind == "head" { heading_weight } else { body_weight };
            let text = words_of(block);
            if text.is_empty() && block.spans.is_empty() {
                faults.push(String::from(
                    "there are no words in it. Name the words in \"text\", or the runs of colour in \"spans\".",
                ));
            }
            let room = inner.w;
            let runs = runs_of(block, &made.colour, weight, heading_weight);
            let lines = wrap_runs(fonts, &body, &runs, size, room);
            made.step = size * leading;
            let mark_room = if kind == "quote" { size * 1.1 } else { 0.0 };
            made.content = lines.len() as f64 * made.step + mark_room;
            made.lines = lines;
        }
        "bullets" => {
            let items = items_of(block);
            if items.is_empty() {
                faults.push(String::from(
                    "there is nothing in the list. Name the lines in \"items\", or write them one to a line in \"text\".",
                ));
            }
            let marker = word(
                &block.marker,
                "dot",
                &["dot", "check", "cross", "arrow", "number", "none"],
            );
            let bullet = size * 1.15;
            let gutter = if marker == "none" { 0.0 } else { bullet * 1.6 };
            let between = if block.gap > 0.0 {
                share(block.gap, 0.012, board)
            } else {
                size * 0.5
            };
            let mut used = 0.0;
            for (at, item) in items.iter().enumerate() {
                let runs = runs_of_item(item, &palette.ink, body_weight, heading_weight);
                let lines = wrap_runs(fonts, &body, &runs, size, (inner.w - gutter).max(20.0));
                let taken = lines.len() as f64 * size * leading;
                let mut sub_taken = 0.0;
                let mut sub: Vec<Line> = Vec::new();
                if !item.sub.is_empty() {
                    let sub_size = size * 0.82;
                    let runs = runs_of_text(
                        &item.sub,
                        &palette.muted,
                        400,
                        heading_weight,
                    );
                    sub = wrap_runs(fonts, &body, &runs, sub_size, (inner.w - gutter).max(20.0));
                    sub_taken = sub.len() as f64 * sub_size * leading + size * 0.12;
                }
                let height = taken + sub_taken;
                made.rows.push(Row {
                    role: String::from(R_ITEM),
                    bx: Rect::new(inner.x, inner.y + used, inner.w, height),
                    lines,
                    sub,
                    colour: named_or(&item.colour, palette.ink.clone()),
                    mark: if item.mark.is_empty() {
                        marker.clone()
                    } else {
                        word(&item.mark, "dot", &["dot", "check", "cross", "arrow", "number", "none"])
                    },
                    value: (at + 1) as f64,
                    at: item.value,
                    ..Default::default()
                });
                used += height + between;
            }
            made.content = (used - between).max(0.0);
            made.step = size * leading;
        }
        "stat" => {
            made.number = number_words(block.value, &block.prefix, &block.suffix, block.decimals);
            let wide = fonts.width(&made.number, &body, heading_weight, false, size);
            let room = inner.w.max(10.0);
            let held = if wide > room { size * (room / wide) * 0.98 } else { size };
            made.size = held;
            made.number_size = held;
            let label = words_of(block);
            made.label = label.clone();
            let label_size = held * 0.26;
            let lines = if label.is_empty() {
                Vec::new()
            } else {
                wrap_runs(
                    fonts,
                    &body,
                    &runs_of_text(&label, &palette.muted, 400, heading_weight),
                    label_size,
                    room,
                )
            };
            let label_room = if lines.is_empty() {
                0.0
            } else {
                lines.len() as f64 * label_size * leading + held * 0.14
            };
            made.content = held * 1.0 + label_room;
            made.lines = lines;
            made.step = held;
        }
        "bars" | "line" => {
            let (sets, labels) = sets_of(block, palette);
            if sets.is_empty() {
                faults.push(String::from(
                    "there are no numbers in it. Name them in \"values\", or in \"series\" with a name and numbers beside each.",
                ));
            }
            let legend_on = block.show_legend.unwrap_or(sets.len() > 1);
            let axis_on = block.show_axis.unwrap_or(true);
            let legend_room = if legend_on { size * 2.1 } else { 0.0 };
            let label_room = if axis_on { size * 1.9 } else { 0.0 };
            let field = Rect::new(
                inner.x,
                inner.y,
                inner.w,
                (inner.h - legend_room).max(10.0),
            );
            let (top, floor) = axis_ends(&numbers_of(&sets));
            let axis_size = size * 0.78;
            let gutter = if axis_on {
                let widest = (0..=4)
                    .map(|step| {
                        let value = floor + (top - floor) * f64::from(step) / 4.0;
                        fonts.width(
                            &number_words(value, &block.prefix, &block.suffix, decimals_for(top, floor, block.decimals)),
                            &body,
                            400,
                            false,
                            axis_size,
                        )
                    })
                    .fold(0.0_f64, f64::max);
                widest + size * 0.5
            } else {
                size * 0.2
            };
            let plot = Rect::new(
                field.x + gutter,
                field.y,
                (field.w - gutter).max(20.0),
                (field.h - label_room).max(20.0),
            );
            let zero = value_y(top, floor, 0.0, plot);
            let mut chart = Chart {
                plot,
                top,
                floor,
                zero,
                ..Default::default()
            };
            if axis_on {
                chart.ticks = (0..=4)
                    .map(|step| {
                        let value = floor + (top - floor) * f64::from(step) / 4.0;
                        Tick {
                            y: value_y(top, floor, value, plot),
                            words: number_words(
                                value,
                                &block.prefix,
                                &block.suffix,
                                decimals_for(top, floor, block.decimals),
                            ),
                        }
                    })
                    .collect();
            }
            let horizontal = block.direction == "horizontal";
            let groups = labels.len().max(1);
            if kind == "bars" {
                let sets_in = sets.len().max(1);
                let band = if horizontal {
                    plot.h / groups as f64
                } else {
                    plot.w / groups as f64
                };
                let group_room = band * 0.68;
                let each = (group_room / sets_in as f64).max(2.0);
                for (set, one) in sets.iter().enumerate() {
                    for (at, value) in one.values.iter().enumerate() {
                        let length = (plot.w * (value.abs() / (top - floor).max(0.0001))).max(0.0);
                        let across = (plot.h * (value.abs() / (top - floor).max(0.0001))).max(0.0);
                        let bx2 = if horizontal {
                            let y = plot.y + band * at as f64 + (band - group_room) / 2.0
                                + each * set as f64;
                            let from = value_x(top, floor, 0.0, plot);
                            let to = value_x(top, floor, *value, plot);
                            Rect::new(from.min(to), y, (to - from).abs(), each * 0.88)
                        } else {
                            let x = plot.x + band * at as f64 + (band - group_room) / 2.0
                                + each * set as f64;
                            let from = zero;
                            let to = value_y(top, floor, *value, plot);
                            Rect::new(x, from.min(to), each * 0.88, (from - to).abs())
                        };
                        let _ = (length, across);
                        chart.bars.push(Bar {
                            label: labels.get(at).cloned().unwrap_or_default(),
                            value: *value,
                            colour: one.colour.clone(),
                            bx: bx2,
                            set,
                        });
                    }
                }
            } else {
                for one in &sets {
                    let last = (one.values.len().max(2) - 1) as f64;
                    let points: Vec<(f64, f64)> = one
                        .values
                        .iter()
                        .enumerate()
                        .map(|(at, value)| {
                            let x = if last <= 0.0 {
                                plot.centre()
                            } else {
                                plot.x + plot.w * (at as f64) / last
                            };
                            (x, value_y(top, floor, *value, plot))
                        })
                        .collect();
                    chart.lines.push(LineChart {
                        name: one.name.clone(),
                        colour: one.colour.clone(),
                        fill: one.fill.unwrap_or(false),
                        dashed: one.dashed.unwrap_or(false),
                        width: share(block.stroke, 0.0035, board),
                        points,
                    });
                }
            }
            made.chart = chart;
            made.rows = axis_rows(fonts, &body, &labels, plot, horizontal, size * 0.82, axis_on);
            if legend_on {
                made.rows.extend(legend_rows(
                    &sets,
                    Rect::new(inner.x, field.bottom() + size * 0.3, inner.w, legend_room),
                    size * 0.85,
                ));
            }
            made.content = inner.h;
        }
        "pie" => {
            let (sets, _) = sets_of(block, palette);
            let total: f64 = sets.iter().map(|one| one.values.iter().sum::<f64>()).sum();
            if total <= 0.0 {
                faults.push(String::from("every number of the pie is 0, so nothing can be drawn."));
            }
            let legend_on = block.show_legend.unwrap_or(true);
            let legend_w = if legend_on { inner.w * 0.36 } else { 0.0 };
            let diameter = (inner.w - legend_w).min(inner.h).max(10.0);
            let mut chart = Chart {
                cx: inner.x + diameter / 2.0,
                cy: inner.middle(),
                radius: diameter / 2.0,
                plot: Rect::new(inner.x, inner.middle() - diameter / 2.0, diameter, diameter),
                ..Default::default()
            };
            let mut angle = 0.0;
            for (at, one) in sets.iter().enumerate() {
                let value: f64 = one.values.iter().sum();
                let share = if total > 0.0 { value / total } else { 0.0 };
                chart.wedges.push(Wedge {
                    label: one.name.clone(),
                    value,
                    colour: one.colour.clone(),
                    from: angle,
                    to: angle + share * 360.0,
                    share,
                });
                angle += share * 360.0;
            }
            made.chart = chart;
            if legend_on {
                made.rows = legend_rows(
                    &sets,
                    Rect::new(
                        inner.x + diameter + size * 1.2,
                        inner.middle() - sets.len() as f64 * size * 0.95,
                        legend_w,
                        inner.h,
                    ),
                    size * 0.95,
                );
            }
            made.content = inner.h;
        }
        "compare" => {
            let groups = groups_of(block);
            if groups.len() < 2 {
                faults.push(String::from(
                    "a compare needs two sides. Name them in \"groups\", each with a \"title\" and its \"items\".",
                ));
            }
            let gap = inner.w * 0.05;
            let each = (inner.w - gap) / 2.0;
            let mut tallest = 0.0f64;
            for (at, group) in groups.iter().take(2).enumerate() {
                let side = Rect::new(inner.x + (each + gap) * at as f64, inner.y, each, inner.h);
                let head_size = size * 1.1;
                let heads = wrap_runs(
                    fonts,
                    &body,
                    &runs_of_text(&group.0, &palette.accent, heading_weight, heading_weight),
                    head_size,
                    side.w,
                );
                let mut used = heads.len() as f64 * head_size * leading + size * 0.75;
                made.rows.push(Row {
                    role: String::from(R_HEAD),
                    bx: Rect::new(side.x, side.y, side.w, used),
                    title: heads,
                    colour: palette.accent.clone(),
                    value: at as f64,
                    ..Default::default()
                });
                for line in &group.1 {
                    let lines = wrap_runs(
                        fonts,
                        &body,
                        &runs_of_text(line, &palette.ink, body_weight, heading_weight),
                        size,
                        (side.w - size * 1.3).max(20.0),
                    );
                    let taken = lines.len() as f64 * size * leading;
                    made.rows.push(Row {
                        role: String::from(R_ITEM),
                        bx: Rect::new(side.x, side.y + used, side.w, taken),
                        lines,
                        colour: palette.ink.clone(),
                        mark: String::from(if at == 0 { "check" } else { "cross" }),
                        value: at as f64,
                        ..Default::default()
                    });
                    used += taken + size * 0.45;
                }
                tallest = tallest.max(used);
            }
            made.content = tallest;
        }
        "steps" => {
            let items = items_of(block);
            if items.is_empty() {
                faults.push(String::from(
                    "there are no steps in it. Name them in \"items\", each with a \"title\" and words.",
                ));
            }
            let row_wise = block.direction != "column";
            let count = items.len().max(1);
            let gap = if row_wise {
                (inner.w * 0.035).max(6.0)
            } else {
                (inner.h * 0.035).max(6.0)
            };
            let each = if row_wise {
                (inner.w - gap * (count - 1) as f64) / count as f64
            } else {
                (inner.h - gap * (count - 1) as f64) / count as f64
            };
            let mut deepest = 0.0f64;
            for (at, item) in items.iter().enumerate() {
                let box_of = if row_wise {
                    Rect::new(inner.x + (each + gap) * at as f64, inner.y, each, inner.h)
                } else {
                    Rect::new(inner.x, inner.y + (each + gap) * at as f64, inner.w, each)
                };
                let title_size = size;
                let title_words = if item.title.is_empty() {
                    format!("{}", at + 1)
                } else {
                    item.title.clone()
                };
                let title = wrap_runs(
                    fonts,
                    &body,
                    &runs_of_text(&title_words, &palette.accent, heading_weight, heading_weight),
                    title_size,
                    box_of.w,
                );
                let body_size = size * 0.88;
                let lines = wrap_runs(
                    fonts,
                    &body,
                    &runs_of_text(&item.text, &palette.ink, body_weight, heading_weight),
                    body_size,
                    box_of.w,
                );
                let taken = title.len() as f64 * title_size * leading
                    + lines.len() as f64 * body_size * leading
                    + size * 0.55;
                made.rows.push(Row {
                    role: String::from(R_STEP),
                    bx: box_of,
                    title,
                    lines,
                    colour: palette.accent.clone(),
                    value: (at + 1) as f64,
                    ..Default::default()
                });
                deepest = deepest.max(taken);
            }
            made.content = if row_wise { deepest } else { inner.h };
        }
        "timeline" => {
            let items = items_of(block);
            if items.is_empty() {
                faults.push(String::from(
                    "there is nothing on the line. Name the points in \"items\".",
                ));
            }
            let count = items.len().max(1);
            let line_y = inner.y + inner.h * 0.4;
            let each = inner.w / count as f64;
            let mut deepest = 0.0f64;
            for (at, item) in items.iter().enumerate() {
                let share = if item.at > 0.0 {
                    item.at.clamp(0.0, 1.0)
                } else {
                    (at as f64 + 0.5) / count as f64
                };
                let cx = inner.x + inner.w * share;
                let box_x = (cx - each * 0.48).max(inner.x);
                let room = (each * 0.96).max(20.0);
                let when_size = size * 0.82;
                let title_size = size;
                let body_size = size * 0.82;
                let when = wrap_runs(
                    fonts,
                    &body,
                    &runs_of_text(&item.when, &palette.accent, heading_weight, heading_weight),
                    when_size,
                    room,
                );
                let title = wrap_runs(
                    fonts,
                    &body,
                    &runs_of_text(&item.title, &palette.ink, heading_weight, heading_weight),
                    title_size,
                    room,
                );
                let lines = wrap_runs(
                    fonts,
                    &body,
                    &runs_of_text(&item.text, &palette.muted, body_weight, heading_weight),
                    body_size,
                    room,
                );
                let taken = when.len() as f64 * when_size * leading
                    + title.len() as f64 * title_size * leading
                    + lines.len() as f64 * body_size * leading
                    + size * 0.5;
                made.rows.push(Row {
                    role: String::from(R_POINT),
                    bx: Rect::new(box_x, line_y, room, taken),
                    when,
                    title,
                    lines,
                    colour: palette.accent.clone(),
                    at: share,
                    value: at as f64,
                    ..Default::default()
                });
                deepest = deepest.max(taken);
            }
            made.chart.plot = Rect::new(inner.x, line_y, inner.w, 1.0);
            made.content = inner.h * 0.4 + deepest;
        }
        "table" => {
            let rows = table_rows(block);
            if rows.is_empty() {
                faults.push(String::from(
                    "there are no rows in it. Name the head in \"labels\" and the rows in \"rows\".",
                ));
            }
            let columns = rows.iter().map(|one| one.len()).max().unwrap_or(1).max(1);
            let mut widths: Vec<f64> = vec![0.0; columns];
            for row in &rows {
                for (at, cell) in row.iter().enumerate() {
                    let wide = fonts.width(cell, &body, 400, false, size);
                    if wide > widths[at] {
                        widths[at] = wide;
                    }
                }
            }
            let pad = size * 0.55;
            let needed: f64 = widths.iter().sum::<f64>() + pad * columns as f64;
            if needed > inner.w && needed > 0.0 {
                let want = inner.w / needed;
                for one in &mut widths {
                    *one *= want;
                }
            }
            let step = size * 1.75;
            let mut used = 0.0;
            for (at, row) in rows.iter().enumerate() {
                let mut cells: Vec<String> = Vec::new();
                for (place, cell) in row.iter().enumerate() {
                    let room = widths.get(place).copied().unwrap_or(40.0);
                    let cut = fonts.wrap(
                        cell,
                        &body,
                        if at == 0 { heading_weight } else { body_weight },
                        false,
                        size,
                        room.max(10.0),
                    );
                    cells.push(cut.join("\n"));
                }
                made.rows.push(Row {
                    role: String::from(R_ROW),
                    bx: Rect::new(inner.x, inner.y + used, inner.w, step),
                    words: cells,
                    value: at as f64,
                    ..Default::default()
                });
                used += step;
            }
            made.content = used;
            made.step = step;
        }
        "code" => {
            let text = words_of(block);
            if text.is_empty() {
                faults.push(String::from("there is no code in it. Name the words in \"text\"."));
            }
            let mono = fonts.mono.clone();
            let mut lines: Vec<Line> = text
                .lines()
                .map(|one| Line {
                    runs: vec![Run {
                        text: String::from(one),
                        colour: made.colour.clone(),
                        weight: 400,
                        italic: false,
                    }],
                    width: fonts.width(one, &mono, 400, false, size),
                    size,
                })
                .collect();
            let widest = lines.iter().map(|one| one.width).fold(0.0_f64, f64::max);
            if widest > inner.w && widest > 0.0 {
                let want = inner.w / widest;
                if want >= SMALLEST {
                    for line in &mut lines {
                        line.size *= want;
                        line.width *= want;
                    }
                    made.size = size * want;
                    made.shrunk = true;
                } else {
                    faults.push(format!(
                        "the longest line stands {} wide and the box holds {}; shrinking to {} of the size is not enough. Break the line in the words of the block.",
                        whole(widest),
                        whole(inner.w),
                        percent(SMALLEST)
                    ));
                }
            }
            let step = made.size * 1.45;
            made.step = step;
            made.content = lines.len() as f64 * step;
            made.lines = lines;
        }
        "pill" => {
            let words = pill_words(block);
            if words.is_empty() {
                faults.push(String::from(
                    "there is no word in it. Name the words in \"text\" or in \"items\".",
                ));
            }
            let height = size * 1.9;
            let gap = size * 0.45;
            let mut x = inner.x;
            let mut y = inner.y;
            for one in &words {
                let wide = fonts.width(one, &body, heading_weight, false, size) + size * 1.5;
                if x > inner.x && x + wide > inner.right() {
                    x = inner.x;
                    y += height + gap;
                }
                made.rows.push(Row {
                    role: String::from(R_PILL),
                    bx: Rect::new(x, y, wide, height),
                    words: vec![one.clone()],
                    colour: made.colour.clone(),
                    ..Default::default()
                });
                x += wide + gap;
            }
            made.content = y + height - inner.y;
        }
        "progress" => {
            let value = block.value;
            made.number = number_words(value, &block.prefix, &block.suffix, block.decimals);
            made.label = words_of(block);
            let bar_h = (inner.h * 0.4).max(size * 0.75);
            let head_room = size * 1.5;
            made.chart.plot = Rect::new(inner.x, inner.y + head_room, inner.w, bar_h);
            made.content = head_room + bar_h + size * 0.4;
        }
        "image" => {
            let name = block.path.trim();
            if name.is_empty() {
                faults.push(String::from(
                    "no file is named. Name a picture of the room of the chat in \"path\".",
                ));
            } else {
                match read_picture(name, room) {
                    Ok(held) => made.image = Some(held),
                    Err(why) => faults.push(why),
                }
            }
            let caption = words_of(block);
            made.label = caption.clone();
            let caption_room = if caption.is_empty() {
                0.0
            } else {
                let lines = fonts.wrap(&caption, &body, 400, false, size, inner.w);
                lines.len() as f64 * size * leading + size * 0.4
            };
            made.content = bx.h;
            made.chart.plot = Rect::new(
                bx.x,
                bx.y,
                bx.w,
                (bx.h - caption_room).max(10.0),
            );
        }
        "icon" => {
            if marks::of(&block.mark).is_none() {
                faults.push(format!(
                    "\"{}\" is not a mark of this program. The marks are: {}.",
                    block.mark,
                    marks::names()
                ));
            }
            made.content = bx.h;
        }
        "shape" | "rule" => {
            made.content = bx.h;
        }
        "svg" => {
            if block.svg.trim().is_empty() {
                faults.push(String::from(
                    "there is no drawing in it. Name the words of a picture of your own in \"svg\".",
                ));
            }
            made.content = bx.h;
        }
        _ => {
            faults.push(String::from("this kind is not known to the layout."));
        }
    }
    (made, faults)
}

// ---------------------------------------------------------------- the parts

// The size of the words of a block: the size it named, or the size its kind
// brings with it. A size under 0.2 is a share of the height of the picture; a
// bigger number is pixels.
fn size_of(block: &Block, kind: &str, height: f64) -> f64 {
    if block.size > 0.0 {
        if block.size < 0.2 {
            return block.size * height;
        }
        return block.size;
    }
    kind_size(kind, height)
}

fn kind_size(kind: &str, height: f64) -> f64 {
    let share: f64 = match kind {
        "head" => 0.075,
        "stat" => 0.16,
        "quote" => 0.05,
        "text" => 0.032,
        "bullets" => 0.032,
        "compare" => 0.03,
        "steps" => 0.027,
        "timeline" => 0.024,
        "table" => 0.026,
        "code" => 0.024,
        "pill" => 0.022,
        "progress" => 0.025,
        "image" => 0.02,
        _ => 0.03,
    };
    share * height
}

// The words a block draws out of its text.
fn words_of(block: &Block) -> String {
    if !block.text.trim().is_empty() {
        return block.text.trim().to_string();
    }
    if block.kind == "quote" && !block.items.is_empty() {
        return block
            .items
            .iter()
            .map(|one| one.text.clone())
            .collect::<Vec<String>>()
            .join(" ");
    }
    String::new()
}

// The items of a block, or the lines of its text when the model wrote its list
// as lines.
fn items_of(block: &Block) -> Vec<Item> {
    if !block.items.is_empty() {
        return block.items.clone();
    }
    if !block.labels.is_empty() && matches!(block.kind.as_str(), "bullets" | "steps" | "timeline") {
        return block
            .labels
            .iter()
            .map(|one| Item {
                text: one.clone(),
                ..Default::default()
            })
            .collect();
    }
    if !block.text.is_empty() && matches!(block.kind.as_str(), "bullets" | "steps" | "pill" | "timeline")
    {
        return block
            .text
            .lines()
            .filter(|one| !one.trim().is_empty())
            .map(|one| Item {
                text: String::from(one.trim()),
                ..Default::default()
            })
            .collect();
    }
    Vec::new()
}

// The words of the pills of a block.
fn pill_words(block: &Block) -> Vec<String> {
    if !block.items.is_empty() {
        return block
            .items
            .iter()
            .map(|one| String::from(one.text.trim()))
            .filter(|one| !one.is_empty())
            .collect();
    }
    if block.text.contains('\n') {
        return block
            .text
            .lines()
            .map(|one| String::from(one.trim()))
            .filter(|one| !one.is_empty())
            .collect();
    }
    block
        .text
        .split(',')
        .map(|one| String::from(one.trim()))
        .filter(|one| !one.is_empty())
        .collect()
}

// The two sides of a compare.
fn groups_of(block: &Block) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for group in &block.groups {
        let mut lines: Vec<String> = group
            .items
            .iter()
            .map(|one| one.text.clone())
            .filter(|one| !one.trim().is_empty())
            .collect();
        if lines.is_empty() && !group.text.is_empty() {
            lines = group
                .text
                .lines()
                .filter(|one| !one.trim().is_empty())
                .map(String::from)
                .collect();
        }
        out.push((group.title.clone(), lines));
    }
    if out.is_empty() {
        let items = items_of(block);
        if !items.is_empty() {
            let half = (items.len() + 1) / 2;
            let left: Vec<String> = items.iter().take(half).map(|one| one.text.clone()).collect();
            let right: Vec<String> = items.iter().skip(half).map(|one| one.text.clone()).collect();
            out.push((
                block.labels.first().cloned().unwrap_or_else(|| String::from("Before")),
                left,
            ));
            out.push((
                block
                    .labels
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| String::from("After")),
                right,
            ));
        }
    }
    out
}

// The rows of a table.
fn table_rows(block: &Block) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    if !block.labels.is_empty() {
        out.push(block.labels.clone());
    }
    for row in &block.rows {
        if !row.is_empty() {
            out.push(row.clone());
        }
    }
    if out.is_empty() {
        for item in &block.items {
            let mut row: Vec<String> = Vec::new();
            if !item.when.is_empty() {
                row.push(item.when.clone());
            }
            if !item.title.is_empty() {
                row.push(item.title.clone());
            }
            if !item.text.is_empty() {
                row.push(item.text.clone());
            }
            if !row.is_empty() {
                out.push(row);
            }
        }
    }
    out
}

// One set of numbers of a chart, with the colour it is drawn in.
#[derive(Clone, Debug)]
pub struct Set {
    pub name: String,
    pub values: Vec<f64>,
    pub colour: String,
    pub fill: Option<bool>,
    pub dashed: Option<bool>,
}

fn sets_of(block: &Block, palette: &Palette) -> (Vec<Set>, Vec<String>) {
    let labels = if !block.labels.is_empty() {
        block.labels.clone()
    } else if !block.items.is_empty() && matches!(block.kind.as_str(), "bars" | "line" | "pie") {
        block
            .items
            .iter()
            .map(|one| if one.title.is_empty() { one.text.clone() } else { one.title.clone() })
            .collect()
    } else {
        Vec::new()
    };
    let mut out: Vec<Set> = Vec::new();
    if !block.series.is_empty() {
        for (at, one) in block.series.iter().enumerate() {
            out.push(Set {
                name: if one.name.is_empty() {
                    format!("Set {}", at + 1)
                } else {
                    one.name.clone()
                },
                values: one.values.clone(),
                colour: if !one.colour.is_empty() {
                    one.colour.clone()
                } else {
                    script::series_colour(palette, at)
                },
                fill: one.fill,
                dashed: one.dashed,
            });
        }
    } else if !block.values.is_empty() {
        out.push(Set {
            name: if block.kind == "pie" {
                String::from("Total")
            } else {
                String::new()
            },
            values: block.values.clone(),
            colour: if !block.colour.is_empty() {
                block.colour.clone()
            } else {
                script::series_colour(palette, 0)
            },
            fill: None,
            dashed: None,
        });
    } else if !block.items.is_empty() {
        let values: Vec<f64> = block.items.iter().map(|one| one.value).collect();
        if values.iter().any(|one| *one != 0.0) {
            out.push(Set {
                name: String::new(),
                values,
                colour: if !block.colour.is_empty() {
                    block.colour.clone()
                } else {
                    script::series_colour(palette, 0)
                },
                fill: None,
                dashed: None,
            });
        }
    }
    if block.kind == "pie" && out.len() == 1 && out[0].values.len() > 1 {
        // A pie drawn from one set of numbers takes one wedge per number, and
        // the labels are the names of the wedges.
        let held = out.remove(0);
        for (at, value) in held.values.iter().enumerate() {
            out.push(Set {
                name: labels
                    .get(at)
                    .cloned()
                    .unwrap_or_else(|| format!("Part {}", at + 1)),
                values: vec![*value],
                colour: script::series_colour(palette, at),
                fill: None,
                dashed: None,
            });
        }
        return (out, Vec::new());
    }
    (out, labels)
}

fn numbers_of(sets: &[Set]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for one in sets {
        out.extend(one.values.iter().copied());
    }
    out
}

// The two ends of the value axis: the greatest number rounded up, and the least
// rounded down, with zero always between them.
fn axis_ends(values: &[f64]) -> (f64, f64) {
    let greatest = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let least = values.iter().cloned().fold(f64::INFINITY, f64::min);
    if !greatest.is_finite() || !least.is_finite() {
        return (1.0, 0.0);
    }
    let top = if greatest <= 0.0 { 0.0 } else { nice_up(greatest) };
    let floor = if least >= 0.0 { 0.0 } else { -nice_up(least.abs()) };
    if top - floor <= 0.0 {
        return (1.0, 0.0);
    }
    (top, floor)
}

// A number rounded up to a step that reads well.
fn nice_up(value: f64) -> f64 {
    if value <= 0.0 {
        return 0.0;
    }
    let power = 10f64.powf(value.log10().floor());
    let share = value / power;
    let held = if share <= 1.0 {
        1.0
    } else if share <= 1.2 {
        1.2
    } else if share <= 1.5 {
        1.5
    } else if share <= 2.0 {
        2.0
    } else if share <= 2.5 {
        2.5
    } else if share <= 3.0 {
        3.0
    } else if share <= 4.0 {
        4.0
    } else if share <= 5.0 {
        5.0
    } else if share <= 6.0 {
        6.0
    } else if share <= 8.0 {
        8.0
    } else {
        10.0
    };
    held * power
}

// How many places after the point the axis numbers need.
fn decimals_for(top: f64, floor: f64, named: i32) -> i32 {
    if named >= 0 {
        return named;
    }
    let span = top - floor;
    if span >= 10.0 {
        0
    } else if span >= 1.0 {
        1
    } else {
        2
    }
}

// The height of a value on the plot.
fn value_y(top: f64, floor: f64, value: f64, plot: Rect) -> f64 {
    let span = (top - floor).max(0.0001);
    plot.bottom() - plot.h * ((value - floor) / span)
}

// The width of a value on the plot, for a chart that runs the other way.
fn value_x(top: f64, floor: f64, value: f64, plot: Rect) -> f64 {
    let span = (top - floor).max(0.0001);
    plot.x + plot.w * ((value - floor) / span)
}

// The words under the bars or under the points of a line.
fn axis_rows(
    fonts: &Fonts,
    family: &str,
    labels: &[String],
    plot: Rect,
    horizontal: bool,
    size: f64,
    on: bool,
) -> Vec<Row> {
    if !on || labels.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Row> = Vec::new();
    let last = (labels.len().max(2) - 1) as f64;
    for (at, one) in labels.iter().enumerate() {
        // A word that will not stand beside the one before it is left out, so a
        // chart of forty bars still reads.
        let every = ((one.chars().count() as f64 * size * 1.15)
            / ((plot.w / last).max(1.0)))
        .ceil()
        .max(1.0) as usize;
        let show = horizontal || at % every == 0 || at + 1 == labels.len();
        let cut = if horizontal {
            vec![one.clone()]
        } else {
            fonts.wrap(one, family, 400, false, size, (plot.w / last * 1.1).max(30.0))
        };
        let x = if horizontal {
            plot.x - size
        } else if last <= 0.0 {
            plot.centre()
        } else {
            plot.x + plot.w * (at as f64) / last
        };
        let y = if horizontal {
            plot.y + (plot.h / labels.len() as f64) * (at as f64 + 0.5)
        } else {
            plot.bottom() + size * 0.5
        };
        out.push(Row {
            role: String::from(R_AXIS),
            bx: Rect::new(x, y, (plot.w / last).max(30.0), cut.len() as f64 * size * 1.2),
            lines: cut
                .iter()
                .map(|one| Line {
                    runs: vec![Run {
                        text: one.clone(),
                        colour: String::new(),
                        weight: 400,
                        italic: false,
                    }],
                    width: fonts.width(one, family, 400, false, size),
                    size,
                })
                .collect(),
            words: vec![if show { one.clone() } else { String::new() }],
            value: at as f64,
            ..Default::default()
        });
    }
    out
}

// The key of a chart: one line per set, with the square of its colour.
fn legend_rows(sets: &[Set], place: Rect, size: f64) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    let mut y = place.y;
    for (at, one) in sets.iter().enumerate() {
        out.push(Row {
            role: String::from(R_LEGEND),
            bx: Rect::new(place.x, y, place.w.max(10.0), size * 1.8),
            words: vec![one.name.clone()],
            colour: one.colour.clone(),
            value: at as f64,
            ..Default::default()
        });
        y += size * 1.9;
    }
    out
}

// ---------------------------------------------------------------- the words

// The runs of words of a block: the spans it named, or its text read for the
// marks of `**bold**` and `*italic*`.
pub fn runs_of(block: &Block, colour: &str, weight: u16, bold: u16) -> Vec<Run> {
    if !block.spans.is_empty() {
        let mut out: Vec<Run> = Vec::new();
        for one in &block.spans {
            out.push(Run {
                text: one.text.clone(),
                colour: if one.colour.is_empty() {
                    String::from(colour)
                } else {
                    one.colour.clone()
                },
                weight: if one.weight.is_empty() {
                    weight
                } else {
                    fonts::weight_of(&one.weight)
                },
                italic: false,
            });
        }
        return out;
    }
    runs_of_text(&words_of(block), colour, weight, bold)
}

fn runs_of_item(
    item: &Item,
    colour: &str,
    weight: u16,
    bold: u16,
) -> Vec<Run> {
    runs_of_text(&item.text, if item.colour.is_empty() { colour } else { &item.colour }, weight, bold)
}

// Words read for the marks: `**bold**` takes the bold weight, `*italic*` takes
// the sloped shape.
pub fn runs_of_text(text: &str, colour: &str, weight: u16, bold: u16) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut current = String::new();
    let mut weight_now = weight;
    let mut italic = false;
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        if chars[at] == '*' {
            let stars = chars[at..].iter().take_while(|c| **c == '*').count();
            let held = if stars >= 2 { 2 } else { 1 };
            // The mark ends the run before it and starts the run of its own.
            if !current.is_empty() {
                out.push(Run {
                    text: std::mem::take(&mut current),
                    colour: String::from(colour),
                    weight: weight_now,
                    italic,
                });
            }
            if held == 2 {
                let next = weight_now != bold;
                weight_now = if next { bold } else { weight };
            } else {
                italic = !italic;
            }
            at += held;
            continue;
        }
        current.push(chars[at]);
        at += 1;
    }
    if !current.is_empty() {
        out.push(Run {
            text: current,
            colour: String::from(colour),
            weight: weight_now,
            italic,
        });
    }
    if out.is_empty() {
        out.push(Run {
            text: String::new(),
            colour: String::from(colour),
            weight,
            italic: false,
        });
    }
    out
}

// Runs of words cut into lines that fit a width. A word keeps the run it came
// from, so a colour does not stop at the end of a line.
// One word with the space that may stand before it, and the run it belongs to.
struct Token {
    word: String,
    run: usize,
    space: bool,
}

// The words of several runs, cut into lines that fit. The runs are kept apart
// so a line may change colour in the middle of a sentence.
pub fn wrap_runs(fonts: &Fonts, family: &str, runs: &[Run], size: f64, width: f64) -> Vec<Line> {
    let width = width.max(10.0);
    let mut tokens: Vec<Token> = Vec::new();
    for (at, run) in runs.iter().enumerate() {
        let words = run.text.split_whitespace();
        for word in words {
            tokens.push(Token {
                word: String::from(word),
                run: at,
                space: !tokens.is_empty(),
            });
        }
    }
    let mut out: Vec<Line> = Vec::new();
    let mut line: Vec<(Token, f64)> = Vec::new();
    let mut used = 0.0;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let space_w = if token.space {
            fonts.width(" ", family, runs[token.run].weight, false, size)
        } else {
            0.0
        };
        let word_w = fonts.width(&token.word, family, runs[token.run].weight, false, size);
        if !line.is_empty() && used + space_w + word_w > width {
            out.push(make_line(fonts, &line, runs, family, size));
            line.clear();
            used = 0.0;
            // The first word of a line carries no space with it.
            tokens[index].space = false;
            continue;
        }
        let moved = Token {
            word: token.word.clone(),
            run: token.run,
            space: token.space,
        };
        used += space_w + word_w;
        line.push((moved, used));
        index += 1;
    }
    if !line.is_empty() {
        out.push(make_line(fonts, &line, runs, family, size));
    }
    if out.is_empty() {
        out.push(Line {
            runs: Vec::new(),
            width: 0.0,
            size,
        });
    }
    out
}

// The runs of one line, glued back together word by word.
fn make_line(fonts: &Fonts, tokens: &[(Token, f64)], runs: &[Run], family: &str, size: f64) -> Line {
    let mut out: Vec<Run> = Vec::new();
    let mut width = 0.0;
    for (token, _) in tokens {
        let run = &runs[token.run];
        let word = if token.space {
            format!(" {}", token.word)
        } else {
            token.word.clone()
        };
        width += fonts.width(&word, family, run.weight, false, size);
        match out.last_mut() {
            Some(last)
                if last.colour == run.colour
                    && last.weight == run.weight
                    && last.italic == run.italic =>
            {
                last.text.push_str(&word);
            }
            _ => out.push(Run {
                text: word,
                colour: run.colour.clone(),
                weight: run.weight,
                italic: run.italic,
            }),
        }
    }
    Line {
        runs: out,
        width,
        size,
    }
}

// A number written as the block asked: with the prefix, the suffix and the
// places after the point it named.
pub fn number_words(value: f64, prefix: &str, suffix: &str, decimals: i32) -> String {
    let held = if decimals < 0 {
        plain(value)
    } else {
        format!("{:.*}", decimals as usize, value)
    };
    format!("{prefix}{held}{suffix}")
}

// A number with nothing after the point taken off, and with the thousands told
// with a comma: 12345.6 reads "12,345.6".
fn plain(value: f64) -> String {
    let whole = if value == value.trunc() && value.abs() < 1.0e15 {
        format!("{}", value as i64)
    } else {
        let mut made = format!("{value:.2}");
        while made.ends_with('0') {
            made.pop();
        }
        if made.ends_with('.') {
            made.pop();
        }
        made
    };
    let (sign, rest) = match whole.strip_prefix('-') {
        Some(rest) => (String::from("-"), String::from(rest)),
        None => (String::new(), whole),
    };
    let (head, tail) = match rest.split_once('.') {
        Some((head, tail)) => (String::from(head), format!(".{tail}")),
        None => (rest, String::new()),
    };
    let mut grouped = String::new();
    for (at, c) in head.chars().rev().enumerate() {
        if at > 0 && at % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let grouped: String = grouped.chars().rev().collect();
    format!("{sign}{grouped}{tail}")
}

// ---------------------------------------------------------------- the small

fn named_or(named: &str, fell: String) -> String {
    if named.is_empty() {
        fell
    } else {
        String::from(named)
    }
}

fn default_colour(kind: &str, palette: &Palette) -> String {
    match kind {
        "stat" | "pill" | "progress" => palette.accent.clone(),
        "rule" | "shape" => palette.line.clone(),
        _ => palette.ink.clone(),
    }
}

fn default_align(kind: &str) -> &'static str {
    match kind {
        "head" | "stat" | "quote" | "pill" | "icon" => "center",
        _ => "left",
    }
}

fn default_valign(kind: &str) -> &'static str {
    match kind {
        "head" | "stat" | "quote" | "pill" | "icon" | "rule" => "middle",
        _ => "top",
    }
}

fn default_reveal(kind: &str) -> &'static str {
    match kind {
        "shape" | "rule" | "image" | "svg" => "fade",
        _ => "rise",
    }
}

fn default_pad(kind: &str) -> f64 {
    match kind {
        "shape" | "rule" | "image" | "svg" | "icon" => 0.0,
        _ => 0.012,
    }
}

fn default_stroke(kind: &str) -> f64 {
    match kind {
        "rule" => 0.0022,
        "shape" => 0.0022,
        _ => 0.0018,
    }
}

// A share of the shorter side of the picture, or the pixels the block named.
fn share(named: f64, fell: f64, board: Rect) -> f64 {
    let side = board.w.min(board.h);
    let part = if named > 0.0 { named } else { fell };
    if part >= 1.0 {
        return part;
    }
    part * side
}

// A word of a fixed list: the one it named, the one it falls back to, or nothing
// at all when the word names nothing of the list.
fn word(named: &str, fell: &str, known: &[&str]) -> String {
    let said = named.trim().to_ascii_lowercase();
    if said.is_empty() {
        return String::from(fell);
    }
    if known.contains(&said.as_str()) {
        return said;
    }
    String::from(fell)
}

fn whole(value: f64) -> String {
    format!("{}", value.round() as i64)
}

// A picture of the room of the chat, read for the drawing.
fn read_picture(name: &str, room: Option<&std::path::Path>) -> Result<Image, String> {
    let place = if std::path::Path::new(name).is_absolute() {
        std::path::PathBuf::from(name)
    } else {
        match room {
            Some(root) => root.join(name),
            None => std::path::PathBuf::from(name),
        }
    };
    if !place.is_file() {
        return Err(format!(
            "the picture \"{name}\" is not a file of the room of the chat. Write it there first, or name the place it stands at."
        ));
    }
    let bytes =
        std::fs::read(&place).map_err(|why| format!("the picture \"{name}\" could not be read: {why}"))?;
    if bytes.len() as u64 > 32 * 1024 * 1024 {
        return Err(format!(
            "the picture \"{name}\" is over 32 Mbyte, which is more than a frame will carry"
        ));
    }
    let tail = place
        .extension()
        .map(|one| one.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let mime = match tail.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        other => {
            return Err(format!(
                "the picture \"{name}\" is of a kind no frame can draw (.{other}). The kinds are: png, jpg, gif, webp, svg, bmp."
            ))
        }
    };
    Ok(Image {
        data: bytes,
        mime: String::from(mime),
        name: String::from(name),
    })
}

// The ground of a scene: the scene's own when it names one, the video's when it
// does not.
pub fn ground(named: &Option<Background>, of_project: &Background) -> Background {
    match named {
        Some(one) if !one.kind.is_empty() || !one.from.is_empty() => one.clone(),
        _ => of_project.clone(),
    }
}
