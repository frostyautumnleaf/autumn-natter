// A live check of a summarisation that is started again, not for the repository.
//
//     cargo test -p autumn-natter-core --test live_retry -- --nocapture --test-threads=1

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

fn engine_for(root: &PathBuf, name: &str) -> Engine {
    let scratch = root.join("probe/send");
    let data = root.join(format!("uidata/{}", name));
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
    Engine::start(cfg)
}

// Talk until the room is past the share set for the summarisation. True when a
// summarisation was asked for.
fn talk_until_cut(engine: &Engine) -> bool {
    for n in 1..14 {
        engine.prompt(format!("message {}", n), Vec::new());
        wait_for("an answer", || {
            let s = engine.snapshot();
            (!s.generating && !s.busy).then_some(())
        });
        sleep(Duration::from_millis(300));
        if !engine.snapshot().handoffs.is_empty() {
            return true;
        }
    }
    false
}

#[test]
fn a_summarisation_started_again_keeps_its_place_and_the_chat_goes_on() {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let bad = root.join("probe/send/summary-bad");
    let _ = std::fs::remove_file(&bad);

    let engine = engine_for(&root, "live-retry");
    engine.new_chat();
    // The summary server is having a day, so the mark is left empty.
    std::fs::write(&bad, b"").expect("the mark of a bad summary");
    assert!(talk_until_cut(&engine), "the chat was never cut");
    sleep(Duration::from_millis(600));
    let s = engine.snapshot();
    println!(
        "the bad one: marks={:?} status={}",
        s.handoffs
            .iter()
            .map(|h| (h.number, h.state.to_string()))
            .collect::<Vec<_>>(),
        s.status
    );
    assert!(
        s.handoffs.iter().any(|h| h.state == "owed"),
        "the failed summary is not owed"
    );

    // Started again, this time with a server that answers.
    std::fs::remove_file(&bad).expect("the day is over");
    engine.restart_handoff();
    wait_for("the summary again", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy && s.handoffs.iter().all(|h| h.state != "writing"))
            .then_some(())
    });
    sleep(Duration::from_millis(600));
    let s = engine.snapshot();
    let marks: Vec<(usize, String)> = s
        .handoffs
        .iter()
        .map(|h| (h.number, h.state.to_string()))
        .collect();
    println!("after the retry: marks={:?} status={}", marks, s.status);
    assert_eq!(marks.len(), 1, "the retry made a second summarisation");
    assert_eq!(marks[0].0, 1, "the number of the retried mark moved");
    assert_eq!(marks[0].1, "done", "the summary was never written");

    // The chat takes a message again.
    engine.prompt("after the summary".to_string(), Vec::new());
    sleep(Duration::from_millis(600));
    let bodies: Vec<String> = engine
        .snapshot()
        .messages
        .iter()
        .filter(|m| m.role == "You")
        .map(|m| m.body.clone())
        .collect();
    println!("reader messages: {:?}", bodies);
    assert!(
        bodies.iter().any(|b| b == "after the summary"),
        "the chat would not take a message after the summarisation: {}",
        engine.snapshot().status
    );
}
