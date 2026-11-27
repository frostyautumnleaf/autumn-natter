// The search tool: it looks an item up on the web, and reads a page of it.
//
// One name does two items. Given text to search for, the tool asks the engines in order and
// returns the titles, the addresses and the short text of the places it found. Given an
// address, it reads that page and returns its text, cut of their markers and truncated. The
// model is the user of both answers, and the model reads text, so nothing of the markers of a
// page is kept unless they say something.
//
// The form of these answers was first made in another program, whose code is under the AGPL.
// Nothing was copied. The engines this computer can reach are not the engines of that program
// either, so the reading of each answer was written again here, from what the engine actually
// sends back.
//
// No new part is added to the program for this. The text of a page is reads by a small user of
// its own, and the markers of a page are taken out by a rule of its own, so the binary keeps
// the weight it had.

use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;
use url::Url;

use crate::tools::{self, with_rule};

// The name the model uses to look an item up.
pub const NAME: &str = "web_search";

// The most bytes of one page that are read. A page of a news house may hold three times this in
// the markers of its header alone, and none of those markers are text.
const MAX_FETCH_BYTES: usize = 512 * 1024;
const MAX_BROWSER_FETCH_BYTES: usize = 1024 * 1024;

// The most text of a page, or of a whole answer, that reach the model. The size of the context
// is small, and the start of a page holds the best of it.
const MAX_PAGE_CHARS: usize = 16_000;

// The most places one search names.
const MAX_PLACES: usize = 6;

// The text between two places of the answer, and the text at the end that tell the model what
// to do with an address it likes.
const BETWEEN: &str = "\n\n---\n\n";
const LAST: &str = "\n\n---\n\nThese are short summaries of each result. To read a whole page, call again with its address, as {\"url\": \"https://...\"}.";

// The text that remains after a page of more text than this program sends.
const CUT: &str = "(the text of the page was cut here, because the page was longer than this program sends)";

// The engines, in the order they are worth asking. Two of the well known engines answer this
// computer with a test to prove that it is not a house of robots, so the engine that asks no
// such test comes first, and a failure of one engine is passed on quietly to the next. The
// second of them answers with such a test today as well, and it is kept as a second try for the
// day the test is removed.
const ENGINES: [&str; 2] = ["brave", "duckduckgo"];

// The marker that is in the tag opening one place of a Brave answer. The whole tag cannot be
// written out: the class of it carries a marker of the build of the engine, and that marker
// changes while the text of the page remains. The place of one answer therefore begins after
// the tag this marker is in, and ends where the next such tag remains.
const BRAVE_MARK: &str = "data-type=\"web\"";

// The address of the encyclopedia, which is the last place to look and holds text of one type
// only. It is named out loud when it answered, so the model does not take it for all of the
// web.
const ENCYCLOPEDIA: &str = "https://en.wikipedia.org/w/api.php?action=query&format=json&list=search&srlimit=6&srsearch=";
const ENCYCLOPEDIA_PAGE: &str = "https://en.wikipedia.org/wiki/";

// The text a browser of the computer reports, and the language to request. An engine that is
// not shown a browser answers with a test instead of an answer.
const AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

// The text of the call, and the text the user is shown.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": with_rule(String::from(
                "Search the web with `query` for ranked results and short summaries. To inspect a result, call again with its returned `url`; do not guess addresses. Page text is truncated, and nothing read is stored.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search terms or a question."
                    },
                    "url": {
                        "type": "string",
                        "description": "Returned page URL to read instead of searching."
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

// What the call named: the text to search for, and the address to read. A model writes a name
// other than the one that was requested more often than one would think, so the other names a
// local model reaches for are read as well.
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

// The one item the call named, as far as it has come. The user watches this while the call is
// written. An address is worth showing whole, for the text to search for may not have come yet.
pub fn partial_words(raw: &str) -> String {
    let text = raw.trim_start();
    let query = tools::partial_text(text, "query");
    if !query.is_empty() {
        return query;
    }
    tools::partial_text(text, "url")
}

// Look an item up, or read a page. The answer is the text the model is given.
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
                "The call needs text to search for, or the address of a page to read",
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
            "Nothing was found for \"{}\". {}",
            first_words(wanted, 120),
            trouble.join("; ")
        ),
    }
}

