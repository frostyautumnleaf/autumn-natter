// The text of a document.
//
// A text file goes to the model as it is now. A document of the types that keep their text
// inside a form of their own does not: the text of a PDF page is among the markers that say
// where on the page they are put, and they are written through a code of the document. A chat
// that was hung on such a file was informed the name of it and nothing else, which is no help
// at all to a model asked what the file reports.
//
// This module reads the text out of the form. Only what the form reports is given: the reading
// is no typesetting, the order of the text is the order they were written in, and a page whose
// text is an image of text -- a scan, or a page of a drawing -- has no text to give. A document
// whose text is kept behind a code the program cannot read records it plainly, rather than
// sending a wall of markers that only looks like text.

use crate::util;

// The ending of a PDF.
pub const PDF_ENDING: &str = "pdf";

// The ending of an RTF file, which keeps its text among the markers of the form it is written
// in.
pub const RTF_ENDING: &str = "rtf";

// The longest run of text this module returns. The size of the context is paid for, and a
// document of a thousand pages is not read into a chat whole.
pub const GREATEST_WORDS: usize = 200_000;

// The most bytes one stream of a document is created plain into. A stream that grows past this
// is a page of markers no reading of a document needs.
const GREATEST_STREAM: usize = 8 * 1024 * 1024;

// True when a file of this name and type is one of the documents the program reads the text out
// of.
pub fn is_document(name: &str, mime: &str) -> bool {
    ending_of(name).as_deref() == Some(PDF_ENDING)
        || ending_of(name).as_deref() == Some(RTF_ENDING)
        || mime == "application/pdf"
        || mime == "application/rtf"
        || mime == "text/rtf"
}

// The text of a document, out of the bytes of it. Nothing returns when the file is none of the
// forms this module reads, and an empty string returns when it is one of them but has no text
// that can be extracted from it.
pub fn words_of(name: &str, mime: &str, bytes: &[u8]) -> Option<String> {
    if ending_of(name).as_deref() == Some(PDF_ENDING) || mime == "application/pdf" {
        let words = pdf_words(bytes);
        if words.trim().is_empty() {
            return Some(String::new());
        }
        return Some(cut(&words));
    }
    if ending_of(name).as_deref() == Some(RTF_ENDING)
        || mime == "application/rtf"
        || mime == "text/rtf"
    {
        let words = rtf_words(bytes);
        if words.trim().is_empty() {
            return Some(String::new());
        }
        return Some(cut(&words));
    }
    None
}

// The text of a document that remains as a data URL, which is the form the records of the
// attachments keep a file in.
pub fn words_of_data_url(name: &str, mime: &str, data: &str) -> Option<String> {
    let bytes = bytes_of_data_url(data)?;
    words_of(name, mime, &bytes)
}

// The bytes behind a data URL. A marker that is no data URL gives nothing, and a run of base64
// that is no run of base64 gives nothing either.
pub fn bytes_of_data_url(text: &str) -> Option<Vec<u8>> {
    let comma = text.find(',')?;
    let head = &text[..comma];
    if !head.starts_with("data:") || !head.contains(";base64") {
        return None;
    }
    util::b64_of(text[comma + 1..].trim())
}

// The ending of a name, in lower case, or nothing when the name has none.
fn ending_of(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    if dot + 1 >= name.len() {
        return None;
    }
    Some(name[dot + 1..].to_lowercase())
}

// Text cut at the longest run this module returns, with a marker that shows the rest was left
// out.
fn cut(words: &str) -> String {
    if words.chars().count() <= GREATEST_WORDS {
        return words.to_string();
    }
    let head: String = words.chars().take(GREATEST_WORDS).collect();
    format!(
        "{}\n(the words of the document were cut here, because they are longer than this program sends)",
        head
    )
}

