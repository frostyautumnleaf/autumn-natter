// A live check of the room the talk stands in, not for the repository.
//
// Two ways the handoff summary is come to, both against `probe/room/llama-server`:
//
//   * the model is writing, and the talk comes to the limit under the answer.
//     The writing is stopped, the room is made, and the answer goes on;
//   * the model calls a tool, and the receipt of that tool puts the talk past
//     the limit. The room is made before the model is asked again, so the ask
//     does not go out over the limit and come back with nothing in it.
//
//     cargo test -p autumn-natter-core --test live_room -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn wait_for<T>(what: &str, mut look: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(180);
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(Instant::now() < until, "waited too long for {}", what);
        sleep(Duration::from_millis(100));
    }
}

// The engine, the model file, and the folder the scripted server stands in.
fn stand_up(room: u32, mood: &str) -> (Engine, PathBuf) {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let scratch = root.join("probe/room");
    let data = root.join("uidata/live-room");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    std::fs::write(scratch.join("mode"), mood).expect("the mood of the server");
    config::set_data_dir(data);

    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("a model file to choose")
        .display()
        .to_string();
    let ms = ModelSettings {
        ctx_size: room,
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
    // The limit the settings stand at by default: the handoff summary, at a
    // share of the room.
    assert_eq!(cfg.chat_compaction, "handoff");
    assert_eq!(cfg.chat_compact_by, "percent");
    assert_eq!(cfg.chat_compact_percent, 85);
    (Engine::start(cfg), scratch)
}

// The answer is stopped while the model writes, the room is made out of a
// handoff summary, and the answer goes on after it.
#[test]
fn the_summarisation_kicks_in_while_the_model_is_writing() {
    let (engine, scratch) = stand_up(8192, "ramble");
    engine.new_chat();
    engine.prompt("Tell me the whole story of the war.".to_string(), Vec::new());

    // The mood is changed under the answer: as soon as the ask for the summary
    // stands at the server, the model has something to say at the next ask.
    let asks = scratch.join("asks.jsonl");
    let snap = wait_for("the answer to come to an end", || {
        let text = std::fs::read_to_string(&asks).unwrap_or_default();
        if text.contains("\"kind\": \"summary\"") {
            let _ = std::fs::write(scratch.join("mode"), "answer");
        }
        let s = engine.snapshot();
        (!s.generating
            && !s.busy
            && s.messages
                .iter()
                .any(|m| m.role == "Summary" && m.body.trim().len() > 20)
            && s.messages.iter().any(|m| {
                m.role == "Assistant" && m.body.contains("finished")
            }))
            .then_some(s)
    });
    println!("status: {}", snap.status);
    println!(
        "the chat: {:?}",
        snap.messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.body.chars().take(40).collect::<String>()))
            .collect::<Vec<_>>()
    );
    assert!(
        !snap.status.contains("not written") && !snap.status.contains("gave out"),
        "the engine said: {}",
        snap.status
    );
    let summary = snap
        .messages
        .iter()
        .find(|m| m.role == "Summary")
        .expect("a summarisation stands in the chat");
    assert!(
        summary.body.trim().len() > 20,
        "the summarisation is empty: {:?}",
        summary.body
    );
    // The writing of the ramble was stopped, not let run to the end.
    let log = std::fs::read_to_string(scratch.join("server.log")).unwrap_or_default();
    println!("server log:\n{}", log);
    assert!(
        log.contains("hung up after"),
        "the writing was never stopped:\n{}",
        log
    );
    // And the answer went on after the room was made.
    let last = snap.messages.last().expect("the chat did not end empty");
    assert_eq!(last.role, "Assistant");
    assert!(last.body.contains("finished"), "the answer never came: {:?}", last.body);
    engine.shutdown();
}

// A tool whose receipt puts the talk past the limit is followed by the room
// being made, and not by an ask that comes back with nothing in it.
#[test]
fn the_summarisation_kicks_in_after_a_tool_filled_the_room() {
    let (engine, scratch) = stand_up(16384, "tool");
    engine.new_chat();
    engine.set_code(true);
    engine.set_sandbox(false);
    // A message long enough that the talk stands under the limit before the
    // tool and past it after: the receipt of the tool is the longest thing in
    // the talk, and it is what puts the talk over the limit.
    let mut words = String::from("Here is the file I want changed. ");
    for n in 0..950 {
        words.push_str(&format!("Line {} of the file says something about the work.\n", n));
    }
    engine.prompt(words.clone(), Vec::new());

    let snap = wait_for("the room to be made after the tool, and the answer to come", || {
        let s = engine.snapshot();
        (!s.generating
            && !s.busy
            && s.messages.iter().any(|m| m.role == "Tool")
            && s.messages.iter().any(|m| m.role == "Summary")
            && s.messages
                .iter()
                .any(|m| m.role == "Assistant" && m.body.contains("finished")))
            .then_some(s)
    });
    println!("status: {}", snap.status);
    println!(
        "the chat: {:?}",
        snap.messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.body.chars().take(40).collect::<String>()))
            .collect::<Vec<_>>()
    );
    let told: Vec<String> = snap
        .messages
        .iter()
        .filter(|m| m.role == "Note")
        .map(|m| m.body.clone())
        .collect();
    assert!(
        !told.iter().any(|m| m.contains("neither words nor a call")),
        "the ask went out over the limit: {:?}",
        told
    );
    assert!(
        !snap.status.contains("not written") && !snap.status.contains("gave out"),
        "the engine said: {}",
        snap.status
    );
    let log = std::fs::read_to_string(scratch.join("server.log")).unwrap_or_default();
    println!("server log:\n{}", log);
    // The room was made before the ask that needed it: the mark of the
    // summarisation stands above the answer the model gave afterwards.
    let summary_at = snap
        .messages
        .iter()
        .position(|m| m.role == "Summary")
        .expect("a summarisation stands in the chat");
    let answer_at = snap
        .messages
        .iter()
        .position(|m| m.role == "Assistant" && m.body.contains("finished"))
        .expect("the answer came");
    assert!(
        summary_at < answer_at,
        "the room was not made before the answer was asked for"
    );
    assert_eq!(
        snap.messages.iter().filter(|m| m.role == "Summary").count(),
        1,
        "more than one room was made for one answer"
    );
    engine.shutdown();
}
