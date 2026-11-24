// A live check of a tool call bigger than the space, not for the repository.
//
// The server here is not llama.cpp but `probe/cut/llama-server`: a script that
// streams a tool call whose text never stop coming, while the size of the context
// climbs on every piece of it. A model that is writing such a call never stops of
// itself, so the check is that the program stops the writing at the limit set for
// the summarisation, that the call which never ran is in the chat where the
// writing stopped, that the space is then made by a handoff summary written of the
// telling of that call, and that the text of a call that never ran go back to the
// model no more.
//
//     cargo test --test live_cut -- --nocapture --test-threads=1

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

// The number in the text "hung up after 42 parts", which the server writes when the client is
// removed in the middle of a call.
fn pieces_before_hanging_up(log: &str) -> Option<usize> {
    let before = log.split("hung up after ").nth(1)?;
    let digits: String = before.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[test]
fn a_tool_call_bigger_than_the_room_is_cut_off_and_told_of() {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let scratch = root.join("probe/cut");
    let data = root.join("uidata/live-cut");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    config::set_data_dir(data);

    // The model file is only wanted so the engine has a model to choose: the server under test
    // is the script, and it reads no model at all.
    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("a model file to choose")
        .display()
        .to_string();
    // A small space, so a few hundred words of a file run past the limit of it.
    let ms = ModelSettings {
        ctx_size: 1024,
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
    // The limit the settings are at by default: the handoff summary, at a share of the space.
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

    // The writing is stopped, the space is freed, and the model is asked again and answers.
    // Then the chat stays still, with the cancelled call in it.
    let snap = wait_for("the answer to come to an end", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy && s.messages.iter().any(|m| m.cancelled)).then_some(s)
    });
    println!("status: {}", snap.status);
    assert!(
        !snap.status.contains("not written") && !snap.status.contains("cannot"),
        "the engine stated: {}",
        snap.status
    );

    // The call that never ran is in the chat, named for the file it was pointed at, with the
    // text of it had reached.
    let cut_at = snap
        .messages
        .iter()
        .position(|m| m.cancelled)
        .expect("a call stands cancelled");
    let cut = &snap.messages[cut_at];
    assert_eq!(cut.role, "Tool");
    assert_eq!(cut.tool_name, "edit_file");
    assert!(cut.failed, "a call that never ran is not a run that came off");
    assert!(
        cut.tool_args.starts_with("{\"path\":\"notes/big.md\""),
        "the text the model had written were lost: {}",
        cut.tool_args
    );
    assert!(cut.tool.chars().count() > 20, "the block reports nothing of it");
    println!("the cancelled call: {} [{}]", cut.body, cut.tool);

    // And it is before the summarisation, where the writing stopped: the request for the
    // summary is made of the conversation as it is now, so it carries the report of the call,
    // and the marker then cuts the report off.
    let summary_at = snap
        .messages
        .iter()
        .position(|m| m.role == "Summary" && !m.body.trim().is_empty())
        .expect("the space was freed by a handoff summary");
    assert!(
        cut_at < summary_at,
        "the block of the call is under the heading of the summarisation"
    );

    // What the three asks looked like from the side of a server.
    let asks = wait_for("the three asks of the server", || {
        let text = std::fs::read_to_string(scratch.join("asks.jsonl")).unwrap_or_default();
        (text.matches("\"kind\"").count() >= 3).then_some(text)
    });
    let lines: Vec<&str> = asks.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(lines[0].contains("\"kind\": \"call\""), "the first ask: {}", lines[0]);
    assert!(lines[1].contains("\"kind\": \"summary\""), "the second ask: {}", lines[1]);
    assert!(lines[2].contains("\"kind\": \"answer\""), "the third ask: {}", lines[2]);
    // The request for the summary carries the report of the call that never ran: the model that
    // writes the summary reads what it was put to, and what it had written when the writing
    // stopped.
    assert!(
        lines[1].contains("A tool call was cancelled"),
        "the request for the summary carried no telling of the call"
    );
    // The request that came after the space was freed carries no telling of it, for the marker
    // cuts it off with the rest of the text the summary means. The conversation ends on the
    // handoff block, which shows in its own text that the work is to be continued.
    assert!(
        !lines[2].contains("A tool call was cancelled"),
        "the report of the call went back to the model after the summarisation"
    );
    assert!(
        !lines[2].contains("tool_calls"),
        "the text of the cut-off call went back as a call"
    );
    let third: serde_json::Value = serde_json::from_str(lines[2]).expect("an ask of the server");
    let items = third["messages"].as_array().expect("the conversation of the request");
    let last = items.last().expect("a conversation with a last message");
    assert_eq!(
        last["role"].as_str().expect("a role"),
        "user",
        "the request after the space was freed did not end on the handoff block"
    );
    assert!(
        last["content"]
            .as_str()
            .unwrap_or_default()
            .starts_with("[The session was compacted"),
        "the request after the space was freed did not end on the handoff block: {}",
        last
    );

    // The client was removed in the middle of the call, which is the whole point: no answer
    // that ran to the end of it could have been cut.
    let log = std::fs::read_to_string(scratch.join("server.log")).unwrap_or_default();
    println!("server log:\n{}", log);
    let pieces = pieces_before_hanging_up(&log).expect("the server was hung up on");
    assert!(pieces >= 2, "the call was hung up on after {} parts", pieces);
    assert!(pieces < 400, "all of the call came: {} parts", pieces);

    engine.shutdown();
}
