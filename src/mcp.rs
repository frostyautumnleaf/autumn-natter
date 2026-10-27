// Custom MCP servers.
//
// MCP is the Model Context Protocol: a model is told what a program can do, and
// the program does it when the model asks. The words between the two are
// JSON-RPC 2.0, and the parts of the protocol this program reads are
// `initialize`, `notifications/initialized`, `tools/list` and `tools/call`.
// The client of the protocol is described at py.sdk.modelcontextprotocol.io.
//
// Three kinds of server stand here.
//
//   * `stdio`: a program of this machine that speaks the protocol over its
//     standard input and its standard output. The reader names the whole line.
//   * `http`: a server on the network, spoken to over Streamable HTTP. One
//     POST of JSON-RPC to one address, and the answer comes back as JSON, or
//     as a stream of `data:` lines.
//   * `internal`: a server made of words the reader pasted into the sheet of
//     settings. This file writes the plumbing around those words - a small
//     runner that speaks the protocol - and starts the pair as a stdio server.
//     The code the reader writes needs nothing but the library that stands with
//     Python itself. A new server of that kind begins with an example of its
//     own, `TEMPLATE`, which the sheet of settings puts in the field of the
//     code. The head of that example is written for an AI, so a reader may hand
//     the whole of the field to an AI and get back a server that works.
//
// The tools of a server reach the model under a name of its own:
// `mcp__<server>__<tool>`, for the tools of two servers may share a name, and
// a name of a tool has to say where the tool came from. The name is kept inside
// the shape a chat template allows: letters, digits, the dash and the mark
// below, no more than sixty four characters.
//
// A server is reached for one look and let go again: the connection is opened,
// the work is done, and the process or the session is closed. Nothing of a
// server is left standing behind a closed window, and a server that hangs is
// waited for so long and no longer.
//
// A server of any kind runs in the program, as the web search and the tools
// that work on the files do. It is never put inside the jail of a command, for
// the jail has no network and no program of its own to run.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::config;
use crate::tools;
use crate::util;

// A server that stands as a program of this machine.
pub const KIND_STDIO: &str = "stdio";
// A server that stands on the network.
pub const KIND_HTTP: &str = "http";
// A server made of words the reader pasted, run by the program itself.
pub const KIND_INTERNAL: &str = "internal";

// The mark at the head of the name of a tool of a server of this program.
pub const NAME_MARK: &str = "mcp__";

// The mark between the name of a server and the name of a tool of it.
const SEP: &str = "__";

// The longest name of a tool a chat template can read. llama.cpp reads a call
// out of an answer for a name of letters, digits, the dash and the mark below,
// and nothing longer than this.
pub const GREATEST_NAME: usize = 64;

// The longest part of the name a server is known by. The room of the name of
// the tool itself has to be kept as well.
const GREATEST_SLUG: usize = 20;

// The icons the reader may put on a server. The sheet of settings and the menu
// of the badge both draw the icon of a server by its number, so the number
// stands between the program and the page and the drawing stands in the page.
pub const ICON_COUNT: usize = 16;

// The longest name of a server, as it stands in the menu and in the sheet.
pub const GREATEST_SERVER_NAME: usize = 48;

// The most tools of one server that reach the model.
pub const GREATEST_TOOLS: usize = 32;

// The most words of the code of a server of the program itself.
pub const GREATEST_CODE: usize = 200_000;

// The most words of the answer of a tool that reach the model. The room of the
// talk is paid for by every word, and a server that answers with a whole file
// would take a great deal of it.
pub const GREATEST_OUTPUT: usize = 8000;

// The version of the protocol this program asks for. A server that does not
// know it answers with the newest version it does know, and that answer is
// taken as it stands.
const PROTOCOL_ASKED: &str = "2025-06-18";

// How long the two sides take hands with each other before the program gives
// up on a server that does not answer.
const HANDSHAKE_WAIT: Duration = Duration::from_secs(20);
// How long the answer of one tool is waited for. A tool that works for a while
// is worked out, so this is longer than the handshake.
const CALL_WAIT: Duration = Duration::from_secs(60);

// One server the reader added. It stands in settings.json, and the sheet of
// settings writes it and reads it back.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Server {
    // Ten characters of the system random source, as `store::new_id` makes them.
    pub id: String,
    // The name the reader gave the server. It stands in the menu and in the sheet.
    pub name: String,
    // One of the three kinds above.
    pub kind: String,
    // `stdio`: the whole line that starts the server, quotes and all.
    pub command: String,
    // `http`: the address of the server.
    pub url: String,
    // `internal`: the words of the server, as the reader wrote them.
    pub code: String,
    // Which of the ICON_COUNT icons the menu draws for this server.
    pub icon: usize,
    // Whether this server stands in the drop-down of the MCP badge. A server
    // the reader added but does not want in the menu is kept here all the
    // same, and comes back into the menu when the switch is moved again.
    pub shown: bool,
    // The part of the name of a tool that stands for this server. It is made
    // once, when the server is added, and never moved again: renaming a server
    // must not move the names of its tools, for a chat that is already written
    // holds those names in its talk.
    pub slug: String,
}

impl Server {
    // A short name of this server, of the parts a name of a tool is made of.
    pub fn slug(&self) -> String {
        if !self.slug.is_empty() {
            return self.slug.clone();
        }
        slug_of(&self.name)
    }

    // What this server is: one of the three kinds. A word that is none of them
    // gives the stdio server, which is what a new server of this program is.
    pub fn kind(&self) -> String {
        match self.kind.as_str() {
            KIND_HTTP => String::from(KIND_HTTP),
            KIND_INTERNAL => String::from(KIND_INTERNAL),
            _ => String::from(KIND_STDIO),
        }
    }

    // The one thing this kind of server needs to be started at all: the line of
    // a program, the address of a server, or the words of one. Empty says that
    // the server is not ready yet.
    pub fn need(&self) -> &str {
        match self.kind().as_str() {
            KIND_HTTP => self.url.trim(),
            KIND_INTERNAL => self.code.trim(),
            _ => self.command.trim(),
        }
    }

    // Whether enough stands written for this server to be reached.
    pub fn ready(&self) -> bool {
        !self.need().is_empty()
    }

    // The server as this program keeps it: the words of each part trimmed, the
    // name and the code cut inside their bounds, the kind one of the three, and
    // the number of the icon one of the icons. The sheet is written by a person,
    // and a person may write a name of a thousand characters into it.
    pub fn clean(&self) -> Server {
        let mut name: String = self.name.trim().chars().take(GREATEST_SERVER_NAME).collect();
        name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        let command: String = self.command.trim().chars().take(4000).collect();
        let url: String = self.url.trim().chars().take(2000).collect();
        let code: String = self.code.chars().take(GREATEST_CODE).collect();
        Server {
            id: self.id.trim().chars().take(40).collect(),
            name,
            kind: self.kind(),
            command,
            url,
            code,
            icon: self.icon % ICON_COUNT,
            shown: self.shown,
            slug: self.slug.trim().chars().take(GREATEST_SLUG + 8).collect(),
        }
    }

    // Whether this server may be kept. A server with nothing written in the
    // place its kind asks for is refused, for it could never be reached, and a
    // reader left with a server that does nothing wonders about the program.
    pub fn check(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err(String::from("A server needs a name"));
        }
        if self.kind() == KIND_HTTP {
            address_of(&self.url)?;
        }
        if !self.ready() {
            return Err(match self.kind().as_str() {
                KIND_HTTP => String::from("An HTTP server needs an address"),
                KIND_INTERNAL => String::from(
                    "A server of this program needs the words of it: write a function, or two",
                ),
                _ => String::from("An external server needs the line that starts it"),
            });
        }
        Ok(())
    }

    // The words the sheet and the menu put beside the name of a tool of this
    // server, so the reader knows where a tool came from.
    pub fn where_from(&self) -> String {
        match self.kind().as_str() {
            KIND_HTTP => format!("{} over HTTP", self.name),
            KIND_INTERNAL => format!("{} of this program", self.name),
            _ => format!("{} of this machine", self.name),
        }
    }
}

