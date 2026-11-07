// The letters of an infographic, and what they measure.
//
// The layout of a frame must know how wide a word is before the word is drawn.
// If the guess and the picture disagree, the words run out of their box, and a
// video of broken boxes is worse than no video. So the measure here is the same
// reading the renderer takes: the fonts of the machine are found once with
// `fontdb`, and each letter is measured with `ttf-parser` from the same face the
// renderer will use.
//
// A machine with no fonts at all is still answered with a number, not with a
// fault. The estimate is the width of a plain sans letter, and a frame built on
// it is close enough to look at.
//
// The database is kept behind an `Arc`, because the renderer takes it too. It is
// read only, never changed after the fonts are made.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use fontdb::{Database, Family, ID, Query, Style, Weight};
use ttf_parser::Face;

// The families the program asks for by name. The first one the machine has wins.
// A name nothing has is answered by the generic family at the end of the list.
const BODIES: &[&str] = &[
    "Inter",
    "DejaVu Sans",
    "Noto Sans",
    "Liberation Sans",
    "Arimo",
    "Ubuntu",
    "Cantarell",
    "Segoe UI",
    "Arial",
    "Helvetica",
];
const MONOS: &[&str] = &[
    "JetBrains Mono",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "Ubuntu Mono",
    "Menlo",
    "Consolas",
    "Courier New",
];

// The width of a letter with no face behind it, as a share of the size.
const GUESSED: f64 = 0.54;
// The height of one line with no face behind it, as a share of the size.
const GUESSED_LINE: f64 = 1.22;

// The fonts of one run of the renderer.
pub struct Fonts {
    // The fonts of the machine, read only.
    db: Arc<Database>,
    // The family of the body of the frames, as the machine named it.
    pub body: String,
    // The family of the heads. Most machines have no separate head family, and
    // then the head is the body at a heavier weight.
    pub heading: String,
    // The family of the numbers and of the code.
    pub mono: String,
    // Which face was chosen for one family, weight and slant. Nothing is
    // searched twice.
    chosen: RefCell<HashMap<(String, u16, bool), Option<ID>>>,
    // The advance of one letter of one face, as a share of the size.
    advances: RefCell<HashMap<(ID, char), f64>>,
    // The height of one face: the rise, the fall below the line, and the gap.
    heights: RefCell<HashMap<ID, (f64, f64, f64)>>,
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    // The fonts of this machine. Nothing is asked of the network and nothing is
    // written.
    pub fn new() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();

        let body = first_family(&db, BODIES).unwrap_or_else(|| "sans-serif".to_string());
        let mono = first_family(&db, MONOS).unwrap_or_else(|| "monospace".to_string());
        // A head family is not looked for: a head is the body of the frame made
        // heavier, which is what every skin of the program does too.
        let heading = body.clone();

        // The generic names are pointed at the families that were found, so a
        // frame that asks for `sans-serif` gets the same letters as one that
        // asks for the body by name.
        if first_family(&db, BODIES).is_some() {
            db.set_sans_serif_family(&body);
            db.set_serif_family(&body);
        }
        if first_family(&db, MONOS).is_some() {
            db.set_monospace_family(&mono);
        }

