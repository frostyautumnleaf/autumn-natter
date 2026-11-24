// A live check that the reading of the oldest part of a conversation is never waited for without
// an end. Not for the repository.
//
// When a conversation has outgrown the share of the window a request for a handoff summary may
// carry, the oldest part of it is read on its own first and written short, and that read is one
// ask of the server. That ask names the slot of the chat, and a server that is asked for a slot
// it is still working in takes the request and holds it, sending nothing at all back: the face of
// it is a chat saying that the oldest part is being read, with nothing read and nothing written,
// for as long as the server holds it. The server under test here does exactly that -- it is
// `probe/foldstall/llama-server`, and it never answers the read -- so what the check is of is the
// program, and not of a server:
//
//   * the read is waited out at a stated time and not for ever, and the words on the line say
//     that it could not be done;
//   * the summary is written from the rest of the conversation all the same, and kept;
//   * the answer the user asked for comes after it, so the chat is not left standing in the
//     middle of making space for itself.
//
// The wait of the read is shortened with `AUTUMN_NATTER_FOLD_WAIT_SECS`, which nothing sets in
// the program of a user; see `engine::fold_ask_wait`.
//
//     cargo test -p autumn-natter-core --test live_summary_stall -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

// The shortened wait of the read of the oldest part, in seconds.
const FOLD_WAIT: &str = "4";
// The longest the whole of the turn may take. The wait of the read is four seconds of it, so a
// run that went over this bound was waited for by something else.
const WHOLE: Duration = Duration::from_secs(120);

// The conversation is weighed on characters: four of them go to a token. At a window of 8192 the
// request for a summary is kept within 70% of it, which is 5734 tokens, and one read of the
// oldest part is kept within 700 less than that. The limit the summarisation comes at is 85% of
// the window, which is 6963 tokens. The first message is small enough to go out and be answered,
// and small enough with the lines of the program at the head of the conversation for the read of
// the oldest part to reach it; the second brings the conversation past the limit, and is big
// enough that the oldest part of it - the first message, with the answer that came back to it -
// has to be read on its own before the summary is asked for.
const ONE_OF_IT: &str = "The databases were laid under the data folder, and the work directories \
                         were left out of them; the migrations ran twice and the logs remained \
                         files. ";
const THE_REST_OF_IT: &str = "The planning contents moved into the chats database, and the \
                              loose chat work directories stayed on the disk as work directories \
                              only. ";

fn text_of(least: usize, words: &str) -> String {
    let mut said = String::new();
    while said.chars().count() < least {
        said.push_str(words);
    }
    said
}

// The words the line of the state has said so far, gathered while the turn goes on. Some of them
// are on the line for a moment only, between two asks, and are missed by a look taken after the
// turn is over.
#[derive(Default)]
struct Lines {
    said: Vec<String>,
}

impl Lines {
    fn look(&mut self, engine: &Engine) {
        let snap = engine.snapshot();
        if !snap.status.is_empty() && self.said.last().map(String::as_str) != Some(snap.status.as_str())
        {
            self.said.push(snap.status.clone());
        }
    }
}

fn holds(lines: &Lines, needle: &str) -> bool {
    lines.said.iter().any(|one| one.contains(needle))
}

// The messages of the model that are in the chat.
fn answers(engine: &Engine) -> usize {
    engine
        .snapshot()
        .messages
        .iter()
        .filter(|one| one.role == "Assistant" && one.body.contains("The work is going on"))
        .count()
}

