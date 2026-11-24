// A live check that a tool called while a handoff summary is being written is cancelled where it
// stands, and that the writing of the summary goes on from there. Not for the repository.
//
// A model that has read a conversation full of tool calls sometimes answers the request for a
// summary with a call of its own. That is no reason to throw the summary away: the call never
// runs, the model is told in the result of the call that tool calls are not allowed while a
// handoff summary is being written, and the same request is gone on with from where the writing
// stood. What the model wrote beside a cancelled call is part of the summary and is kept, and a
// second call is answered the same way as the first.
//
// The server under test is `probe/summarytool/llama-server`: the first ask of the chat reports a
// context past the summarisation limit, the ask for the summary is answered with a few words of
// summary and a call, the next ask - known by the result of the cancelled call standing in it -
// with more summary and a second call, and the ask after that with the last of the summary.
// Every ask is written to `asks.jsonl`, so the test reads what the model was really told.
//
//     cargo test -p autumn-natter-core --test live_summary_tool -- --nocapture --test-threads=1

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
fn a_tool_called_while_a_summary_is_written_is_cancelled_and_the_summary_goes_on() {
    let root = PathBuf::from("..").canonicalize().expect("the root of the work");
    let scratch = root.join("probe/summarytool");
    let data = root.join("uidata/live-summary-tool");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    config::set_data_dir(data);

    // The model file is only wanted so the engine has a model to choose: the server under test is
    // the script, and it reads no model at all.
    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("a model file to choose")
        .display()
        .to_string();
    // A small space, so the first answer of the chat runs past the limit the summarisation
    // comes at.
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
    assert_eq!(cfg.chat_compaction, "handoff");
    assert_eq!(cfg.chat_compact_percent, 85);

    let engine = Engine::start(cfg);
    engine.new_chat();
    engine.prompt("Tell me what we have done so far.".to_string(), Vec::new());

    // The chat comes to rest with a summary written in it. The summary is the whole of what the
    // model wrote: the words before its first call, the words between the two calls, and the
    // words after the last one was cancelled, in the order they came.
    let snap = wait_for("the summary to be written", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy && s.messages.iter().any(|m| m.role == "Summary" && !m.body.trim().is_empty()))
            .then_some(s)
    });
    println!("status: {}", snap.status);
    assert!(
        !snap.status.contains("not written") && !snap.status.contains("Start it again"),
        "the engine stated: {}",
        snap.status
    );
    let summary = snap
        .messages
        .iter()
        .find(|m| m.role == "Summary" && !m.body.trim().is_empty())
        .expect("a handoff summary stands in the chat");
    println!("the summary: {}", summary.body);
    let one_before = summary
        .body
        .find("The work so far")
        .expect("the words the model wrote before its first call were lost");
    let between = summary
        .body
        .find("The databases were checked")
        .expect("the words the model wrote between the two calls were lost");
    let one_after = summary
        .body
        .find("the databases were moved")
        .expect("the summary did not go on after the last call was cancelled");
    assert!(
        one_before < between && between < one_after,
        "the parts of the summary are out of the order they were written in: {}",
        summary.body
    );

    // Neither call ran: the chat holds no block of a run of either tool.
    for name in ["run_terminal", "read_terminal"] {
        assert!(
            snap.messages
                .iter()
                .filter(|m| m.tool_name == name)
                .all(|m| m.cancelled),
            "a call made while the summary was written was run: {:?}",
            snap.messages
                .iter()
                .filter(|m| m.tool_name == name)
                .map(|m| &m.body)
                .collect::<Vec<_>>()
        );
    }

    // What the asks looked like from the side of a server. The summarisation is one request gone
    // on with, and not a summarisation begun again: the request for the summary is made once, and
    // each cancelled call is answered inside it.
    let asks = wait_for("the asks of the server", || {
        let text = std::fs::read_to_string(scratch.join("asks.jsonl")).unwrap_or_default();
        (text.matches("summary-resumed").count() >= 2).then_some(text)
    });
    let lines: Vec<&str> = asks.lines().filter(|l| !l.trim().is_empty()).collect();
    let started = lines
        .iter()
        .filter(|one| one.contains("\"summary\""))
        .count();
    assert_eq!(
        started, 1,
        "the summarisation was begun more than once over two cancelled calls: {asks}"
    );
    let resumed: Vec<&&str> = lines
        .iter()
        .filter(|one| one.contains("summary-resumed"))
        .collect();
    assert_eq!(resumed.len(), 2, "the cancelled calls: {asks}");
    // The first cancelled call went back answered, and so did the second: a request that holds a
    // call with no result beside it is a request the server refuses.
    assert!(
        resumed[1].contains("call-in-summary-1") && resumed[1].contains("call-in-summary-2"),
        "a cancelled call went back unanswered: {}",
        resumed[1]
    );
    for one in &resumed {
        assert!(
            one.contains("Tool calls are not allowed while writing a handoff summary"),
            "the model was never told why its call was cancelled: {one}"
        );
        assert!(
            one.contains("Do not call tools while writing this handoff summary"),
            "the words a model has to obey were not in the result: {one}"
        );
    }
    // The conversation the summary is written of is worked through once: the ask that goes on with
    // the summary is the first ask with the two entries of the cancelled call hung onto it, and it
    // holds the same conversation above them.
    let first: &str = lines
        .iter()
        .find(|one| one.contains("\"summary\""))
        .expect("the ask for the summary");
    let first_messages = first.matches("\"role\"").count();
    let went_on = resumed[1]
        .matches("\"role\"")
        .count()
        .saturating_sub(first_messages);
    assert!(
        went_on <= 5,
        "the whole conversation was worked through again, so the space the summary was meant to \
         free was spent instead: {} messages over the {} of the first ask",
        went_on + first_messages,
        first_messages
    );
    let log = std::fs::read_to_string(scratch.join("server.log")).unwrap_or_default();
    println!("server log:\n{}", log);
    assert!(
        log.contains("the summary was gone on with after the second cancelled call"),
        "the summary was not gone on with to the end: {log}"
    );

    engine.shutdown();
}
