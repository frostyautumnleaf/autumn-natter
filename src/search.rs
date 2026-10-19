// The search tool: it looks a thing up on the web, and reads a page of it.
//
// One name does two things. Given words to look for, the tool asks the engines
// in order and gives back the titles, the addresses and the short words of the
// places it found. Given an address, it reads that page and gives back its
// words, cut of their marks and cut short. The model is the reader of both
// answers, and the model reads words, so nothing of the marks of a page is kept
// unless they say something.
//
// The shape of these answers was first made in another program, whose code is
// under the AGPL. Nothing was copied. The engines this machine can reach are not
// the engines of that program either, so the reading of each answer was written
// again here, from what the engine actually sends back.
//
// No new part is added to the program for this. The words of a page are read by
// a small reader of its own, and the marks of a page are taken out by a rule of
// its own, so the binary keeps the weight it had.

use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;

use crate::tools;

// The name the model uses to look a thing up.
pub const NAME: &str = "web_search";

// The most bytes of one page that are read. A page of a news house may hold
// three times this in the marks of its head alone, and none of those marks are
// words.
const MAX_FETCH_BYTES: usize = 512 * 1024;

// The most words of a page, or of a whole answer, that reach the model. The room
// of the talk is small, and the front of a page holds the best of it.
const MAX_PAGE_CHARS: usize = 16_000;

// The most places one search names.
const MAX_PLACES: usize = 6;

// The words between two places of the answer, and the words at the end that tell
// the model what to do with an address it likes.
const BETWEEN: &str = "\n\n---\n\n";
const LAST: &str = "\n\n---\n\nThese are short words of each place. To read a whole page, ask again with the address of it, as {\"url\": \"https://...\"}.";

// The words that stand after a page of more words than this program sends.
const CUT: &str = "(the words of the page were cut here, for the page held more than this program sends)";

// The engines, in the order they are worth asking. Two of the well known engines
// answer this machine with a test to prove that it is not a house of robots, so
// the engine that asks no such test comes first, and a failure of one engine is
// passed on quietly to the next. The second of them answers with such a test
// today as well, and it is kept as a second try for the day the test goes away.
const ENGINES: [&str; 2] = ["brave", "duckduckgo"];

// The mark that stands in the tag opening one place of a Brave answer. The whole
// tag cannot be written out: the class of it carries a mark of the build of the
// engine, and that mark changes while the words of the page stand. The place of
// one answer therefore begins after the tag this mark stands in, and ends where
// the next such tag stands.
const BRAVE_MARK: &str = "data-type=\"web\"";

// The address of the encyclopedia, which is the last place to look and holds
// words of one kind only. It is named out loud when it answered, so the model
// does not take it for the whole of the web.
const ENCYCLOPEDIA: &str = "https://en.wikipedia.org/w/api.php?action=query&format=json&list=search&srlimit=6&srsearch=";
const ENCYCLOPEDIA_PAGE: &str = "https://en.wikipedia.org/wiki/";

// The words a browser of the machine says, and the language to ask for. An
// engine that is not shown a browser answers with a test instead of an answer.
const AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

// The words of the call, and the words the reader is shown.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": "Look a thing up on the web, and read a page of it. With words to look for it gives back the titles, the addresses and the short words of the places it found. With an address it reads that page and gives back its words, so use the address of a place you liked rather than guess at it. The answer of an engine is short words only, so a second call with the address of it is often the better part of the work. Nothing is kept of what was read.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The words to look for."
                    },
                    "url": {
                        "type": "string",
                        "description": "The address of a page to read, instead of looking about."
                    }
                },
                "required": []
            }
        }
    })
}

// One place of a search answer.
struct Place {
    title: String,
    address: String,
    words: String,
}

