// A live check that a media marker quoted in text cannot stop a chat, and that a
// image still rides across. Not for the repository.
//
// llama.cpp markers the place of an image in the text of a prompt with a string
// of its own, counts those markers in the text it is sent, and refuses the whole
// ask when a marker is where no bitmap is. The error the user met came from a
// tool result that carried the marker of the server it had just read the
// properties of, and every later ask of that chat was refused with it.
//
// The server under test is `probe/mediamark/llama-server`: it counts the markers of
// its own in the text it is sent and refuses a request exactly as the real server
// does, and it answers any other ask with the number of images it was sent. It
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

// The marker of the server under test. It is made of parts here so that all of it never is in
// one piece of this file, in the log of a run, or in the conversation of the one who reads it.
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

// The messages the chat holds now, so the answer to the next send is found after it and not
// read off the answer before it.
fn held(engine: &Engine) -> usize {
    engine.snapshot().messages.len()
}

// The text of the answer to a send: the conversation grows by the user's message and by the
// answer, so the answer is the newest message of the model after the place the conversation was
// at when the send was created.
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

// One ask to a server of the same type, sent straight over HTTP: the text it carries are used
// as they are now, with nothing broken.
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
    wait_for("the server to remain ready", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy && !s.model_name.is_empty()).then_some(())
    });

    // The marker, as the server of the chat names it.
    let mark = the_mark();

    // The first ask: text that quote the marker, and no image. The marker is broken on the way
    // out, so the server has no place to search for a bitmap and answers.
    engine.new_chat();
    let before = held(&engine);
    engine.prompt(
        format!("The properties stated media_marker is '{}'. Say the number of images.", mark),
        vec![],
    );
    let said = answer_after(&engine, before);
    println!("the server answered: {}", said.trim());
    assert!(said.contains("Pictures received: 0"), "no answer came: {}", said);
    assert!(said.contains("Media mark present: no"), "a marker reached the server");

    // The second ask: the same quoted marker, and an image hung on the message. The image is
    // not a marker in the text and must ride across untouched.
    let before = held(&engine);
    engine.prompt(
        format!("Look at the image. The properties stated media_marker is '{}'.", mark),
        vec![Incoming {
            name: "orange-swatch.png".to_string(),
            mime: "image/png".to_string(),
            data: ORANGE.to_string(),
            text: String::new(),
        }],
    );
    let said = answer_after(&engine, before);
    println!("the server answered: {}", said.trim());
    assert!(said.contains("Pictures received: 1"), "the image did not ride across: {}", said);

    // What left the client, read off the log of the server. The bytes of the image is there
    // whole, and no whole marker remains anywhere.
    let written = std::fs::read_to_string(&asks).expect("the asks were written down");
    assert!(!written.contains(&mark), "a whole marker was sent to the server");
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
    assert!(carries_the_bytes, "the bytes of the image were changed");

    // And the trap under the check: the same server refuses a request that carries the marker
    // unbroken. Without the breaking, this test would fail.
    let port: u16 = 8177;
    let mut child = std::process::Command::new(root.join("probe/mediamark/llama-server"))
        .arg("--port")
        .arg(port.to_string())
        .spawn()
        .expect("the second server");
    let agent = ureq::AgentBuilder::new().timeout_read(Duration::from_secs(10)).build();
    wait_for("the second server to remain ready", || {
        agent
            .get(&format!("http://127.0.0.1:{}/health", port))
            .call()
            .ok()
            .map(|_| ())
    });
    let broken = post_words(port, "nothing of a marker here");
    let whole = post_words(port, &format!("media_marker is '{}'", mark));
    let mended = post_words(port, &format!("media_marker is '{}'", mark.replacen("<__", "< __", 1)));
    println!("plain: {} whole mark: {} broken mark: {}", broken, whole, mended);
    assert_eq!(broken, 200);
    assert_eq!(whole, 400, "the server did not refuse a whole marker");
    assert_eq!(mended, 200, "the broken marker was still read as one");
    let _ = child.kill();
}
