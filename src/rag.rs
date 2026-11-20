// The sources of retrieval: the folder they live in, the index of their vectors, and the search
// of it.
//
// RAG is one tool, `search_sources`, and one folder, `.sources`. The folder is inside the working
// directory of the chat - the directory of the repository, or `work/<chat id>/` for a loose chat
// - beside `.attachments`, and it is hidden for the same reason: it holds files of the program's
// own in a directory a model works in. The tree the user dropped is kept as it was, so a source
// that arrived as `docs/api/auth.md` is still that path when the model reads it back.
//
// The vectors are not kept in the work directory. They are rows in `sources.db` under the data
// folder of the program, keyed by the path of the working directory, exactly as the tasks are in
// `tasks.db` (see `crate::tasks`). A command a model runs can then read the sources, which is
// what a source is for, and cannot corrupt the index of them.
//
// A file that is stored is useful even when nothing is installed to count its words: it is on the
// disk, the model can read it by name, and it is embedded later, in a pass of its own, as soon as
// an embedding model is installed. The state of each file - stored, counted, or failed, with the
// reason - is a row, and the pop-up of the sources shows it.
//
// The vectors are stored normalised (see `crate::embed`), so the dot product of two of them is
// their cosine similarity and the search is a scan of the chunks of one working directory. A
// directory with a few thousand chunks is searched in a few milliseconds, which is why no index
// of vectors is needed.
//
// The search returns the path of each piece it found, its rank, and the text of the piece. A
// model that wants more of a source than one piece reads the file itself with the tools it
// already has, which is why the pieces are kept short.

use rusqlite::params;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::attach;
use crate::config;
use crate::db;
use crate::documents;
use crate::embed;
use crate::proglog;
use crate::tasks;
use crate::tools;

// The name the model calls the tool by.
pub const NAME: &str = "search_sources";

// The name of the folder of the sources, inside the working directory of a chat.
pub const FOLDER: &str = ".sources";

// One piece of a source, in the index. A piece is a few paragraphs: long enough to answer a
// question in one piece, short enough that a dozen pieces of a dozen files fit beside the
// conversation of a chat.
pub const CHUNK_CHARS: usize = 900;

// The most pieces of one file that go into the index. A file of more is indexed as far as this
// and the rest is left out, which is reported in the row of the file.
pub const GREATEST_CHUNKS: usize = 2000;

// The most text of one file that is read out of it, in characters. A file of more is read as far
// as this.
pub const GREATEST_CHARS: usize = 2_000_000;

// The most bytes of one file that the program takes from the page. A source of more is refused
// with the reason, rather than cut without a word about it.
pub const GREATEST_BYTES: u64 = 64 * 1024 * 1024;

// The most places one search answers with.
pub const GREATEST_PLACES: usize = 12;
pub const DEFAULT_PLACES: usize = 6;

// The most files one call of the page may carry. The pop-up sends a folder a few files at a
// time, so a big folder is a few calls and not one enormous one.
pub const GREATEST_FILES: usize = 64;

// The deepest a path inside the folder may be, in parts. A drop of a directory of a hundred
// levels is a mistake or an attack, and either way it is not put into the index.
pub const GREATEST_DEPTH: usize = 16;

// True for the name of the tool of the suite.
pub fn is_tool(name: &str) -> bool {
    name == NAME
}

// What turns texts into vectors. A trait, so the rules of the index - how a file is stored, how
// its text is cut into pieces, how the places are ranked - can be tried without a model.
pub trait Embedder: Send + Sync {
    // The vectors of a list of texts, in the order the texts were given.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;

    // True when the model needed for the counting is on this computer. The search says so when
    // it is not, rather than failing at the first ask.
    fn ready(&self) -> bool {
        true
    }

    // True when a vision projector is with the model, which is when the contents of an image are
    // counted as well as the words of a page.
    fn vision(&self) -> bool {
        false
    }

    // The vector of one image. An error when there is no projector, which is the usual case.
    fn embed_image(&self, _name: &str, _bytes: &[u8]) -> Result<Vec<f32>, String> {
        Err(String::from(
            "The embedding model has no vision projector, so the contents of an image are not counted",
        ))
    }
}

// The embedder the settings ask for. Nothing is started here: the server that counts the words
// starts when a text is actually counted.
pub fn embedder_for(cfg: &config::Config) -> Option<Arc<dyn Embedder>> {
    if cfg.llama_dir.trim().is_empty() {
        return None;
    }
    let binary = config::server_in(Path::new(&cfg.llama_dir))?;
    Some(embed::server(
        &binary,
        &cfg.embedding_model,
        &cfg.embedding_quant,
        &cfg.embedding_mmproj,
    ))
}

// ---------------------------------------------------------------- the folder

// The folder the sources of one working directory live in.
pub fn folder_in(workdir: &Path) -> PathBuf {
    workdir.join(FOLDER)
}

// Make the folder of the sources. It is asked for when the user opens the pop-up of the sources
// and when the first file arrives, so a repository that has none never holds an empty hidden
// folder.
pub fn make_folder(workdir: &Path) -> PathBuf {
    let place = folder_in(workdir);
    let _ = fs::create_dir_all(&place);
    place
}

// The key the sources of one working directory are under in `sources.db`. It is the key of the
// tasks of the same directory, so the two lists of one directory are keyed the same way and a
// directory that is removed is removed from both.
pub fn key_of(workdir: &Path) -> String {
    tasks::key_of(workdir)
}