// The parts of the name of a tool that stand for a server: lowercase letters,
// digits and the mark below. A name of nothing usable gives an empty name, and
// the caller keeps the room of the name of the tool itself.
pub fn slug_of(name: &str) -> String {
    let mut out = String::new();
    let mut last = '_';
    for ch in name.chars() {
        let keep = if ch.is_ascii_uppercase() {
            Some(ch.to_ascii_lowercase())
        } else if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            Some(ch)
        } else {
            Some('_')
        };
        let Some(kept) = keep else { continue };
        if kept == '_' && (last == '_' || out.is_empty()) {
            continue;
        }
        out.push(kept);
        last = kept;
    }
    while out.ends_with('_') {
        out.pop();
    }
    out.chars().take(GREATEST_SLUG).collect()
}

// A name of a server that no other server of this list holds. The name the
// reader wrote is the first choice; a second server of the same name takes a
// number behind it, for two servers whose tools answer one another would else
// be told apart by nothing at all.
pub fn slug_for(name: &str, taken: &[&Server]) -> String {
    let base = slug_of(name);
    let base = if base.is_empty() {
        String::from("server")
    } else {
        base
    };
    if !taken.iter().any(|other| other.slug() == base) {
        return base;
    }
    for number in 2..1000 {
        let next = format!("{}_{}", base, number);
        if !taken.iter().any(|other| other.slug() == next) {
            return next;
        }
    }
    base
}

// The name a tool of a server stands under between this program and the model.
pub fn tool_name(slug: &str, tool: &str) -> String {
    let clean = name_part(tool);
    let clean = if clean.is_empty() {
        String::from("tool")
    } else {
        clean
    };
    let head = format!("{}{}__", NAME_MARK, slug);
    let room = GREATEST_NAME.saturating_sub(head.chars().count());
    let taken: String = clean.chars().take(room.max(1)).collect();
    format!("{}{}", head, taken)
}

// The name of a tool out of the words a server gave, made of what a name of a
// tool may hold.
fn name_part(tool: &str) -> String {
    let mut out = String::new();
    for ch in tool.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

// Whether this name of a tool is the name of a tool of a server.
pub fn is_tool(name: &str) -> bool {
    name.starts_with(NAME_MARK)
}

// The two parts of the name of a tool: the part that stands for a server, and
// the name the server gave the tool. A name of a tool may itself hold the mark
// of two parts below, so the split is made at the first of them after the head.
pub fn split_tool(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix(NAME_MARK)?;
    let (slug, tool) = rest.split_once(SEP)?;
    if slug.is_empty() || tool.is_empty() {
        return None;
    }
    Some((slug.to_string(), tool.to_string()))
}

// The name of a tool of a server, as the reader wrote it. The name of a tool of
// a server of this program is `mcp__<slug>__<tool>`, and the reader is given
// back `<tool>`; a name the program does not know is given back as it stands.
pub fn shown_tool(name: &str) -> String {
    match split_tool(name) {
        Some((_, tool)) => tool,
        None => name.to_string(),
    }
}

// The words of a call that is still being written, as the reader is given them.
// The parts of a tool of a server are known only to the server, so the first
// text of the parts is what the reader watches come in; with no text among them
// yet, the words of the call as they stand are given.
pub fn partial_words(raw: &str) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return String::new();
    }
    // The first part of the call that holds words of its own.
    let mut best = String::new();
    for key in ["text", "query", "path", "url", "message", "name", "prompt"] {
        let found = tools::partial_text(text, key);
        if !found.is_empty() {
            best = found;
            break;
        }
    }
    if !best.is_empty() {
        return best;
    }
    let one = text.lines().next().unwrap_or("").trim();
    let taken: String = one.chars().take(120).collect();
    taken
}

// One tool a server offered. `list` gives the whole of the list, and each entry
// is kept as the server gave it, so a server with a schema of its own is sent
// to the model with that schema and not with a guess of one.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Tool {
    // The name the server gave the tool.
    pub name: String,
    // A name for a reader, when the server gave one.
    pub title: String,
    // What the tool does, in the words of the server.
    pub description: String,
    // The shape of the parts the tool asks for, as JSON Schema.
    pub schema: Value,
}

impl Tool {
    // The name a reader of the block is given: the name for a reader when the
    // server gave one, and the name of the tool when it did not.
    pub fn shown(&self) -> String {
        let title = self.title.trim();
        if title.is_empty() {
            return self.name.clone();
        }
        title.to_string()
    }

    // The definition of this tool as the model is sent it, under the name of
    // the server it came from.
    pub fn definition(&self, server: &Server) -> Value {
        let mut said = String::new();
        said.push_str(&format!(
            "[{}] This tool comes from a server of the reader's own, reached over MCP ({}). ",
            server.name,
            server.where_from()
        ));
        if !self.description.trim().is_empty() {
            said.push_str(self.description.trim());
            said.push(' ');
        }
        json!({
            "type": "function",
            "function": {
                "name": tool_name(&server.slug(), &self.name),
                "description": tools::with_rule(said),
                "parameters": schema_of(self),
            }
        })
    }
}

// The shape of the parts of a tool, as the model must be sent it. A server that
// named no shape is given an object of no parts, which lets the model call a
// tool that asks for nothing.
pub fn schema_of(tool: &Tool) -> Value {
    match &tool.schema {
        Value::Object(shape) if shape.get("type").is_some() => tool.schema.clone(),
        Value::Object(shape) if shape.get("properties").is_some() => {
            let mut made = shape.clone();
            made.insert("type".to_string(), json!("object"));
            Value::Object(made)
        }
        Value::Object(shape) if !shape.is_empty() => {
            let mut made = shape.clone();
            made.insert("type".to_string(), json!("object"));
            Value::Object(made)
        }
        _ => json!({ "type": "object", "properties": {} }),
    }
}

// The words a server of the network is reached at. Only an address of the web
// is let through, so a server of the sheet can never name a file of the machine
// or a road of its own.
pub fn address_of(text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(String::from(
            "The address of the server must begin with http:// or https://",
        ));
    }
    if trimmed.chars().count() <= "http://".len() {
        return Err(String::from("The address of the server names nothing"));
    }
    Ok(trimmed.to_string())
}

// What one run of one tool of a server gave back.
#[derive(Clone, Debug, Default)]
pub struct Report {
    // False when the server said the tool failed, or when the server could not
    // be reached at all.
    pub ok: bool,
    // The words the model reads.
    pub output: String,
}

// ------------------------------------------------------------------ the client

// One open way to one server. A stdio server is a child process of this one,
// spoken to with one JSON object to a line; an HTTP server is one address,
// spoken to with one POST of JSON-RPC.
enum Pipe {
    Stdio(StdioLink),
    Http(HttpLink),
}

struct StdioLink {
    // The server itself, so it can be stopped when the look is over.
    child: Child,
    // Where the requests go. It is taken when the child is stopped.
    stdin: Option<ChildStdin>,
    // The lines the server prints, brought over by a thread of its own. The
    // thread ends when the server closes its standard output.
    lines: mpsc::Receiver<String>,
    // What the server wrote to its own error output, kept short. A server that
    // will not start says why there, and that is the one thing worth showing.
    errors: std::sync::Arc<std::sync::Mutex<String>>,
}

struct HttpLink {
    url: String,
    // The id of the session the server gave this client. It goes with every
    // request after the handshake, and a server that never gave one is spoken
    // to without it.
    session: Option<String>,
    agent: ureq::Agent,
}

