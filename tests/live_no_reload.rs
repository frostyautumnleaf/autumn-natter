// A live check that a badge moved does not load the model again, not for the
// repository.
//
// The server here is not llama.cpp but `probe/noreload/llama-server`: a script
// that writes one line to starts.jsonl for every process that runs, and one
// line to asks.jsonl for every ask, with the words of `chat_template_kwargs`
// and the count of the tools it was sent.
//
// The check is the whole of the rule the program stands on now. The thinking
// mode, the tools and the jail of the sandbox go out with the ask, so a chat
// that moves all three between two answers is answered by the server that is
// already up: one start for the session, and two asks that say different
// things about the thinking. Before this, each of those moves took the running
// server with it and the model was loaded over again.
//
//     cargo test -p autumn-natter-core --test live_no_reload -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

// Wait until the answer is over and the worker has let go of the chat.
fn wait_for_answer(engine: &Engine) {
    let until = std::time::Instant::now() + Duration::from_secs(120);
    loop {
        let s = engine.snapshot();
        if !s.generating && !s.busy {
            return;
        }
        assert!(std::time::Instant::now() < until, "the answer never came to an end");
        sleep(Duration::from_millis(100));
    }
}

// Wait until the script has taken one more ask than it had. The worker takes
// the work out of a queue, so a prompt is not on its way until the moment the
// worker has it, and the state says nothing of the answer before that.
fn wait_for_ask(asks: &PathBuf, before: usize) {
    let until = std::time::Instant::now() + Duration::from_secs(120);
    loop {
        if rows(asks).len() > before {
            return;
        }
        assert!(
            std::time::Instant::now() < until,
            "the ask never reached the script"
        );
        sleep(Duration::from_millis(100));
    }
}

// One json object of a line, read back from a log of the script.
fn rows(place: &PathBuf) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(place).unwrap_or_default();
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[test]
fn a_badge_moved_loads_no_model_again() {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let scratch = root.join("probe/noreload");
    let starts = scratch.join("starts.jsonl");
    let asks = scratch.join("asks.jsonl");
    let data = root.join("uidata/live-no-reload");
    let _ = std::fs::remove_file(&starts);
    let _ = std::fs::remove_file(&asks);
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    config::set_data_dir(data);

    // The model file is only wanted so the engine has a model to choose, and so
    // that its chat template can be read: the script answers every ask.
    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("a model file to choose")
        .display()
        .to_string();
    let ms = ModelSettings {
        ctx_size: 4096,
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
    cfg.chat_compaction = String::from("off");

    let engine = Engine::start(cfg);
    engine.new_chat();
    engine.set_thinking("off");
    engine.set_code(true);
    engine.set_sandbox(true);

    // The first answer: thinking off, the tools of the program on.
    engine.prompt("one".to_string(), Vec::new());
    wait_for_ask(&asks, 0);
    wait_for_answer(&engine);
    let first = rows(&asks).pop().expect("the first ask reached the script");
    println!(
        "first ask: kwargs={} tools={}",
        first["kwargs"], first["tools"]
    );
    assert_eq!(first["kwargs"]["enable_thinking"], serde_json::json!(false));
    assert!(
        first["tools"].as_u64().unwrap_or(0) > 0,
        "the first ask carried no tool"
    );

    // Every badge that used to move the server is moved, and a second answer is
    // asked for.
    engine.set_thinking("on");
    engine.set_code(false);
    engine.set_search(true);
    engine.set_sandbox(false);
    engine.prompt("two".to_string(), Vec::new());
    wait_for_ask(&asks, 1);
    wait_for_answer(&engine);
    let second = rows(&asks).pop().expect("the second ask reached the script");
    println!(
        "second ask: kwargs={} tools={}",
        second["kwargs"], second["tools"]
    );
    assert_eq!(second["kwargs"]["enable_thinking"], serde_json::json!(true));

    // One server ran over the session, and it answered both asks.
    let began = rows(&starts);
    println!("servers that ran: {}", began.len());
    for one in &began {
        println!("  start on port {}", one["port"]);
    }
    assert_eq!(
        began.len(),
        1,
        "a badge moved, and the model was loaded again"
    );
}