// The text of a PDF.
//
// The text of a page is in the streams of the file, among the markers that place it on the
// page, and a stream is often written as a run of compressed bytes. Every stream of the file is
// checked: one that was compressed is created plain first, and the markers of the text
// operators are extracted from what returns. A stream with no text operator in it gives
// nothing, so the tables, the images and the notes of the file are passed over.
pub fn pdf_words(bytes: &[u8]) -> String {
    if !bytes.starts_with(b"%PDF-") && !contains_stream(bytes) {
        return String::new();
    }
    let mut out = String::new();
    for stream in streams(bytes) {
        let words = one_stream_words(&stream);
        if words.trim().is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(words.trim());
        out.push('\n');
    }
    let read = out.trim().to_string();
    if !words_are_words(&read) {
        return String::new();
    }
    read
}

// Where a run of markers first is in a run of bytes, or nothing when it is not there at all.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|part| part == needle)
}

// True when the file holds the marker of a stream at all. A file that does not is no PDF to
// read, whatever its name reports.
fn contains_stream(bytes: &[u8]) -> bool {
    find(bytes, b"stream").is_some()
}

// The streams of a PDF, in the order they are in the file. The bytes of one stream run from the
// marker of its start to the marker of its end, and the markers of the dictionary before it say
// whether the bytes were compressed.
fn streams(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut found: Vec<Vec<u8>> = Vec::new();
    let mut at = 0usize;
    while let Some(place) = find(&bytes[at..], b"stream") {
        let head = at + place;
        // The word `stream` is followed by a new line, and the bytes of the stream begin after
        // it. A carriage return before the marker belongs to the dictionary, and the carriage
        // return of the pair is the one that ends the line.
        let mut from = head + b"stream".len();
        if bytes.get(from) == Some(&b'\r') {
            from += 1;
        }
        if bytes.get(from) == Some(&b'\n') {
            from += 1;
        } else {
            // Not the marker of a stream after all, but a note about a dictionary or of a name.
            at = head + b"stream".len();
            continue;
        }
        let Some(tail) = find(&bytes[from..], b"endstream") else {
            break;
        };
        let mut to = from + tail;
        // The new line before the marker of the end belongs to the stream of most writers, and
        // is no word of it.
        if bytes.get(to.saturating_sub(1)) == Some(&b'\n') {
            to -= 1;
            if bytes.get(to.saturating_sub(1)) == Some(&b'\r') {
                to -= 1;
            }
        }
        let part = if to > from { &bytes[from..to] } else { &bytes[from..from] };
        // The dictionary of the object is before the marker of the stream. Only a little of it
        // is checked, for the marker of the form of the bytes is at its front more often than
        // not.
        let dict_start = head.saturating_sub(300);
        let dict = String::from_utf8_lossy(&bytes[dict_start..head]);
        found.push(if dict.contains("FlateDecode") {
            match util::inflate(part, GREATEST_STREAM) {
                Some(made) => made,
                // A run of bytes that is not a run of compressed bytes after all, or one the
                // user could not finish. The plain bytes are kept, for a stream of markers is
                // better answered than lost.
                None => part.to_vec(),
            }
        } else {
            part.to_vec()
        });
        at = to + b"endstream".len();
    }
    found
}

