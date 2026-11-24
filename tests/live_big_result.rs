// A live check that the result of a call is kept inside the space the chat has, not for the
// repository.
//
// The server here is not llama.cpp but `probe/bigresult/llama-server`: a script that answers
// the first ask of the turn with a call of the terminal tool, and the command it calls prints
// thousands of lines. That result is many times the space the chat has left, and the meter of
// the context - the number the server spoke for the ask that was already made - stands well
// under the limit set for the summarisation while the conversation the next ask carries stands
// far past it. That is the shape of the fault this checks: weighed on the meter, the making of
// the space answered that it had made it, the doomed request went out, the writing was stopped
// under its first piece, and the answer came to an end with nothing said of why.
//
// Beside the fake server stands `probe/bigresult/llama-tokenize`, a script that answers with
// two bytes to a token. That is more than the measure of the text the program takes when no
// tokenizer is there (four characters to a token), so the cut seen here is a cut the counting
// of the model brought about, and the counting is written down beside the script.
//
// What the check is of:
//
//   * the result of the call is counted and fitted to the space before it goes out, and the ask
//     that carries it is weighed under the limit set for the summarisation - the doomed request
//     go out no more;
//   * the copy of the result the model is given keeps its beginning and its end, and states
//     between them how much of it is not there. The call is not reported as cancelled: the
//     command ran;
//   * the chat keeps the whole of what the command printed, so the shortening of a request is
//     never a loss of the transcript shown to the user;
//   * the handoff summary of the turn is written, and the answer goes on behind it;
//   * nothing on the line says that no space could be made.
//
//     cargo test -p autumn-natter-core --test live_big_result -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

// The window the check works with, and the limit the settings set for the summarisation of it.
const WINDOW: u64 = 4_096;
const LIMIT: u64 = WINDOW * 85 / 100;
// The longest the whole of the turn may take.
const WHOLE: Duration = Duration::from_secs(180);
// What the command of the call prints: `seq 1 6000` is 6000 lines, of 28 893 characters with
// the newlines. The cut of it is asked to be well under half of that.
const LINES_PRINTED: usize = 6_000;

// Wait until a look gives an answer, or until the whole of the turn has been waited out.
fn wait_for<T>(what: &str, limit: Duration, mut look: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + limit;
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(Instant::now() < until, "waited too long for {}", what);
        sleep(Duration::from_millis(100));
    }
}

// The size of the conversation of one ask, measured the way the program measures it: four
// characters of the text to a token, a few tokens for every message, and the calls of the tools
// with the text they carry.
fn measured(items: &[serde_json::Value]) -> u64 {
    let mut chars = 0u64;
    for item in items {
        match item.get("content") {
            Some(serde_json::Value::String(text)) => chars += text.chars().count() as u64,
            Some(serde_json::Value::Array(parts)) => {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        chars += text.chars().count() as u64;
                    }
                }
            }
            _ => {}
        }
        if let Some(said) = item
            .get("reasoning_content")
            .and_then(|t| t.as_str())
        {
            chars += said.chars().count() as u64;
        }
        if let Some(calls) = item.get("tool_calls").and_then(|c| c.as_array()) {
            for one in calls {
                let held = |where_: &serde_json::Value, key: &str| -> u64 {
                    where_
                        .get(key)
                        .and_then(|v| v.as_str())
                        .map(|said| said.chars().count() as u64)
                        .unwrap_or_default()
                };
                let function = one.get("function").cloned().unwrap_or(serde_json::Value::Null);
                chars += held(&function, "name") + held(&function, "arguments");
                chars += held(one, "name") + held(one, "arguments");
                chars += held(one, "id");
                chars += 24;
            }
        }
        chars += 8;
    }
    chars / 4
}

// The asks of the conversation, in the order they came, each with the kind the server took it
// for. Asks of a kind that came more than once are kept in the order they came in.
fn asks_of(scratch: &Path, what: &str) -> Vec<(String, serde_json::Value)> {
    let text = wait_for(what, WHOLE, || {
        let text = std::fs::read_to_string(scratch.join("asks.jsonl")).unwrap_or_default();
        (!text.is_empty()).then_some(text)
    });
    text.lines()
        .filter(|one| !one.trim().is_empty())
        .filter_map(|one| {
            let made: serde_json::Value = serde_json::from_str(one).ok()?;
            let kind = made.get("kind")?.as_str()?.to_string();
            Some((kind, made))
        })
        .collect()
}