// What the call named: the words to look for, and the address to read. A model
// writes a name other than the one that was asked for more often than one would
// think, so the other names a local model reaches for are read as well.
pub fn words_of(raw: &str) -> (String, String) {
    let Some(call) = object(raw) else {
        let part = tools::partial_text(raw.trim_start(), "query");
        let rest = tools::partial_text(raw.trim_start(), "url");
        return (part, rest);
    };
    (
        first(&call, &["query", "q", "search_query", "search", "text"]),
        first(&call, &["url", "uri", "href", "link"]),
    )
}

// The one thing the call named, as far as it has come. The reader watches this
// while the call is written. An address is worth showing whole, for the words to
// look for may not have come yet.
pub fn partial_words(raw: &str) -> String {
    let text = raw.trim_start();
    let query = tools::partial_text(text, "query");
    if !query.is_empty() {
        return query;
    }
    tools::partial_text(text, "url")
}

// Look a thing up, or read a page. The answer is the words the model is given.
pub fn run(raw: &str) -> tools::Report {
    let (query, address) = words_of(raw);
    if !address.trim().is_empty() {
        return read_page(address.trim());
    }
    let wanted = query.trim();
    if wanted.is_empty() {
        return tools::Report {
            ok: false,
            output: String::from(
                "the call needs words to look for, or the address of a page to read",
            ),
        };
    }
    let mut trouble: Vec<String> = Vec::new();
    for engine in ENGINES {
        match ask(engine, wanted) {
            Ok(places) if !places.is_empty() => return told(places, false),
            Ok(_) => trouble.push(format!("{} found nothing", engine)),
            Err(problem) => trouble.push(format!("{}: {}", engine, problem)),
        }
    }
    // The encyclopedia is the last place, and it is named as such in the answer.
    match encyclopedia(wanted) {
        Ok(places) if !places.is_empty() => return told(places, true),
        Ok(_) => trouble.push(String::from("the encyclopedia found nothing")),
        Err(problem) => trouble.push(format!("the encyclopedia: {}", problem)),
    }
    tools::Report {
        ok: false,
        output: format!(
            "nothing was found for \"{}\". {}",
            first_words(wanted, 120),
            trouble.join("; ")
        ),
    }
}

// The answer of a search, in the shape the model reads: a place, then the words
// between two places, and at the end the word about reading a whole page.
fn told(places: Vec<Place>, encyclopedia: bool) -> tools::Report {
    let mut parts: Vec<String> = Vec::new();
    for place in places.iter().take(MAX_PLACES) {
        parts.push(format!(
            "Title: {}\nURL: {}\nWords: {}",
            flat(&place.title, 200),
            place.address,
            flat(&place.words, 600)
        ));
    }
    let mut output = parts.join(BETWEEN);
    if encyclopedia {
        output = format!(
            "The engines of the web could not be reached, or found nothing usable. These are words of the encyclopedia, which is not the news of today.\n\n{}",
            output
        );
    }
    output.push_str(LAST);
    if output.chars().count() > MAX_PAGE_CHARS {
        output = cut_words(&output, MAX_PAGE_CHARS);
    }
    tools::Report {
        ok: true,
        output,
    }
}

// Read one page of the web, and give back its words.
fn read_page(address: &str) -> tools::Report {
    if !address.starts_with("http://") && !address.starts_with("https://") {
        return tools::Report {
            ok: false,
            output: format!(
                "\"{}\" is not an address to read. An address starts with http:// or https://",
                first_words(address, 120)
            ),
        };
    }
    match fetch(address) {
        Err(problem) => tools::Report {
            ok: false,
            output: format!("the page at {} could not be read: {}", address, problem),
        },
        Ok(page) => {
            let mut words = to_words(&page);
            if words.chars().count() > MAX_PAGE_CHARS {
                words = cut_words(&words, MAX_PAGE_CHARS);
                words.push('\n');
                words.push_str(CUT);
            }
            if words.trim().is_empty() {
                return tools::Report {
                    ok: false,
                    output: format!("the page at {} held no words to read", address),
                };
            }
            tools::Report {
                ok: true,
                output: format!("The words of {}:\n\n{}", address, words),
            }
        }
    }
}