// The text of one stream of a page. A stream with no text operator in it is not a page of text,
// and gives nothing.
fn one_stream_words(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if !looks_like_content(&text) {
        return String::new();
    }
    let mut out = String::new();
    let mut at = 0usize;
    let mut held: Vec<String> = Vec::new();
    let marks: Vec<char> = text.chars().collect();
    while at < marks.len() {
        let one = marks[at];
        if one == '(' {
            let (words, next) = literal(&marks, at + 1);
            held.push(words);
            at = next;
            continue;
        }
        if one == '<' && marks.get(at + 1) == Some(&'<') {
            // The marker of a dictionary, which holds no text of the page.
            at = skip_marks(&marks, at + 2, '>');
            held.clear();
            continue;
        }
        if one == '<' {
            let (words, next) = hex(&marks, at + 1);
            held.push(words);
            at = next;
            continue;
        }
        if one == '[' {
            // The marker of an array: the text inside it are read one after another by the same
            // walk.
            at += 1;
            continue;
        }
        if one == ']' {
            at += 1;
            continue;
        }
        if one == '/' || one.is_ascii_whitespace() {
            if one == '/' {
                // The name of a font or of something else. Any text held from before belong to
                // the operator that came, not to the next one.
                held.clear();
            }
            at += 1;
            continue;
        }
        if one == '%' {
            // A comment of the file, to the end of its line.
            while at < marks.len() && marks[at] != '\n' {
                at += 1;
            }
            continue;
        }
        if one.is_ascii_digit() || one == '-' || one == '+' || one == '.' {
            let began = at;
            while at < marks.len()
                && (marks[at].is_ascii_digit()
                    || marks[at] == '-'
                    || marks[at] == '+'
                    || marks[at] == '.')
            {
                at += 1;
            }
            // A number between two strings of an array moves the next string a little way along
            // the line. Very many of them together is a gap of a whole space, which is how a
            // writer puts a space between two words of one line; a small one is nothing but the
            // shaping of the letters, and joins no text.
            if !held.is_empty() {
                let written: String = marks[began..at].iter().collect();
                if written.parse::<f64>().unwrap_or(0.0) <= -150.0 {
                    held.push(String::from(" "));
                }
            }
            continue;
        }
        // An operator: a note about letters, of which only a few have to do with the text of
        // the page.
        let began = at;
        while at < marks.len() && marks[at].is_ascii_alphabetic() {
            at += 1;
        }
        if at == began {
            at += 1;
            continue;
        }
        let operator: String = marks[began..at].iter().collect();
        match operator.as_str() {
            "Tj" => {
                if let Some(last) = held.last() {
                    out.push_str(last);
                }
            }
            "'" | "\"" => {
                out.push('\n');
                if let Some(last) = held.last() {
                    out.push_str(last);
                }
            }
            "TJ" => {
                for one in &held {
                    out.push_str(one);
                }
            }
            "Td" | "TD" | "T*" | "ET" => out.push('\n'),
            _ => {}
        }
        held.clear();
    }
    out
}

// True when a stream is the content of a page: it carries one of the markers of the text
// operators. The markers are short, and are searched for as letters alone, for a stream of a
// page is a run of short operators.
fn looks_like_content(text: &str) -> bool {
    text.contains("Tj")
        || text.contains("TJ")
        || (text.contains("Tf") && text.contains("Td"))
        || text.contains("BT")
}

// A literal string of the PDF: the text between the two markers, with the markers of its own
// escapes read. The marker of the end is given back as well, for the walk has to go on after
// it.
fn literal(marks: &[char], mut at: usize) -> (String, usize) {
    let mut out = String::new();
    let mut depth = 1usize;
    while at < marks.len() {
        let one = marks[at];
        if one == '\\' {
            let next = marks.get(at + 1).copied().unwrap_or(' ');
            match next {
                'n' => out.push('\n'),
                'r' => out.push('\n'),
                't' => out.push('\t'),
                'b' | 'f' => {}
                '(' => out.push('('),
                ')' => out.push(')'),
                '\\' => out.push('\\'),
                '0'..='7' => {
                    // An octal number of at most three figures.
                    let mut code = String::new();
                    let mut at2 = at + 1;
                    while code.len() < 3 && at2 < marks.len() && ('0'..='7').contains(&marks[at2])
                    {
                        code.push(marks[at2]);
                        at2 += 1;
                    }
                    let value = u8::from_str_radix(&code, 8).unwrap_or(b' ');
                    out.push(value as char);
                    at = at2;
                    continue;
                }
                '\r' | '\n' => {
                    // A new line in the middle of a literal string is nothing but the line of
                    // the file being broken.
                }
                other => out.push(other),
            }
            at += 2;
            continue;
        }
        if one == '(' {
            depth += 1;
            out.push(one);
            at += 1;
            continue;
        }
        if one == ')' {
            depth -= 1;
            if depth == 0 {
                return (out, at + 1);
            }
            out.push(one);
            at += 1;
            continue;
        }
        out.push(one);
        at += 1;
    }
    (out, at)
}