impl Session {
    // Open one way to one server and shake hands over it. Nothing is asked of
    // the tools of the server here, so a look at the tools of a server and a
    // run of one of them both begin the same way.
    fn open(server: &Server) -> Result<Session, String> {
        let link = match server.kind().as_str() {
            KIND_HTTP => Pipe::Http(HttpLink {
                url: address_of(&server.url)?,
                session: None,
                agent: ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(10))
                    .timeout_read(Duration::from_secs(120))
                    .build(),
            }),
            // A server of the program itself is written out to the folder of
            // the servers before it is started, so what the reader wrote in the
            // sheet is what runs.
            kind => {
                let (program, args, dir) = start_line(server, kind)?;
                let mut command = Command::new(&program);
                command
                    .args(&args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                if let Some(place) = dir {
                    command.current_dir(place);
                }
                // The runner of a server of the program itself prints as it
                // reads; without this its lines wait in a buffer of Python and
                // arrive too late or never.
                command.env("PYTHONUNBUFFERED", "1");
                let mut child = command.spawn().map_err(|problem| {
                    format!(
                        "The server could not be started ({}): {}",
                        program,
                        problem
                    )
                })?;
                let stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| String::from("The server has no standard input"))?;
                let out = child
                    .stdout
                    .take()
                    .ok_or_else(|| String::from("The server has no standard output"))?;
                let err = child.stderr.take();
                // The lines of the server are read by a thread of their own, so
                // a server that prints nothing never holds this thread, and a
                // server that prints a great deal never stops printing.
                let (sender, lines) = mpsc::channel::<String>();
                std::thread::spawn(move || {
                    let reader = BufReader::new(out);
                    for line in reader.lines() {
                        match line {
                            Ok(text) => {
                                if sender.send(text).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
                let errors = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
                if let Some(pipe) = err {
                    let kept = errors.clone();
                    std::thread::spawn(move || {
                        let reader = BufReader::new(pipe);
                        for line in reader.lines().map_while(Result::ok) {
                            let Ok(mut held) = kept.lock() else { break };
                            if held.len() < 8000 {
                                held.push_str(&line);
                                held.push('\n');
                            }
                        }
                    });
                }
                Pipe::Stdio(StdioLink {
                    child,
                    stdin: Some(stdin),
                    lines,
                    errors,
                })
            }
        };
        let mut session = Session {
            link,
            next: 0,
        };
        session.shake_hands()?;
        Ok(session)
    }

    // Say who this is and what version of the protocol is spoken, then say
    // that the two sides are ready. Every request of the protocol comes after
    // these two words.
    fn shake_hands(&mut self) -> Result<(), String> {
        let greeting = json!({
            "protocolVersion": PROTOCOL_ASKED,
            "capabilities": {},
            "clientInfo": {
                "name": "Autumn Natter",
                "version": env!("CARGO_PKG_VERSION"),
            },
        });
        self.ask("initialize", &greeting, HANDSHAKE_WAIT)?;
        // The second word is a saying and not a question: no answer comes back.
        let _ = self.tell("notifications/initialized");
        Ok(())
    }

    // One question, and the answer to it. Every question of one connection is
    // numbered, so an answer is matched to the question it answers and not to
    // the last question that was asked.
    fn ask(&mut self, method: &str, params: &Value, wait: Duration) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let line = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        match &mut self.link {
            Pipe::Stdio(pipe) => pipe.ask(&line, id, wait),
            Pipe::Http(pipe) => pipe.ask(&line, id, wait),
        }
    }

    // One saying, with no answer looked for.
    fn tell(&mut self, method: &str) -> Result<(), String> {
        let line = json!({ "jsonrpc": "2.0", "method": method, "params": {} });
        match &mut self.link {
            Pipe::Stdio(pipe) => pipe.write_line(&line),
            Pipe::Http(pipe) => pipe.post(&line).map(|_| ()),
        }
    }

    // Let the server go: the pipe is closed and the child is stopped, or the
    // session of the server is ended.
    fn close(&mut self) {
        match &mut self.link {
            Pipe::Stdio(pipe) => pipe.close(),
            Pipe::Http(pipe) => pipe.close(),
        }
    }
}

// One open way to one server, with the number the next question carries. The
// session is closed when it is thrown away, so a child server is never left
// standing behind a look that went wrong on the way out.
struct Session {
    link: Pipe,
    next: u64,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

impl StdioLink {
    fn write_line(&mut self, line: &Value) -> Result<(), String> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(String::from("The server has no standard input to write to"));
        };
        let mut text = line.to_string();
        text.push('\n');
        stdin
            .write_all(text.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|problem| format!("The server could not be written to: {}", problem))
    }

    fn ask(&mut self, line: &Value, id: u64, wait: Duration) -> Result<Value, String> {
        self.write_line(line)?;
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(self.fault("the server gave no answer"));
            }
            match self.lines.recv_timeout(left) {
                Ok(text) => {
                    let line = text.trim();
                    if line.is_empty() || !line.starts_with('{') {
                        // Words that are not an object are the server talking
                        // to itself, and are passed by.
                        continue;
                    }
                    let Ok(value) = serde_json::from_str::<Value>(line) else {
                        continue;
                    };
                    if value.get("id").and_then(|sent| sent.as_u64()) != Some(id) {
                        // A saying of the server, or the answer to an older
                        // question: neither is the answer looked for here.
                        continue;
                    }
                    return answer_of(&value);
                }
                Err(_) => return Err(self.fault("the server closed its output")),
            }
        }
    }

    // The words of the fault: what the program found, and what the server wrote
    // to its own error output, for that is where a server that will not start
    // says why.
    fn fault(&self, said: &str) -> String {
        let mut text = String::from(said);
        if let Ok(held) = self.errors.lock() {
            let note = held.trim();
            if !note.is_empty() {
                let tail: String = note.chars().rev().collect();
                let tail: String = tail.chars().take(600).collect();
                let tail: String = tail.chars().rev().collect();
                text.push_str(". The server said: ");
                text.push_str(tail.trim());
            }
        }
        text
    }

    fn close(&mut self) {
        // The standard input goes first: a server that reads until the end of
        // its input sees that end and leaves of its own accord. It is given a
        // moment to do it, and only then stopped, for a server that is killed
        // on the way out may leave a file half written behind.
        self.stdin = None;
        let patience = Instant::now() + Duration::from_millis(400);
        while Instant::now() < patience {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl HttpLink {
    fn ask(&mut self, line: &Value, id: u64, wait: Duration) -> Result<Value, String> {
        // The wait of the caller is the wait of the connection: a server that
        // takes its time over one tool is waited for, and no longer.
        let _ = wait;
        let body = self.post(line)?;
        let Some(text) = body else {
            return Err(String::from("The server answered with nothing at all"));
        };
        for value in messages(&text) {
            if let Some(error) = value.get("error") {
                if error.is_null() {
                    continue;
                }
                return Err(rpc_fault(error));
            }
            if value.get("id").and_then(|sent| sent.as_u64()) == Some(id) {
                return Ok(value.get("result").cloned().unwrap_or(Value::Null));
            }
        }
        Err(String::from(
            "The server answered, but not to the question that was asked",
        ))
    }

    // One POST of one message. The answer is the words of the body, or nothing
    // when the server answered with no body at all, which is what it does for a
    // saying.
    fn post(&mut self, line: &Value) -> Result<Option<String>, String> {
        let mut request = self
            .agent
            .post(&self.url)
            .set("content-type", "application/json")
            // A server may answer a POST with a stream instead of one object.
            .set("accept", "application/json, text/event-stream");
        if let Some(id) = self.session.as_ref() {
            request = request.set("mcp-session-id", id);
        }
        let body = line.to_string();
        let response = request
            .send_string(&body)
            .map_err(|problem| format!("The server did not answer: {}", problem))?;
        // The id of the session comes with the answer, and goes with every
        // message after it. A server that gives none is spoken to without one.
        if let Some(id) = response.header("mcp-session-id") {
            if !id.trim().is_empty() {
                self.session = Some(id.to_string());
            }
        }
        let mut text = String::new();
        let _ = response.into_reader().read_to_string(&mut text);
        if text.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(text))
    }

    fn close(&mut self) {
        // A session of a server is ended by a DELETE of the same address. A
        // server that does not know the word says nothing useful, and is left
        // to itself.
        let Some(id) = self.session.as_ref() else { return };
        let _ = self
            .agent
            .delete(&self.url)
            .set("mcp-session-id", id)
            .call();
        self.session = None;
    }
}

// The messages of the body of an answer. A server that answers with JSON gives
// one message; a server that answers with a stream gives several, each behind
// the word `data:`. Both are read here, so a client of this program need not
// know which way its server chose.
fn messages(text: &str) -> Vec<Value> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        return match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => vec![value],
            Err(_) => Vec::new(),
        };
    }
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("data:") else {
            continue;
        };
        let body = rest.trim();
        if body.is_empty() || body == "[DONE]" {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(body) {
            out.push(value);
        }
    }
    out
}