// The answer of a search, in the form the model reads: a place, then the text between two
// places, and at the end the word about reading a whole page.
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
            "The web engines could not be reached, or found nothing usable. The text below is from the encyclopedia, which is not current news.\n\n{}",
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

// Read one page of the web, and give back its text.
fn read_page(address: &str) -> tools::Report {
    if !address.starts_with("http://") && !address.starts_with("https://") {
        return tools::Report {
            ok: false,
            output: format!(
                "\"{}\" is not an address that can be read. An address starts with http:// or https://",
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
                    output: format!("the page at {} held no text to read", address),
                };
            }
            tools::Report {
                ok: true,
                output: format!("The text of {}:\n\n{}", address, words),
            }
        }
    }
}

// Ask one engine for the text of a search.
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

// The places of an answer of Brave. Every place of its own accord opens with a marker that
// shows what type of item it is.
fn brave_places(page: &str) -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();
    let mut from = 0usize;
    while out.len() < MAX_PLACES {
        let Some(here) = find_from(page, from, BRAVE_MARK) else {
            break;
        };
        // The text of the place remains after the tag the marker is in, and not after the
        // marker itself, which is in the middle of that tag.
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

// One place of a Brave answer: its address, its title, and its short text. A block that is not
// in the form that was searched for is given up on, and the next block is read.
fn brave_block(block: &str) -> Option<Place> {
    let mut at = 0usize;
    let rest = past(block, &mut at, "<a href=\"")?;
    let end = rest.find('"')?;
    let address = clean(rest.get(..end)?);
    if !address.starts_with("http") {
        return None;
    }
    // The title hangs on the marker that opened the part of it.
    let rest = past(block, &mut at, "<div class=\"title")?;
    let title = quoted(rest, "title=\"")?;
    // The short text is in a part of their own after the title.
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

// The places of a DuckDuckGo answer. The address of a place hides behind the address of the
// engine, and is set free before the model is given it.
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
        // The short text is in the link of their own after the title.
        let Some(mark) = find_from(page, close, "class=\"result__snippet\"") else {
            break;
        };
        let body = mark + "class=\"result__snippet\"".len();
        let Some(open) = find_from(page, body, ">") else { break };
        let begin = open + 1;
        let Some(close) = find_from(page, begin, "</a>") else { break };
        let said = words_of_marks(page.get(begin..close).unwrap_or(""));
        // The reading carries on after the short text, so the next place read is the next one,
        // and not the same one again.
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

// The place a marker is in, at or after a place we had read to, when the marker is in what was
// left.
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

// Ask the encyclopedia. It is not an engine of the whole web: its text is of one type, and the
// answer records it out loud when it was the one that answered.
fn encyclopedia(wanted: &str) -> Result<Vec<Place>, String> {
    let address = format!("{}{}", ENCYCLOPEDIA, urlencoding::encode(wanted));
    let page = fetch(&address)?;
    Ok(wiki_places(&page))
}

// The places of an answer of the encyclopedia. Its text is JSON, and the short text of a place
// hold markers of their own.
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

// Get one address, and read what returns. The bytes are stopped at the cap, so a page that goes
// on for ever does not hold the program.
fn fetch(address: &str) -> Result<String, String> {
    fetch_document(address, MAX_FETCH_BYTES).map(|(_, page)| page)
}

// Fetch one bounded public page and retain the final address after checked redirects. Browser
// previews use the same reader as search, so their navigation cannot bypass its URL checks.
fn fetch_document(address: &str, limit: usize) -> Result<(String, String), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(20))
        .redirects(0)
        .build();
    let mut current = Url::parse(address).map_err(|_| String::from("invalid URL"))?;
    for _ in 0..=5 {
        validate_public_url(&current)?;
        let response = agent.get(current.as_str())
            .set("user-agent", AGENT)
            .set("accept-language", "en")
            .call()
            .map_err(|problem| first_words(&problem.to_string(), 200))?;
        if (300..400).contains(&response.status()) {
            let location = response.header("Location").ok_or_else(|| String::from("redirect had no destination"))?;
            current = current.join(location).map_err(|_| String::from("redirect destination was invalid"))?;
            continue;
        }
        let mut bytes: Vec<u8> = Vec::new();
        response.into_reader().take(limit as u64).read_to_end(&mut bytes)
            .map_err(|problem| format!("The page could not be read: {problem}"))?;
        return Ok((current.to_string(), String::from_utf8_lossy(&bytes).to_string()));
    }
    Err(String::from("the page redirected too many times"))
}

