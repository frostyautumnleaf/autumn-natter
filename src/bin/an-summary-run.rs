
// A live run of the engine against a real model, for the handoff summary.
//
// The engine drives llama-server as a window does. This program sends a long
// working chat through it, with the summarisation threshold stood low enough
// that the room runs short again and again, and reads back what the window
// would have shown: how many summarisations came, whether the model went on
// writing after each of them, and what the meter of the talk read.
//
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;

fn arg(name: &str, fallback: &str) -> String {
    let argv: Vec<String> = std::env::args().collect();
    if let Some(at) = argv.iter().position(|a| a == name) {
        if argv.len() > at + 1 {
            return argv[at + 1].clone();
        }
    }
    String::from(fallback)
}

fn number(name: &str, fallback: i64) -> i64 {
    arg(name, &fallback.to_string()).parse().unwrap_or(fallback)
}

// A turn of the talk. Each one makes the model call a tool of the program and
// answer what came back, so a call of a tool stands in the talk and has to be
// answered again after a handoff summary -- which is the case that left the chat
// standing at the heading of a summarisation with no answer after it.
// How long the program waits for the engine to move before it calls the chat
// dead. A model that is thinking, or a summary that is being written, moves the
// clock again; a chat that stands still for this long had nothing to go on with.
const STALL_SECONDS: u64 = 300;

const TURNS: [&str; 12] = [
    "Remember this number for the whole chat: 4127. Then run this command with the terminal tool: echo one > notes.txt. Name the file you wrote.",
    "Run this command with the terminal tool: echo two >> notes.txt. Then name the file you changed.",
    "Run this command with the terminal tool: cat notes.txt. Read the lines it printed back to me.",
    "Make a file called todo.txt with the create_file tool. Put three items in it and name them.",
    "Run this command with the terminal tool: cat todo.txt. Read the three items back to me.",
    "Run this command with the terminal tool: echo three >> notes.txt. Say whether it worked.",
    "Run this command with the terminal tool: ls. Name every file you can see.",
    "Run this command with the terminal tool: cat notes.txt. How many lines are in it?",
    "Run this command with the terminal tool: wc -c notes.txt todo.txt. Read both numbers back to me.",
    "Run this command with the terminal tool: tail -n 2 notes.txt. Read back what it printed, and say the number I asked you to remember at the start of this chat.",
    "Run this command with the terminal tool: head -n 3 notes.txt. Read it back, name every file in this folder, and say the number again.",
    "Say in one line the number I asked you to remember at the start of this chat, and every file we have worked on.",
];

// The last lines of the log of the model server. The engine keeps ten of them,
// one per start, and the newest of them says what the server of this run said.
fn tail_server_log(data: &std::path::Path, wanted: usize) -> String {
    let dir = data.join("logs");
    let mut names: Vec<_> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries.flatten().map(|e| e.path()).collect(),
        Err(_) => return String::from("no logs"),
    };
    names.sort();
    names.reverse();
    let Some(newest) = names.first().cloned() else {
        return String::from("no logs");
    };
    let text = std::fs::read_to_string(&newest).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(wanted);
    format!(
        "{}\n{}",
        newest.display(),
        lines[from..].join("\n")
    )
}