// Ask one engine for the words of a search.
fn ask(engine: &str, wanted: &str) -> Result<Vec<Place>, String> {
    let address = match engine {
        "brave" => format!(
            "https://search.brave.com/search?q={}",
            urlencoding::encode(wanted)
        ),
        "duckduckgo" => format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(wanted)
        ),
        other => return Err(format!("no engine of that name: {}", other)),
    };
    let page = fetch(&address)?;
    Ok(match engine {
        "brave" => brave_places(&page),
        _ => duckduckgo_places(&page),
    })
}

// The places of an answer of Brave. Every place of its own accord opens with a
// mark that says what kind of thing it is.
fn brave_places(page: &str) -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();
    let mut from = 0usize;
    while out.len() < MAX_PLACES {
        let Some(here) = find_from(page, from, BRAVE_MARK) else {
            break;
        };
        // The words of the place stand after the tag the mark stands in, and not
        // after the mark itself, which is in the middle of that tag.
        let Some(open) = find_from(page, here, ">") else {
            break;
        };
        let start = open + 1;
        let next = find_from(page, start, BRAVE_MARK).unwrap_or(page.len());
        if let Some(one) = brave_block(page.get(start..next).unwrap_or("")) {
            out.push(one);
        }
        from = next.max(here + 1);
    }
    out
}

// One place of a Brave answer: its address, its title, and its short words. A
// block that is not in the shape that was looked for is given up on, and the
// next block is read.
fn brave_block(block: &str) -> Option<Place> {
    let mut at = 0usize;
    let rest = past(block, &mut at, "<a href=\"")?;
    let end = rest.find('"')?;
    let address = clean(rest.get(..end)?);
    if !address.starts_with("http") {
        return None;
    }
    // The title hangs on the mark that opened the part of it.
    let rest = past(block, &mut at, "<div class=\"title")?;
    let title = quoted(rest, "title=\"")?;
    // The short words stand in a part of their own after the title.
    let rest = past(block, &mut at, "<div class=\"content")?;
    let rest = match rest.find('>') {
        Some(at) => rest.get(at + 1..).unwrap_or(rest),
        None => rest,
    };
    let words = words_till(rest, "</div>")?;
    Some(Place {
        title,
        address,
        words: words_of_marks(&words),
    })
}

// The places of a DuckDuckGo answer. The address of a place hides behind the
// address of the engine, and is set free before the model is given it.
fn duckduckgo_places(page: &str) -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();
    let mut at = 0usize;
    while out.len() < MAX_PLACES {
        let Some(link) = find_from(page, at, "class=\"result__a\"") else {
            break;
        };
        let Some(href) = find_from(page, link, "href=\"") else {
            break;
        };
        let from = href + "href=\"".len();
        let Some(end) = find_from(page, from, "\"") else { break };
        let address = unhide(page.get(from..end).unwrap_or(""));
        let Some(open) = find_from(page, from, ">") else { break };
        let begin = open + 1;
        let Some(close) = find_from(page, begin, "</a>") else { break };
        let title = words_of_marks(page.get(begin..close).unwrap_or(""));
        // The short words stand in the link of their own after the title.
        let Some(mark) = find_from(page, close, "class=\"result__snippet\"") else {
            break;
        };
        let body = mark + "class=\"result__snippet\"".len();
        let Some(open) = find_from(page, body, ">") else { break };
        let begin = open + 1;
        let Some(close) = find_from(page, begin, "</a>") else { break };
        let said = words_of_marks(page.get(begin..close).unwrap_or(""));
        // The reading carries on after the short words, so the next place read is
        // the next one, and not the same one again.
        at = close;
        if !address.starts_with("http") {
            continue;
        }
        out.push(Place {
            title,
            address,
            words: said,
        });
    }
    out
}

// The place a mark stands in, at or after a place we had read to, when the mark
// is in what was left.
fn find_from(text: &str, at: usize, mark: &str) -> Option<usize> {
    let rest = text.get(at..)?;
    rest.find(mark).map(|found| at + found)
}