#[test]
fn a_tool_result_bigger_than_the_space_is_cut_before_it_is_sent() {
    let root = PathBuf::from("..").canonicalize().expect("the root of the work");
    let scratch = root.join("probe/bigresult");
    let data = root.join("uidata/live-big-result");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    config::set_data_dir(data.clone());

    // The model file is only wanted so the engine has a model to choose: the server under test
    // is the script, and it reads no model at all. The path of the model is what the counting of
    // the result is asked for, and the stub tokenizer reads no more of it than the server does.
    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("a model file to choose")
        .display()
        .to_string();
    let ms = ModelSettings {
        ctx_size: WINDOW as u32,
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
    assert_eq!(cfg.chat_compact_by, "percent");
    assert_eq!(cfg.chat_compact_percent, 85);

    let engine = Engine::start(cfg);
    engine.new_chat();
    engine.set_code(true);
    // The commands run out of the sandbox, so the check is not of whether bubblewrap is on the
    // machine. What a command prints is the same either way.
    engine.set_sandbox(false);

    let began = Instant::now();
    engine.prompt(
        "Count the lines of the build log: run_terminal with the command `seq 1 6000`.".to_string(),
        Vec::new(),
    );

    // The turn comes to rest with a handoff summary written in it: the result of the call was
    // cut, the answer came, and the summarisation of the turn followed it.
    let snap = loop {
        let snap = engine.snapshot();
        if !snap.generating
            && !snap.busy
            && snap
                .messages
                .iter()
                .any(|one| one.role == "Summary" && !one.body.trim().is_empty())
        {
            break snap;
        }
        assert!(
            began.elapsed() < WHOLE,
            "waited {} seconds for the turn to come to rest. The line said {:?} and the chat held \
             {} messages",
            began.elapsed().as_secs(),
            snap.status,
            snap.messages.len(),
        );
        sleep(Duration::from_millis(100));
    };
    println!(
        "the turn came to rest in {} seconds with {} messages. The line said {:?}",
        began.elapsed().as_secs(),
        snap.messages.len(),
        snap.status,
    );

    // The line never said that the space could not be made, which is what the fault ended with.
    assert!(
        !snap.status.contains("no space could be made")
            && !snap.status.contains("past the summarisation limit"),
        "the line stated: {:?}",
        snap.status,
    );

    // The call ran, so it is not reported as cancelled, and the whole of what it printed stands
    // in the chat: only the copy of the result that goes out with the request is fitted to the
    // space, and that is what the server was asked with below.
    assert!(
        !snap.messages.iter().any(|one| one.cancelled),
        "a call that ran was reported as cancelled"
    );
    let kept = snap
        .messages
        .iter()
        .rev()
        .find(|one| one.role == "Tool" && one.call_id == "call-count")
        .expect("the result of the call stands in the chat");
    assert!(
        !kept.tool.contains("whole of the result of this call is not here"),
        "the shortening of the request reached the chat: the result shown to the user is {}...",
        kept.tool.chars().take(200).collect::<String>(),
    );
    assert!(
        kept.tool.contains("5999\n6000"),
        "the end of what the command printed is not in the chat: {}...",
        kept.tool.chars().rev().take(60).collect::<String>().chars().rev().collect::<String>(),
    );
    assert!(
        kept.tool.starts_with("1\n2\n3"),
        "the beginning of what the command printed is not in the chat: {}...",
        kept.tool.chars().take(60).collect::<String>(),
    );

    // What the server was really asked with. The ask that carries the result of the call is the
    // one that was doomed: it must be weighed under the limit set for the summarisation, and the
    // result in it must be the cut one.
    let asks = asks_of(&scratch, "the asks of the conversation");
    for one in &asks {
        println!("ask of kind {}", one.0);
    }
    let kinds: Vec<&str> = asks.iter().map(|one| one.0.as_str()).collect();
    let at_call = kinds
        .iter()
        .position(|one| *one == "call")
        .expect("the first ask was never answered with a call of the tool");
    let at_result = kinds
        .iter()
        .skip(at_call)
        .position(|one| *one == "result")
        .expect("the ask that carries the result of the call never came");
    let at_result = at_call + at_result;
    let at_summary = kinds
        .iter()
        .skip(at_result)
        .position(|one| *one == "summary")
        .expect("no handoff summary was asked for after the result went out");
    let at_summary = at_result + at_summary;
    assert!(
        kinds[at_summary..].contains(&"goon"),
        "the answer did not go on behind the summarisation: {:?}",
        kinds,
    );

    let result_ask = &asks[at_result].1;
    let items = result_ask["messages"].as_array().expect("the conversation of the request");
    let size = measured(items);
    assert!(
        size <= LIMIT + LIMIT / 20,
        "the request that carried the result of the call went out at {} tokens, past the limit of \
         {} set for the summarisation",
        size,
        LIMIT,
    );
    let told = items
        .iter()
        .filter(|item| item.get("role").and_then(|r| r.as_str()) == Some("tool"))
        .filter_map(|item| item.get("content").and_then(|c| c.as_str()))
        .next()
        .expect("the request carried no result of the call");
    println!(
        "the result the model was given: {} characters of the {} that were printed. It begins {} \
         and ends {}",
        told.chars().count(),
        PRINTED_CHARS,
        told.chars().take(24).collect::<String>(),
        told.chars().rev().take(24).collect::<String>(),
    );
    assert!(
        told.contains("whole of the result of this call is not here"),
        "the result the model was given says nothing of what it left out"
    );
    let head: String = told.chars().take(12).collect();
    assert!(
        head.starts_with("1\n2\n3"),
        "the beginning of the result is not in what the model was given: {}...",
        told.chars().take(60).collect::<String>(),
    );
    let near_end: String = told.chars().skip(told.chars().count().saturating_sub(200)).collect();
    assert!(
        // The last lines of what the command printed, a little way back from the end of the
        // result, for the program writes its own word under what a command printed.
        near_end.contains("5999\n6000"),
        "the end of the result is not in what the model was given: ...{}",
        near_end.chars().rev().take(60).collect::<String>().chars().rev().collect::<String>(),
    );
    assert!(
        told.chars().count() * 2 < PRINTED_CHARS,
        "the result the model was given is {} characters of the {} printed: it was hardly cut",
        told.chars().count(),
        PRINTED_CHARS,
    );
    // What the model is to do about a result it cannot hold is not the program's business.
    for forbidden in ["smaller parts", "read the file again", "ask for"] {
        assert!(
            !told.to_lowercase().contains(forbidden),
            "the telling tells the model what to do ({})",
            forbidden,
        );
    }

    // The counting of the model is what brought the cut about. The stub tokenizer beside the
    // server writes every counting down, and it counts two bytes to a token where the measure
    // of the text would have taken four characters.
    let counted = wait_for("the counting of the result", Duration::from_secs(10), || {
        let text = std::fs::read_to_string(scratch.join("countings.log")).unwrap_or_default();
        (!text.trim().is_empty()).then_some(text)
    });
    println!("the tokenizer answered: {}", counted.replace('\n', "; "));
    let of_stub: u64 = counted
        .lines()
        .next()
        .and_then(|one| one.split_whitespace().next())
        .and_then(|one| one.parse().ok())
        .expect("a counting from the tokenizer");
    let by_text = (PRINTED_CHARS / 4) as u64;
    assert!(
        of_stub > by_text,
        "the counting of the tokenizer ({}) was under the measure of the text ({}) of the result: \
         the check could not tell the two apart",
        of_stub,
        by_text,
    );

    // The summary of the turn is in the chat, after the cut result.
    let at_summary = snap
        .messages
        .iter()
        .rposition(|one| one.role == "Summary" && !one.body.trim().is_empty())
        .expect("a handoff summary was written");
    let at_kept = snap
        .messages
        .iter()
        .rposition(|one| one.role == "Tool" && one.call_id == "call-count")
        .expect("the result of the call stands in the chat");
    assert!(
        at_kept < at_summary,
        "the summary was written before the result of the call was in the chat"
    );
    let went_on = snap
        .messages
        .iter()
        .skip(at_summary)
        .filter(|one| one.role == "Assistant")
        .count();
    assert!(
        went_on >= 1,
        "the answer did not go on behind the summarisation: {} messages after the mark",
        snap.messages.len() - at_summary - 1,
    );
    println!(
        "{} lines were printed as {} characters; the limit set for the summarisation is {} of a \
         window of {}",
        LINES_PRINTED, PRINTED_CHARS, LIMIT, WINDOW,
    );

    engine.shutdown();
}

// The characters `seq 1 6000` prints, with the newlines: nine of one digit, ninety of two, nine
// hundred of three, and the rest of four, and a newline under every line.
const PRINTED_CHARS: usize = 9 + 90 * 2 + 900 * 3 + (6_000 - 999) * 4 + 6_000;