// The path of a source, as the page named it, made safe to write. The separators are turned
// around, a part that goes up out of the folder is refused, and so is an absolute path or an
// empty one. The path the user dropped is kept in every other way, so the tree of a folder that
// was dropped stays the tree that is indexed.
pub fn tidy_path(raw: &str) -> Option<String> {
    let words = raw.replace('\\', "/");
    if words.is_empty() || words.len() > 400 {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in words.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        // A part that goes up, a part with a colon in it (a drive letter, or a name of a stream
        // of NTFS), and a part with a control character in it are all refused.
        if part == ".." || part.contains(':') {
            return None;
        }
        if part.chars().any(|one| (one as u32) < 0x20) {
            return None;
        }
        parts.push(part);
        if parts.len() > GREATEST_DEPTH {
            return None;
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

// ------------------------------------------------------------------- database

// The database the index of every working directory is in.
fn database() -> Result<Arc<db::Db>, String> {
    db::open(&config::paths().data.join(db::SOURCES_DB), db::Kind::Sources)
}

// One file of the sources, as its row holds it.
#[derive(Clone, Debug, Default)]
pub struct FileRow {
    pub path: String,
    // "text", "document", "image" or "other". The word is the one `kind_of` gives, and it is a
    // String here because it is read back out of a column.
    pub kind: String,
    pub bytes: i64,
    pub words: i64,
    pub chunks: i64,
    // True when the pieces of the file are counted. A file that is stored and not counted is
    // still read by a model that is told its name, and it is counted later.
    pub counted: bool,
    // The reason the file is not counted, when it is not.
    pub fault: String,
    pub updated: i64,
}

// One piece of a source, out of the index.
struct Chunk {
    path: String,
    text: String,
    vector: Vec<f32>,
}

// The rows of one working directory, in the order of their paths.
fn rows(work: &str) -> Vec<FileRow> {
    let Ok(kept) = database() else {
        return Vec::new();
    };
    kept.read(|conn| {
        let mut said = conn
            .prepare(
                "SELECT path, kind, bytes, words, chunks, counted, fault, updated FROM files
                 WHERE work = ?1 ORDER BY path ASC",
            )
            .map_err(db::fault)?;
        let made = said
            .query_map(params![work], |row| {
                Ok(FileRow {
                    path: row.get(0)?,
                    kind: row.get(1)?,
                    bytes: row.get(2)?,
                    words: row.get(3)?,
                    chunks: row.get(4)?,
                    counted: db::mark_of(row.get(5)?),
                    fault: row.get(6)?,
                    updated: row.get(7)?,
                })
            })
            .map_err(db::fault)?;
        Ok(made.filter_map(|one| one.ok()).collect::<Vec<FileRow>>())
    })
    .unwrap_or_default()
}

// How many files, pieces and uncounted files one working directory has. The badge of the chat
// reports these, so a user can see that a source is there before they ask about it. One query of
// three numbers, for the badge asks on every look of the state.
pub fn counts(work: &str) -> (usize, usize, usize) {
    let Ok(kept) = database() else {
        return (0, 0, 0);
    };
    kept.read(|conn| {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(chunks), 0),
                    COALESCE(SUM(CASE WHEN counted = 0 AND fault = '' THEN 1 ELSE 0 END), 0)
             FROM files WHERE work = ?1",
            params![work],
            |row| Ok((row.get::<_, i64>(0)? as usize, row.get::<_, i64>(1)? as usize, row.get::<_, i64>(2)? as usize)),
        )
        .map_err(db::fault)
    })
    .unwrap_or((0, 0, 0))
}

// How many files of one working directory are counted. The account of the sources reports this,
// so a model is told whether a search of the folder can answer at all.
pub fn counted_files(work: &str) -> usize {
    let Ok(kept) = database() else {
        return 0;
    };
    kept.read(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM files WHERE work = ?1 AND counted = 1",
            params![work],
            |row| row.get::<_, i64>(0).map(|one| one as usize),
        )
        .map_err(db::fault)
    })
    .unwrap_or(0)
}

// The working directories that have files stored and not counted. The key of a directory is its
// path, so a key names the folder its sources are in. This is what the counting that follows the
// install of an embedding model works through.
pub fn waiting_work() -> Vec<String> {
    let Ok(kept) = database() else {
        return Vec::new();
    };
    kept.read(|conn| {
        let mut said = conn
            .prepare("SELECT DISTINCT work FROM files WHERE counted = 0 AND fault = '' ORDER BY work ASC")
            .map_err(db::fault)?;
        let made = said
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db::fault)?;
        Ok(made.filter_map(|one| one.ok()).collect::<Vec<String>>())
    })
    .unwrap_or_default()
}

// The pieces of one working directory, with their vectors.
fn chunks(work: &str) -> Vec<Chunk> {
    let Ok(kept) = database() else {
        return Vec::new();
    };
    kept.read(|conn| {
        let mut said = conn
            .prepare("SELECT path, text, vector FROM chunks WHERE work = ?1 ORDER BY path ASC, idx ASC")
            .map_err(db::fault)?;
        let made = said
            .query_map(params![work], |row| {
                let blob: Vec<u8> = row.get(2)?;
                Ok(Chunk {
                    path: row.get(0)?,
                    text: row.get(1)?,
                    vector: numbers_of(&blob),
                })
            })
            .map_err(db::fault)?;
        Ok(made.filter_map(|one| one.ok()).collect::<Vec<Chunk>>())
    })
    .unwrap_or_default()
}

// The bytes of one vector, as they are written into a column.
fn bytes_of(vector: &[f32]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(vector.len() * 4);
    for one in vector {
        out.extend_from_slice(&one.to_le_bytes());
    }
    out
}

// A vector out of the bytes of a column. A row whose length is not a whole number of numbers is
// left out by the search rather than guessed at.
fn numbers_of(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|one| f32::from_le_bytes([one[0], one[1], one[2], one[3]]))
        .collect()
}

// ------------------------------------------------------------------- the text

// What kind of file a source is, in the word its row holds.
pub fn kind_of(name: &str, mime: &str) -> &'static str {
    if attach::is_image(mime, name) {
        "image"
    } else if documents::is_document(name, mime) {
        "document"
    } else if attach::is_text(mime, name) {
        "text"
    } else {
        "other"
    }
}

// The text of a source, if it has any that the counting is worth. `None` means the file has no
// text to cut into pieces: an image, which the vision projector reads, or a file of a type that
// holds nothing a model could read.
pub fn words_of(kind: &str, name: &str, mime: &str, bytes: &[u8]) -> Option<String> {
    match kind {
        "document" => documents::words_of(name, mime, bytes),
        "text" => {
            let made = String::from_utf8_lossy(bytes);
            let cut = cut_chars(&made, GREATEST_CHARS);
            if cut.trim().is_empty() {
                None
            } else {
                Some(cut)
            }
        }
        // A file of a type nothing is known about is put in the folder and left alone: a model
        // that is told its name can do with it what it can.
        _ => None,
    }
}

// The first so many characters of a text, in a way that never cuts a character in two.
fn cut_chars(words: &str, most: usize) -> String {
    words.chars().take(most).collect()
}

// One line out of a text, for the header of the block of a call.
fn one_line(words: &str, most: usize) -> String {
    let made = words.replace(['\n', '\r'], " ").trim().to_string();
    if made.chars().count() > most {
        format!("{}...", cut_chars(&made, most))
    } else {
        made
    }
}