// The address behind the address of the engine.
fn unhide(text: &str) -> String {
    let Some(at) = text.find("uddg=") else {
        return clean(text);
    };
    let rest = &text[at + "uddg=".len()..];
    let cut = rest.split('&').next().unwrap_or(rest);
    match urlencoding::decode(cut) {
        Ok(made) => clean(&made),
        Err(_) => clean(text),
    }
}

// Ask the encyclopedia. It is not an engine of the whole web: its words are of
// one kind, and the answer says so out loud when it was the one that answered.
fn encyclopedia(wanted: &str) -> Result<Vec<Place>, String> {
    let address = format!("{}{}", ENCYCLOPEDIA, urlencoding::encode(wanted));
    let page = fetch(&address)?;
    Ok(wiki_places(&page))
}

// The places of an answer of the encyclopedia. Its words are JSON, and the short
// words of a place hold marks of their own.
fn wiki_places(page: &str) -> Vec<Place> {
    let Ok(value) = serde_json::from_str::<Value>(page) else {
        return Vec::new();
    };
    let mut out: Vec<Place> = Vec::new();
    let Some(items) = value
        .pointer("/query/search")
        .and_then(|v| v.as_array())
    else {
        return out;
    };
    for item in items.iter().take(MAX_PLACES) {
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if title.is_empty() {
            continue;
        }
        let snippet = item.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
        out.push(Place {
            address: format!("{}{}", ENCYCLOPEDIA_PAGE, urlencoding::encode(&title.replace(' ', "_"))),
            title,
            words: words_of_marks(snippet),
        });
    }
    out
}

// Get one address, and read what comes back. The bytes are stopped at the cap,
// so a page that goes on for ever does not hold the program.
fn fetch(address: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(20))
        .redirects(5)
        .build();
    let made = agent
        .get(address)
        .set("user-agent", AGENT)
        .set("accept-language", "en")
        .call();
    let response = match made {
        Ok(response) => response,
        Err(problem) => return Err(first_words(&problem.to_string(), 200)),
    };
    let mut bytes: Vec<u8> = Vec::new();
    let mut reader = response.into_reader();
    let _ = reader
        .by_ref()
        .take(MAX_FETCH_BYTES as u64)
        .read_to_end(&mut bytes);
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

// The words of a page of marks. The parts that hold no words for a reader are
// dropped whole: the head of the page, the programmes of it, the styles of it,
// and the notes of the writer. A mark that ends a block of words leaves a new
// line behind, and the mark of a list leaves a dash. The rest of the marks go.
fn to_words(page: &str) -> String {
    let mut out = String::new();
    let mut rest = page;
    // A page of nothing but marks would go on for as long as it is long, so the
    // reading has a ceiling of its own.
    let mut guard = 0usize;
    while let Some(at) = rest.find('<') {
        out.push_str(&decode(&rest[..at]));
        guard += 1;
        if guard > 100_000 {
            break;
        }
        // A part with no words for a reader is left out whole.
        if let Some(after) = dropped(rest, at) {
            rest = after;
            continue;
        }
        if starts_any(&rest[at..], BLOCK_ENDS) {
            out.push('\n');
        }
        if starts_any(&rest[at..], LIST_MARKS) {
            out.push_str("\n- ");
        }
        let Some(gt) = mark_end(&rest[at..]) else { break };
        rest = &rest[at + gt + 1..];
    }
    if !rest.is_empty() {
        out.push_str(&decode(rest));
    }
    tidy_words(&out)
}

// The parts of a page that hold nothing a reader would want to read, with the
// mark that closes each of them. Marks of a page are written in either half of
// the alphabet, so each pair is looked for without minding the case.
//
// The marks of the house of a page stand here as well: the menu of it, the tools
// at its side and the words at its foot stand the same on every page of the
// house, and they are not the words the page was written about. The mark of a
// header is not among them, for many a page hangs the words of its front on one.
const DROPS: [(&str, &str); 11] = [
    ("<script", "</script>"),
    ("<style", "</style>"),
    ("<!--", "-->"),
    ("<noscript", "</noscript>"),
    ("<svg", "</svg>"),
    ("<template", "</template>"),
    ("<head", "</head>"),
    ("<nav", "</nav>"),
    ("<aside", "</aside>"),
    ("<form", "</form>"),
    ("<footer", "</footer>"),
];

