// A live check of a call cut short, and of the call the model wrote after the
// room was made -- not for the repository.
//
// The server here is not llama.cpp but `probe/cut2/llama-server`: a script that
// answers an ask offering tools with a call whose words never stop coming, and
// answers an ask that ends on the handoff block of a summarisation with a smaller
// call that does come to an end. That is what a model does with the room a
// handoff summary just made, and the whole of what the reader was owed is that the
// answer goes on after the room: the small call is let come, it runs, and an
// answer of the step stands after it.
//
// The check also reads the two asks the fault turned on. The ask for the summary
// carries the telling of the call that was stopped, for the block of that call
// stands where the writing stopped and the summary is written of the talk as it
// stands. The ask that follows the mark carries the telling no more: the mark cuts
// it off with the rest of the words the summary stands for.
//
//     cargo test --test live_cut_twice -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

// Wait until a look gives an answer, or until five minutes have passed.
fn wait_for<T>(what: &str, mut look: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(300);
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(Instant::now() < until, "waited too long for {}", what);
        sleep(Duration::from_millis(200));
    }
}

#[test]
fn a_call_cut_short_is_followed_by_an_answer_and_not_by_a_turn_with_none() {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let scratch = root.join("probe/cut2");
    let data = root.join("uidata/live-cut-twice");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    config::set_data_dir(data);

    // The model file is only wanted so the engine has a model to choose: the
    // server under test is the script, and it reads no model at all.
    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("a model file to choose")
        .display()
        .to_string();
    // A small room, so a few hundred words of a file run past the limit of it.
    // The room has to hold the words of the program at the head of the talk and
    // the telling of the call that was stopped besides, for the ask for the
    // summary carries the talk as it stands: a room that is too small for both
    // gives no room for the summary to be written in.
    let ms = ModelSettings {
        ctx_size: 2048,
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
    assert_eq!(cfg.chat_compact_by, "percent");
    assert_eq!(cfg.chat_compact_percent, 85);

    let engine = Engine::start(cfg);
    engine.new_chat();
    engine.set_code(true);
    engine.prompt(
        "Write the file notes/big.md with the edit_file tool.".to_string(),
        Vec::new(),
    );

    // The answer comes to an end. Before the fix it ended under the block of the
    // second call with nothing of an answer in it.
    let snap = wait_for("the answer to come to an end", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy && s.messages.iter().any(|m| m.cancelled)).then_some(s)
    });
    println!("status: {}", snap.status);
    println!(
        "the chat: {:?}",
        snap.messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.body.chars().take(60).collect::<String>()))
            .collect::<Vec<String>>()
    );

    // The call that never ran stands before the summarisation, where the writing
    // stopped, and not under the heading of it.
    let cut_at = snap
        .messages
        .iter()
        .position(|m| m.cancelled)
        .expect("a call stands cancelled");
    let summary_at = snap
        .messages
        .iter()
        .position(|m| m.role == "Summary" && !m.body.trim().is_empty())
        .expect("the room was made by a handoff summary");
    assert!(
        cut_at < summary_at,
        "the block of the call stands under the summarisation"
    );

    // The whole of the fault: the model gave no answer, and the chat stood under
    // the block of the call with a line saying so.
    let quiet = snap
        .messages
        .iter()
        .filter(|m| m.body.contains("the model gave no answer"))
        .count();
    assert_eq!(quiet, 0, "the turn with no answer of the model stands in the chat");
    assert!(
        !snap.status.contains("not written") && !snap.status.contains("cannot"),
        "the engine said: {}",
        snap.status
    );

    // The asks, as a server saw them. The ask for the summary carries the telling
    // of the call that never ran, and the ask that follows the mark carries it no
    // more.
    let asks = wait_for("the asks of the server", || {
        let text = std::fs::read_to_string(scratch.join("asks.jsonl")).unwrap_or_default();
        (text.matches("\"kind\"").count() >= 3).then_some(text)
    });
    let lines: Vec<&str> = asks.lines().filter(|l| !l.trim().is_empty()).collect();
    println!("the asks: {}", lines.len());
    // The asks are read by what they were, for a summary the room did not like is
    // asked again with the short prompt of the settings, and the count of the asks
    // is not a thing a check should lean on.
    let asked_for = |what: &str| {
        lines
            .iter()
            .find(|line| line.contains(&format!("\"kind\": \"{}\"", what)))
            .copied()
            .unwrap_or_default()
            .to_string()
    };
    let first = asked_for("call");
    let summary = asked_for("summary");
    let twice = asked_for("twice");
    assert!(!first.is_empty(), "no ask for an answer: {}", lines.len());
    assert!(!summary.is_empty(), "no ask for a summary");
    assert!(
        !twice.is_empty(),
        "the ask that ends on the handoff block was never made"
    );
    assert!(
        summary.contains("A tool call was cancelled"),
        "the ask for the summary carried no telling of the call"
    );
    assert!(
        !twice.contains("A tool call was cancelled"),
        "the telling of the call went back to the model after the summarisation"
    );
    // The ask after the room was made ends on the handoff block, which says in its
    // own words that the work is to be gone on with. No word of the program rides
    // behind it.
    let third: serde_json::Value = serde_json::from_str(&twice).expect("an ask of the server");
    let items = third["messages"].as_array().expect("the talk of the ask");
    let last = items.last().expect("a talk with a last message");
    assert_eq!(
        last["role"].as_str().expect("a role"),
        "user",
        "the ask after the room was made did not end on the handoff block"
    );
    assert!(
        last["content"]
            .as_str()
            .unwrap_or_default()
            .starts_with("[The session was compacted"),
        "the ask after the room was made did not end on the handoff block: {}",
        last
    );
    // Nothing of the call that never ran goes back as a call: the words of it stop
    // in the middle, and a server set to read arguments out of them would fault
    // over them.
    assert!(
        !twice.contains("tool_calls"),
        "the telling of the first call went back to the server as a call"
    );

    // The second call came to an end and was run, so its block stands after the
    // telling of the first one. This is the whole of what the reader was owed: the
    // chat did not stop under the block of a call that never ran.
    let ran = snap
        .messages
        .iter()
        .skip(cut_at + 1)
        .filter(|m| m.role == "Tool" && !m.cancelled)
        .count();
    assert!(
        ran > 0,
        "nothing ran after the cancelled call, so the answer stopped there"
    );

    engine.shutdown();
}