// The text of a source, cut into pieces. Paragraphs are packed until a piece is as long as
// `CHUNK_CHARS`; a paragraph longer than that is cut between words. The order of the pieces is
// the order of the text, so a model that reads three pieces of one file reads them one after
// another.
pub fn chunk_words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut piece = String::new();
    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if para.chars().count() > CHUNK_CHARS {
            if !piece.is_empty() {
                out.push(std::mem::take(&mut piece));
            }
            out.extend(cut_long(para));
            continue;
        }
        if piece.chars().count() + para.chars().count() + 2 > CHUNK_CHARS && !piece.is_empty() {
            out.push(std::mem::take(&mut piece));
        }
        if !piece.is_empty() {
            piece.push('\n');
            piece.push('\n');
        }
        piece.push_str(para);
    }
    if !piece.is_empty() {
        out.push(piece);
    }
    out.truncate(GREATEST_CHUNKS);
    out
}

// A paragraph longer than one piece, cut between words.
fn cut_long(para: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut piece = String::new();
    for word in para.split_whitespace() {
        if piece.chars().count() + word.chars().count() + 1 > CHUNK_CHARS && !piece.is_empty() {
            out.push(std::mem::take(&mut piece));
        }
        if !piece.is_empty() {
            piece.push(' ');
        }
        piece.push_str(word);
    }
    if !piece.is_empty() {
        out.push(piece);
    }
    out
}

// ---------------------------------------------------------------- the tool

// The text of the call, and the text the user is shown.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": tools::with_rule(String::from(
                "Search the sources of this working directory for text that answers a question. The sources are the files the user put in the hidden .sources folder - documents, pages of writing, tables, code, and with a vision projector, pictures. The search counts words, so it finds the places that talk about the thing asked for even when they do not use the same words as the question. It returns the path of each place, its rank, and the text of it. Nothing of the conversation is looked at, and nothing is stored.",
            )),
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The question or the words to look for in the sources."
                    },
                    "places": {
                        "type": "integer",
                        "description": "How many places to return, from 1 to 12. Six are returned when nothing is given."
                    }
                },
                "required": ["query"]
            }
        }
    })
}

// What the call named: the text to search for, and how many places to answer with. The other
// names a local model reaches for are read as well.
pub fn asked_of(raw: &str) -> (String, usize) {
    let Some(call) = serde_json::from_str::<Value>(raw).ok() else {
        // The call is still being written. The words of the question so far are worth showing,
        // and there is nothing to search for yet.
        let part = tools::partial_text(raw.trim_start(), "query");
        return (part, DEFAULT_PLACES);
    };
    let query = first_text(&call, &["query", "q", "text", "search", "question"]);
    let most = ["places", "limit", "top_k", "k"]
        .iter()
        .filter_map(|name| call.get(*name))
        .find_map(|one| {
            one.as_u64()
                .or_else(|| one.as_str().and_then(|words| words.parse::<u64>().ok()))
        })
        .map(|one| one.clamp(1, GREATEST_PLACES as u64) as usize)
        .unwrap_or(DEFAULT_PLACES);
    (query, most)
}

// The one item the call named, as far as it has come.
pub fn partial_words(raw: &str) -> String {
    let (query, _) = asked_of(raw);
    one_line(&query, 120)
}

// The first field of a call that holds one of these names.
fn first_text(call: &Value, names: &[&str]) -> String {
    for name in names {
        if let Some(one) = call.get(*name) {
            let words = match one {
                Value::String(one) => one.clone(),
                Value::Number(one) => one.to_string(),
                _ => String::new(),
            };
            if !words.trim().is_empty() {
                return words;
            }
        }
    }
    String::new()
}

// Run a call of the tool. The embedder is what the settings ask for, or nothing when the program
// has no llama-server to count with.
pub fn run(work: &str, embedder: Option<&dyn Embedder>, arguments: &str) -> tools::Report {
    let (query, most) = asked_of(arguments);
    if query.trim().is_empty() {
        return tools::Report {
            ok: false,
            output: String::from(
                "No text to search for was given. Write a question or the words to look for.",
            ),
        };
    }
    let Some(worker) = embedder else {
        return tools::Report {
            ok: false,
            output: String::from(
                "The sources cannot be searched: llama-server was not found, so nothing can count the words of a source. Its directory is set in the settings.",
            ),
        };
    };
    if !worker.ready() {
        return tools::Report {
            ok: false,
            output: String::from(
                "The sources cannot be searched: no embedding model is installed. Choose one in the settings of the program and install it.",
            ),
        };
    }
    let asked = match worker.embed(&[query.trim().to_string()]) {
        Ok(list) if !list.is_empty() => list[0].clone(),
        Ok(_) => {
            return tools::Report {
                ok: false,
                output: String::from(
                    "The search gave no vector for that text. Ask again in a few words.",
                ),
            }
        }
        Err(problem) => {
            return tools::Report {
                ok: false,
                output: format!("The search could not count the question: {}", problem),
            }
        }
    };
    let held = chunks(work);
    if held.is_empty() {
        return tools::Report {
            ok: true,
            output: String::from(
                "There is nothing indexed in the .sources folder of this working directory. The files may be stored but not counted yet; the sources panel counts them once an embedding model is installed.",
            ),
        };
    }
    let mut found: Vec<(f32, &Chunk)> = held
        .iter()
        .map(|one| (similar(&asked, &one.vector), one))
        .collect();
    found.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    found.truncate(most);
    let least = found.first().map(|one| one.0).unwrap_or(0.0);
    let mut out = String::new();
    out.push_str(&format!(
        "The sources of this working directory answer with {} of {} pieces.\n\n",
        found.len(),
        held.len()
    ));
    for (at, (score, one)) in found.iter().enumerate() {
        out.push_str(&format!(
            "{}. {} (match {:.2})\n{}\n\n",
            at + 1,
            one.path,
            score,
            one.text.trim()
        ));
    }
    if least <= 0.0 {
        out.push_str("None of these answers the question. The pieces are named because the folder holds nothing else; read the file itself if it looks likely.\n\n");
    }
    out.push_str("To read more of one source, open the file named: it is in .sources of the working directory.");
    tools::Report {
        ok: true,
        output: out.trim_end().to_string(),
    }
}

// How much two vectors agree. Both are the length one, so this is the cosine of the angle
// between them.
pub fn similar(a: &[f32], b: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for (one, other) in a.iter().zip(b.iter()) {
        sum += one * other;
    }
    sum
}