#[test]
fn the_oldest_part_of_a_conversation_is_never_waited_for_for_ever() {
    let root = PathBuf::from("..").canonicalize().expect("the root of the work");
    let scratch = root.join("probe/foldstall");
    let data = root.join("uidata/live-summary-stall");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    config::set_data_dir(data.clone());
    // The wait of the read, shortened for this check alone.
    std::env::set_var("AUTUMN_NATTER_FOLD_WAIT_SECS", FOLD_WAIT);

    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("a model file to choose")
        .display()
        .to_string();
    let ms = ModelSettings {
        ctx_size: 8192,
        max_tokens: 200,
        gpu_enabled: vec![false; 8],
        ..Default::default()
    };
    let mut cfg = Config {
        llama_dir: scratch.display().to_string(),
        models_dir: root.join("testing_model").display().to_string(),
        model_id: model.clone(),
        ..Default::default()
    };
    cfg.model_settings.insert(model, ms);
    assert_eq!(cfg.chat_compaction, "handoff");
    assert!(cfg.chat_context_files, "the asks name the slot of the chat");

    let engine = Engine::start(cfg);
    let mut lines = Lines::default();
    engine.new_chat();
    // The first message, answered at once: it is small enough for the window, and it is the part
    // of the conversation that the server is later asked to read on its own.
    engine.prompt(text_of(6_000, ONE_OF_IT), Vec::new());
    let first = Instant::now();
    loop {
        lines.look(&engine);
        let snap = engine.snapshot();
        if !snap.generating && !snap.busy && answers(&engine) >= 1 {
            break;
        }
        assert!(
            first.elapsed() < WHOLE,
            "waited for the first message to be answered; the line said {:?}",
            lines.said,
        );
        sleep(Duration::from_millis(100));
    }
    // The second message brings the conversation past the limit set for the summarisation, and
    // the making of the space begins: the oldest part of the conversation is read on its own
    // first, and that is the read this server holds with nothing at all sent back.
    let began = Instant::now();
    engine.prompt(text_of(22_000, THE_REST_OF_IT), Vec::new());
    // The chat comes to rest with a summary written in it. The read of the oldest part is held by
    // the server, so the summary comes after the wait of that read.
    let snap = loop {
        lines.look(&engine);
        let snap = engine.snapshot();
        if !snap.generating
            && !snap.busy
            && snap
                .messages
                .iter()
                .any(|one| one.role == "Summary" && !one.body.trim().is_empty())
        {
            break snap;
        }
        assert!(
            began.elapsed() < WHOLE,
            "waited {} seconds for the summary; the line said {:?}. The read of the oldest part \
             is held by the server for the whole of that time, so the wait of it is what is being \
             checked",
            began.elapsed().as_secs(),
            lines.said,
        );
        sleep(Duration::from_millis(200));
    };
    let took = began.elapsed();
    println!(
        "the turn came to rest in {} seconds. The line said:",
        took.as_secs()
    );
    for one in &lines.said {
        println!("  {}", one);
    }
    assert!(
        took < WHOLE,
        "the making of the space took {} seconds with a wait of the read of {} seconds",
        took.as_secs(),
        FOLD_WAIT,
    );

    // The read of the oldest part was asked for, and it is the ask the server held.
    let asks = std::fs::read_to_string(scratch.join("asks.jsonl")).unwrap_or_default();
    let folds = asks
        .lines()
        .filter(|one| one.contains("\"kind\": \"fold\""))
        .collect::<Vec<&str>>();
    assert!(
        !folds.is_empty(),
        "the oldest part of the conversation was never read on its own, so nothing was held: {asks}"
    );
    // Every read of the oldest part asked the server for the state of its prompt work. Without
    // that the server says nothing at all while it works the part through, so a read that is slow
    // because the part is big reads as a server that took the request and is not answering -- and
    // the wait of the read would then be set to cut off work that was going on.
    for one in &folds {
        assert!(
            one.contains("\"progress\": true"),
            "a read of the oldest part was sent without asking for the state of the prompt work: {one}"
        );
    }
    // The summary was written once, out of the conversation with the part that could not be read
    // folded by hand.
    let summaries = asks.matches("\"kind\": \"summary\"").count();
    assert_eq!(
        summaries, 1,
        "the summary was asked {} times over one conversation that could not be read whole",
        summaries
    );
    let summary = snap
        .messages
        .iter()
        .find(|one| one.role == "Summary" && !one.body.trim().is_empty())
        .expect("a handoff summary stands in the chat");
    assert!(
        summary.body.contains("databases were laid"),
        "the summary the server wrote was not kept: {}",
        summary.body
    );
    assert!(
        !snap.status.contains("not written") && !snap.status.contains("Start it again"),
        "the summarisation was given up when the read of the oldest part could not be done: {}",
        snap.status
    );

    // The words that tell the user the read could not be done are on the line of the state, or in
    // the log of the program. The line goes its own way as the asks come and go, so the log is
    // the second place they may stand.
    let log = program_log(&data);
    assert!(
        holds(&lines, "could not be read on its own")
            || log.contains("was not read on its own"),
        "nothing was said about the read that could not be done. The line said {:?}, and the log \
         of the program holds:",
        lines.said,
    );

    // And the chat went on to answer, which is the whole point: the message the user sent was
    // answered behind the summary, and not left standing in the middle of the space being made.
    let answered = loop {
        lines.look(&engine);
        let snap = engine.snapshot();
        // The answer behind the summary is the second message of the model in the chat: the first
        // is the answer to the message that was small enough for the window.
        if answers(&engine) >= 2 {
            break true;
        }
        if !snap.generating
            && !snap.busy
            && snap.messages.iter().any(|one| {
                one.role == "Note" && one.body.contains("no space could be made")
            })
        {
            break false;
        }
        if began.elapsed() >= WHOLE {
            break false;
        }
        sleep(Duration::from_millis(200));
    };
    assert!(
        answered,
        "the answer never came behind the summary. The line said {:?}. The server log holds:\n{}",
        lines.said,
        std::fs::read_to_string(scratch.join("server.log")).unwrap_or_default(),
    );
    let _ = snap;
    engine.shutdown();
}

// The log of the program of this run, which the engine writes into the folder of the data. The
// newest of them is the run this check made, for the folder was made afresh.
fn program_log(data: &PathBuf) -> String {
    let place = data.join("logs");
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    let Ok(entries) = std::fs::read_dir(&place) else {
        return String::new();
    };
    for entry in entries.flatten() {
        let name = entry.file_name().display().to_string();
        if !name.starts_with("program-") {
            continue;
        }
        let made = entry
            .metadata()
            .and_then(|one| one.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if newest.as_ref().map(|(one, _)| made >= *one).unwrap_or(true) {
            newest = Some((made, entry.path()));
        }
    }
    let (_, file) = newest.unwrap_or((std::time::SystemTime::UNIX_EPOCH, PathBuf::new()));
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    println!("the log of the program is {}, of it:", file.display());
    for one in text.lines().rev().take(40) {
        println!("  {}", one);
    }
    text
}