// Never let a page reader reach the local machine or private network, including after redirects.
fn validate_public_url(url: &Url) -> Result<(), String> {
    use std::net::ToSocketAddrs;
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
        return Err(String::from("only public http and https addresses can be opened"));
    }
    let host = url.host_str().ok_or_else(|| String::from("the address has no host"))?;
    let low = host.trim_end_matches('.').to_ascii_lowercase();
    if low == "localhost" || low.ends_with(".localhost") || low.ends_with(".local")
        || low == "metadata.google.internal" || low.ends_with(".internal") {
        return Err(String::from("local network addresses are not available to web tools"));
    }
    let port = url.port_or_known_default().ok_or_else(|| String::from("the address has no port"))?;
    let addresses = (host, port).to_socket_addrs().map_err(|_| String::from("the host could not be resolved"))?;
    let mut found = false;
    for address in addresses {
        found = true;
        if !public_ip(address.ip()) {
            return Err(String::from(
                "local network addresses are not available to web tools",
            ));
        }
    }
    if !found { return Err(String::from("the host has no public address")); }
    Ok(())
}

/// Whether an address is globally routable enough for a browser or page-reader request.
pub fn public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_broadcast()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 192 && b == 0 && c == 0)
                && !(a == 192 && b == 0 && c == 2)
                && !(a == 192 && b == 88 && c == 99)
                && !(a == 198 && (b == 18 || b == 19 || b == 51 && c == 100))
                && !(a == 203 && b == 0 && c == 113)
        }
        std::net::IpAddr::V6(ip) => {
            let segments = ip.segments();
            let globally_routed = (segments[0] & 0xe000) == 0x2000;
            globally_routed
                && !ip.is_multicast()
                && !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && ip.to_ipv4_mapped().is_none()
                // Special-purpose 2001::/23 assignments and the 6to4 and documentation ranges.
                && !(segments[0] == 0x2001 && segments[1] <= 0x01ff)
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && segments[1] <= 0x0fff)
        }
    }
}

// The native browser shares the page reader's public-address rule for top-level navigation.
pub fn validate_public_browser_url(address: &str) -> Result<Url, String> {
    let url = Url::parse(address).map_err(|_| String::from("invalid URL"))?;
    validate_public_url(&url)?;
    Ok(url)
}