// The text that goes with the tool. It is sent only while RAG is on, and it says what the tool
// is for and where the files themselves are, so the model does not have to guess whether to read
// a source whole.
pub fn system_prompt(files: usize, counted: usize, vision: bool) -> String {
    let state = if files == 0 {
        String::from(
            "Nothing has been put into the .sources folder yet, so a search of it answers with nothing.",
        )
    } else if counted == 0 {
        format!(
            "There are {} files in the folder and none of them is counted yet, so a search of them answers with the start of the folder and nothing more.",
            files
        )
    } else {
        format!(
            "There are {} files in the folder and {} of them are counted, so a search of them answers with the places that talk about what was asked.",
            files, counted
        )
    };
    let images = if vision {
        String::from(
            " The embedding model of this chat has a vision projector, so the contents of a picture in the folder are counted as well.",
        )
    } else {
        String::new()
    };
    format!(
        "Retrieval is on for this chat. The tool `search_sources` searches the hidden folder `.sources` of the working directory you are in, and returns the path of each place it found, its rank, and the text of it. Use it before you say that something is not known to you, whenever the question is about the material of this working directory rather than about the conversation.{}\n\
         {}.\n\
         The files themselves are in `.sources/`, with the tree the user gave them kept: a source named `docs/api/auth.md` in an answer is the file `.sources/docs/api/auth.md`, and you may read it whole with the tools you already have when one piece is not enough.\n\
         Quote what a source says rather than what you remember of its subject, and name the path of the source you took a thing from. When a search returns nothing useful, say what you searched for, so the user can tell you where to look instead.",
        images, state
    )
}

// ------------------------------------------------------------------- storing

// One file the page sent: the path it keeps inside the folder, its type, and its bytes as a data
// URL.
#[derive(Clone, Debug, Default)]
pub struct Given {
    pub path: String,
    pub mime: String,
    pub data: String,
}

// The files of a call of the page. A file whose path is refused, or whose bytes are not a data
// URL, or which is bigger than the program takes, is left out and reported in the note the job
// ends with.
pub fn parse_given(value: &Value) -> (Vec<Given>, Vec<String>) {
    let mut given: Vec<Given> = Vec::new();
    let mut refused: Vec<String> = Vec::new();
    let Some(list) = value.get("files").and_then(|one| one.as_array()) else {
        return (given, refused);
    };
    for one in list.iter().take(GREATEST_FILES) {
        let raw = one
            .get("path")
            .and_then(|one| one.as_str())
            .unwrap_or_default();
        let Some(path) = tidy_path(raw) else {
            refused.push(format!("{} is not a path this program will write", short_of(raw)));
            continue;
        };
        let data = one
            .get("data")
            .and_then(|one| one.as_str())
            .unwrap_or_default()
            .to_string();
        if data.is_empty() {
            refused.push(format!("{} came with no bytes", path));
            continue;
        }
        let mime = one
            .get("mime")
            .and_then(|one| one.as_str())
            .unwrap_or("application/octet-stream")
            .to_string();
        given.push(Given { path, mime, data });
    }
    if list.len() > GREATEST_FILES {
        refused.push(format!(
            "Only {} files are taken in one call, and {} were sent. Send the rest again.",
            GREATEST_FILES,
            list.len()
        ));
    }
    (given, refused)
}

// A path the user wrote, cut short, for a text about a file that was refused.
fn short_of(words: &str) -> String {
    let made = words.replace('\n', " ");
    if made.chars().count() > 80 {
        format!("{}...", made.chars().take(80).collect::<String>())
    } else if made.is_empty() {
        String::from("(no path)")
    } else {
        made
    }
}

// The job that is going on: the storing of a drop of files, or the counting of the files that
// were stored before an embedding model was installed.
#[derive(Clone, Debug, Default)]
pub struct Job {
    pub active: bool,
    // "storing" or "counting".
    pub stage: String,
    pub done: usize,
    pub total: usize,
    // The file the job is at now.
    pub file: String,
    pub note: String,
    pub failed: bool,
    pub cancel: bool,
}

static JOB: OnceLock<Mutex<Job>> = OnceLock::new();

fn job() -> &'static Mutex<Job> {
    JOB.get_or_init(|| Mutex::new(Job::default()))
}

// The job as the page reads it.
pub fn job_value() -> Value {
    let held = job().lock().unwrap_or_else(|gone| gone.into_inner());
    json!({
        "active": held.active,
        "stage": held.stage,
        "done": held.done,
        "total": held.total,
        "file": held.file,
        "note": held.note,
        "failed": held.failed,
        "percent": if held.total > 0 {
            ((held.done as f64 / held.total as f64) * 100.0).round() as i64
        } else { 0 },
    })
}

// True while a job of the sources is going on.
pub fn busy() -> bool {
    job().lock().unwrap_or_else(|gone| gone.into_inner()).active
}

// Ask a job that is going on to stop. What is done already stays done: a file that was stored
// and counted is a file that is in the index.
pub fn cancel() {
    let mut held = job().lock().unwrap_or_else(|gone| gone.into_inner());
    if held.active {
        held.cancel = true;
    }
}

// True when the user asked the job to stop.
fn cancelled() -> bool {
    job().lock().unwrap_or_else(|gone| gone.into_inner()).cancel
}

// Ask for the start of a job. A job that is already going is refused, so two drops of files do
// not write into the same folder against one another.
fn begin(stage: &str, total: usize) -> Result<(), String> {
    let mut held = job().lock().unwrap_or_else(|gone| gone.into_inner());
    if held.active {
        return Err(String::from(
            "The sources are busy with the files that were sent before these. Wait for that to end.",
        ));
    }
    held.active = true;
    held.stage = stage.to_string();
    held.done = 0;
    held.total = total;
    held.file = String::new();
    held.note = String::new();
    held.failed = false;
    held.cancel = false;
    Ok(())
}

// Write one word into the line of the job.
fn tell(file: &str, done: usize) {
    let mut held = job().lock().unwrap_or_else(|gone| gone.into_inner());
    held.file = file.to_string();
    held.done = done;
}

// End the job with the word the user reads.
fn end(note: String, failed: bool) {
    let mut held = job().lock().unwrap_or_else(|gone| gone.into_inner());
    held.active = false;
    held.note = note;
    held.failed = failed;
    if failed {
        held.stage = String::from("failed");
    }
}

// Store the files of a drop in the folder of the sources and count their words. The files are
// written first, in the order they came, so a folder that was dropped arrives whole; the counting
// follows, a few pieces at a time. The call returns at once and the work goes on in a thread of
// its own, which is what the pop-up of the sources is built for: it shows the files coming in and
// stays open while they do.
pub fn store(work: &str, workdir: &Path, given: Vec<Given>, embedder: Option<Arc<dyn Embedder>>) -> Result<(), String> {
    let total = given.len();
    begin("storing", total)?;
    let key = work.to_string();
    let place = workdir.to_path_buf();
    std::thread::Builder::new()
        .name("an-sources-store".to_string())
        .spawn(move || {
            let note = store_and_count(&key, &place, &given, embedder.as_deref());
            end(note, false);
        })
        .map_err(|why| format!("The files could not be started: {}", why))?;
    Ok(())
}

