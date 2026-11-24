// The words of a frame: read for their marks, cut into lines, measured, and counted.
//
// A frame is drawn from the same reading. The layout asks how wide a word is before it puts the
// word in a box, and the renderer answers with the same number from the same font, so a caption
// that the check said fitted does not run over the edge of the picture when it is drawn.
//
// Three marks are read in the text of a layer:
//
// * `**bold**` - the heavy weight of the family.
// * `*italic*` - the sloped form.
// * `{#c2410c}words{}` - a colour, as a hex of three or six digits or a name. The colour ends at
//   `{}` or at the end of the text.
//
// A line is kept as the words that make it, and each word knows the run it came from. That is
// what lets a caption be typed a word at a time, and lets one line change colour in the middle of
// a sentence without the cut being wrong.

use crate::infographic::anim;
use crate::infographic::fonts::{self, Fonts};

// A length of text that is drawn the same way throughout.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    // Nothing means the colour of the layer.
    pub colour: Option<String>,
    pub weight: u16,
    pub italic: bool,
}

// One word of a line, and the run it came from.
#[derive(Clone, Debug)]
pub struct Word {
    pub run: usize,
    pub text: String,
    // Where the word begins and ends within the line, and how wide it is.
    pub x: f64,
    pub width: f64,
}

// One line of a paragraph.
#[derive(Clone, Debug)]
pub struct Line {
    pub words: Vec<Word>,
    pub width: f64,
    // The height one line takes, which is the same for all the lines of one paragraph.
    pub height: f64,
}

impl Line {
    // The text of the line, as it is read out.
    pub fn text(&self) -> String {
        self.words.iter().map(|one| one.text.as_str()).collect::<Vec<&str>>().join(" ")
    }

    // The first `count` words, in the order they stand. A count past the end is the whole line.
    pub fn shown(&self, count: usize) -> String {
        self.words
            .iter()
            .take(count)
            .map(|one| one.text.as_str())
            .collect::<Vec<&str>>()
            .join(" ")
    }
}

// Text read for its marks. `bold` is the weight a bold word takes, which a heading of the frame
// may raise.
pub fn runs_of(text: &str, colour: Option<&str>, weight: u16, bold: u16) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut held = String::new();
    let mut weight_now = weight;
    let mut italic = false;
    let mut colour_now: Option<String> = colour.map(String::from);
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    while at < chars.len() {
        let c = chars[at];
        // `**` and `*` turn the weight and the slant on and off.
        if c == '*' {
            let stars = chars[at..].iter().take_while(|one| **one == '*').count().min(2);
            push(&mut out, &mut held, colour_now.clone(), weight_now, italic);
            if stars == 2 {
                weight_now = if weight_now == bold { weight } else { bold };
            } else {
                italic = !italic;
            }
            at += stars;
            continue;
        }
        // `{#c2410c}` opens a colour, `{}` closes it.
        if c == '{' {
            if let Some(end) = closing(&chars, at) {
                let inner: String = chars[at + 1..end].iter().collect();
                let said = inner.trim();
                if said.is_empty() {
                    push(&mut out, &mut held, colour_now.clone(), weight_now, italic);
                    colour_now = colour.map(String::from);
                } else if anim::colour_of(said).is_some() {
                    push(&mut out, &mut held, colour_now.clone(), weight_now, italic);
                    colour_now = Some(String::from(said));
                } else {
                    held.push(c);
                    at += 1;
                    continue;
                }
                at = end + 1;
                continue;
            }
        }
        held.push(c);
        at += 1;
    }
    push(&mut out, &mut held, colour_now, weight_now, italic);
    if out.is_empty() {
        out.push(Run {
            text: String::new(),
            colour: colour.map(String::from),
            weight,
            italic: false,
        });
    }
    out
}

// Where the `}` of a colour mark is, when the text between the marks is a colour or nothing.
fn closing(chars: &[char], from: usize) -> Option<usize> {
    for at in from + 1..chars.len().min(from + 12) {
        if chars[at] == '}' {
            let inner: String = chars[from + 1..at].iter().collect();
            let said = inner.trim();
            if said.is_empty() || anim::colour_of(said).is_some() {
                return Some(at);
            }
            return None;
        }
        if chars[at] == '{' || chars[at] == '\n' {
            return None;
        }
    }
    None
}

fn push(out: &mut Vec<Run>, held: &mut String, colour: Option<String>, weight: u16, italic: bool) {
    if held.is_empty() {
        return;
    }
    out.push(Run { text: std::mem::take(held), colour, weight, italic });
}