// A string of a PDF written as figures: two markers, and the bytes of it as pairs of
// hexadecimal figures. A run of an odd number of figures takes a zero behind it, as the form of
// the file asks.
fn hex(marks: &[char], mut at: usize) -> (String, usize) {
    let mut bytes: Vec<u8> = Vec::new();
    let mut high: Option<u8> = None;
    while at < marks.len() {
        let one = marks[at];
        if one == '>' {
            if let Some(first) = high {
                bytes.push(first << 4);
            }
            return (text_of(&bytes), at + 1);
        }
        let figure = one.to_digit(16);
        if figure.is_none() {
            at += 1;
            continue;
        }
        let value = (figure.unwrap_or(0) & 0xf) as u8;
        match high {
            None => high = Some(value),
            Some(first) => {
                bytes.push((first << 4) | value);
                high = None;
            }
        }
        at += 1;
    }
    (text_of(&bytes), at)
}

// The bytes of a string of a document. A run that begins with the marker of sixteen-bit text is
// read as such: the text of a page is often kept that way. Any other run is read as the form of
// a Western file asks, which is one byte to one letter.
fn text_of(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xfe && bytes[1] == 0xff {
        return utf16_be(&bytes[2..]);
    }
    bytes.iter().map(|one| *one as char).collect()
}

// A run of text of sixteen bits, the great end first.
fn utf16_be(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut at = 0usize;
    while at + 1 < bytes.len() {
        let value = u16::from_be_bytes([bytes[at], bytes[at + 1]]);
        at += 2;
        match char::from_u32(u32::from(value)) {
            Some(one) => out.push(one),
            // A note about a pair is with the one behind it, and the two together are a letter
            // of a great alphabet. Those are rare in the strings of a page, and are passed over
            // rather than guessed at.
            None => out.push('\u{fffd}'),
        }
    }
    out
}

// Go over the markers of a form until the marker that ends it comes, or the end of the bytes.
fn skip_marks(marks: &[char], mut at: usize, last: char) -> usize {
    while at < marks.len() {
        if marks[at] == last {
            return at + 1;
        }
        at += 1;
    }
    at
}

// True when a run of text reads as text of some language, rather than as the markers a code
// left behind. The text of a page whose letters is kept behind a code the program cannot read
// return as a run of forms that no writing is made of, and those are refused rather than sent.
fn words_are_words(words: &str) -> bool {
    let mut letters = 0usize;
    let mut total = 0usize;
    let mut shape = 0usize;
    for one in words.chars() {
        total += 1;
        // A zero, or any other marker no writing is ever made of, is the work of a code and not
        // of a page.
        if one == '\0' || (one.is_control() && one != '\n' && one != '\t' && one != '\r') {
            shape += 1;
        }
        if one.is_alphabetic() || one.is_numeric() {
            letters += 1;
        }
        if total >= 400 {
            break;
        }
    }
    if total == 0 {
        return false;
    }
    // One marker in twenty that no writing is made of is a page with a stray byte in it. More
    // than that is a code the program cannot read.
    if shape * 20 > total {
        return false;
    }
    // The text of a page is near all letters and figures; the markers of a code are near none
    // of them.
    letters * 10 >= total * 4
}