        Fonts {
            db: Arc::new(db),
            body,
            heading,
            mono,
            chosen: RefCell::new(HashMap::new()),
            advances: RefCell::new(HashMap::new()),
            heights: RefCell::new(HashMap::new()),
        }
    }

    // The fonts, for the renderer. The frames ask for a family by name, and the
    // renderer finds it here.
    pub fn database(&self) -> Arc<Database> {
        self.db.clone()
    }

    // The face for one family, weight and slant. The nearest weight of the
    // family is taken when the asked weight is not on the machine.
    fn face(&self, family: &str, weight: u16, italic: bool) -> Option<ID> {
        let key = (family.to_ascii_lowercase(), weight, italic);
        if let Some(found) = self.chosen.borrow().get(&key) {
            return *found;
        }
        let wanted = [
            Family::Name(family),
            Family::Name(self.body.as_str()),
            Family::SansSerif,
        ];
        let asked = Query {
            families: &wanted,
            weight: Weight(weight),
            style: if italic { Style::Italic } else { Style::Normal },
            ..Query::default()
        };
        // A family with no slant is often asked for an italic. When the machine
        // has no slanted face, the upright one is used and the renderer slants
        // it, so the measure of the two is close enough.
        let found = self.db.query(&asked).or_else(|| {
            let plain = Query {
                style: Style::Normal,
                ..asked
            };
            self.db.query(&plain)
        });
        self.chosen.borrow_mut().insert(key, found);
        found
    }

    // The rise, the fall and the gap of one face, as shares of the size.
    fn height(&self, id: ID) -> (f64, f64, f64) {
        if let Some(one) = self.heights.borrow().get(&id) {
            return *one;
        }
        let read = self
            .db
            .with_face_data(id, |data, index| match Face::parse(data, index) {
                Ok(face) => {
                    let upm = face.units_per_em().max(1) as f64;
                    (
                        face.ascender() as f64 / upm,
                        face.descender().abs() as f64 / upm,
                        face.line_gap() as f64 / upm,
                    )
                }
                Err(_) => (0.78, 0.22, 0.0),
            })
            .unwrap_or((0.78, 0.22, 0.0));
        self.heights.borrow_mut().insert(id, read);
        read
    }

    // The width of one letter of one face, as a share of the size.
    fn advance(&self, id: ID, letter: char) -> f64 {
        let key = (id, letter);
        if let Some(one) = self.advances.borrow().get(&key) {
            return *one;
        }
        let read = self
            .db
            .with_face_data(id, |data, index| match Face::parse(data, index) {
                Ok(face) => {
                    let upm = face.units_per_em().max(1) as f64;
                    match face.glyph_index(letter).and_then(|g| face.glyph_hor_advance(g)) {
                        Some(wide) => wide as f64 / upm,
                        // No letter of that shape: a space is still measured for
                        // a space, and anything else takes the plain width.
                        None => {
                            if letter == ' ' {
                                face.glyph_index(' ')
                                    .and_then(|g| face.glyph_hor_advance(g))
                                    .map(|g| g as f64 / upm)
                                    .unwrap_or(GUESSED)
                            } else {
                                GUESSED
                            }
                        }
                    }
                }
                Err(_) => GUESSED,
            })
            .unwrap_or(GUESSED);
        self.advances.borrow_mut().insert(key, read);
        read
    }

    // How wide a word is, in the picture.
    pub fn width(&self, text: &str, family: &str, weight: u16, italic: bool, size: f64) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let id = self.face(family, weight, italic);
        let mut total = 0.0;
        for letter in text.chars() {
            let share = match id {
                Some(one) => self.advance(one, letter),
                None => GUESSED,
            };
            total += share * size;
        }
        total
    }

    // The height of one line of a family, as a share of the size. The renderer
    // puts the lines of a paragraph this far apart, and the layout must count
    // the same space or the paragraph runs out of its box.
    pub fn line_em(&self, family: &str, weight: u16) -> f64 {
        match self.face(family, weight, false) {
            Some(id) => {
                let (rise, fall, gap) = self.height(id);
                let whole = rise + fall + gap;
                if whole <= 0.0 {
                    GUESSED_LINE
                } else {
                    whole
                }
            }
            None => GUESSED_LINE,
        }
    }

    // The words of a text, cut into lines that fit a width. A new line in the
    // words is kept. A single word wider than the width is cut in the middle of
    // the letters, so nothing is ever drawn outside its box.
    pub fn wrap(
        &self,
        text: &str,
        family: &str,
        weight: u16,
        italic: bool,
        size: f64,
        widest: f64,
    ) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if widest <= 1.0 || size <= 0.0 {
            for part in text.split('\n') {
                out.push(part.to_string());
            }
            return out;
        }
        for para in text.split('\n') {
            if para.trim().is_empty() {
                out.push(String::new());
                continue;
            }
            let mut line = String::new();
            let mut used = 0.0;
            let space = self.width(" ", family, weight, italic, size);
            for word in para.split_whitespace() {
                let wide = self.width(word, family, weight, italic, size);
                if line.is_empty() {
                    // A word alone on a line that is still too wide is cut.
                    if wide <= widest {
                        line.push_str(word);
                        used = wide;
                    } else {
                        cut(word, family, weight, italic, size, widest, &mut out, self);
                    }
                    continue;
                }
                if used + space + wide <= widest {
                    line.push(' ');
                    line.push_str(word);
                    used += space + wide;
                } else {
                    out.push(line);
                    line = String::new();
                    used = 0.0;
                    if wide <= widest {
                        line.push_str(word);
                        used = wide;
                    } else {
                        cut(word, family, weight, italic, size, widest, &mut out, self);
                    }
                }
            }
            if !line.is_empty() {
                out.push(line);
            }
        }
        if out.is_empty() {
            out.push(String::new());
        }
        out
    }
}