/// Parse a browser navigation without resolving DNS on the GUI thread. Actual requests still go
/// through the checked browser proxy, which resolves the host and rejects private destinations.
pub fn parse_public_browser_url(address: &str) -> Result<Url, String> {
    let url = Url::parse(address).map_err(|_| String::from("invalid URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(String::from("only public http and https addresses can be opened"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| String::from("the address has no host"))?;
    let low = host.trim_end_matches('.').to_ascii_lowercase();
    if low == "localhost"
        || low.ends_with(".localhost")
        || low.ends_with(".local")
        || low == "metadata.google.internal"
        || low.ends_with(".internal")
    {
        return Err(String::from("local network addresses are not available to web tools"));
    }
    if matches!(url.host(), Some(url::Host::Ipv4(_) | url::Host::Ipv6(_))) {
        validate_public_url(&url)?;
    }
    Ok(url)
}

// The text of a page of markers. The parts that hold no text for a user are dropped whole: the
// header of the page, the programmes of it, the styles of it, and the notes of the writer. A
// marker that ends a block of text leaves a new line behind, and the marker of a list leaves a
// dash. The rest of the markers go.
fn to_words(page: &str) -> String {
    let mut out = String::new();
    let mut rest = page;
    // A page of nothing but markers would go on for as long as it is long, so the reading has a
    // ceiling of its own.
    let mut guard = 0usize;
    while let Some(at) = rest.find('<') {
        out.push_str(&decode(&rest[..at]));
        guard += 1;
        if guard > 100_000 {
            break;
        }
        // A part with no text for a user is left out whole.
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

// A browser snapshot of a page and the links the model may choose to follow.
pub(crate) fn browser_read(address: &str) -> Result<(String, String, Vec<(String, String)>, String), String> {
    let (final_url, page) = fetch_document(address, MAX_BROWSER_FETCH_BYTES)?;
    let mut links = Vec::new();
    let mut from = 0usize;
    while links.len() < 30 {
        let Some(open) = find_ignore(&page[from..], "<a") else { break };
        let start = from + open;
        let Some(end) = mark_end(&page[start..]) else { break };
        let tag = &page[start..start + end + 1];
        let Some(href_at) = find_ignore(tag, "href") else { from = start + end + 1; continue };
        let after = tag.get(href_at + 4..).unwrap_or("").trim_start();
        let Some(after) = after.strip_prefix('=') else { from = start + end + 1; continue };
        let value = after.trim_start();
        let raw = if let Some(rest) = value.strip_prefix('"') { rest.split('"').next().unwrap_or("") }
            else if let Some(rest) = value.strip_prefix('\'') { rest.split('\'').next().unwrap_or("") }
            else { value.split_whitespace().next().unwrap_or("").trim_end_matches('>') };
        if raw.is_empty() || raw.starts_with('#') || raw.starts_with("javascript:") { from = start + end + 1; continue; }
        let label_start = start + end + 1;
        let label = find_ignore(&page[label_start..], "</a").map(|n| &page[label_start..label_start + n]).unwrap_or("");
        let label = first_words(&to_words(label), 160);
        if let Ok(base) = Url::parse(&final_url) {
            if let Ok(target) = base.join(raw) {
                links.push((if label.is_empty() { target.as_str().to_string() } else { label }, target.to_string()));
            }
        }
        from = start + end + 1;
    }
    Ok((final_url, to_words(&page), links, page))
}

// The parts of a page that hold nothing a user would want to read, with the marker that closes
// each of them. Markers of a page are written in either half of the alphabet, so each pair is
// searched for without minding the case.
//
// The markers of the site of a page is here as well: the menu of it, the tools at its side and
// the text at the bottom remain the same on every page of the site, and they are not the text
// the page was written about. The marker of a header is not among them, for many a page hangs
// the text of its front on one.
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

// The page after a part of it that holds no text, when the marker at the place requested opens
// such a part. Nothing is left when the marker that ends the part is not in the page at all,
// which is how a page truncated in the middle of a programme of its own is left remaining
// there. The text of such a programme is no text for a user, and reading it would fill the
// answer of them.
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
    drop_hidden_element(page, at)
}

// Hidden fallback markup is not part of the page a user sees. Pages such as GitHub's login
// page put a generic fragment-load error inside a hidden element, then replace that fragment
// in the browser. The fallback can also become visible when only that fragment fails, even
// though the rest of the page loaded successfully.
fn drop_hidden_element(page: &str, at: usize) -> Option<&str> {
    let head = page.get(at..)?;
    let end = mark_end(head)?;
    let opening = head.get(..=end)?;
    let (name, closing, self_closing) = html_tag(opening)?;
    let hidden = has_html_attribute(opening, "hidden");
    let fallback = has_html_attribute(opening, "data-show-on-forbidden-error");
    if closing || !(hidden || fallback) {
        return None;
    }
    if self_closing || is_void_element(name) {
        return head.get(end + 1..);
    }

    let mut depth = 1usize;
    let mut offset = end + 1;
    while let Some(rest) = page.get(at + offset..) {
        let Some(next) = rest.find('<') else {
            return Some("");
        };
        let start = offset + next;
        let marker = page.get(at + start..)?;
        let Some(marker_end) = mark_end(marker) else {
            return Some("");
        };
        let Some(tag) = marker.get(..=marker_end) else {
            return Some("");
        };
        if let Some((found, is_closing, is_self_closing)) = html_tag(tag) {
            if found.eq_ignore_ascii_case(name) {
                if is_closing {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return page.get(at + start + marker_end + 1..);
                    }
                } else if !is_self_closing && !is_void_element(found) {
                    depth += 1;
                }
            }
        }
        offset = start + marker_end + 1;
    }
    Some("")
}

fn html_tag(tag: &str) -> Option<(&str, bool, bool)> {
    let end = mark_end(tag)?;
    let mut at = 1usize;
    let closing = tag.as_bytes().get(at) == Some(&b'/');
    if closing {
        at += 1;
    }
    while tag
        .as_bytes()
        .get(at)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        at += 1;
    }
    let start = at;
    while tag.as_bytes().get(at).is_some_and(|byte| {
        !byte.is_ascii_whitespace() && *byte != b'/' && *byte != b'>'
    }) {
        at += 1;
    }
    let name = tag.get(start..at)?;
    if name.is_empty() {
        return None;
    }
    let self_closing = !closing && tag.get(..end)?.trim_end().ends_with('/');
    Some((name, closing, self_closing))
}

fn has_html_attribute(opening: &str, wanted: &str) -> bool {
    let Some(end) = mark_end(opening) else {
        return false;
    };
    let Some((_, closing, _)) = html_tag(opening) else {
        return false;
    };
    if closing {
        return false;
    }
    let bytes = opening.as_bytes();
    let mut at = 1usize;
    while bytes
        .get(at)
        .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'/')
    {
        at += 1;
    }
    while bytes.get(at).is_some_and(|byte| {
        !byte.is_ascii_whitespace() && *byte != b'/' && *byte != b'>'
    }) {
        at += 1;
    }
    while at < end {
        while bytes
            .get(at)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'/')
        {
            at += 1;
        }
        let start = at;
        while bytes.get(at).is_some_and(|byte| {
            !byte.is_ascii_whitespace() && *byte != b'=' && *byte != b'/' && *byte != b'>'
        }) {
            at += 1;
        }
        if at == start {
            at += 1;
            continue;
        }
        if opening.get(start..at).is_some_and(|name| name.eq_ignore_ascii_case(wanted)) {
            return true;
        }
        while bytes
            .get(at)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            at += 1;
        }
        if bytes.get(at) == Some(&b'=') {
            at += 1;
            while bytes
                .get(at)
                .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                at += 1;
            }
            if bytes
                .get(at)
                .is_some_and(|byte| *byte == b'\'' || *byte == b'"')
            {
                let quote = bytes[at];
                at += 1;
                while at < end && bytes.get(at) != Some(&quote) {
                    at += 1;
                }
                if at < end {
                    at += 1;
                }
            } else {
                while bytes
                    .get(at)
                    .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'>')
                {
                    at += 1;
                }
            }
        }
    }
    false
}