// The page after a part of it that holds no words, when the mark at the place
// asked for opens such a part. Nothing is left when the mark that ends the part
// is not in the page at all, which is how a page cut short in the middle of a
// programme of its own is left standing there. The words of such a programme are
// no words for a reader, and reading them would fill the answer of them.
fn dropped(page: &str, at: usize) -> Option<&str> {
    let head = page.get(at..)?;
    for (open, close) in DROPS {
        if !opens(head, open) {
            continue;
        }
        let rest = head.get(open.len()..)?;
        let Some(end) = find_ignore(rest, close) else {
            return Some("");
        };
        return rest.get(end + close.len()..);
    }
    None
}

// The marks that end a block of words, and the marks that open a part of a list.
const BLOCK_ENDS: &[&str] = &[
    "</p", "</div", "<br", "</li", "</h1", "</h2", "</h3", "</h4", "</h5", "</h6", "</tr",
    "</table", "</section", "</article", "</blockquote", "</header", "</footer", "</nav",
    "<hr", "</figure", "</pre", "</dl", "</ul", "</ol",
];
const LIST_MARKS: &[&str] = &["<li", "<dt", "<dd", "<tr"];

// True when the mark at the front of the words is the name that was asked for,
// and that name is the whole of the word. The mark of the head of a page is not
// the mark of a header of it, and reading one for the other would throw away the
// words of the page.
fn opens(head: &str, name: &str) -> bool {
    let Some(tail) = head.get(..name.len()) else {
        return false;
    };
    if !tail.eq_ignore_ascii_case(name) {
        return false;
    }
    match head.get(name.len()..name.len() + 1) {
        Some(next) => !next.chars().next().is_some_and(|ch| ch.is_ascii_alphanumeric()),
        None => false,
    }
}

// True when the words at the front begin with any of the marks. The front is
// taken with a look that minds the end of a character: the words of a page may
// begin with a sign of more than one byte, and cutting a mark out of the middle
// of one would stop the program.
fn starts_any(head: &str, marks: &[&str]) -> bool {
    marks.iter().any(|mark| {
        head.get(..mark.len())
            .is_some_and(|front| front.eq_ignore_ascii_case(mark))
    })
}

// The place a run of words stands in, without minding the case of it. Marks of a
// page are written in both halves of the alphabet by different houses.
fn find_ignore(text: &str, wanted: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let needle = wanted.as_bytes();
    if needle.is_empty() || needle.len() > bytes.len() {
        return None;
    }
    let mut at = 0usize;
    while at + needle.len() <= bytes.len() {
        if bytes[at..at + needle.len()].eq_ignore_ascii_case(needle) {
            return Some(at);
        }
        at += 1;
    }
    None
}

// The words of a page, made ready: the spaces of the marks go, many new lines in
// a row become two, and the words of one line stand together.
fn tidy_words(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let kept = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if kept.is_empty() {
            // A line of nothing is worth keeping between two parts, and nowhere
            // else.
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.is_empty() && !out.ends_with("\n\n") {
                out.push('\n');
            }
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&kept);
        out.push('\n');
    }
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out.trim().to_string()
}