// All of the work of a drop: write the files, then count what was written. The reason the user
// reads comes back from here.
fn store_and_count(work: &str, workdir: &Path, given: &[Given], embedder: Option<&dyn Embedder>) -> String {
    let folder = make_folder(workdir);
    let mut stored = 0usize;
    let mut said: Vec<String> = Vec::new();
    for (at, one) in given.iter().enumerate() {
        if cancelled() {
            said.push(String::from("the rest of the files were given up"));
            break;
        }
        tell(&one.path, at);
        match write_one(&folder, one) {
            Ok(true) => stored += 1,
            Ok(false) => said.push(format!(
                "{} is bigger than this program takes and was not written",
                one.path
            )),
            Err(problem) => said.push(format!("{}: {}", one.path, problem)),
        }
    }
    // The counting follows. A file whose words are known goes into the index at once; a file that
    // has no words to count, or a run with nothing installed to count with, stays in the folder
    // with its row saying so.
    let counted = count_all(work, &folder, given, embedder);
    if counted == 0 {
        match embedder {
            Some(worker) if !worker.ready() => said.push(String::from(
                "no embedding model is installed, so the files are stored but not counted",
            )),
            None => said.push(String::from(
                "llama-server was not found, so the files are stored but not counted",
            )),
            Some(_) => {}
        }
    }
    said.push(format!(
        "{} files are in the sources{}",
        stored,
        if counted > 0 {
            format!(", and {} of them are counted", counted)
        } else {
            String::new()
        }
    ));
    said.join(". ")
}

// Write one file into the folder. A file bigger than the program takes is refused with `Ok(false)`
// rather than with an error, because a folder that holds one enormous file is not a reason to
// refuse the other ninety-nine.
fn write_one(folder: &Path, one: &Given) -> Result<bool, String> {
    let Some(bytes) = documents::bytes_of_data_url(&one.data) else {
        return Err(String::from("the bytes did not come as they should"));
    };
    if bytes.len() as u64 > GREATEST_BYTES {
        return Ok(false);
    }
    let place = folder.join(&one.path);
    if let Some(parent) = place.parent() {
        fs::create_dir_all(parent)
            .map_err(|why| format!("the folder of it could not be made: {}", why))?;
    }
    fs::write(&place, bytes).map_err(|why| format!("it could not be written: {}", why))?;
    Ok(true)
}

// The rows of the files of a drop, and the pieces of them. The pieces are counted in batches, so
// a hundred files of a directory cost a few asks of the counting server rather than hundreds.
fn count_all(work: &str, folder: &Path, given: &[Given], embedder: Option<&dyn Embedder>) -> usize {
    let mut counted = 0usize;
    for one in given {
        if cancelled() {
            break;
        }
        tell(&one.path, 0);
        let place = folder.join(&one.path);
        let Ok(bytes) = fs::read(&place) else { continue };
        let row = FileRow {
            path: one.path.clone(),
            kind: kind_of(&one.path, &one.mime).to_string(),
            bytes: bytes.len() as i64,
            ..FileRow::default()
        };
        if count_one(work, row, &one.mime, &bytes, embedder) {
            counted += 1;
        }
    }
    counted
}

// The index of one file. True when the pieces of it went into the database. A file that is stored
// and not counted is not a failure: its row says what it waits for, and the counting pass takes
// it again when that thing arrives.
fn count_one(
    work: &str,
    mut row: FileRow,
    mime: &str,
    bytes: &[u8],
    embedder: Option<&dyn Embedder>,
) -> bool {
    let mut made: Vec<(String, Vec<f32>)> = Vec::new();
    if row.kind == "image" {
        // The picture itself is the piece, and its text is the path of it: that is what the model
        // reads back, and what it can open again with the tools it has.
        match embedder {
            Some(worker) if worker.vision() => {
                let words = format!("image {}", row.path);
                match worker.embed_image(&row.path, bytes) {
                    Ok(vector) if !vector.is_empty() => made.push((words, vector)),
                    Ok(_) => row.fault = String::from("the counting of the picture gave nothing"),
                    Err(problem) => row.fault = problem,
                }
            }
            // No projector is not a fault of the file. It is a projector that is missing, and the
            // file is counted as soon as one is installed.
            Some(_) | None => {}
        }
    } else {
        let pieces = words_of(&row.kind, &row.path, mime, bytes)
            .map(|words| chunk_words(&words))
            .unwrap_or_default();
        row.words = pieces.iter().map(|one| one.chars().count() as i64).sum();
        if pieces.is_empty() {
            if row.kind == "text" {
                row.fault = String::from("the file holds no text");
            }
        } else {
            made = count_pieces(&row, &pieces, embedder);
        }
    }
    row.counted = !made.is_empty();
    row.chunks = made.len() as i64;
    let Ok(kept) = database() else {
        return false;
    };
    let key = work.to_string();
    let path = row.path.clone();
    let fault = row.fault.clone();
    let kind = row.kind.clone();
    let wrote = kept.write(|conn| {
        put_file(
            conn,
            &key,
            &path,
            &kind,
            row.bytes,
            row.words,
            row.chunks,
            row.counted,
            &fault,
        )?;
        conn.execute(
            "DELETE FROM chunks WHERE work = ?1 AND path = ?2",
            params![key, path],
        )
        .map_err(db::fault)?;
        for (at, (text, vector)) in made.iter().enumerate() {
            conn.execute(
                "INSERT INTO chunks (work, path, idx, text, vector) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![key, path, at as i64, text, bytes_of(vector)],
            )
            .map_err(db::fault)?;
        }
        Ok(())
    });
    match wrote {
        Ok(()) => row.counted,
        Err(problem) => {
            proglog::note(
                proglog::STORE,
                &format!("the source {} could not be indexed: {}", row.path, problem),
            );
            false
        }
    }
}

// The vectors of the pieces of one file, in the order of the pieces. An ask that fails leaves the
// file uncounted, with the reason in its row, rather than half indexed.
fn count_pieces(
    row: &FileRow,
    pieces: &[String],
    embedder: Option<&dyn Embedder>,
) -> Vec<(String, Vec<f32>)> {
    let Some(worker) = embedder else {
        return Vec::new();
    };
    if !worker.ready() {
        return Vec::new();
    }
    // A batch is a few dozen pieces: long enough that the wait for the server is paid once,
    // short enough that one ask of a big file does not hold up the line of the job.
    const BATCH: usize = 24;
    let mut out: Vec<(String, Vec<f32>)> = Vec::with_capacity(pieces.len());
    let mut at = 0usize;
    while at < pieces.len() {
        let end = (at + BATCH).min(pieces.len());
        match worker.embed(&pieces[at..end]) {
            Ok(vectors) => {
                for (piece, vector) in pieces[at..end].iter().zip(vectors.into_iter()) {
                    out.push((piece.clone(), vector));
                }
                at = end;
            }
            Err(problem) => {
                proglog::note(
                    proglog::STORE,
                    &format!("the source {} was not counted: {}", row.path, problem),
                );
                return Vec::new();
            }
        }
    }
    out
}

