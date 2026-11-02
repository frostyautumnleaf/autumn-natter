// A live check of the space that gave out under a turn of nothing, not for the
// repository.
//
// The server here is not llama.cpp but `probe/gone/llama-server`: a script that
// answers an ask of the talk with the numbers of a talk past the limit of the room
// and with no word at all, which is what the server of a chat that stands over its
// room answers with -- the room it counts holds the list of the tools and the marks
// of the template, which stand before the words of the talk and which the program
// does not count of its own accord. The ask therefore comes back with neither words
// nor a call. What the check stands for:
//
//   * the writing is not stopped over a step that wrote nothing of itself, for
//     there is no word in it to carry on, and stopping it is what left the chat
//     under the words of a space that gave out;
//   * the room is made out of a handoff summary, and the model is put back to work
//     behind it of its own accord: the talk goes out again from the first message
//     of the user with the block of the summarisation after it, and none of the
//     words the summary stands for;
//   * the model then answers, and the reader is left with the words of an answer
//     and not with a line about a space that gave out.
//
//     cargo test -p autumn-natter-core --test live_gone -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

// The first words of the block that goes round a handoff summary, as they stand in
// the ask. `engine::HANDOFF_FIRST` is the same words; they are written out here so
// that a change to them is a change to this file as well.
const HANDOFF: &str = "[The session was compacted";

fn wait_for<T>(what: &str, mut look: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(180);
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(Instant::now() < until, "waited too long for {}", what);
        sleep(Duration::from_millis(100));
    }
}

// The asks the script answered, out of its log. An ask with no talk in it is the
// program asking the server for a number, and is passed over.
fn kinds(scratch: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(scratch.join("asks.jsonl")).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|one| one["kind"].as_str().map(String::from))
                .filter(|kind| kind != "warm")
        })
        .collect()
}

// Wait until the script has answered this many asks of the talk. The state of the
// program is not looked at here, for it is idle before the worker has begun and
// idle again after it.
fn wait_asks(scratch: &Path, most: usize) -> Vec<String> {
    let until = Instant::now() + Duration::from_secs(180);
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

// The asks of the talk, whole, so that the words the model was sent with can be
// read out of them.
fn asks(scratch: &Path) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(scratch.join("asks.jsonl")).unwrap_or_default();
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|one| one["kind"].as_str().unwrap_or_default() != "warm")
        .collect()
}

// The rows of the chat, as `role body` lines.
fn rows(engine: &Engine) -> Vec<String> {
    engine
        .snapshot()
        .messages
        .iter()
        .map(|m| format!("{} {}", m.role, m.body))
        .collect()
}

// The program, the script, and the chat. The model file is only wanted so the
// engine has a model to choose: the script reads no model at all.
fn start(named: &str, scratch_name: &str) -> (Engine, PathBuf) {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let scratch = root.join(scratch_name);
    let data = root.join(format!("uidata/{}", named));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    std::fs::create_dir_all(&scratch).expect("a place for the script");
    std::fs::write(scratch.join("mode"), "gone").expect("the mood of the server");
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
    assert_eq!(cfg.chat_compaction, "handoff");
    assert_eq!(cfg.chat_compact_by, "percent");
    assert_eq!(cfg.chat_compact_percent, 85);
    let engine = Engine::start(cfg);
    engine.new_chat();
    engine.set_code(true);
    (engine, scratch)
}

// The space gave out under a step that wrote neither words nor a call. The answer
// is not ended there: the room is made out of a handoff summary, the talk goes out
// again from the first message of the user with the summary after it, and the model
// goes on with the work.
#[test]
fn a_space_that_gave_out_under_nothing_is_followed_by_the_work_going_on() {
    let (engine, scratch) = start("live-gone", "probe/gone");
    engine.prompt("Write me a page about the show".to_string(), Vec::new());
    // The ask the space gave out under, the ask for the summary, and the ask that
    // puts the model back to work behind it.
    let asked = wait_asks(&scratch, 3);
    wait_for("the answer to come to an end", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy).then_some(())
    });
    sleep(Duration::from_millis(400));
    let said = rows(&engine);
    let seen = asks(&scratch);
    println!("asks: {:?}", asked);
    for line in &said {
        println!("chat: {}", line);
    }
    println!("status: {}", engine.snapshot().status);
    assert_eq!(
        asked,
        vec!["gone", "summary", "handoff"],
        "the work was not gone on with behind the summarisation"
    );
    // The ask that put the model back to work carried the first message of the user
    // and the block of the summarisation, and nothing of the words the summary
    // stands for.
    let going_on = seen
        .iter()
        .find(|one| one["kind"].as_str() == Some("handoff"))
        .expect("the ask behind the summarisation");
    let messages = going_on["messages"].as_array().expect("the talk of the ask");
    let word_of = |one: &serde_json::Value| {
        one["content"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| one["content"].to_string())
    };
    let words: Vec<String> = messages.iter().map(word_of).collect();
    let roles: Vec<String> = messages
        .iter()
        .map(|one| one["role"].as_str().unwrap_or_default().to_string())
        .collect();
    println!("the ask behind the summarisation: {:?}", roles);
    // The block of the summarisation ends the talk, for it is the last word the
    // model is sent with.
    assert!(
        words
            .last()
            .map(|last| last.starts_with(HANDOFF))
            .unwrap_or(false),
        "the block of the summarisation did not stand at the end of the ask: {:?}",
        words
    );
    // The talk that goes out is the first message of the user and the block: the
    // room begun afresh from the first prompt of the chat, with none of the words
    // the summary stands for -- the turn of nothing among them.
    let of_the_talk = messages
        .iter()
        .filter(|one| one["role"].as_str() == Some("user"))
        .map(word_of)
        .collect::<Vec<String>>();
    assert_eq!(
        of_the_talk.len(),
        2,
        "the ask behind the summarisation carried more of the talk than the first message of the user and the block: {:?}",
        of_the_talk
    );
    assert!(
        of_the_talk
            .first()
            .map(|first| first.contains("Write me a page about the show"))
            .unwrap_or(false),
        "the first message of the user did not stand at the front of the ask: {:?}",
        of_the_talk
    );
    // The model answered, and the reader reads an answer.
    let joined = said.join("\n");
    assert!(
        joined.contains("Going on with the work"),
        "the words of the model behind the summarisation never reached the chat"
    );
    assert!(
        !joined.contains("The space gave out"),
        "the reader was told the space gave out, although the work went on"
    );
    assert!(
        !engine.snapshot().status.contains("gave out"),
        "the top bar says the space gave out: {}",
        engine.snapshot().status
    );
    // The chat was summarised, and the mark of it stands where the reader can see
    // it: the room was made, and the line of the summarisations says so.
    assert!(
        engine
            .snapshot()
            .handoffs
            .iter()
            .any(|one| one.state == "done"),
        "the summarisation does not stand in the line of them"
    );
}