// The words with the marks of the page turned back into the characters they stand
// for. A mark that is not known stands as it was written, which reads better than
// a word that went missing.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        // A name of a mark is short. A longer run of words behind an ampersand is
        // an ampersand, and not a mark.
        let Some(end) = tail.find(';').filter(|place| *place < 12) else {
            out.push('&');
            rest = tail;
            continue;
        };
        match entity(&tail[..end]) {
            Some(ch) => {
                out.push(ch);
                rest = &tail[end + 1..];
            }
            None => {
                // A mark that is not one of ours stands as it was written, for a
                // word with a hole in it reads worse than one with a strange sign.
                out.push('&');
                out.push_str(&tail[..end]);
                out.push(';');
                rest = &tail[end + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

// The character a mark of a page stands for. The short words of a page are full
// of the marks that no key of a keyboard has.
fn entity(name: &str) -> Option<char> {
    let name = name.trim();
    if let Some(rest) = name.strip_prefix('#') {
        let number = match rest.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => rest.parse::<u32>().ok()?,
        };
        return char::from_u32(number);
    }
    match name.to_ascii_lowercase().as_str() {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" | "ensp" | "emsp" | "thinsp" => Some(' '),
        "middot" | "bull" => Some('-'),
        "hellip" | "mldr" => Some('…'),
        "mdash" => Some('—'),
        "ndash" => Some('–'),
        "lsquo" => Some('‘'),
        "rsquo" => Some('’'),
        "ldquo" => Some('“'),
        "rdquo" => Some('”'),
        "laquo" => Some('«'),
        "raquo" => Some('»'),
        "copy" => Some('©'),
        "reg" => Some('®'),
        "trade" => Some('™'),
        "deg" => Some('°'),
        "euro" => Some('€'),
        "pound" => Some('£'),
        _ => None,
    }
}

// The end of a mark of a page, counted from the sign that opened it. A page of
// today carries whole answers of its own in the words between two quotes of a
// mark, and such a run may hold the sign that seems to close the mark. Reading
// the mark as ended there would pour the words of the attribute into the answer.
fn mark_end(text: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (at, ch) in text.char_indices() {
        match quote {
            Some(opened) => {
                if ch == opened {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '>' => return Some(at),
                _ => {}
            },
        }
    }
    // A mark whose quote never ends is read to its first sign, which is what a
    // reader without the words between the quotes would have done.
    text.find('>')
}

// The words of a piece of a page that still holds marks: the marks go, and what
// is left is one line of words.
fn words_of_marks(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        out.push_str(&decode(&rest[..at]));
        // The mark ends at the sign that closes it, and not at a sign in front
        // of where it opened, which would eat words that were no mark.
        let Some(gt) = mark_end(&rest[at..]) else { break };
        rest = &rest[at + gt + 1..];
    }
    if !rest.is_empty() {
        out.push_str(&decode(rest));
    }
    flat(&out, MAX_PAGE_CHARS)
}

// Move a place in a page past a mark, and give back what is left after it.
// Nothing comes back when the mark is not in what was left, which is how a page
// of a shape that was not expected is given up on.
fn past<'a>(page: &'a str, at: &mut usize, mark: &str) -> Option<&'a str> {
    let rest = page.get(*at..)?;
    let found = rest.find(mark)?;
    *at += found + mark.len();
    page.get(*at..)
}

// The value of one named part of a mark, such as the title behind title=". The
// words of it are turned back into characters, for an engine writes its titles
// with the marks of a page around them.
fn quoted(text: &str, key: &str) -> Option<String> {
    let at = text.find(key)? + key.len();
    let rest = text.get(at..)?;
    let end = rest.find('"')?;
    Some(one_line(rest.get(..end)?))
}

// The words between where the reading stands and the mark that ends them.
fn words_till(text: &str, end_mark: &str) -> Option<String> {
    let end = text.find(end_mark)?;
    Some(text[..end].to_string())
}

// Words made one line, and cut at the length asked for. The words stand as they
// were written, marks and all.
fn flat(text: &str, most: usize) -> String {
    let kept = text.split_whitespace().collect::<Vec<_>>().join(" ");
    first_words(&kept, most)
}

// Words of a page made one line: the marks of the page behind them are turned
// back, the spaces of them go, and the line is cut at the length asked for.
fn one_line(text: &str) -> String {
    let kept = decode(text).split_whitespace().collect::<Vec<_>>().join(" ");
    first_words(&kept, MAX_PAGE_CHARS)
}