fn is_void_element(name: &str) -> bool {
    [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
        "param", "source", "track", "wbr",
    ]
        .iter()
        .any(|void| name.eq_ignore_ascii_case(void))
}

// The markers that end a block of text, and the markers that open a part of a list.
const BLOCK_ENDS: &[&str] = &[
    "</p", "</div", "<br", "</li", "</h1", "</h2", "</h3", "</h4", "</h5", "</h6", "</tr",
    "</table", "</section", "</article", "</blockquote", "</header", "</footer", "</nav",
    "<hr", "</figure", "</pre", "</dl", "</ul", "</ol",
];
const LIST_MARKS: &[&str] = &["<li", "<dt", "<dd", "<tr"];

// True when the marker at the start of the text is the name that was requested, and that name
// is all of the word. The marker of the header of a page is not the marker of a header of it,
// and reading one for the other would delete the text of the page.
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

// True when the text at the front begin with any of the markers. The front is taken with an
// appearance that minds the end of a character: the text of a page may begin with a sign of
// more than one byte, and cutting a marker out of the middle of one would stop the program.
fn starts_any(head: &str, marks: &[&str]) -> bool {
    marks.iter().any(|mark| {
        head.get(..mark.len())
            .is_some_and(|front| front.eq_ignore_ascii_case(mark))
    })
}