// The runs of several lines of text, with the break kept as a break.
#[derive(Clone, Debug)]
pub struct Paragraph {
    pub runs: Vec<Run>,
    // A new line the writer put in, as the number of the run it belongs to.
    pub breaks: Vec<usize>,
}

// Runs, keeping the new lines of the text.
pub fn paragraph_of(text: &str, colour: Option<&str>, weight: u16, bold: u16) -> Paragraph {
    let mut runs: Vec<Run> = Vec::new();
    let mut breaks: Vec<usize> = Vec::new();
    for (place, one) in text.split('\n').enumerate() {
        if place > 0 {
            breaks.push(runs.len());
        }
        let mut part = runs_of(one, colour, weight, bold);
        if part.is_empty() {
            part.push(Run {
                text: String::new(),
                colour: colour.map(String::from),
                weight,
                italic: false,
            });
        }
        runs.extend(part);
    }
    Paragraph { runs, breaks }
}

// The lines of a paragraph that fit a width. The width of a space is taken from the run it falls
// in, so a bold word does not change the width of the space before it. `max_lines` cuts the
// paragraph at a line, and the answer says whether it was cut and how much height the whole of
// the paragraph would have needed.
pub struct Cut {
    pub lines: Vec<Line>,
    // The height of all of the lines the text needed, whether or not they were kept.
    pub needed: f64,
    pub cut: bool,
    // The words that did not fit, which a check reports.
    pub lost: usize,
}

pub fn layout(
    fonts: &Fonts,
    family: &str,
    para: &Paragraph,
    size: f64,
    widest: f64,
    line_height: f64,
    letter_space: f64,
    max_lines: usize,
) -> Cut {
    let height = fonts.line_em(family, 400) * size * line_height.max(0.5);
    let space = fonts.width(" ", family, 400, false, size);
    // The runs are cut at the new lines the writer put in, and every part is laid out on its own.
    // A part always takes a line, even when it holds no word: that is what a written break means.
    let mut parts: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for one in para.breaks.iter() {
        let at = (*one).clamp(start, para.runs.len());
        parts.push((start, at));
        start = at;
    }
    parts.push((start, para.runs.len()));
    let mut lines: Vec<Line> = Vec::new();
    for (from, to) in parts {
        let mut words: Vec<Word> = Vec::new();
        let mut used = 0.0;
        for (at, run) in para.runs.iter().enumerate().take(to).skip(from) {
            for word in run.text.split_whitespace() {
                let wide = fonts.width(word, family, run.weight, run.italic, size)
                    + letter_space * word.chars().count() as f64;
                if !words.is_empty() && used + space + wide > widest {
                    let made = make(fonts, family, std::mem::take(&mut words), size, height);
                    used = 0.0;
                    lines.push(made);
                }
                if !words.is_empty() {
                    used += space;
                }
                words.push(Word {
                    run: at,
                    text: String::from(word),
                    x: used,
                    width: wide,
                });
                used += wide;
            }
        }
        let made = make(fonts, family, std::mem::take(&mut words), size, height);
        lines.push(made);
    }

    let all = lines.len();
    let needed = all as f64 * height;
    let (kept, cut, lost) = if max_lines > 0 && all > max_lines {
        let gone: usize = lines.iter().skip(max_lines).map(|one| one.words.len()).sum();
        lines.truncate(max_lines);
        (lines, true, gone)
    } else {
        (lines, false, 0usize)
    };
    Cut { lines: kept, needed, cut, lost }
}

// The width of a line, and the height of it.
fn make(fonts: &Fonts, family: &str, words: Vec<Word>, size: f64, height: f64) -> Line {
    let width = words
        .last()
        .map(|one| one.x + one.width)
        .unwrap_or(0.0);
    let _ = (fonts, family, size);
    Line { words, width, height }
}

// How wide a line of text is, in the image.
pub fn width_of(fonts: &Fonts, family: &str, text: &str, weight: u16, size: f64) -> f64 {
    let runs = runs_of(text, None, weight, weight);
    runs.iter()
        .map(|one| fonts.width(&one.text, family, one.weight, one.italic, size))
        .sum()
}

// A number as it is read on a chart: the thousands markers, the decimals, the unit.
pub fn number(value: f64, decimals: i32, before: &str, after: &str) -> String {
    let rounded = round(value, decimals);
    let whole = rounded.trunc();
    let mut text = group(whole);
    if decimals > 0 {
        let part = rounded - whole;
        let digits = format!("{:.1$}", part.abs().min(0.999_999), decimals as usize);
        let tail = digits.split('.').nth(1).unwrap_or("");
        text.push('.');
        text.push_str(tail);
    }
    if rounded < 0.0 {
        text = format!("-{}", text.trim_start_matches('-'));
    }
    let mut out = String::new();
    out.push_str(before);
    out.push_str(&text);
    out.push_str(after);
    out
}