// The first words of a run, cut at the length asked for, with a mark that says
// the rest was left out.
fn first_words(text: &str, most: usize) -> String {
    let out: String = text.chars().take(most).collect();
    if out.chars().count() < text.chars().count() {
        return format!("{} ...", out);
    }
    out
}

// Words cut at the length asked for, at the end of a whole word.
fn cut_words(text: &str, most: usize) -> String {
    let mut out: String = text.chars().take(most).collect();
    if out.chars().count() == text.chars().count() {
        return out;
    }
    if let Some(at) = out.rfind(' ') {
        out.truncate(at);
    }
    out
}

// The words of the call, made into a thing with parts in it.
fn object(raw: &str) -> Option<Value> {
    let text = raw.trim();
    let cut = tools::extract_object(text).unwrap_or(text);
    serde_json::from_str::<Value>(cut)
        .ok()
        .filter(|value| value.is_object())
}

// The first of a list of names that holds words. A model writes "q" for the words
// to look for, or "uri" for an address, more often than one would think.
fn first(call: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(text) = call.get(*key).and_then(|v| v.as_str()) {
            if !text.trim().is_empty() {
                return text.trim().to_string();
            }
        }
    }
    String::new()
}

// An address made plain: the marks of a page behind it are turned back, and the
// spaces of it go, for an address has none.
fn clean(text: &str) -> String {
    let kept: String = decode(text).chars().filter(|ch| !ch.is_whitespace()).collect();
    first_words(&kept, 600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_needs_one_of_the_two_things() {
        let out = run("{}");
        assert!(!out.ok);
        assert!(out.output.contains("words to look for"));
    }

    #[test]
    fn a_mark_that_is_ours_and_one_that_is_not() {
        assert_eq!(decode("a &amp; b &lt; c &nbsp; d &rocks;"), "a & b < c   d &rocks;");
    }

    #[test]
    fn the_words_of_a_page_are_cut_of_their_marks() {
        let page = "<head><title>ti &amp; tle</title></head><h1>Lake</h1><p>a &amp; b</p><script>no</script><ul><li>one</li></ul>";
        assert_eq!(to_words(page), "Lake\na & b\n\n- one");
    }

    #[test]
    fn a_brave_answer_gives_its_places() {
        let page = r#"<div class="s" data-type="web"><a href="https://example.org/a"><div class="title x" title="A &amp; B">A &amp; B</div></a><div class="content">one &lt; two</div></div>"#;
        let found = brave_places(page).remove(0);
        assert_eq!(found.address, "https://example.org/a");
        assert_eq!(found.title, "A & B");
        assert_eq!(found.words, "one < two");
    }

    // The shape of a place of an answer of Brave, taken from the page of the
    // engine on the day this was written. The class of the mark that opens a
    // place carries a mark of the build of the engine, so the words that name a
    // build are not looked for, and the old shape of the block above is read as
    // well as this one.
    #[test]
    fn a_brave_answer_of_today_gives_its_places() {
        let page = concat!(
            r#"<div class="snippet svelte-jmfu5f" data-pos="0" data-type="web" data-keynav="true">"#,
            r#"<div class="result-body"><div class="result-content">"#,
            r#"<a href="https://en.wikipedia.org/wiki/Lake_Erie" target="_self">"#,
            r#"<div class="site-name-content"><div>Wikipedia</div></div>"#,
            r#"<div class="title search-snippet-title line-clamp-1" title="Lake Erie - Wikipedia">Lake Erie - Wikipedia</div></a>"#,
            r#"<div class="generic-snippet"><div class="content line-clamp-dynamic">"#,
            r#"<!--[0--><span class="t-secondary">6 days ago -</span><!--]--> "#,
            r#"<strong>Lake</strong> <strong>Erie</strong> is the fourth-largest lake.</div></div>"#,
            r#"</div></div></div>"#,
            r#"<div class="snippet svelte-jmfu5f" data-pos="1" data-type="web">"#,
            r#"<a href="https://www.britannica.com/place/Lake-Erie">"#,
            r#"<div class="title" title="Lake Erie">Lake Erie</div></a>"#,
            r#"<div class="content">Lake Erie, the fourth largest of North America's Great Lakes.</div>"#,
            r#"</div>"#,
        );
        let found = brave_places(page);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].address, "https://en.wikipedia.org/wiki/Lake_Erie");
        assert_eq!(found[0].title, "Lake Erie - Wikipedia");
        assert!(
            found[0].words.starts_with("6 days ago - Lake Erie is the"),
            "{}",
            found[0].words
        );
        assert_eq!(found[1].address, "https://www.britannica.com/place/Lake-Erie");
        assert_eq!(found[1].title, "Lake Erie");
    }

    // A page cut short in the middle of a programme of its own holds no words
    // past the place it was cut at, for the words of a programme are no words
    // for a reader. The menu of the house of a page, the tools at its side and
    // the words at its foot are left out whole.
    #[test]
    fn the_house_of_a_page_is_left_out_of_its_words() {
        let cut = "<p>words</p><script>var marks = {\"a\": \"b\"";
        assert_eq!(to_words(cut), "words");
        let house = "<nav><a>menu</a></nav><p>the words</p><aside>the side</aside><footer>the foot</footer>";
        assert_eq!(to_words(house), "the words");
    }

    // A mark of a page may carry the sign that seems to close it in the words of
    // a part of its own, for a page of today writes whole answers into the marks
    // it opens with. The words behind such a sign are no words of the page.
    #[test]
    fn a_mark_may_hold_the_sign_that_closes_another_mark() {
        let page = "<p>x <span data-mw='{\"wt\":\"a > b\"}'>and</span> y</p><p>words</p>";
        let made = to_words(page);
        assert_eq!(made, "x and y\nwords");
    }

    // The mark of a page may hold a sign of more than one byte, for a page is
    // written in the tongues of the world. The twelfth byte of the mark below
    // stands in the middle of a sign, and a mark of twelve bytes looked for by
    // its length alone would be cut out of it.
    #[test]
    fn a_mark_of_a_page_may_hold_a_sign_of_more_than_one_byte() {
        let head = "<p class=\"a\u{e9}\">x";
        assert!(!starts_any(head, BLOCK_ENDS));
        assert_eq!(to_words(head), "x");
    }

    #[test]
    fn a_duckduckgo_answer_gives_its_places() {
        let page = r##"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fx&amp;rut=1">Lake &amp; fish</a><a class="result__snippet" href="#">deep &lt; wide</a>"##;
        let found = duckduckgo_places(page).remove(0);
        assert_eq!(found.address, "https://example.org/x");
        assert_eq!(found.title, "Lake & fish");
        assert_eq!(found.words, "deep < wide");
    }

    #[test]
    fn the_encyclopedia_answer_gives_its_places() {
        let page = r#"{"query":{"search":[{"title":"Lake Erie","snippet":"a <b>big</b> lake"}]}}"#;
        let found = wiki_places(page).remove(0);
        assert_eq!(found.title, "Lake Erie");
        assert_eq!(found.address, "https://en.wikipedia.org/wiki/Lake_Erie");
        assert_eq!(found.words, "a big lake");
    }

    #[test]
    fn the_names_a_model_uses_for_its_two_parts() {
        assert_eq!(words_of("{\"q\":\"Lake Erie\"}").0, "Lake Erie");
        assert_eq!(words_of("{\"uri\":\"https://e.org\"}").1, "https://e.org");
        assert_eq!(partial_words("{\"query\":\"La"), "La");
    }

    #[test]
    fn every_place_of_the_answer_gives_its_three_things() {
        let places = vec![Place { title: "Lake".into(), address: "https://e.org".into(), words: "deep".into() }];
        let made = told(places, false).output;
        assert!(made.contains("Title: Lake\nURL: https://e.org\nWords: deep"), "{}", made);
        assert!(made.contains("---"), "{}", made);
    }
}