// The answer to one question: the words of the result, or the words of the
// fault the server gave.
fn answer_of(value: &Value) -> Result<Value, String> {
    if let Some(error) = value.get("error") {
        if !error.is_null() {
            return Err(rpc_fault(error));
        }
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

// The words of a fault of the protocol. A code of its own is said with the
// words, for `-32601` says that no such thing exists and that is worth reading.
fn rpc_fault(error: &Value) -> String {
    let message = error
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("the server gave no words of the fault");
    match error.get("code").and_then(|v| v.as_i64()) {
        Some(-32601) => format!("the server has no such thing to offer ({})", message),
        Some(code) => format!("{} (the code was {})", message, code),
        None => message.to_string(),
    }
}

// ------------------------------------------------------------------ the verbs

// The tools a server offers. The server is opened, asked what it has, and let
// go again.
pub fn list(server: &Server) -> Result<Vec<Tool>, String> {
    if !server.ready() {
        return Err(String::from(
            "The server has nothing written in the place that starts it",
        ));
    }
    let mut session = Session::open(server)?;
    let asked = session.ask("tools/list", &json!({}), CALL_WAIT);
    match asked {
        Ok(result) => Ok(tools_of(&result)),
        Err(problem) => Err(problem),
    }
    // The session goes here, and with it the child server, whatever the answer
    // was.
}

// The tools of the answer of `tools/list`. A server with nothing to offer is
// answered with an empty list, and not with a fault: a server of prompts only
// is a whole server, and it has no tools.
pub fn tools_of(result: &Value) -> Vec<Tool> {
    let Some(list) = result.get("tools").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for one in list.iter().take(GREATEST_TOOLS) {
        let name = one
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        out.push(Tool {
            name,
            title: one
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            description: one
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            schema: one
                .get("inputSchema")
                .or_else(|| one.get("input_schema"))
                .cloned()
                .unwrap_or(Value::Null),
        });
    }
    out
}

// One run of one tool of one server. `arguments` are the words of the call, as
// the model wrote them: they are read as JSON, and words that are no JSON are
// taken as an object of nothing.
pub fn run(server: &Server, tool: &str, arguments: &str) -> Report {
    match run_of(server, tool, arguments) {
        Ok(report) => report,
        Err(problem) => Report {
            ok: false,
            output: cut(&format!(
                "The server {} could not be reached: {}",
                server.name, problem
            )),
        },
    }
}

fn run_of(server: &Server, tool: &str, arguments: &str) -> Result<Report, String> {
    if !server.ready() {
        return Ok(Report {
            ok: false,
            output: String::from(
                "This server has nothing written in the place that starts it, so nothing ran",
            ),
        });
    }
    let parts = match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(parts)) => Value::Object(parts),
        Ok(Value::Null) => json!({}),
        // A model that sent a line of words with no object about it is answered
        // as the server of the program itself would be: one part named `text`.
        Ok(Value::String(ref text)) if text.trim().is_empty() => json!({}),
        Ok(Value::String(text)) => json!({ "text": text }),
        Ok(other) => json!({ "value": other }),
        Err(_) => json!({}),
    };
    let mut session = Session::open(server)?;
    let asked = session.ask(
        "tools/call",
        &json!({ "name": tool, "arguments": parts }),
        CALL_WAIT,
    );
    match asked {
        Ok(result) => Ok(report_of(&result)),
        Err(problem) => Err(problem),
    }
    // The session goes here, and with it the child server.
}

