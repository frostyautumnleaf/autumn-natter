// A live check that a media mark quoted in words cannot stop a chat, and that a
// picture still rides across. Not for the repository.
//
// llama.cpp marks the place of a picture in the words of a prompt with a string
// of its own, counts those marks in the words it is sent, and refuses the whole
// ask when a mark stands where no bitmap is. The fault the reader met came from a
// tool receipt that carried the mark of the server it had just read the
// properties of, and every later ask of that chat was refused with it.
//
// The server under test is `probe/mediamark/llama-server`: it counts the marks of
// its own in the words it is sent and refuses an ask exactly as the real server
// does, and it answers any other ask with the number of pictures it was sent. It
// writes every ask whole to `asks.jsonl`, so the test reads the bytes that left
// the client.
//
//     cargo test -p autumn-natter-core --test live_media_mark -- --nocapture --test-threads=1

use autumn_natter_core::attach::Incoming;
use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

// The mark of the server under test. It is made of pieces here so that the whole
// of it never stands in one piece of this file, in the log of a run, or in the
// talk of the one who reads it.
fn the_mark() -> String {
    format!("{}{}{}", "<__", "media_", "TESTMARK00000000__>")
}

// A swatch of one colour, whole, as a data url.
const ORANGE: &str = concat!(
    "data:image/png;base64,",
    "iVBORw0KGgoAAAANSUhEUgAAAGAAAABgCAIAAABt+uBvAAAAjUlEQVR42u3QMQ0AAAgDsOlAM57B",
    "ASdXkypopotDFAgSJEiQIEGCBCFIkCBBggQJEoQgQYIECRIkSBCCBAkSJEiQIEGCECRIkCBBggQJ",
    "QpAgQYIECRIkCEGCBAkSJEiQIEEIEiRIkCBBggQhSJAgQYIECRKEIEGCBAkSJEiQIAQJEiRIkCBB",
    "ghAkSNCfBWZSN3ZEY/gyAAAAAElFTkSuQmCC"
);

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

// The messages the chat holds now, so the answer to the next send is found
// after it and not read off the answer before it.
fn held(engine: &Engine) -> usize {
    engine.snapshot().messages.len()
}

// The words of the answer to a send: the talk grows by the reader's message and
// by the answer, so the answer is the newest message of the model after the
// place the talk stood at when the send was made.
fn answer_after(engine: &Engine, before: usize) -> String {
    let snap = wait_for("the answer to end", || {
        let s = engine.snapshot();
        (s.messages.len() > before + 1 && !s.generating).then_some(s)
    });
    if snap.status.contains("tokenize") || snap.status.contains("400") {
        panic!("the ask was refused: {}", snap.status);
    }
    snap.messages
        .iter()
        .skip(before + 1)
        .rfind(|one| one.role == "Assistant")
        .map(|one| one.body.clone())
        .unwrap_or_default()
}

// One ask to a server of the same kind, sent straight over HTTP: the words it
// carries are used as they stand, with nothing broken.
fn post_words(port: u16, words: &str) -> u16 {
    let body = serde_json::json!({
        "model": "not a model",
        "messages": [{ "role": "user", "content": words }],
        "stream": false,
    });
    let agent = ureq::AgentBuilder::new().timeout_read(Duration::from_secs(20)).build();
    match agent
        .post(&format!("http://127.0.0.1:{}/v1/chat/completions", port))
        .send_json(body)
    {
        Ok(_) => 200,
        Err(ureq::Error::Status(code, _)) => code,
        Err(_) => 0,
    }
}