// Put the row of one file in place of the older one of the same name. `counted` is a marker of
// the file and not of the pass that wrote it: a file with no pieces in the index is not counted,
// whatever wrote its row.
#[allow(clippy::too_many_arguments)]
fn put_file(
    conn: &rusqlite::Connection,
    work: &str,
    path: &str,
    kind: &str,
    bytes: i64,
    words: i64,
    chunks: i64,
    counted: bool,
    fault: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO files (work, path, kind, bytes, words, chunks, counted, fault, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(work, path) DO UPDATE SET kind = ?3, bytes = ?4, words = ?5,
           chunks = ?6, counted = ?7, fault = ?8, updated = ?9",
        params![
            work,
            path,
            kind,
            bytes,
            words,
            chunks,
            db::mark_num(counted),
            fault,
            crate::store::now_secs()
        ],
    )
    .map_err(db::fault)?;
    Ok(())
}

// Count the files of one working directory that are stored but not counted. This is what happens
// when an embedding model is installed after the files came in, and what the button of the
// sources panel does. The call returns at once.
pub fn count_pending(work: &str, workdir: &Path, embedder: Arc<dyn Embedder>) -> Result<usize, String> {
    let held = rows(work);
    let wanted: Vec<FileRow> = held
        .iter()
        .filter(|one| !one.counted && one.fault.is_empty())
        .cloned()
        .collect();
    if wanted.is_empty() {
        return Ok(0);
    }
    let total = wanted.len();
    begin("counting", total)?;
    let key = work.to_string();
    let place = workdir.to_path_buf();
    std::thread::Builder::new()
        .name("an-sources-count".to_string())
        .spawn(move || {
            let mut done = 0usize;
            let mut made = 0usize;
            for one in &wanted {
                if cancelled() {
                    break;
                }
                tell(&one.path, done);
                done += 1;
                let place = folder_in(&place).join(&one.path);
                let Ok(bytes) = fs::read(&place) else { continue };
                let mime = attach::mime_of(&one.path);
                let row = FileRow {
                    path: one.path.clone(),
                    kind: one.kind.clone(),
                    bytes: bytes.len() as i64,
                    ..FileRow::default()
                };
                if count_one(&key, row, mime, &bytes, Some(embedder.as_ref())) {
                    made += 1;
                }
            }
            end(
                format!("{} files were counted, and {} of them went into the index", done, made),
                false,
            );
        })
        .map_err(|why| format!("The counting could not be started: {}", why))?;
    Ok(total)
}

// The files of one working directory, for the panel of the sources.
pub fn list(work: &str, workdir: &Path) -> Value {
    let held = rows(work);
    let (files, chunks, pending) = counts(work);
    let lines: Vec<Value> = held
        .iter()
        .map(|one| {
            json!({
                "path": one.path,
                "kind": one.kind,
                "bytes": one.bytes,
                "words": one.words,
                "chunks": one.chunks,
                "counted": one.counted,
                "fault": one.fault,
                "when": crate::store::format_when(one.updated),
            })
        })
        .collect();
    json!({
        "ok": true,
        "files": lines,
        "folder": folder_in(workdir).display().to_string(),
        "counts": { "files": files, "chunks": chunks, "pending": pending },
        "job": job_value(),
    })
}

// Take the sources of one working directory out of the index. `paths` of nothing means all of
// them, which is what the button of the panel that says 'take them all out' sends. The files in
// the folder are left alone: the panel says so, and a user who wants them gone deletes them
// themselves.
pub fn remove(work: &str, paths: &[String]) -> usize {
    let Ok(kept) = database() else {
        return 0;
    };
    let key = work.to_string();
    let wanted: Vec<String> = if paths.is_empty() {
        rows(work).into_iter().map(|one| one.path).collect()
    } else {
        paths.iter().filter_map(|one| tidy_path(one)).collect()
    };
    if wanted.is_empty() {
        return 0;
    }
    kept.write(|conn| {
        let mut gone = 0usize;
        for path in &wanted {
            let hits = conn
                .execute(
                    "DELETE FROM files WHERE work = ?1 AND path = ?2",
                    params![key, path],
                )
                .map_err(db::fault)?;
            conn.execute(
                "DELETE FROM chunks WHERE work = ?1 AND path = ?2",
                params![key, path],
            )
            .map_err(db::fault)?;
            gone += hits;
        }
        Ok(gone)
    })
    .unwrap_or(0)
}

// Take the sources of one working directory out of the index because the directory itself is
// going. See `crate::tasks::drop_work`.
pub fn drop_work(work: &str) -> usize {
    if work.trim().is_empty() {
        return 0;
    }
    let Ok(kept) = database() else {
        return 0;
    };
    let key = work.to_string();
    kept.write(|conn| {
        let files = conn
            .execute("DELETE FROM files WHERE work = ?1", params![key])
            .map_err(db::fault)?;
        conn.execute("DELETE FROM chunks WHERE work = ?1", params![key])
            .map_err(db::fault)?;
        Ok(files)
    })
    .unwrap_or(0)
}

// Everything the badge of a chat and the panel of the sources need at once.
pub fn state(work: &str, workdir: &Path, embedder: Option<&dyn Embedder>) -> Value {
    let (files, chunks, pending) = counts(work);
    json!({
        "files": files,
        "chunks": chunks,
        "pending": pending,
        "folder": folder_in(workdir).display().to_string(),
        "ready": embedder.map(|one| one.ready()).unwrap_or(false),
        "vision": embedder.map(|one| one.vision()).unwrap_or(false),
        "job": job_value(),
    })
}

#[cfg(test)]
mod checks {
    use super::*;

    // A counting of words that never asks for a model: every text becomes a vector of the same
    // length out of the words in it, so a piece that holds the word asked for ranks above a piece
    // that does not.
    struct Echo;

