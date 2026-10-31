// A live check of sending a message after a handoff summary, not for the repository.
//
//     cargo test -p autumn-natter-core --test live_send -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn wait_for<T>(what: &str, mut look: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(Instant::now() < until, "waited too long for {}", what);
        sleep(Duration::from_millis(100));
    }
}

fn reader_words(engine: &Engine) -> Vec<String> {
    engine
        .snapshot()
        .messages
        .iter()
        .filter(|m| m.role == "You")
        .map(|m| m.body.clone())
        .collect()
}

#[test]
fn a_message_may_be_sent_after_a_summarisation() {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let scratch = root.join("probe/send");
    let data = root.join("uidata/live-send");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    config::set_data_dir(data);

    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("a model file to choose")
        .display()
        .to_string();
    let mut ms = ModelSettings::default();
    ms.ctx_size = 8192;
    ms.gpu_enabled = vec![false; 8];
    let mut cfg = Config::default();
    cfg.llama_dir = scratch.display().to_string();
    cfg.models_dir = root.join("testing_model").display().to_string();
    cfg.model_id = model.clone();
    cfg.model_settings.insert(model, ms);
    cfg.chat_compaction = "handoff".to_string();
    cfg.chat_compact_by = "percent".to_string();
    cfg.chat_compact_percent = 70;

    let engine = Engine::start(cfg);
    engine.new_chat();

    // Talk until the room is past the share set for the summarisation. The
    // script makes the room grow with the number of messages and fall back
    // again when a summary cuts the talk short.
    let mut made = false;
    for n in 1..12 {
        engine.prompt(format!("message {}", n), Vec::new());
        sleep(Duration::from_millis(120));
        let q = engine.snapshot();
        println!("  just after prompt n={} status={} you={}", n, q.status, reader_words(&engine).len());
        wait_for("an answer", || {
            let s = engine.snapshot();
            (!s.generating && !s.busy).then_some(())
        });
        sleep(Duration::from_millis(300));
        let s = engine.snapshot();
        println!("n={} status={} you={}", n, s.status, reader_words(&engine).len());
        if !s.handoffs.is_empty() {
            println!("summarised after message {}: handoffs={} status={}", n, s.handoffs.len(), s.status);
            made = true;
            break;
        }
    }
    assert!(made, "the chat was never summarised");
    let s = engine.snapshot();
    println!(
        "after: generating={} busy={} summarising={} marks={:?}",
        s.generating,
        s.busy,
        s.summarising,
        s.handoffs.iter().map(|h| (h.number, h.state.to_string())).collect::<Vec<_>>()
    );

    // Send again. The chat must take the message.
    engine.prompt("after the summary".to_string(), Vec::new());
    sleep(Duration::from_millis(500));
    let mid = engine.snapshot();
    println!("after the send: generating={} status={}", mid.generating, mid.status);
    let bodies = reader_words(&engine);
    println!("reader messages: {:?}", bodies);
    assert!(
        bodies.iter().any(|b| b == "after the summary"),
        "the message written after the summarisation never reached the chat"
    );
}