// What the answer of a tool means. `content` is the words the model reads; a
// picture or a file that came back is named and not carried, for the room of
// the talk is paid for by every word and the model was not offered a picture.
// `isError` says that the tool worked and what it worked on failed, which the
// model has to hear in order to try something else.
pub fn report_of(result: &Value) -> Report {
    let failed = result
        .get("isError")
        .or_else(|| result.get("is_error"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut text = String::new();
    if let Some(blocks) = result.get("content").and_then(|v| v.as_array()) {
        for block in blocks {
            let kind = block
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("text");
            match kind {
                "text" => {
                    let words = block.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    if !words.is_empty() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(words);
                    }
                }
                "image" | "audio" => {
                    let note = format!(
                        "[a {} came back and is not shown here]",
                        if kind == "image" { "picture" } else { "sound" }
                    );
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&note);
                }
                "resource_link" => {
                    let name = block
                        .get("uri")
                        .or_else(|| block.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("a resource of the server");
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!("[a resource stands at {}]", name));
                }
                "resource" => {
                    let inner = block.get("resource");
                    let words = inner
                        .and_then(|r| r.get("text"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    if words.is_empty() {
                        text.push_str("[a resource of the server came back with no words]");
                    } else {
                        text.push_str(words);
                    }
                }
                _ => {}
            }
        }
    }
    // A tool that answers with a shape of its own and no words of its own is
    // answered with that shape, for the model can read it.
    if text.trim().is_empty() {
        if let Some(made) = result.get("structuredContent") {
            if !made.is_null() {
                text = made.to_string();
            }
        }
    }
    if text.trim().is_empty() {
        text = String::from("The tool ran and gave nothing back");
    }
    Report {
        ok: !failed,
        output: cut(&text),
    }
}

// Keep the words of a run inside the room the talk can pay for.
fn cut(text: &str) -> String {
    let count = text.chars().count();
    if count <= GREATEST_OUTPUT {
        return text.to_string();
    }
    let mut kept: String = text.chars().take(GREATEST_OUTPUT).collect();
    kept.push_str(&format!(
        "\n... cut at {} characters of {}",
        GREATEST_OUTPUT, count
    ));
    kept
}

// ---------------------------------------------------------------- one server

// Look at one server: what it is called, and what it has to offer. The answer
// is the tools, or the reason the server could not be looked at.
pub fn look(server: &Server) -> Result<Vec<Tool>, String> {
    list(server)
}

// The words of the state line after a server was looked at.
pub fn looked_words(count: usize) -> String {
    if count == 1 {
        String::from("1 tool")
    } else {
        format!("{} tools", count)
    }
}

// ------------------------------------------------------------------ internal

// The folder the servers of the program itself stand in.
pub fn dir() -> PathBuf {
    config::paths().data.join("mcp")
}

// The folder of one server of the program itself.
pub fn folder(id: &str) -> PathBuf {
    dir().join(id)
}

// Take the folder of a server away. The sheet does this when the reader takes
// the server away, so the words of a server that is gone do not stand on the
// machine for ever.
pub fn remove_folder(id: &str) {
    let place = folder(&safe_id(id));
    let _ = std::fs::remove_dir_all(place);
}

// Only the characters an id of this program is made of are let into a path, so
// a name out of the sheet can never name a folder of the machine.
fn safe_id(id: &str) -> String {
    let clean: String = id
        .chars()
        .filter(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        .collect();
    if clean.is_empty() {
        return String::from("server");
    }
    clean.chars().take(40).collect()
}

// Whether the words of a server of the program itself bring their own way of
// speaking. A reader who writes a whole server - one that reads the library of
// MCP itself - is answered with their own words run as they stand, and the
// runner of this program is left out.
pub fn brings_its_own_server(code: &str) -> bool {
    for mark in [
        "from mcp", "import mcp", "from \"mcp\"", "MCPServer(", "FastMCP(", ".run_stdio",
        "mcp.run(",
    ] {
        if code.contains(mark) {
            return true;
        }
    }
    false
}

// The line that starts a server: the program, its arguments, and the folder to
// start it in. A server of the program itself is written out first, so the
// words of the reader are what runs.
fn start_line(server: &Server, kind: &str) -> Result<(String, Vec<String>, Option<PathBuf>), String> {
    if kind != KIND_INTERNAL {
        let mut parts = util::split_args(server.command.trim());
        if parts.is_empty() {
            return Err(String::from(
                "No line was written for the server to start with",
            ));
        }
        let program = parts.remove(0);
        Ok((program, parts, None))
    } else {
        write_internal(server).map(|(program, args, place)| (program, args, Some(place)))
    }
}

// Write the words of a server of the program itself next to the runner of this
// program, and give back the line that starts them. Two files stand in the
// folder of the server: the words of the reader, and the runner. They are
// written again on every start, so what runs is always what stands in the sheet.
fn write_internal(server: &Server) -> Result<(String, Vec<String>, PathBuf), String> {
    let place = folder(&safe_id(&server.id));
    std::fs::create_dir_all(&place).map_err(|problem| {
        format!(
            "The folder of the server could not be made: {}",
            problem
        )
    })?;
    // The two files are named to the server by the whole of their road, for the
    // server is started inside the folder they stand in and a road of its own
    // would be looked for from there.
    let place = place.canonicalize().unwrap_or(place);
    let program = interpreter(&server.command);
    if brings_its_own_server(&server.code) {
        let file = place.join("server.py");
        std::fs::write(&file, server.code.as_bytes())
            .map_err(|problem| format!("The server could not be written: {}", problem))?;
        return Ok((
            program,
            vec![file.display().to_string()],
            place,
        ));
    }
    let code = place.join("natter_code.py");
    std::fs::write(&code, server.code.as_bytes())
        .map_err(|problem| format!("The server could not be written: {}", problem))?;
    let runner = place.join("natter_runner.py");
    std::fs::write(&runner, RUNNER.as_bytes())
        .map_err(|problem| format!("The runner of the server could not be written: {}", problem))?;
    Ok((
        program,
        vec![runner.display().to_string(), code.display().to_string()],
        place,
    ))
}

// The program that reads the words of a server of the program itself. The
// reader may name one in the field of the command; with the field left empty,
// the machine is asked for the newest Python it has.
pub fn interpreter(named: &str) -> String {
    let named = named.trim();
    if !named.is_empty() {
        return named.to_string();
    }
    if let Ok(found) = std::env::var("AUTUMN_NATTER_PYTHON") {
        if !found.trim().is_empty() {
            return found.trim().to_string();
        }
    }
    python()
}

// The Python of this machine, found once and kept. A machine with neither of
// the two names is answered with the first of them, for the fault of a server
// that cannot be started is then told by the start of it, and the reader hears
// the reason in the sheet.
pub fn python() -> String {
    static HELD: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HELD.get_or_init(|| {
        for name in ["python3", "python"] {
            if util::capture(name, &["--version"], Duration::from_secs(5))
                .ok
                .eq(&true)
            {
                return String::from(name);
            }
        }
        String::from("python3")
    })
    .clone()
}

// The words a new server of the program itself begins with. The MCP tab of the
// sheet puts them in the field of the code when the reader moves the kind of a
// new server to that kind, so the shape the runner wants is seen before a word
// is written. The head of these words is written for an AI as much as for a
// person: a reader hands the whole of the field to an AI, and the AI writes a
// server that works from these words alone.
//
// The marks of a whole server, the other kind of server of the program, are
// kept out of these words on purpose. The program looks at the whole of a file
// for them, so one of them named in a doc string would move every new server of
// this kind into the other kind, where the runner is left out and the package
// it asks for is not installed. A test keeps them out.
pub const TEMPLATE: &str = r##"
"""An example server of the "Built in the program" kind of Autumn Natter.

This file is the whole of the server. Autumn Natter writes it beside a runner of
its own and starts the two of them together as a server that speaks MCP over its
standard input and its standard output. Nothing beyond the library that stands
with Python is needed.

The rules below are for an AI, and for the person who hands this file to one. A
server written to these rules works in this program. Keep the rules when a new
server is written, and keep this doc string at the head of it.

HOW THIS FILE IS RUN

  * Autumn Natter writes these words to a file, writes its runner beside them,
    and starts the runner with this file as its first argument. The runner reads
    the file and then answers the protocol.
  * Do not write a loop for the server, and do not read the standard input or
    write the standard output yourself. The runner does both. A line of ours
    among its messages breaks the talk with the program.
  * Do not reach for a library of the Model Context Protocol of your own. It is
    not installed here, and this program installs nothing. Do not write the name
    of that package, the name of a class of it, or the name of its run over the
    standard input anywhere in this file, not even in a comment. The program
    looks at the whole of a file for the marks of such a server, and one of them
    anywhere makes it run this file as a server of that kind, with the runner of
    the program left out.
  * The names tool and note already stand in the room of this file. Do not
    import them, and do not write over them.
  * Only the library that stands with Python may be imported. One import that
    fails takes the whole server away, and the reason stands in the sheet of
    settings.
  * Each look at this server starts a fresh process, and so does each call of
    one of its tools. The process is let go again when the answer came back, so
    nothing stands between two calls. Do not keep what a call found in a name of
    the file: give it back, and ask for it again on the next call.
  * Write no code that runs when the file is read. Such code is worked through
    before every call, and a fault in it takes the server away. Put the work
    inside a tool.
  * The server starts in the folder of this one server, under the data folder of
    the program. This file is the only file of the server, so do not open a file
    beside it. Words a tool needs stand inside this file.
  * A tool runs in the program, and not in the sandbox of a command, so it can
    reach the files of this machine and the network. Do that only when the
    reader asked for it.

WHAT BECOMES A TOOL

  * A function with @tool above it is a tool of the server.
  * When no function of the file holds @tool, every function of the file with a
    doc string of its own becomes a tool instead. So write @tool above the
    functions the model may use, or the model is offered the helpers too.
  * A tool is one function of this file. It is called with the parts of the call
    by their names, so it may not ask for what a call cannot give. A method of a
    class is not a tool, for self is never given.
  * @tool(name="another_name") gives the tool another name, and
    @tool(description="...") gives a description in the place of the doc string.
  * A helper of our own stands without @tool, so the model is never offered it.
    See the foot of this file.
  * The list of the tools is read when the server is saved. A new function, or a
    new name, needs the server saved again, or the button that asks the server
    what it has to offer.

WHAT THE MODEL IS TOLD ABOUT A TOOL

  * The doc string of the function is the description of the tool. Every line of
    it is joined into one line, so write one clear sentence: what the tool does,
    and what it gives back. A model reads one sentence better than two of them
    joined together.
  * The doc string is the only place the model is told anything. A comment of
    ours is sent nowhere at all.
  * Write a type for every argument: str, int, float, bool, list, dict. An
    argument with no type is offered to the model as a line of words.
  * An argument with no default is wanted, and the model must give it. An
    argument with a default may be left out.
  * A description of one argument is not read. When the name of an argument does
    not say what it is for, say it in the doc string of the function.
  * The parts come to the function as the model sent them, and nothing is turned
    into another kind first. A model that sends "12" for a number has sent
    words: turn the part over with int() or float(), and answer the model in
    words when it cannot be turned over.

WHAT A TOOL GIVES BACK

  * Give back a line of words. That is what the model reads.
  * Any other value is turned into JSON words, so a dict or a list is all right.
  * Give back nothing at all, and the model reads that the tool ran and gave
    nothing back.
  * Raise a fault when a call cannot be worked. The runner catches it and tells
    the model the last line of it, so raise ValueError("the words are empty")
    with a short reason of its own.
  * Words that are printed go to the error output of the process and come into
    the log of the program. They never reach the model. note("...") says the
    same thing on purpose.
  * The answer of a tool is cut at eight thousand characters. Answer with what
    the model needs, and no more.

THE TWO TOOLS BELOW

  * hello takes a name and gives back a line of words. It shows one argument the
    model must give and one it may leave out.
  * word_count takes words and gives back a shape. It shows a fault raised, a
    dict given back, and a helper called.
  * Read both, and the lines above them, before you write your own. Take both
    away when you have written one of your own, for a tool the model is offered
    is a tool the model may use.
"""

# The imports of the standard library stand here, and nothing else. None is
# needed below. The names tool and note come from the runner of the program and
# need no import of their own.


# The tools stand between these two lines. Every tool is one function with
# @tool above it, a doc string of one line for the model to read, and its work.
# Write your own tools here.


@tool
def hello(name: str, loud: bool = False) -> str:
    """Say hello to one person, and give back the line of words to show them."""
    said = "Hello, " + str(name).strip() + "!"
    return said.upper() if loud else said


@tool
def word_count(text: str, keep: int = 0) -> dict:
    """Count the words of some text, and give back the count and the first of them."""
    words = str(text).split()
    if not words:
        raise ValueError("No word came with the call, so there is nothing to count")
    held = words if int(keep) <= 0 else words[: int(keep)]
    return {"count": len(words), "words": one_run(held)}


# A helper of our own stands under the tools, without @tool, so the model is
# never offered it. A helper with a doc string is not offered either, once one
# function of the file carries the mark.


def one_run(words):
    # The words of a list in one line, cut short when the line grows long.
    line = " ".join(str(one) for one in words)
    if len(line) <= 200:
        return line
    return line[:200] + " ..."
"##;

// The program of the servers of the program itself. It is written beside the
// words of the reader and started with them, and it needs nothing but the
// library that stands with Python.
pub const RUNNER: &str = r##"
# The runner of a server of the program itself, of Autumn Natter.
#
# Autumn Natter writes this file beside the words of one server and starts the
# two of them as a server that speaks MCP over its standard input and its
# standard output. It needs nothing but the library that stands with Python.
#
# The words of the server stand in the file named as the first argument. Every
# function of them that carries the mark of @tool becomes a tool of the server.
# When no function carries the mark, every function of the words with a first
# line of its own becomes one, so a reader who pasted plain functions is not
# left without tools.
#
# One JSON object stands on one line, both ways, as the protocol of MCP wants.
# The words a tool prints are turned aside to the error output of the program,
# so nothing of them can be taken for a message of the protocol.

import importlib.util
import inspect
import json
import os
import sys
import traceback
import typing

PROTOCOL = "2025-06-18"
TOOLS = []
BEHIND = {}
FAULTS = []

# The output the protocol is written to. It is taken before the words of the
# server are read, because those words may move sys.stdout about.
OUT = sys.stdout


def tool(func=None, **mark):
    """Mark a function as a tool of this server: write @tool over it."""

    def register(chosen):
        add(chosen, mark.get("name"), mark.get("description"))
        return chosen

    if callable(func):
        return register(func)
    return register


def note(text):
    """Say something in the log of the program, and never to the protocol."""
    sys.stderr.write(str(text))
    if not str(text).endswith("\n"):
        sys.stderr.write("\n")
    sys.stderr.flush()


def json_type(hint):
    """The kind of an argument, in the words of JSON Schema."""
    if hint is None or hint is inspect.Parameter.empty:
        return "string"
    if hint is str:
        return "string"
    if hint is bool:
        return "boolean"
    if hint is int:
        return "integer"
    if hint is float:
        return "number"
    if hint in (list, tuple, set, frozenset):
        return "array"
    if hint is dict:
        return "object"
    args = [one for one in typing.get_args(hint) if one is not type(None)]
    origin = typing.get_origin(hint)
    if origin is not None:
        if origin in (list, tuple, set, frozenset):
            return "array"
        if origin is dict:
            return "object"
    if len(args) == 1:
        return json_type(args[0])
    return "string"


def one_line(text):
    """The words of a doc name, made of one run of spaces and no more."""
    return " ".join(str(text).split())


def add(chosen, named=None, described=None):
    """Put one function in the list of the tools of this server."""
    name = str(named or getattr(chosen, "__name__", "")).strip()
    if not name or name in BEHIND:
        return
    said = str(described).strip() if described else (inspect.getdoc(chosen) or "")
    shape = {"type": "object", "properties": {}, "required": []}
    try:
        hints = typing.get_type_hints(chosen)
    except Exception:
        hints = {}
    for key, part in inspect.signature(chosen).parameters.items():
        if part.kind in (part.VAR_POSITIONAL, part.VAR_KEYWORD):
            continue
        if part.name in ("self", "cls"):
            continue
        entry = {"type": json_type(hints.get(part.name, inspect.Parameter.empty))}
        if part.default is inspect.Parameter.empty:
            shape["required"].append(part.name)
        else:
            try:
                json.dumps(part.default)
                entry["default"] = part.default
            except Exception:
                pass
        shape["properties"][part.name] = entry
    TOOLS.append(
        {
            "name": name,
            "title": one_line(name.replace("_", " ")),
            "description": one_line(said) or ("Call " + name + "."),
            "inputSchema": shape,
        }
    )
    BEHIND[name] = chosen


def read(path):
    """Read the words of the server. The mark of @tool stands in their own room."""
    holder = {"tool": tool, "note": note, "__name__": "natter_server_code"}
    with open(path, "r", encoding="utf-8") as held:
        words = held.read()
    real = sys.stdout
    sys.stdout = sys.stderr
    try:
        exec(compile(words, path, "exec"), holder)
    finally:
        sys.stdout = real
    marked = list(BEHIND)
    if marked:
        return
    for key, value in holder.items():
        if key.startswith("_") or key in ("tool", "note") or not callable(value):
            continue
        if getattr(value, "__module__", None) != "natter_server_code":
            continue
        if not (inspect.getdoc(value) or "").strip():
            continue
        add(value)


def call(name, parts):
    """Run one tool and give back what the model is to read."""
    chosen = BEHIND.get(name)
    if chosen is None:
        return {"content": [{"type": "text", "text": "No tool of this server is called " + str(name) + "."}], "isError": True}
    given = {}
    for key, part in inspect.signature(chosen).parameters.items():
        if part.kind in (part.VAR_POSITIONAL, part.VAR_KEYWORD):
            continue
        if key in parts:
            given[key] = parts[key]
        elif part.default is inspect.Parameter.empty:
            return {"content": [{"type": "text", "text": "The tool " + name + " wants " + key + ", and nothing came with the call."}], "isError": True}
    real = sys.stdout
    sys.stdout = sys.stderr
    try:
        made = chosen(**given)
    except Exception:
        sys.stdout = real
        said = traceback.format_exc().strip().splitlines()
        return {"content": [{"type": "text", "text": "The tool " + name + " failed: " + (said[-1] if said else "nothing was said")}], "isError": True}
    finally:
        sys.stdout = real
    if made is None:
        text = "The tool ran and gave nothing back."
    elif isinstance(made, str):
        text = made
    else:
        try:
            text = json.dumps(made, indent=2, ensure_ascii=False, default=str)
        except Exception:
            text = str(made)
    return {"content": [{"type": "text", "text": text}]}


def answer(id, result):
    OUT.write(json.dumps({"jsonrpc": "2.0", "id": id, "result": result}, ensure_ascii=False))
    OUT.write("\n")
    OUT.flush()


def refuse(id, code, message):
    OUT.write(json.dumps({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}))
    OUT.write("\n")
    OUT.flush()


def main():
    if len(sys.argv) > 1:
        try:
            read(sys.argv[1])
        except Exception:
            FAULTS.append(traceback.format_exc())
            note("the server could not be read:\n" + FAULTS[-1])
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except Exception:
            continue
        asked = message.get("id")
        method = message.get("method")
        params = message.get("params") or {}
        if asked is None:
            # A saying and not a question: nothing answers it.
            continue
        if method == "initialize":
            answer(asked, {
                "protocolVersion": params.get("protocolVersion") or PROTOCOL,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": os.path.basename(sys.argv[1] if len(sys.argv) > 1 else "natter"), "version": "0.1.0"},
                "instructions": FAULTS[-1] if FAULTS else "",
            })
        elif method == "ping":
            answer(asked, {})
        elif method == "tools/list":
            answer(asked, {"tools": TOOLS})
        elif method == "tools/call":
            answer(asked, call(params.get("name", ""), params.get("arguments") or {}))
        elif method == "shutdown":
            answer(asked, {})
        else:
            refuse(asked, -32601, "the server has nothing called " + str(method))


if __name__ == "__main__":
    main()
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_of_a_tool_names_its_server() {
        assert_eq!(tool_name("bookshop", "search_books"), "mcp__bookshop__search_books");
        assert!(is_tool("mcp__bookshop__search_books"));
        assert!(!is_tool("run_terminal"));
        let (slug, tool) = split_tool("mcp__bookshop__lookup_book").expect("the parts of the name");
        assert_eq!(slug, "bookshop");
        assert_eq!(tool, "lookup_book");
        assert!(split_tool("mcp__bookshop").is_none());
        assert!(split_tool("run_terminal").is_none());
        // A name of a tool that holds the mark of two parts below keeps the
        // whole of itself, for the split is made at the first of them.
        let (_, tool) = split_tool("mcp__shop__a__b").expect("the parts of the name");
        assert_eq!(tool, "a__b");
        // The whole name stands inside the shape a chat template reads.
        let long = tool_name(&slug_of(&"a very long name of a server indeed"), &"t".repeat(90));
        assert!(long.chars().count() <= GREATEST_NAME);
        assert!(long.starts_with(NAME_MARK));
    }

    #[test]
    fn the_name_of_a_server_is_made_of_what_a_name_may_hold() {
        assert_eq!(slug_of("The Book Shop"), "the_book_shop");
        assert_eq!(slug_of("  --  "), "");
        assert_eq!(slug_of("Bookshop!!"), "bookshop");
        assert_eq!(slug_of("BookShop2"), "bookshop2");
        let first = Server {
            name: String::from("Bookshop"),
            slug: String::from("bookshop"),
            ..Server::default()
        };
        let second = Server {
            name: String::from("Bookshop"),
            ..Server::default()
        };
        assert_eq!(slug_for("Bookshop", &[]), "bookshop");
        assert_eq!(slug_for("Bookshop", &[&first]), "bookshop_2");
        assert_eq!(slug_for("", &[&first]), "server");
        // A name of nothing usable still gives a name no other server holds.
        assert_eq!(slug_for("!!!", &[&first, &second]), "server");
    }

    // The tools of two servers that share a name are told apart by the name of
    // the server, and the model is sent the schema the server gave.
    #[test]
    fn two_servers_of_one_tool_name_are_told_apart() {
        let one = Server {
            id: String::from("one"),
            name: String::from("One"),
            slug: String::from("one"),
            kind: String::from(KIND_STDIO),
            command: String::from("one-server"),
            ..Server::default()
        };
        let other = Server {
            id: String::from("two"),
            name: String::from("Two"),
            slug: String::from("two"),
            kind: String::from(KIND_HTTP),
            url: String::from("http://127.0.0.1:9/mcp"),
            ..Server::default()
        };
        let tool = Tool {
            name: String::from("search"),
            title: String::new(),
            description: String::from("Search the catalog."),
            schema: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        };
        let made = tool.definition(&one);
        assert_eq!(made["function"]["name"], "mcp__one__search");
        assert_eq!(made["function"]["parameters"]["required"][0], "query");
        assert_eq!(tool.definition(&other)["function"]["name"], "mcp__two__search");
        // The words of the server stand in the description, so the model knows
        // where the tool came from.
        assert!(made["function"]["description"]
            .as_str()
            .unwrap_or("")
            .contains("Search the catalog."));
    }

    // The shape of the parts of a tool. A server that gave a schema is believed;
    // a server that gave none is given an object of nothing, so the model may
    // still call the tool.
    #[test]
    fn the_shape_of_the_parts_comes_from_the_server() {
        let given = Tool {
            name: String::from("go"),
            schema: json!({"type":"object","properties":{"where":{"type":"string"}}}),
            ..Tool::default()
        };
        assert_eq!(schema_of(&given)["type"], "object");
        assert!(schema_of(&given)["properties"]["where"].is_object());
        let bare = Tool {
            name: String::from("go"),
            ..Tool::default()
        };
        assert_eq!(schema_of(&bare), json!({"type":"object","properties":{}}));
        let empty = Tool {
            name: String::from("go"),
            schema: json!({}),
            ..Tool::default()
        };
        assert_eq!(schema_of(&empty), json!({"type":"object","properties":{}}));
    }

    // The answer of a tool, read the way the protocol means it: the words of the
    // content, and the mark of a failure that worked and failed.
    #[test]
    fn the_answer_of_a_tool_is_read_as_the_protocol_means_it() {
        let good = report_of(&json!({
            "content": [{"type":"text","text":"Found 3 books"}],
            "isError": false,
        }));
        assert!(good.ok);
        assert_eq!(good.output, "Found 3 books");
        let failed = report_of(&json!({
            "content": [{"type":"text","text":"No book titled Solaris"}],
            "isError": true,
        }));
        assert!(!failed.ok);
        assert!(failed.output.contains("Solaris"));
        // A picture is named and not carried: the room of the talk is paid for
        // by every word, and no picture was offered to the model.
        let picture = report_of(&json!({
            "content": [
                {"type":"text","text":"Here is the swatch"},
                {"type":"image","data":"AAAA","mimeType":"image/png"},
            ],
        }));
        assert!(picture.ok);
        assert!(picture.output.contains("picture came back"));
        assert!(!picture.output.contains("AAAA"));
        // A tool that answers with a shape of its own and no words is answered
        // with that shape.
        let shaped = report_of(&json!({ "structuredContent": {"title":"Dune"} }));
        assert!(shaped.output.contains("Dune"));
        let nothing = report_of(&json!({"content": []}));
        assert!(nothing.output.contains("nothing back"));
    }

    // A run of words past the ceiling is cut, and says that it was.
    #[test]
    fn a_long_answer_is_cut_and_says_so() {
        let long = "x".repeat(GREATEST_OUTPUT + 1000);
        let made = cut(&long);
        assert!(made.chars().count() < long.chars().count());
        assert!(made.contains("cut at"));
    }

    // The words of `tools/list`. A server with nothing to offer is answered with
    // an empty list and not with a fault, and the two spellings of the shape of
    // the parts are both read.
    #[test]
    fn the_list_of_the_tools_of_a_server_is_read() {
        let list = tools_of(&json!({
            "tools": [
                {"name":"one","title":"One","description":"First.","inputSchema":{"type":"object"}},
                {"name":"two","input_schema":{"type":"object","properties":{}}},
                {"description":"a tool with no name, which is no tool"},
            ]
        }));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].shown(), "One");
        assert_eq!(list[1].shown(), "two");
        assert_eq!(list[0].schema["type"], "object");
        assert!(tools_of(&json!({})).is_empty());
    }

    // The words of a server of the program itself that bring their own way of
    // speaking are run as they stand; plain functions are run by the runner.
    #[test]
    fn a_whole_server_is_known_by_its_own_words() {
        assert!(brings_its_own_server(
            "from mcp.server import MCPServer\nmcp = MCPServer('Bookshop')\n"
        ));
        assert!(brings_its_own_server(
            "from mcp.server.fastmcp import FastMCP\napp = FastMCP()\n"
        ));
        assert!(!brings_its_own_server(
            "@tool\ndef add(a: int, b: int) -> int:\n    return a + b\n"
        ));
        assert!(!brings_its_own_server("import numpy as np\n"));
    }

    // The runner speaks the verbs of the protocol, and says so in words that a
    // reader of the file can find.
    #[test]
    fn the_runner_knows_the_verbs_of_the_protocol() {
        for verb in [
            "initialize",
            "tools/list",
            "tools/call",
            "jsonrpc",
            "inputSchema",
            "isError",
        ] {
            assert!(
                RUNNER.contains(verb),
                "the runner has nothing called {}",
                verb
            );
        }
        assert!(RUNNER.contains("def tool("));
    }

    // The words a new server of the program itself begins with are a server of
    // that kind: the mark of @tool over a function, a doc string of its own for
    // every tool, nothing that is not the library of Python, and the rules for
    // an AI at the head of it.
    #[test]
    fn the_template_is_a_server_of_the_program_itself() {
        // The marks of a whole server are the trap. One of them in the words of
        // the template would move every new server of this kind into the other
        // kind, where the runner is left out.
        assert!(
            !brings_its_own_server(TEMPLATE),
            "the template names a mark of a whole server, so it is not run by the runner"
        );
        assert!(TEMPLATE.contains("@tool"), "the template has no tool of its own");
        assert!(
            TEMPLATE.trim_start().starts_with("\"\"\""),
            "the template has no head of its own"
        );
        // The head is written for an AI, so it has to name the things an AI
        // would else guess wrong.
        for word in [
            "AI",
            "doc string",
            "library that stands with Python",
            "fresh process",
            "eight thousand",
            "int() or float()",
            "@tool(name=",
        ] {
            assert!(
                TEMPLATE.contains(word),
                "the template says nothing of {}",
                word
            );
        }
        // Every tool of the template has a doc string of one line, for that is
        // the whole of what the model is told about it.
        assert!(TEMPLATE.contains("Say hello to one person"));
        assert!(TEMPLATE.contains("Count the words of some text"));
        // A helper of our own, without the mark, so the model is not offered it.
        assert!(TEMPLATE.contains("def one_run(words):"));
    }

    // The template run for real, the way a new server of this kind is run: the
    // tools it offers, the shape of their parts, and the three ways a tool
    // answers.
    #[test]
    fn the_template_of_a_server_is_run_for_real() {
        let root = std::env::temp_dir().join("an-mcp-template-check");
        let held = root.join("settings");
        std::fs::create_dir_all(&held).expect("the place of the check is made");
        config::set_data_dir(held);
        let server = Server {
            id: String::from("templatecheck"),
            name: String::from("Template"),
            slug: String::from("template"),
            kind: String::from(KIND_INTERNAL),
            code: String::from(TEMPLATE),
            ..Server::default()
        };
        let tools = match list(&server) {
            Ok(list) => list,
            Err(problem) => {
                println!("no Python on this machine: {}", problem);
                assert!(problem.contains("could not be started") || problem.contains("Python"));
                return;
            }
        };
        let names: Vec<&str> = tools.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, vec!["hello", "word_count"], "the tools of the template");
        // The helper is not offered, and the doc string of a tool is what the
        // model reads. An argument with a default of its own is not wanted.
        assert!(!names.contains(&"one_run"));
        let hello = tools
            .iter()
            .find(|one| one.name == "hello")
            .expect("the tool of the template");
        assert!(hello.description.contains("Say hello to one person"));
        assert_eq!(hello.schema["properties"]["name"]["type"], "string");
        assert_eq!(hello.schema["properties"]["loud"]["type"], "boolean");
        assert_eq!(hello.schema["required"], json!(["name"]));
        // A line of words, a shape turned into words, and a fault told.
        let said = run(&server, "hello", r#"{"name":"leaf","loud":true}"#);
        assert!(said.ok, "{}", said.output);
        assert_eq!(said.output, "HELLO, LEAF!");
        let counted = run(&server, "word_count", r#"{"text":"one two three","keep":"2"}"#);
        assert!(counted.ok, "{}", counted.output);
        assert!(counted.output.contains("\"count\": 3"));
        assert!(counted.output.contains("one two"));
        let empty = run(&server, "word_count", r#"{"text":"   "}"#);
        assert!(!empty.ok, "a fault of a tool was taken for an answer");
        assert!(empty.output.contains("nothing to count"));
        let _ = std::fs::remove_dir_all(&root);
    }

    // A server is only reached when the one thing its kind needs stands in the
    // sheet, and the kind of a word that is none of the three is the stdio
    // server a new server of this program is.
    #[test]
    fn a_server_needs_the_one_thing_its_kind_asks_for() {
        let mut server = Server::default();
        assert_eq!(server.kind(), KIND_STDIO);
        assert!(!server.ready());
        server.url = String::from("http://127.0.0.1:8080/mcp");
        assert!(!server.ready(), "an address is nothing to a stdio server");
        server.command = String::from("  natter-mcp-demo  ");
        assert!(server.ready());
        server.kind = String::from("http");
        assert!(server.ready(), "an address is the whole of an HTTP server");
        server.url = String::new();
        assert!(!server.ready(), "an HTTP server with no address is not ready");
        server.kind = String::from("nonsense");
        assert_eq!(server.kind(), KIND_STDIO);
        server.kind = String::from(KIND_INTERNAL);
        assert!(!server.ready());
        server.code = String::from("@tool\ndef go() -> str:\n    return 'gone'\n");
        assert!(server.ready());
    }

    // Only the characters an id of this program is made of are let into the
    // place a server of the program itself is written to.
    #[test]
    fn the_place_of_a_server_is_named_safely() {
        assert_eq!(safe_id("a1b2c3d4e5"), "a1b2c3d4e5");
        assert_eq!(safe_id("../../etc"), "etc");
        assert_eq!(safe_id(".."), "server");
        assert_eq!(safe_id("a/b"), "ab");
    }

    // The address of a server on the network. Words that are no address of a
    // web page are refused, so a server of the sheet can never name a file of
    // the machine.
    #[test]
    fn the_address_of_a_server_is_an_address_of_the_web() {
        assert_eq!(address_of("http://127.0.0.1:8/mcp").unwrap(), "http://127.0.0.1:8/mcp");
        assert!(address_of("https://example.com/mcp").is_ok());
        assert!(address_of("").is_err());
        assert!(address_of("ftp://example.com").is_err());
        assert!(address_of("/tmp/server.py").is_err());
        assert!(address_of("file:///etc/passwd").is_err());
    }

    // The stream of a server that answers with `data:` lines is read as the
    // several messages it holds, and the answer of one object is read as one.
    #[test]
    fn the_two_ways_a_server_answers_are_both_read() {
        let one = messages("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}");
        assert_eq!(one.len(), 1);
        assert_eq!(one[0]["id"], 1);
        let stream = messages(
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n\n",
        );
        assert_eq!(stream.len(), 1);
        assert_eq!(stream[0]["result"]["tools"], json!([]));
        // A saying of the server, and the end of a stream, are passed by.
        assert!(messages("data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\ndata: [DONE]\n").len() == 1);
        assert!(messages("").is_empty());
    }

    // A fault of the protocol is told in words the reader can use.
    #[test]
    fn a_fault_of_the_protocol_is_told_in_words() {
        assert!(rpc_fault(&json!({"code":-32601,"message":"Method not found"}))
            .contains("no such thing"));
        assert!(rpc_fault(&json!({"code":-32000,"message":"nope"})).contains("-32000"));
        assert_eq!(rpc_fault(&json!({"message":"nope"})), "nope");
    }

    // A server that is not there is answered with words, and not with a fault
    // of the program: the model reads that the server could not be reached and
    // goes on with its talk.
    #[test]
    fn a_server_that_is_not_there_is_answered_with_words() {
        let gone = Server {
            id: String::from("gone"),
            name: String::from("Gone"),
            slug: String::from("gone"),
            kind: String::from(KIND_STDIO),
            command: String::from("natter-server-that-is-not-installed"),
            ..Server::default()
        };
        let report = run(&gone, "go", "{}");
        assert!(!report.ok);
        assert!(report.output.contains("Gone"));
        assert!(report.output.contains("could not be reached"));
    }

    // A whole look at a server of the program itself, over its standard input
    // and its standard output, with the runner of this program and the words of
    // a reader. This is the road a tool of an internal server takes to the
    // model, run for real.
    #[test]
    fn a_server_of_the_program_itself_is_run_for_real() {
        let root = std::env::temp_dir().join("an-mcp-runner-check");
        let _ = std::fs::remove_dir_all(&root);
        let held = root.join("settings");
        std::fs::create_dir_all(&held).expect("the place of the check is made");
        config::set_data_dir(held);
        let server = Server {
            id: String::from("runnercheck"),
            name: String::from("Runner"),
            slug: String::from("runner"),
            kind: String::from(KIND_INTERNAL),
            code: String::from(
                r#"
import json


@tool
def add(a: int, b: int) -> int:
    """Add two whole numbers."""
    return a + b


@tool
def greet(name: str, loud: bool = False) -> str:
    """Say hello to one person."""
    said = "hello " + name
    return said.upper() if loud else said


def helper(x):
    return x
"#,
            ),
            ..Server::default()
        };
        let tools = match list(&server) {
            Ok(list) => list,
            Err(problem) => {
                // A machine with no Python is told, and the check passes for
                // that reason and no other.
                println!("no Python on this machine: {}", problem);
                assert!(problem.contains("could not be started") || problem.contains("Python"));
                return;
            }
        };
        let names: Vec<&str> = tools.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, vec!["add", "greet"], "only the marked tools are offered");
        let add = tools
            .iter()
            .find(|one| one.name == "add")
            .expect("the tool the reader marked");
        assert!(add.description.contains("Add two whole numbers"));
        assert_eq!(add.schema["properties"]["a"]["type"], "integer");
        assert_eq!(add.schema["required"], json!(["a", "b"]));
        // The switch with a default of its own is not wanted, and the two words
        // of the call reach the function.
        let greet = tools
            .iter()
            .find(|one| one.name == "greet")
            .expect("the second tool");
        assert_eq!(greet.schema["required"], json!(["name"]));
        let made = run(&server, "greet", &json!({"name":"leaf","loud":true}).to_string());
        assert!(made.ok);
        assert_eq!(made.output, "HELLO LEAF");
        let sum = run(&server, "add", &json!({"a":20,"b":22}).to_string());
        assert_eq!(sum.output, "42");
        // A part the tool wants and the call did not bring is told, and the
        // model can hear it and ask again.
        let short = run(&server, "add", "{}");
        assert!(!short.ok);
        assert!(short.output.contains("wants a"));
        // A tool that fails says so and says why, and does not stop the server.
        let bad = run(&server, "nope", "{}");
        assert!(!bad.ok);
        assert!(bad.output.contains("no tool") || bad.output.contains("No tool"));
        let _ = std::fs::remove_dir_all(&root);
    }

    // The same kind of server again, and one of its tools run for real: the two
    // hands, the call, and the words the model is to read.
    #[test]
    fn a_tool_of_a_server_of_the_program_itself_runs_for_real() {
        let root = std::env::temp_dir().join("an-mcp-run-check");
        let held = root.join("settings");
        std::fs::create_dir_all(&held).expect("the place of the check is made");
        config::set_data_dir(held);
        let server = Server {
            id: String::from("runchannel"),
            name: String::from("Run"),
            slug: String::from("run"),
            kind: String::from(KIND_INTERNAL),
            code: String::from(
                r#"
@tool
def twice(word: str) -> str:
    """Say the word twice."""
    return word + " " + word
"#,
            ),
            ..Server::default()
        };
        let made = run(&server, "twice", r#"{"word":"ember"}"#);
        if made.output.contains("could not be started") {
            println!("no Python on this machine: {}", made.output);
            return;
        }
        assert!(made.ok, "{}", made.output);
        assert_eq!(made.output, "ember ember");
        // A tool that is not there is told in words, and the telling is a fault
        // of the run and not of the program.
        let missing = run(&server, "no_such_tool", "{}");
        assert!(!missing.ok, "a tool that is not there was taken for a run");
    }
}