    impl Embedder for Echo {
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts.iter().map(|one| shape(one)).collect())
        }
    }

    // The shape of a text: 32 numbers, one for every group of words of it, each counting how many
    // of its words fall in that group.
    fn shape(words: &str) -> Vec<f32> {
        let mut out = vec![0.0f32; 32];
        for word in words.split_whitespace() {
            let sum: usize = word.chars().map(|one| one as usize).sum();
            out[sum % 32] += 1.0;
        }
        let sum: f32 = out.iter().map(|one| one * one).sum();
        if sum > 0.0 {
            let root = sum.sqrt();
            for one in out.iter_mut() {
                *one /= root;
            }
        }
        out
    }

    // The index of the checks is in a folder of its own, and it is emptied before every check,
    // so a check never reads the rows another left behind.
    fn setup() {
        config::set_data_dir(std::path::PathBuf::from("uidata/rag-tests"));
        let _ = config::ensure_dirs();
        crate::migrate::mark_ran(&config::paths().data);
        let kept = database().expect("the database of the checks is left open");
        kept.write(|conn| {
            conn.execute("DELETE FROM chunks", params![])
                .map_err(db::fault)?;
            conn.execute("DELETE FROM files", params![])
                .map_err(db::fault)?;
            Ok(())
        })
        .expect("the index is emptied for the checks");
    }

    // A directory of the computer for one check.
    fn place(what: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("an-rag").join(what);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the place of the check is created");
        dir
    }

    // The base64 of some bytes, for a data URL of the page.
    fn base64_of(bytes: &[u8]) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    // A path the page sent is kept as it is, and a path that would go out of the folder is
    // refused.
    #[test]
    fn a_path_of_a_source_is_kept_or_refused() {
        assert_eq!(tidy_path("notes.md"), Some("notes.md".to_string()));
        assert_eq!(
            tidy_path("docs/api/auth.md"),
            Some("docs/api/auth.md".to_string())
        );
        // The separators of another system are turned around, and a part that means nothing is
        // taken out.
        assert_eq!(
            tidy_path("./docs\\api\\auth.md"),
            Some("docs/api/auth.md".to_string())
        );
        assert_eq!(tidy_path("../elsewhere.md"), None);
        assert_eq!(tidy_path("docs/../../elsewhere.md"), None);
        // The start of a path that names the root is taken out, and the rest of the tree is kept.
        assert_eq!(tidy_path("/etc/passwd"), Some("etc/passwd".to_string()));
        // A drive letter, and a name of a stream of NTFS, both come with a colon.
        assert_eq!(tidy_path("C:/windows/write.exe"), None);
        assert_eq!(tidy_path(""), None);
        assert_eq!(tidy_path("./"), None);
        // A tree of too many parts is refused rather than written.
        let deep = (0..40)
            .map(|one| format!("d{}", one))
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(tidy_path(&deep), None);
    }

    // A piece is a few paragraphs, and the pieces come back in the order of the text.
    #[test]
    fn the_text_of_a_source_is_cut_into_pieces() {
        let text = "One two three.\n\nFour five.\n\nSix seven eight nine ten.";
        let made = chunk_words(text);
        assert_eq!(made.len(), 1, "three short paragraphs are one piece");
        assert!(made[0].contains("One two three"));
        assert!(made[0].ends_with("ten."));
        // Paragraphs are packed until a piece is full, and no piece is longer than the rule.
        let long = (0..60)
            .map(|one| format!("Paragraph {} with a good many words in it to take up room.", one))
            .collect::<Vec<_>>()
            .join("\n\n");
        let made = chunk_words(&long);
        assert!(made.len() > 1, "the text does not fit in one piece");
        for one in &made {
            assert!(
                one.chars().count() <= CHUNK_CHARS,
                "a piece is not longer than the rule"
            );
        }
        // A paragraph of more than a piece is cut between words, and no word is lost.
        let one = (0..600)
            .map(|one| format!("word{}", one))
            .collect::<Vec<_>>()
            .join(" ");
        let made = chunk_words(&one);
        assert!(made.len() > 1);
        assert!(made.concat().contains("word599"));
        assert!(made[0].chars().count() <= CHUNK_CHARS);
    }

    // A file of text is written in the folder with its tree kept, and its pieces go into the index
    // of its working directory. A search of the index answers with the path of the piece it found.
    #[test]
    fn a_source_is_written_and_indexed() {
        let _guard = config::test_turn();
        setup();
        let room = place("source");
        let work = key_of(&room);
        let folder = make_folder(&room);
        assert!(folder.is_dir());
        assert_eq!(folder, room.join(".sources"));
        // The bytes of a data URL are the bytes of the file, and the tree the user named is kept.
        let given = Given {
            path: "docs/notes.md".to_string(),
            mime: "text/markdown".to_string(),
            data: format!(
                "data:text/markdown;base64,{}",
                base64_of(b"# Auth\n\nThe token lasts an hour and is refreshed by the client.")
            ),
        };
        assert_eq!(kind_of(&given.path, &given.mime), "text");
        write_one(&folder, &given).expect("the file is written");
        assert!(folder.join("docs/notes.md").is_file());
        let bytes = fs::read(folder.join("docs/notes.md")).expect("the file is read");
        let counted = count_one(
            &work,
            FileRow {
                path: given.path.clone(),
                kind: "text".to_string(),
                bytes: bytes.len() as i64,
                ..FileRow::default()
            },
            "text/markdown",
            &bytes,
            Some(&Echo),
        );
        assert!(counted, "the pieces of the file went into the index");
        let held = chunks(&work);
        assert_eq!(held.len(), 1);
        assert!(held[0].text.contains("token lasts an hour"));
        let (files, pieces, pending) = counts(&work);
        assert_eq!(files, 1);
        assert_eq!(pieces, 1);
        assert_eq!(pending, 0);
        // The search answers with the path of the piece it found, and its text.
        let made = run(&work, Some(&Echo), r#"{"query":"token lasts an hour"}"#);
        assert!(made.ok);
        assert!(made.output.contains("docs/notes.md"), "{}", made.output);
        assert!(made.output.contains("token lasts an hour"), "{}", made.output);
        // The panel reads the same rows, with the folder the files are in beside them.
        let panel = list(&work, &room);
        assert_eq!(panel["files"].as_array().map(|one| one.len()), Some(1));
        assert_eq!(panel["files"][0]["path"], "docs/notes.md");
        assert_eq!(panel["counts"]["chunks"].as_u64(), Some(1));
        assert_eq!(panel["counts"]["pending"].as_u64(), Some(0));
        assert!(panel["folder"].as_str().unwrap_or_default().ends_with(".sources"));
        // Taking a file out of the index leaves the file itself in the folder.
        assert_eq!(remove(&work, &["docs/notes.md".to_string()]), 1);
        let (files, pieces, _) = counts(&work);
        assert_eq!(files, 0, "the row is gone");
        assert_eq!(pieces, 0, "and its pieces with it");
        assert!(folder.join("docs/notes.md").is_file(), "the file stays");
        let _ = fs::remove_dir_all(&room);
    }

    // Two files of one working directory are indexed apart from two of another, and a directory
    // that goes takes its index with it.
    #[test]
    fn the_index_of_one_directory_is_kept_apart_from_that_of_another() {
        let _guard = config::test_turn();
        setup();
        let going = place("going");
        let staying = place("staying");
        let going_key = key_of(&going);
        let staying_key = key_of(&staying);
        for (key, name) in [(&going_key, "a"), (&going_key, "b"), (&staying_key, "c")] {
            let bytes = format!("text about {} {}", name, key.len());
            let row = FileRow {
                path: format!("{}.md", name),
                kind: "text".to_string(),
                bytes: bytes.len() as i64,
                ..FileRow::default()
            };
            assert!(count_one(key, row, "text/markdown", bytes.as_bytes(), Some(&Echo)));
        }
        assert_eq!(counts(&going_key).0, 2);
        assert_eq!(counts(&staying_key).0, 1);
        assert_eq!(drop_work(""), 0, "no directory has no index to give");
        assert_eq!(drop_work(&going_key), 2, "both rows of the directory went");
        assert_eq!(counts(&going_key).0, 0);
        assert_eq!(counts(&staying_key).0, 1, "the index of another directory remains");
        assert_eq!(drop_work(&going_key), 0, "an index that is removed gives nothing");
        // A search of a directory with nothing in it says so, rather than answering with the
        // index of the one that went.
        let made = run(&going_key, Some(&Echo), r#"{"query":"anything"}"#);
        assert!(made.ok);
        assert!(made.output.contains("nothing indexed"), "{}", made.output);
        let _ = fs::remove_dir_all(&going);
        let _ = fs::remove_dir_all(&staying);
    }

    // A file of a type nothing is known about is stored and reported as waiting, rather than
    // failing the drop of the folder it came in with.
    #[test]
    fn a_file_with_no_words_of_its_own_is_left_in_the_folder() {
        let _guard = config::test_turn();
        setup();
        let room = place("other");
        let work = key_of(&room);
        let folder = make_folder(&room);
        let given = Given {
            path: "thing.bin".to_string(),
            mime: "application/octet-stream".to_string(),
            data: format!(
                "data:application/octet-stream;base64,{}",
                base64_of(b"\x01\x02")
            ),
        };
        assert_eq!(kind_of(&given.path, &given.mime), "other");
        write_one(&folder, &given).expect("the file is written");
        assert!(folder.join("thing.bin").is_file());
        let bytes = fs::read(folder.join("thing.bin")).expect("the file is read");
        let counted = count_one(
            &work,
            FileRow {
                path: given.path.clone(),
                kind: "other".to_string(),
                bytes: bytes.len() as i64,
                ..FileRow::default()
            },
            "application/octet-stream",
            &bytes,
            Some(&Echo),
        );
        assert!(!counted, "a file of no text is not counted");
        // The row says the file is stored and waits, which is what the panel shows.
        let held = rows(&work);
        assert_eq!(held.len(), 1);
        assert!(!held[0].counted);
        assert!(held[0].fault.is_empty(), "waiting is not a fault: {}", held[0].fault);
        assert_eq!(counts(&work).2, 1, "one file waits to be counted");
        let _ = fs::remove_dir_all(&room);
    }

    // The call of the page is read for its files, and a file whose path is refused is reported
    // rather than dropped without a word.
    #[test]
    fn the_files_of_a_call_are_read_and_refused() {
        let value = json!({
            "files": [
                { "path": "ok.md", "mime": "text/markdown", "data": "data:text/markdown;base64,AA==" },
                { "path": "../out.md", "mime": "text/markdown", "data": "data:text/markdown;base64,AA==" },
                { "path": "empty.md", "mime": "text/markdown", "data": "" }
            ]
        });
        let (given, refused) = parse_given(&value);
        assert_eq!(given.len(), 1);
        assert_eq!(given[0].path, "ok.md");
        assert_eq!(refused.len(), 2);
        assert!(refused[0].contains("../out.md"));
        assert!(refused[1].contains("empty.md"));
        // A call with no list of files at all is nothing to store, and says nothing either.
        let (given, refused) = parse_given(&json!({}));
        assert!(given.is_empty());
        assert!(refused.is_empty());
    }

    // The words of the tool: what the call named, and the line the user watches while it comes.
    #[test]
    fn the_call_names_the_text_and_the_number_of_places() {
        let (query, most) = asked_of(r#"{"query":"how is a token refreshed","places":3}"#);
        assert_eq!(query, "how is a token refreshed");
        assert_eq!(most, 3);
        let (query, most) = asked_of(r#"{"q":"auth"}"#);
        assert_eq!(query, "auth");
        assert_eq!(most, DEFAULT_PLACES);
        // A number out of range is brought back into it, and a number written as text is read.
        let (_, most) = asked_of(r#"{"query":"x","places":99}"#);
        assert_eq!(most, GREATEST_PLACES);
        let (query, _) = asked_of(r#"{"query":"x","limit":"2"}"#);
        assert_eq!(query, "x");
        // A call that is still being written gives the words of it so far, which is what the
        // header of the block of the call shows.
        assert_eq!(
            partial_words(r#"{"query":"how is a token ref"#),
            "how is a token ref"
        );
        // A call with nothing in it is refused with a reason.
        let made = run("/any/where", Some(&Echo), "{}");
        assert!(!made.ok);
        assert!(made.output.contains("No text to search for"));
        // With nothing installed to count with, the reason says which part is missing.
        let made = run("/any/where", None, r#"{"query":"anything"}"#);
        assert!(!made.ok);
        assert!(made.output.contains("llama-server"));
    }

    // The text that goes with the tool reports the state of the folder, which is what tells a
    // model whether to search at all.
    #[test]
    fn the_prompt_of_the_tool_reports_the_folder() {
        let none = system_prompt(0, 0, false);
        assert!(none.contains("Nothing has been put"));
        assert!(none.contains("search_sources"));
        let some = system_prompt(9, 7, true);
        assert!(some.contains("9 files"));
        assert!(some.contains("7 of them are counted"));
        assert!(some.contains("vision projector"));
        let cold = system_prompt(3, 0, false);
        assert!(cold.contains("none of them is counted"));
        assert!(!cold.contains("vision projector"));
    }

    // The bytes of a vector, and the vector out of them again.
    #[test]
    fn a_vector_survives_its_bytes() {
        let made = vec![1.0f32, -0.5, 0.0, 12345.75];
        let again = numbers_of(&bytes_of(&made));
        assert_eq!(again, made);
    }
}