// The text of an RTF file: the letters of it with the markers of its form left out. The form is
// a run of groups in braces, each with the text of the page among the markers that say how they
// are to look.
pub fn rtf_words(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if !text.starts_with("{\\rtf") {
        return String::new();
    }
    let mut out = String::new();
    // A group whose name reports it holds the form of the file, and not text of it, is passed
    // over whole, with every group inside it. What the passing over was before the group came
    // is kept for every group, so a table of fonts within a table of fonts is passed over too.
    let mut stack: Vec<bool> = Vec::new();
    let mut skip = false;
    let marks: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    while at < marks.len() {
        let one = marks[at];
        if one == '{' {
            stack.push(skip);
            at += 1;
            continue;
        }
        if one == '}' {
            skip = stack.pop().unwrap_or(false);
            at += 1;
            continue;
        }
        if one == '\\' {
            let next = marks.get(at + 1).copied().unwrap_or(' ');
            if next == '\\' || next == '{' || next == '}' {
                if !skip {
                    out.push(next);
                }
                at += 2;
                continue;
            }
            // The marker of a destination: `\*` before the name of a group the user of the file
            // may pass over. It holds something the text of the page does not need, so it is
            // passed over here.
            if next == '*' {
                skip = true;
                at += 2;
                continue;
            }
            if next.is_ascii_alphabetic() {
                let began = at + 1;
                let mut after = began;
                while after < marks.len() && marks[after].is_ascii_alphabetic() {
                    after += 1;
                }
                let word: String = marks[began..after].iter().collect();
                // A figure behind the name of a marker belongs to the marker.
                while after < marks.len() && marks[after].is_ascii_digit() {
                    after += 1;
                }
                if after < marks.len() && marks[after] == '\n' {
                    after += 1;
                }
                match word.as_str() {
                    "par" | "line" | "sect" | "row" => out.push('\n'),
                    "tab" => out.push('\t'),
                    // A table of the forms of the letters, of the colours, of the styles, the
                    // notes about the file, the running headers, and the images: none of these
                    // are text of the page.
                    "fonttbl"
                    | "colortbl"
                    | "stylesheet"
                    | "info"
                    | "header"
                    | "footer"
                    | "headerl"
                    | "footerl"
                    | "preamble"
                    | "listtable"
                    | "themedata"
                    | "generator"
                    | "datastore"
                    | "filetbl"
                    | "xmlschema"
                    | "rtfgroup"
                    | "latentstyles"
                    | "colorschememapping"
                    | "bin"
                    | "pict"
                    | "shppict"
                    | "nonshppict" => skip = true,
                    _ => {}
                }
                at = after;
                continue;
            }
            // An escape of two characters, as `\'e9` for a letter of another alphabet. The
            // bytes of it are not read here, for the code of them is in the header of the file.
            if next == '\'' {
                at += 4;
                continue;
            }
            at += 2;
            continue;
        }
        if one == '\r' || one == '\n' {
            at += 1;
            continue;
        }
        if !skip {
            out.push(one);
        }
        at += 1;
    }
    let read = out.trim().to_string();
    if !words_are_words(&read) {
        return String::new();
    }
    read
}

#[cfg(test)]
mod tests {
    use super::*;

    // The pages of a PDF are extracted from the streams of it, and nothing of the tables, the
    // images and the notes of the file comes with them.
    #[test]
    fn the_words_of_a_page_are_read_out_of_the_streams() {
        let page = b"BT /F1 12 Tf 10 700 Td (Hello from the plan) Tj 0 -14 Td (second line) Tj ET";
        let file = format!(
            "%PDF-1.4\n1 0 obj\n<< /Type /Page >>\nendobj\n2 0 obj\n<< /Length {} >>\nstream\n",
            page.len()
        );
        let mut made: Vec<u8> = file.into_bytes();
        made.extend_from_slice(page);
        made.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF");
        let words = pdf_words(&made);
        assert!(words.contains("Hello from the plan"), "{}", words);
        assert!(words.contains("second line"), "{}", words);
        // A stream that is no page gives nothing of itself.
        assert!(pdf_words(b"%PDF-1.4\nno stream here at all").is_empty());
    }

    // A stream written as a run of compressed bytes is created plain first. The run here is one
    // written whole, which is the simplest of the forms DEFLATE writes, and the one a test can
    // make without a writer of its own.
    #[test]
    fn a_compressed_stream_is_made_plain() {
        let page = b"BT /F1 12 Tf (Compressed words) Tj ET";
        let plain = stored(page);
        let made = format!(
            "%PDF-1.4\n5 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
            plain.len()
        );
        let mut file: Vec<u8> = made.into_bytes();
        file.extend_from_slice(&plain);
        file.extend_from_slice(b"\nendstream\nendobj\n%%EOF");
        assert!(pdf_words(&file).contains("Compressed words"), "the run was not made plain");
    }

    // A run of bytes written whole, with the markers of a zlib stream about it: the two
    // markers, the marker of no block at the end, the length and its opposite, the bytes, and
    // the check of them.
    fn stored(bytes: &[u8]) -> Vec<u8> {
        let mut made: Vec<u8> = vec![0x78, 0x01, 0x01];
        let length = bytes.len() as u16;
        made.push(u8::try_from(length & 0xff).unwrap_or(0));
        made.push(u8::try_from(length >> 8).unwrap_or(0));
        made.push(u8::try_from(!length & 0xff).unwrap_or(0));
        made.push(u8::try_from((!length >> 8) & 0xff).unwrap_or(0));
        made.extend_from_slice(bytes);
        made.extend_from_slice(&adler(bytes));
        made
    }

