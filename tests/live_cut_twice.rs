// A live check of a call cut short, and the call the telling of it asked for, cut
// short again -- not for the repository.
//
// The server here is not llama.cpp but `probe/cut2/llama-server`: a script that
// answers an ask offering tools with a call whose words never stop coming, and
// answers an ask that carries the telling of a call the room gave out under with
// another such call. That is what a server set to read calls out of an answer does
// behind a telling, for the telling asks the model for the call again and nothing
// of the reader stands after it.
//
// The fault the reader met stands on the branch before this one: the second call
// was cancelled, no room had to be made for the answer the first one came to, and
// the chat stood under the block of the call with "(the model gave no answer)"
// under it. The check is that the answer goes on after the room was made: the
// model is asked with a word of the reader after the telling, so the ask ends on a
// question, and the words of an answer come.
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
    let mut ms = ModelSettings::default();
    // A small room, so a few hundred words of a file run past the limit of it.
    ms.ctx_size = 1024;
    ms.max_tokens = 200;
    ms.gpu_enabled = vec![false; 8];
    let mut cfg = Config::default();
    cfg.llama_dir = scratch.display().to_string();
    cfg.models_dir = root.join("testing_model").display().to_string();
    cfg.model_id = model.clone();
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

    // The call that never ran stands after the summarisation, as it always did.
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
        cut_at > summary_at,
        "the telling of the call stands before the summarisation"
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

    // The asks, as a server saw them. The ask that carries the telling of the first
    // call carries a word of the reader after it, so it ends on a question.
    let asks = wait_for("the asks of the server", || {
        let text = std::fs::read_to_string(scratch.join("asks.jsonl")).unwrap_or_default();
        (text.matches("\"kind\"").count() >= 3).then_some(text)
    });
    let lines: Vec<&str> = asks.lines().filter(|l| !l.trim().is_empty()).collect();
    println!("the asks: {}", lines.len());
    assert!(lines[0].contains("\"kind\": \"call\""), "the first ask: {}", lines[0]);
    assert!(lines[1].contains("\"kind\": \"summary\""), "the second ask: {}", lines[1]);
    assert!(
        lines[2].contains("A tool call was cancelled"),
        "the third ask carries no telling of the call"
    );
    let third: serde_json::Value = serde_json::from_str(lines[2]).expect("an ask of the server");
    let items = third["messages"].as_array().expect("the talk of the ask");
    let last = items.last().expect("a talk with a last message");
    assert_eq!(
        last["role"].as_str().expect("a role"),
        "user",
        "the ask that carries the telling of a call did not end on the reader"
    );
    // The telling of the first call goes back as words and never as a call, for a
    // server set to read arguments out of the words that stop in the middle of a
    // string would fault over them. A call that stands in a later ask is a call the
    // model wrote anew after the room was made, and the program ran it: that is a
    // different thing from a telling, and it is told with its own marks.
    assert!(
        !lines[2].contains("tool_calls"),
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
