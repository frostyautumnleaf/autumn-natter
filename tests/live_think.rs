// A live look at a tool call written between two thoughts, not for the repository.
//
//     cargo test -p autumn-natter-core --test live_think -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[test]
fn a_tool_written_between_thoughts_comes_to_an_end() {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let scratch = root.join("probe/think");
    let data = root.join("uidata/live-think");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    config::set_data_dir(data);

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
    cfg.chat_compaction = "off".to_string();

    let engine = Engine::start(cfg);
    engine.new_chat();
    engine.set_code(true);
    engine.prompt("look at the folder".to_string(), Vec::new());
    sleep(Duration::from_millis(200));
    println!("status just after: {} / model={:?}", engine.snapshot().status, engine.snapshot().model_id);

    let until = Instant::now() + Duration::from_secs(60);
    let _last = String::new();
    loop {
        let s = engine.snapshot();
        let line = format!(
            "gen={} live={:?} live_len={} think={} msgs=[{}]",
            s.generating,
            s.live_tools.iter().map(|o| (o.command.clone(), o.done)).collect::<Vec<_>>(),
            s.live_text.len(), s.live_reasoning.len(),
            s.messages
                .iter()
                .map(|m| format!(
                    "{}:{}:{}",
                    m.role,
                    if m.failed { 'F' } else { '.' },
                    &m.body.replace('\n', " ")[..m.body.len().min(22)]
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!("{}", line);
        // The block of a call stops saying it is being written as soon as the
        // model moves on from it, so a state with a written call standing in the
        // live list is what the fix is about.
        if s.live_tools.iter().any(|one| one.done) {
            println!("   ...a call stands written while the answer goes on: {:?}",
                     s.live_tools.iter().map(|one| (one.command.clone(), one.done)).collect::<Vec<_>>());
        }
        if !s.generating {
            break;
        }
        assert!(Instant::now() < until, "the answer never came to an end");
        sleep(Duration::from_millis(90));
    }
}