    // The check of a zlib stream: the running sum of the bytes, and of the running sums of
    // them.
    fn adler(bytes: &[u8]) -> [u8; 4] {
        let (mut one, mut two) = (1u32, 0u32);
        for part in bytes {
            one = (one + u32::from(*part)) % 65521;
            two = (two + one) % 65521;
        }
        ((two << 16) | one).to_be_bytes()
    }

    // The markers of the strings are read: the escapes of a literal string, the pairs of
    // figures of a string of figures, and the arrays of a line. A great number between two
    // strings of an array is a space; a small one is the shaping of the letters and joins none.
    #[test]
    fn the_marks_of_the_strings_are_read() {
        let page = b"BT (escaped \\(paren\\) and \\n break) Tj [(one) -240 (two)] TJ [(frag) -8 (ment)] TJ <48656c6c6f> Tj ET";
        let words = one_stream_words(page);
        assert!(words.contains("escaped (paren)"), "{}", words);
        assert!(words.contains("one two"), "{}", words);
        assert!(words.contains("fragment"), "{}", words);
        assert!(words.contains("Hello"), "{}", words);
    }

    // A page whose text is kept behind a code the program cannot read returns as nothing,
    // rather than as a wall of markers that only looks like text.
    #[test]
    fn a_code_the_program_cannot_read_is_refused() {
        let page = b"BT <004100a200b300c400d5> Tj <004100a200b300c400d5> Tj ET";
        assert!(pdf_words_from(page).is_empty());
    }

    // The text behind the marker of sixteen-bit text is read as such.
    #[test]
    fn words_of_sixteen_bits_are_read() {
        let page = b"BT <feff00480065006c006c006f> Tj ET";
        assert!(pdf_words_from(page).contains("Hello"));
    }

    // The same page, given as the program is given one.
    fn pdf_words_from(page: &[u8]) -> String {
        let made = format!("%PDF-1.4\n1 0 obj\n<< /Length {} >>\nstream\n", page.len());
        let mut file: Vec<u8> = made.into_bytes();
        file.extend_from_slice(page);
        file.extend_from_slice(b"\nendstream\nendobj\n%%EOF");
        pdf_words(&file)
    }

    // The text of an RTF file, with the markers of its form left out.
    #[test]
    fn the_words_of_an_rtf_file() {
        let made = rtf_words(b"{\\rtf1\\ansi{\\fonttbl\\f0 Helvetica}The plan of the work\\par next line}");
        assert!(made.contains("The plan of the work"), "{}", made);
        assert!(made.contains("next line"), "{}", made);
        assert!(!made.contains("Helvetica"), "{}", made);
        // A file that is no RTF is answered with nothing.
        assert!(rtf_words(b"just text").is_empty());
    }

    // The types the program reads the text out of, and the types it leaves to their name. A
    // text file, as a note or a table of figures, needs no reading of a form: it goes to the
    // model as it is now, and that is the work of the module that sends the attachments, not of
    // this one.
    #[test]
    fn the_kinds_of_the_documents() {
        assert!(is_document("plan.pdf", ""));
        assert!(is_document("PLAN.PDF", "application/octet-stream"));
        assert!(is_document("notes.rtf", ""));
        assert!(is_document("paper", "application/pdf"));
        assert!(!is_document("notes.txt", "text/plain"));
        assert!(!is_document("notes.md", "text/markdown"));
        assert!(!is_document("photo.png", "image/png"));
    }

    // A data URL is undone, and a run of markers that is no data URL is answered with nothing.
    #[test]
    fn the_bytes_of_a_data_url() {
        assert_eq!(bytes_of_data_url("data:application/pdf;base64,QUJD").as_deref(), Some(&b"ABC"[..]));
        assert!(bytes_of_data_url("data:text/plain,words").is_none());
        assert!(bytes_of_data_url("no mark at all").is_none());
    }
}