// The place a run of text is in, without minding the case of it. Markers of a page are written
// in both halves of the alphabet by different houses.
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

// The text of a page, prepared: the spaces of the markers go, many new lines in a row become
// two, and the text of one line remains together.
fn tidy_words(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let kept = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if kept.is_empty() {
            // A line of nothing is worth keeping between two parts, and nowhere else.
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

// The text with the markers of the page turned back into the characters they mean. A marker
// that is not known remains as it was written, which reads better than a word that went
// missing.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        // A name of a marker is short. A longer run of text behind an ampersand is an
        // ampersand, and not a marker.
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
                // A marker that is not one of ours remains as it was written, for a word with a
                // hole in it reads worse than one with a strange sign.
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

// The character a marker of a page means. The short text of a page is full of the markers that
// no key of a keyboard has.
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

// The end of a marker of a page, counted from the sign that opened it. A page of today carries
// whole answers of its own in the text between two quotes of a marker, and such a run may hold
// the sign that seems to close the marker. Reading the marker as ended there would pour the
// text of the attribute into the answer.
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
    // A marker whose quote never ends is read to its first sign, which is what a browser makes
    // of the text between the quotes.
    text.find('>')
}

// The text of a part of a page that still holds markers: the markers go, and what is left is
// one line of text.
fn words_of_marks(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        out.push_str(&decode(&rest[..at]));
        // The marker ends at the sign that closes it, and not at a sign before where it opened,
        // which would eat text that was no marker.
        let Some(gt) = mark_end(&rest[at..]) else { break };
        rest = &rest[at + gt + 1..];
    }
    if !rest.is_empty() {
        out.push_str(&decode(rest));
    }
    flat(&out, MAX_PAGE_CHARS)
}

// Move a place in a page past a marker, and give back what is left after it. Nothing returns
// when the marker is not in what was left, which is how a page of a form that was not expected
// is given up on.
fn past<'a>(page: &'a str, at: &mut usize, mark: &str) -> Option<&'a str> {
    let rest = page.get(*at..)?;
    let found = rest.find(mark)?;
    *at += found + mark.len();
    page.get(*at..)
}

// The value of one named part of a marker, such as the title behind title=". The text of it is
// turned back into characters, for an engine writes its titles with the markers of a page
// around them.
fn quoted(text: &str, key: &str) -> Option<String> {
    let at = text.find(key)? + key.len();
    let rest = text.get(at..)?;
    let end = rest.find('"')?;
    Some(one_line(rest.get(..end)?))
}

// The text between where the reading stops and the marker that ends it.
fn words_till(text: &str, end_mark: &str) -> Option<String> {
    let end = text.find(end_mark)?;
    Some(text[..end].to_string())
}

// Text made one line, and cut at the length requested. The text remains as they were written,
// markers and all.
fn flat(text: &str, most: usize) -> String {
    let kept = text.split_whitespace().collect::<Vec<_>>().join(" ");
    first_words(&kept, most)
}

