// A live check of the line that says what the model is doing, not for the repository.
//
// The line under the chat said what the model was last seen to do, and nothing of what it was
// doing now: once a tool had run, the line went on saying "The model is reading the tool result"
// through all of the thinking, the writing and the tool call of the next step - and it said it
// while the model was writing a file, which is the thing the user noticed. The scripted server of
// `probe/status` is put in the `tool` mood, so one answer holds a command, the report of it, a
// step of thinking, and an answer after it. The line is read over and over while that goes on,
// and the order of what it said is what is checked.
//
//     cargo test -p autumn-natter-core --test live_status -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

// The line of the status, read again and again, with the repeats of one saying left out. The
// answer goes on over threads of the program of its own, so nothing is waited for here that would
// let a phase come to an end unheard of.
fn read_the_line(engine: &Engine) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut worked = false;
    let until = Instant::now() + Duration::from_secs(180);
    loop {
        let snap = engine.snapshot();
        worked |= snap.generating || snap.busy;
        if seen.last().map(|one| one.as_str()) != Some(snap.status.as_str()) {
            println!("  the line: {}", snap.status);
            seen.push(snap.status);
        }
        if worked && !snap.generating && !snap.busy {
            break;
        }
        assert!(Instant::now() < until, "the answer never came to its end");
        sleep(Duration::from_millis(4));
    }
    seen
}

// The place of the first saying after `from` that holds one of `words`, and nothing before it.
fn after(seen: &[String], from: usize, words: &[&str]) -> Option<usize> {
    seen[from + 1..]
        .iter()
        .position(|one| words.iter().any(|word| one.contains(word)))
        .map(|at| at + from + 1)
}

#[test]
fn the_line_follows_the_model_from_a_tool_to_the_answer() {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let scratch = root.join("probe/status");
    let data = root.join("uidata/live-status");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    std::fs::write(scratch.join("mode"), "tool").expect("the mood of the server");
    config::set_data_dir(data);

    // The model file is only wanted so the engine has a model to choose: the server under test is
    // the script, and it reads no model at all.
    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("a model file to choose")
        .display()
        .to_string();
    let ms = ModelSettings {
        // The report of the command is the wall of words the script gives back; the space is big
        // enough that nothing is cut for the room, and the check is about the line alone.
        ctx_size: 16384,
        gpu_enabled: vec![false; 8],
        ..Default::default()
    };
    let mut cfg = Config {
        llama_dir: scratch.display().to_string(),
        models_dir: root.join("testing_model").display().to_string(),
        model_id: model.clone(),
        // The summarisation is left out of the way, so that nothing but the answer moves the line.
        chat_compaction: "off".to_string(),
        ..Default::default()
    };
    cfg.model_settings.insert(model, ms);

    let engine = Engine::start(cfg);
    engine.new_chat();
    // The tool of the terminal is on, so the command the model asks for is really run and its
    // report goes back - which is the step of the answer the line has to follow.
    engine.set_code(true);
    engine.prompt("Show me what the folder holds".to_string(), Vec::new());
    let seen = read_the_line(&engine);
    println!("the whole of the line: {:?}", seen);

    // The command, its report, a step of thinking, the answer, and the chat at rest: in that
    // order, which is what the line was written to say and did not.
    let ran = seen
        .iter()
        .position(|one| one.contains("Running a command"))
        .expect("the line never said a command was being run");
    let read = after(&seen, ran, &["reading the tool result"])
        .expect("the line never said the model was reading the result of the tool");
    let thought = after(&seen, read, &["The model is thinking"]).unwrap_or_else(|| {
        panic!(
            "the line went on saying what the step before was, instead of saying that the model \
             was thinking, after the tool result was sent: the command ran at {}, its report at {}",
            ran, read
        )
    });
    let wrote = after(&seen, thought, &["The model is writing the answer"])
        .expect("the line never said the model was writing the answer");
    assert_eq!(
        "Ready",
        seen.last().map(|one| one.as_str()).unwrap_or(""),
        "the line did not say the chat was at rest when the answer was over"
    );
    assert!(
        after(&seen, read, &["Running a command"]).is_none(),
        "the line went back to a command that had already run, between the tool result and the end"
    );
    println!(
        "the line held: command at {}, report at {}, thinking at {}, writing at {}, rest at the end",
        ran, read, thought, wrote
    );
}