fn round(value: f64, decimals: i32) -> f64 {
    let power = 10f64.powi(decimals.max(0));
    (value * power).round() / power
}

// The whole part of a number with a marker every three digits.
fn group(whole: f64) -> String {
    let digits = format!("{}", whole.abs().trunc());
    let mut out = String::new();
    for (place, c) in digits.chars().rev().enumerate() {
        if place > 0 && place % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    let mut text: String = out.chars().rev().collect();
    if text.starts_with('-') {
        text.remove(0);
    }
    text
}

// The weight of a heading. A heading that named no weight is heavy; a weight the script named is
// taken as it was written.
pub fn heading_weight(word: &str) -> u16 {
    if word.trim().is_empty() {
        return 700;
    }
    fonts::weight_of(word)
}

#[cfg(test)]
mod checks {
    use super::*;

    // The marks of a text are read, and the words between them keep their own weight and colour.
    #[test]
    fn the_marks_of_a_text_are_read() {
        let runs = runs_of("**Bold**, *sloped*, {#c2410c}rust{} and plain", None, 400, 700);
        let said: Vec<(String, u16, bool, Option<String>)> = runs
            .iter()
            .map(|one| (one.text.clone(), one.weight, one.italic, one.colour.clone()))
            .collect();
        assert_eq!(said[0], (String::from("Bold"), 700, false, None), "{said:?}");
        assert_eq!(said[2], (String::from("sloped"), 400, true, None), "{said:?}");
        let rust = said
            .iter()
            .find(|one| one.0 == "rust")
            .expect("the coloured words are a run of their own");
        assert_eq!(rust.3.as_deref(), Some("#c2410c"));
        assert!(said
            .iter()
            .any(|one| one.0.contains("and plain") && one.1 == 400 && one.3.is_none()));
    }

    // A text is cut into lines that fit, and each word knows where it stands.
    #[test]
    fn a_text_is_cut_into_lines_that_fit() {
        let fonts = Fonts::new();
        let para = paragraph_of("one two three four five six", None, 400, 700);
        let made = layout(&fonts, "sans-serif", &para, 40.0, 160.0, 1.2, 0.0, 0);
        assert!(made.lines.len() >= 2, "the lines are cut: {:?}", made.lines.len());
        for one in &made.lines {
            assert!(one.width <= 160.0 + 1.0, "a line of {} wider than the box", one.width);
            assert_eq!(one.words.first().map(|one| one.x), Some(0.0));
        }
        assert!(!made.cut);
    }

    // A paragraph that is cut says how many words went away.
    #[test]
    fn a_cut_paragraph_tells_what_was_lost() {
        let fonts = Fonts::new();
        let para = paragraph_of("one two three four five six seven eight nine ten", None, 400, 700);
        let made = layout(&fonts, "sans-serif", &para, 40.0, 120.0, 1.2, 0.0, 1);
        assert!(made.cut, "a line of a paragraph of ten words");
        assert_eq!(made.lines.len(), 1);
        assert!(made.lost > 0, "words went away");
        assert!(made.needed > made.lines[0].height, "the height needed is more than one line");
    }

    // A new line the writer put in is kept.
    #[test]
    fn a_written_new_line_is_kept() {
        let fonts = Fonts::new();
        let para = paragraph_of("one\n", None, 400, 700);
        let made = layout(&fonts, "sans-serif", &para, 40.0, 400.0, 1.2, 0.0, 0);
        assert_eq!(made.lines.len(), 2, "the break stands");
        assert!(made.lines[1].words.is_empty());
    }

    // Numbers are grouped, and the unit stands at either end.
    #[test]
    fn numbers_are_grouped_and_labeled() {
        assert_eq!(number(1234567.0, 0, "", ""), "1,234,567");
        assert_eq!(number(1234.5678, 2, "$", ""), "$1,234.57");
        assert_eq!(number(-42.0, 0, "", "%"), "-42%");
        assert_eq!(number(0.5, 1, "", ""), "0.5");
    }

    // The weight of a heading is heavy even when the script said nothing.
    #[test]
    fn a_heading_is_heavy() {
        assert_eq!(heading_weight(""), 700);
        assert_eq!(heading_weight("black"), 900);
        assert_eq!(heading_weight("normal"), 400);
    }
}