// The page text flattened to one line: the markup around it is dropped, runs of spaces are
// collapsed, and the line is cut to the length requested.
fn one_line(text: &str) -> String {
    let kept = decode(text).split_whitespace().collect::<Vec<_>>().join(" ");
    first_words(&kept, MAX_PAGE_CHARS)
}

// The first text of a run, cut at the length requested, with a marker that shows the rest was
// left out.
fn first_words(text: &str, most: usize) -> String {
    let out: String = text.chars().take(most).collect();
    if out.chars().count() < text.chars().count() {
        return format!("{} ...", out);
    }
    out
}

// Text cut at the length requested, at the end of a whole word.
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

// The text of the call, made into an item with parts in it.
fn object(raw: &str) -> Option<Value> {
    let text = raw.trim();
    let cut = tools::extract_object(text).unwrap_or(text);
    serde_json::from_str::<Value>(cut)
        .ok()
        .filter(|value| value.is_object())
}

// The first of a list of names that holds text. A model writes "q" for the text to search for,
// or "uri" for an address, more often than one would think.
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

// An address made plain: the markers of a page behind it are turned back, and the spaces of it
// go, for an address has none.
fn clean(text: &str) -> String {
    let kept: String = decode(text).chars().filter(|ch| !ch.is_whitespace()).collect();
    first_words(&kept, 600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_navigation_only_accepts_public_http_addresses() {
        assert_eq!(
            parse_public_browser_url("https://example.org/path?q=one")
                .unwrap()
                .as_str(),
            "https://example.org/path?q=one"
        );
        for address in [
            "file:///etc/passwd",
            "ftp://example.org/",
            "http://localhost/",
            "http://service.local/",
            "http://127.0.0.1/",
            "http://192.168.1.1/",
            "http://[::1]/",
            "https://user:secret@example.org/",
        ] {
            assert!(parse_public_browser_url(address).is_err(), "{address}");
        }
        assert!(public_ip("2001:4860:4860::8888".parse().unwrap()));
        assert!(!public_ip("64:ff9b::c0a8:101".parse().unwrap()));
        assert!(!public_ip("2001:db8::1".parse().unwrap()));
        assert!(!public_ip("192.0.0.1".parse().unwrap()));
    }

    #[test]
    fn the_call_needs_one_of_the_two_things() {
        let out = run("{}");
        assert!(!out.ok);
        assert!(out.output.contains("text to search for"));
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

    // The form of a place of an answer of Brave, taken from the page of the engine on the day
    // this was written. The class of the marker that opens a place carries a marker of the
    // build of the engine, so the text that name a build is not searched for, and the old form
    // of the block above is read as well as this one.
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

    // A page truncated in the middle of a programme of its own holds no text past the place it
    // was cut at, for the text of a programme is no text for a user. The menu of the site of a
    // page, the tools at its side and the text at the bottom are left out whole.
    #[test]
    fn the_house_of_a_page_is_left_out_of_its_words() {
        let cut = "<p>words</p><script>var marks = {\"a\": \"b\"";
        assert_eq!(to_words(cut), "words");
        let house = "<nav><a>menu</a></nav><p>the words</p><aside>the side</aside><footer>the foot</footer>";
        assert_eq!(to_words(house), "the words");
    }

    // A marker of a page may carry the sign that seems to close it in the text of a part of its
    // own, for a page of today writes whole answers into the markers it opens with. The text
    // behind such a sign are no text of the page.
    #[test]
    fn a_mark_may_hold_the_sign_that_closes_another_mark() {
        let page = "<p>x <span data-mw='{\"wt\":\"a > b\"}'>and</span> y</p><p>words</p>";
        let made = to_words(page);
        assert_eq!(made, "x and y\nwords");
    }

    // The marker of a page may hold a sign of more than one byte, for a page is written in the
    // tongues of the world. The twelfth byte of the marker below is in the middle of a sign,
    // and a marker of twelve bytes searched for by its length alone would be cut out of it.
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