fn main() {
    let data = PathBuf::from(arg("--data", "/tmp/anlive2/run"));
    let turns = number("--turns", 20).max(1) as usize;
    let reserve = number("--reserve", 600) as u32;
    let ctx_size = number("--ctx", 8192) as i32;
    let percent = number("--percent", 0) as u32;
    let tools_on = arg("--tools", "on") == "on";
    let _thinking = arg("--thinking", "off");

    let _ = std::fs::create_dir_all(&data);
    for part in ["chats", "repo_chats", "logs", "repositories", "models"] {
        let _ = std::fs::create_dir_all(data.join(part));
    }
    for entry in std::fs::read_dir(data.join("chats")).unwrap().flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
    config::set_data_dir(data.clone());
    let _ = config::ensure_dirs();

    let mut cfg = Config::default();
    cfg.llama_dir = arg("--llama-dir", "/tmp/anlive2");
    cfg.models_dir = arg("--models-dir", "/tmp/anlive2/models");
    cfg.chat_compaction = String::from("handoff");
    cfg.chat_compact_reserve = reserve;
    cfg.chat_compact_percent = if percent > 0 { percent } else { 85 };
    cfg.chat_compact_by = String::from(if percent > 0 { "percent" } else { "tokens" });
    cfg.chat_date_new_chat = false;
    cfg.chat_max_tool_steps = -1;
    cfg.chat_max_answer_steps = -1;
    let engine = Engine::start(cfg);
    let snap = engine.snapshot();
    let Some(model) = snap.models.first().cloned() else {
        eprintln!("no model in the models folder");
        std::process::exit(2);
    };
    let mut settings = ModelSettings::default();
    settings.ctx_size = ctx_size as u32;
    settings.parallel = 1;
    settings.batch_size = 512;
    settings.ubatch_size = 128;
    settings.cache_ram = 0;
    settings.ctx_checkpoints = 0;
    // The testing model thinks a great deal before it writes. A ceiling over the
    // thinking keeps an ask that needs no thinking at all -- the ask for a
    // summary -- from spending the room of the talk on thinking.
    settings.reasoning_budget = 6000;
    settings.no_context_shift = true;
    // The llama-server of the box is a CUDA build and the card of the box is
    // busy with the server of the user, so the model is kept off the card.
    settings.extra_args = String::from("--device none -ngl 0 --fit off");
    engine.select_model(model.id());
    // The card of the box is busy with the server of the user, and its free
    // memory reads as nought, so a CUDA start of a second server dies of it. Every
    // device of the engine is therefore switched off here, so the model of the run
    // is kept on the CPU throughout.
    engine
        .state
        .lock()
        .map(|mut guard| {
            for card in guard.gpus.iter_mut() {
                card.enabled = false;
            }
        })
        .ok();
    say(&format!(
        "model {} ctx {} reserve {} by {} tools {}",
        model.name,
        ctx_size,
        reserve,
        if percent > 0 { "percent" } else { "tokens" },
        tools_on
    ));

    // The badge of the tools stands on for the chat, so a call of a tool stands
    // in the talk and has to be answered again after a summarisation.
    engine.set_code(tools_on);

    // One chat, opened by its id: the first send grows it, and every send after
    // it goes back into the same chat, as it does in the window.
    let mut chat_id = String::new();
    let began = Instant::now();
    let mut asked = 0usize;
    let mut stall_at = Instant::now();
    let mut last_marks = 0usize;
    let mut dead_line: Option<usize> = None;

    loop {
        let (messages, marks) = read_chat(&data, &chat_id);
        let answers = messages
            .iter()
            .filter(|m| m["role"].as_str() == Some("Assistant"))
            .count();
        if marks > last_marks {
            last_marks = marks;
            stall_at = Instant::now();
            say(&format!(
                "[{:6.1}s] summarisation #{} came. The talk now takes {} of {}. Status {:?}",
                began.elapsed().as_secs_f64(),
                marks,
                context_used(&data, &chat_id),
                ctx_size,
                engine.snapshot().status
            ));
        }
        let snap = engine.snapshot();
        if !snap.generating && !snap.busy {
            // The engine stands ready. The answer of the send in hand is in the
            // chat, so the next one may go out, until the turns run out.
            if answers >= asked && asked < turns {
                let words = TURNS[asked % TURNS.len()].to_string();
                say(&format!(
                    "[{:6.1}s] send {} of {}: {} (talk takes {} of {}, marks {})",
                    began.elapsed().as_secs_f64(),
                    asked + 1,
                    turns,
                    first_line(&words),
                    context_used(&data, &chat_id),
                    ctx_size,
                    marks
                ));
                engine.prompt(words, Vec::new());
                asked += 1;
                if chat_id.is_empty() {
                    chat_id = engine.snapshot().chat_id.clone();
                }
                stall_at = Instant::now();
            } else if answers >= asked && asked >= turns {
                // Everything asked for came to an end.
                break;
            }
        }
        if stall_at.elapsed() > Duration::from_secs(STALL_SECONDS) {
            say(&format!(
                "[{:6.1}s] NOTHING MOVED for {} seconds after {} sends: status {:?} generating {} busy {} summarising {} marks {} answers {}",
                began.elapsed().as_secs_f64(),
                STALL_SECONDS,
                asked,
                snap.status,
                snap.generating,
                snap.busy,
                snap.summarising,
                marks,
                answers
            ));
            dead_line = Some(asked);
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    // Let the last answer come to its end.
    let until = Instant::now() + Duration::from_secs(240);
    while Instant::now() < until {
        let snap = engine.snapshot();
        if !snap.generating && !snap.busy {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let snap = engine.snapshot();
    println!("--- final state after {:.1}s", began.elapsed().as_secs_f64());
    say(&format!(
        "status {:?} context {} of {}",
        snap.status, snap.context_used, snap.context_total
    ));
    println!("marks of the summarisation: {}", snap.handoffs.len());
    for mark in &snap.handoffs {
        say(&format!(
            "  #{} {} before {} after {} why {:?} snippet {:?}",
            mark.number,
            mark.state,
            mark.before,
            mark.after,
            mark.why,
            short(&mark.snippet)
        ));
    }
    println!("messages the window is given: {}", snap.messages.len());
    for (at, message) in snap.messages.iter().enumerate() {
        say(&format!(
            "  {:3} {:9} {:?} ctx={}",
            at,
            message.role,
            short(&message.body),
            message.context_after
        ));
    }
    // The chat on disk, with every message of it, hidden ones included.
    let chats = data.join("chats");
    if let Ok(entries) = std::fs::read_dir(&chats) {
        for entry in entries.flatten() {
            let raw = std::fs::read_to_string(entry.path()).unwrap_or_default();
            let value: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
            let Some(items) = value["messages"].as_array() else { continue };
            println!("--- chat file {}", entry.file_name().to_string_lossy());
            for (at, message) in items.iter().enumerate() {
                say(&format!(
                    "  {:3} {:9} body {:?}",
                    at,
                    message["role"].as_str().unwrap_or("?"),
                    short(
                        message["body"]
                            .as_str()
                            .unwrap_or_default()
                    )
                ));
            }
        }
    }
    // What the model server said last. A run that stood still is read best from
    // the server of it: the fault is either in the ask the engine sent or in the
    // answer the server gave back, and only the log of the two says which.
    println!("--- server log\n{}", tail_server_log(data.as_path(), 20));
    if let Some(turn) = dead_line {
        println!("RESULT stalled after send {}", turn);
    } else {
        println!("RESULT all {} sends came to an end", asked);
    }
}


// The messages of one chat of the data folder, as they stand on disk.
fn read_chat(data: &std::path::Path, id: &str) -> (Vec<serde_json::Value>, usize) {
    if id.is_empty() {
        return (Vec::new(), 0);
    }
    let path = data.join("chats").join(format!("{}.json", id));
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return (Vec::new(), 0);
    };
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    let items = value["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let marks = items
        .iter()
        .filter(|m| m["role"].as_str() == Some("Summary"))
        .count();
    (items, marks)
}

// The number the meter of the talk stands on for one chat of the disk.
fn context_used(data: &std::path::Path, id: &str) -> u64 {
    if id.is_empty() {
        return 0;
    }
    let path = data.join("chats").join(format!("{}.json", id));
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return 0;
    };
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    value["meta"]["context_used"].as_u64().unwrap_or(0)
}

// Write a line out at once. The log of a run is read while the run is going on,
// so words that stood in the buffer of the program were not of much use.
fn say(text: &str) {
    println!("{}", text);
    let _ = std::io::stdout().flush();
}


fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let mut out: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        out.push('.');
    }
    out
}

fn short(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<&str>>().join(" ");
    let mut out: String = flat.chars().take(70).collect();
    if flat.chars().count() > 70 {
        out.push('\u{2026}');
    }
    out
}
