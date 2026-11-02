// A live check of the meter of the context when an ask is stopped, not for the
// repository.
//
// The server here is not llama.cpp but `probe/meter/llama-server`: a script that
// speaks a room of 91,000 words on every piece of a slow answer, out of an ask of
// a line or two, and that speaks the number of the whole talk only if nobody has
// gone away. The shape of a long working chat, in which the measure of the words
// that stand reads at a hundredth of what the server counts of the same talk.
//
// The check is the fault of the meter that fell. Stop the answer in the middle and
// the ask speaks no number at its end. The meter is then kept by `meter_after` on
// the room of the words the server was sent, which the pieces of the answer spoke,
// and it does not fall to the measure of the words -- which would read as a chat
// that had the whole of the room to spare -- and it does not keep the number of the
// writing that was stopped, for a stop takes words away.
//
//     cargo test --test live_meter -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

// The numbers the script stands at, which the check is weighed against.
const SENT: u64 = 91_000;

// Wait until a look gives an answer, or until five minutes have passed.
fn wait_for<T>(what: &str, mut look: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(300);
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(Instant::now() < until, "waited too long for {}", what);
        sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_stopped_ask_leaves_the_meter_on_the_words_the_server_was_sent() {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let scratch = root.join("probe/meter");
    let data = root.join("uidata/live-meter");
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
    let ms = ModelSettings {
        ctx_size: 131072,
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

    let engine = Engine::start(cfg);
    engine.new_chat();
    engine.prompt("Tell me a long story.".to_string(), Vec::new());

    // The meter moves with the answer: the pieces of it speak the room of the talk,
    // and the writing of the model climbs above the words that were sent.
    let climbing = wait_for("the meter to move with the answer", || {
        let s = engine.snapshot();
        (s.generating && s.context_used > SENT + 200).then_some(s.context_used)
    });
    println!("the meter climbed to {} while the model wrote", climbing);

    // The hand goes up. The way back to the server is closed before the last piece
    // comes, so the ask speaks no number of the talk at its end.
    engine.cancel();
    let after = wait_for("the answer to come to an end", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy).then_some(s)
    });
    println!(
        "status: {} meter: {} words: {}",
        after.status,
        after.context_used,
        after
            .messages
            .iter()
            .map(|m| m.role.len() + m.body.len())
            .sum::<usize>()
    );
    assert!(
        after.status.contains("Stopped"),
        "the answer did not end on the stop: {}",
        after.status
    );

    // The meter stands on the room of the words the server was sent, and not on the
    // measure of the words that stand: that measure would read as a chat with the
    // whole of the room to spare, which was the fault.
    assert!(
        after.context_used > SENT / 2,
        "the meter fell to the measure of the words: {}",
        after.context_used
    );
    assert_eq!(
        after.context_used, SENT,
        "the meter neither fell nor kept the writing that was stopped"
    );
    // And the number of the stopped writing is not kept over the talk: a stop takes
    // the words it had written away.
    assert!(
        after.context_used < climbing,
        "the meter kept the number of a writing that is gone: {} against {}",
        after.context_used,
        climbing
    );

    // The words the model had written stand in the chat, as they always have: the
    // measure of them rides with the number of the words sent, and the next ask
    // makes the server speak again and takes the meter where it stands.
    let words = after
        .messages
        .last()
        .map(|m| m.body.clone())
        .unwrap_or_default();
    println!("the stopped answer stands at {} characters", words.chars().count());
    assert!(!words.is_empty(), "the stopped answer left nothing behind");

    engine.shutdown();
}
