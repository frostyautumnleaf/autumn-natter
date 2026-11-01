// A live check of a turn of the model with nothing in it, not for the repository.
//
// The server here is not llama.cpp but `probe/emptyturn/llama-server`: a script
// that comes to its own end straight out of a thought, which is what the reader's
// log showed. A turn with no words of its own goes back to the server as a saying
// of the model with nothing after it, and the server is then put to go on where the
// model left off: it leaves the turn empty again, and every send of the chat ends
// under "(the model gave no answer)". The check is that the program asks on with a
// word of its own behind such a turn, that the word never comes into the chat, that
// no ask ever goes out on an empty turn alone, and -- in the stubborn mood, where
// the model has nothing to give at all -- that two turns of nothing end the answer
// and tell the reader.
//
//     cargo test -p autumn-natter-core --test live_empty_turn -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

// Wait until a look gives an answer, or until two minutes have passed.
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

// The asks the script answered, out of its log. The script counts every ask, and
// the program sends one kind of ask that is not a saying of the talk at all (it
// asks for the number of tokens), so only the asks with a shape of their own count.
fn kinds(scratch: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(scratch.join("asks.jsonl")).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|one| one["kind"].as_str().map(String::from))
                .filter(|kind| kind != "none")
        })
        .collect()
}

// Wait until the script has answered this many asks of the talk, or until two
// minutes have passed. The state of the program is not looked at here, for it is
// idle before the worker has begun and idle again after it, and a check that only
// waits for idle passes at once.
fn wait_asks(scratch: &Path, most: usize) -> Vec<String> {
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        let found = kinds(scratch);
        if found.len() >= most {
            return found;
        }
        assert!(
            Instant::now() < until,
            "waited too long for {} asks, saw {:?}",
            most,
            found
        );
        sleep(Duration::from_millis(100));
    }
}

// The rows of the chat, as `role body` lines, so a look shows what the reader has.
fn rows(engine: &Engine) -> Vec<String> {
    engine
        .snapshot()
        .messages
        .iter()
        .map(|m| format!("{} {}", m.role, m.body))
        .collect()
}

// The program, the script, and the chat, with the tools offered so that the model
// has something to fall back on. The model file is only wanted so the engine has a
// model to choose: the script reads no model at all.
fn start(named: &str, scratch_name: &str) -> (Engine, PathBuf) {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let scratch = root.join(scratch_name);
    let data = root.join(format!("uidata/{}", named));
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
    let engine = Engine::start(cfg);
    engine.new_chat();
    engine.set_code(true);
    (engine, scratch)
}

// The model thought, wrote no line of its own, and stopped. The answer is asked
// on with the word of the program behind the empty turn, the model answers it, and
// the reader is left with the words of an answer rather than with a line about an
// answer that never came.
#[test]
fn a_turn_of_nothing_is_asked_on_with_a_word_of_the_program() {
    let (engine, scratch) = start("live-empty-turn", "probe/emptyturn");
    engine.prompt("the first message".to_string(), Vec::new());
    // The first ask, and the ask that carries the word of the program.
    wait_asks(&scratch, 2);
    wait_for("the answer to come to an end", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy).then_some(())
    });
    sleep(Duration::from_millis(300));
    let said = rows(&engine);
    let asked = kinds(&scratch);
    println!("asks: {:?}", asked);
    for line in &said {
        println!("chat: {}", line);
    }
    println!("status: {}", engine.snapshot().status);
    // No ask went out on an empty turn alone, and the second ask carried the word.
    assert_eq!(
        asked,
        vec!["other".to_string(), "word".to_string()],
        "the ask behind a turn of nothing did not carry the word of the program"
    );
    // The row of the turn of nothing stands with nothing in it, as the template of
    // the model writes an empty turn.
    assert!(
        said.iter()
            .any(|line| line.starts_with("Assistant") && line.trim_end() == "Assistant"),
        "the row of the turn of nothing does not stand in the chat"
    );
    // The answer came, so nothing was written about an answer that never came.
    let joined = said.join("\n");
    assert!(
        joined.contains("The two files are written and the tests pass."),
        "the words of the answer never reached the chat"
    );
    assert!(
        !joined.contains("the model gave no answer"),
        "the reader was told there was no answer, although there was one"
    );
    // The word of the program rode on the ask and never came into the chat.
    assert!(
        !joined.contains("You wrote nothing above"),
        "the word of the program came to the window"
    );
}

// The model has nothing to give: it answers with nothing even when the word of the
// program stands behind its empty turn. Two turns of nothing end the answer, the
// reader is told in the chat as well as under the top bar, and no third ask goes
// out for the same shape of nothing.
#[test]
fn two_turns_of_nothing_end_the_answer_and_tell_the_reader() {
    let (engine, scratch) = start("live-empty-turn-stubborn", "probe/emptyturn-stubborn");
    engine.prompt("the first message".to_string(), Vec::new());
    // The first ask and the two that carry the word of the program: after the
    // second of them the answer ends, and no third ask goes out.
    wait_asks(&scratch, 3);
    wait_for("the answer to come to an end", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy).then_some(())
    });
    sleep(Duration::from_millis(300));
    let said = rows(&engine);
    let asked = kinds(&scratch);
    println!("asks: {:?}", asked);
    for line in &said {
        println!("chat: {}", line);
    }
    println!("status: {}", engine.snapshot().status);
    assert!(
        !asked.contains(&"empty".to_string()),
        "an ask went out on an empty turn alone"
    );
    assert_eq!(
        asked.iter().filter(|kind| kind.as_str() == "word").count(),
        2,
        "the model was out-asked with the word of the program"
    );
    let joined = said.join("\n");
    assert!(
        joined.contains("no answer twice over"),
        "the reader was not told in the chat that the answer ended"
    );
    assert!(
        engine.snapshot().status.contains("twice over"),
        "the reader was not told under the top bar"
    );
}