#[test]
fn a_quoted_media_mark_is_broken_before_the_ask_and_a_picture_still_rides() {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let data = root.join("uidata/live-media-mark");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    config::set_data_dir(data);

    let asks = root.join("probe/mediamark/asks.jsonl");
    let _ = std::fs::remove_file(&asks);

    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("the testing model")
        .display()
        .to_string();
    let ms = ModelSettings {
        vision: true,
        ctx_size: 4096,
        max_tokens: 60,
        gpu_enabled: vec![false; 8],
        ..Default::default()
    };
    let mut cfg = Config {
        llama_dir: root.join("probe/mediamark").display().to_string(),
        models_dir: root.join("testing_model").display().to_string(),
        model_id: model.clone(),
        ..Default::default()
    };
    cfg.model_settings.insert(model, ms);

    let engine = Engine::start(cfg);
    wait_for("the server to stand ready", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy && !s.model_name.is_empty()).then_some(())
    });

    // The mark, as the server of the chat names it.
    let mark = the_mark();

    // The first ask: words that quote the mark, and no picture. The mark is
    // broken on the way out, so the server has no place to look for a bitmap and
    // answers.
    engine.new_chat();
    let before = held(&engine);
    engine.prompt(
        format!("The properties said media_marker is '{}'. Say the number of pictures.", mark),
        vec![],
    );
    let said = answer_after(&engine, before);
    println!("the server answered: {}", said.trim());
    assert!(said.contains("Pictures received: 0"), "no answer came: {}", said);
    assert!(said.contains("Media mark present: no"), "a mark reached the server");

    // The second ask: the same quoted mark, and a picture hung on the message.
    // The picture is not a mark in the words and must ride across untouched.
    let before = held(&engine);
    engine.prompt(
        format!("Look at the picture. The properties said media_marker is '{}'.", mark),
        vec![Incoming {
            name: "orange-swatch.png".to_string(),
            mime: "image/png".to_string(),
            data: ORANGE.to_string(),
            text: String::new(),
        }],
    );
    let said = answer_after(&engine, before);
    println!("the server answered: {}", said.trim());
    assert!(said.contains("Pictures received: 1"), "the picture did not ride across: {}", said);

    // What left the client, read off the log of the server. The bytes of the
    // picture stand there whole, and no whole mark stands anywhere.
    let written = std::fs::read_to_string(&asks).expect("the asks were written down");
    assert!(!written.contains(&mark), "a whole mark went out to the server");
    assert!(written.contains("< __"), "no mark was broken, so nothing was quoted");
    let mut pictures = 0usize;
    let mut carries_the_bytes = false;
    for line in written.lines() {
        let ask: serde_json::Value = serde_json::from_str(line).expect("an ask is read as json");
        for message in ask["messages"].as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
            let parts = match message["content"].as_array() {
                Some(parts) => parts,
                None => continue,
            };
            for part in parts {
                if part["type"].as_str() == Some("image_url") {
                    pictures += 1;
                    if part["image_url"]["url"].as_str() == Some(ORANGE) {
                        carries_the_bytes = true;
                    }
                }
            }
        }
    }
    assert_eq!(pictures, 1, "the picture did not stand in the ask");
    assert!(carries_the_bytes, "the bytes of the picture were changed");

    // And the trap under the check: the same server refuses an ask that carries
    // the mark unbroken. Without the breaking, this test would fail.
    let port: u16 = 8177;
    let mut child = std::process::Command::new(root.join("probe/mediamark/llama-server"))
        .arg("--port")
        .arg(port.to_string())
        .spawn()
        .expect("the second server");
    let agent = ureq::AgentBuilder::new().timeout_read(Duration::from_secs(10)).build();
    wait_for("the second server to stand ready", || {
        agent
            .get(&format!("http://127.0.0.1:{}/health", port))
            .call()
            .ok()
            .map(|_| ())
    });
    let broken = post_words(port, "nothing of a mark here");
    let whole = post_words(port, &format!("media_marker is '{}'", mark));
    let mended = post_words(port, &format!("media_marker is '{}'", mark.replacen("<__", "< __", 1)));
    println!("plain: {} whole mark: {} broken mark: {}", broken, whole, mended);
    assert_eq!(broken, 200);
    assert_eq!(whole, 400, "the server did not refuse a whole mark");
    assert_eq!(mended, 200, "the broken mark was still read as one");
    let _ = child.kill();
}