// One word, cut into pieces that fit. The pieces are letters, not words, for a
// word alone is what does not fit.
fn cut(
    word: &str,
    family: &str,
    weight: u16,
    italic: bool,
    size: f64,
    widest: f64,
    out: &mut Vec<String>,
    fonts: &Fonts,
) {
    let mut line = String::new();
    let mut used = 0.0;
    for letter in word.chars() {
        let wide = fonts.width(&letter.to_string(), family, weight, italic, size);
        if used + wide > widest && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            used = 0.0;
        }
        line.push(letter);
        used += wide;
    }
    if !line.is_empty() {
        out.push(line);
    }
}

// The first of these family names the machine has, as the machine named it.
fn first_family(db: &Database, names: &[&str]) -> Option<String> {
    for want in names {
        let asked = Query {
            families: &[Family::Name(want)],
            ..Query::default()
        };
        if db.query(&asked).is_some() {
            return Some((*want).to_string());
        }
    }
    // Nothing on the list. The first family the machine has at all is used, so
    // the frames of a bare container still carry real letters.
    for info in db.faces() {
        if let Some((named, _)) = info.families.first() {
            return Some(named.clone());
        }
    }
    None
}

// The weight a word of the script means. A number is taken as it is. An empty
// word is the plain weight.
pub fn weight_of(word: &str) -> u16 {
    match word.trim().to_ascii_lowercase().as_str() {
        "" | "regular" | "normal" | "book" => 400,
            "thin" | "hairline" => 100,
            "extralight" | "extra light" | "ultralight" => 200,
            "light" => 300,
            "medium" => 500,
            "semibold" | "semi bold" | "demibold" => 600,
            "bold" => 700,
            "extrabold" | "extra bold" | "ultrabold" => 800,
            "black" | "heavy" => 900,
            other => other.parse::<u16>().unwrap_or(400).clamp(1, 1000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_has_a_width() {
        let fonts = Fonts::new();
        let narrow = fonts.width("no", "x", 400, false, 40.0);
        let wide = fonts.width("no no no", "x", 400, false, 40.0);
        assert!(wide > narrow);
        assert!(fonts.width("", "x", 400, false, 40.0) == 0.0);
    }

    #[test]
    fn a_line_em_is_a_sane_share() {
        let fonts = Fonts::new();
        let em = fonts.line_em("x", 400);
        assert!(em > 0.9 && em < 2.2);
    }

    #[test]
    fn words_are_cut_to_the_width() {
        let fonts = Fonts::new();
        let lines = fonts.wrap("one two three four five six", "x", 400, false, 40.0, 120.0);
        assert!(lines.len() > 1);
        for one in &lines {
            assert!(!one.trim().is_empty());
        }
        // A new line in the words is kept.
        let kept = fonts.wrap("a\nb", "x", 400, false, 40.0, 4000.0);
        assert_eq!(kept, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn a_weight_word_is_read() {
        assert_eq!(weight_of(""), 400);
        assert_eq!(weight_of("bold"), 700);
        assert_eq!(weight_of("Semi Bold"), 600);
        assert_eq!(weight_of("650"), 650);
    }
}
