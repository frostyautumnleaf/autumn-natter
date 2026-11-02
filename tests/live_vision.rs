// A live check, not for the repository: it starts the real llama.cpp with the
// testing model on the CPU and hangs a picture on a message, so an answer
// about the colour can only come from the picture riding across. Run with
//
//     cargo test --release --test live_vision -- --nocapture --test-threads=1

// A swatch of one colour, as a whole small picture file. The test hangs it
// on the message, so an answer about the colour can only come from seeing it.
const ORANGE: &str = concat!(
    "data:image/png;base64,",
    "iVBORw0KGgoAAAANSUhEUgAAAGAAAABgCAIAAABt+uBvAAAAjUlEQVR42u3QMQ0AAAgDsOlAM57B",
    "ASdXkypopotDFAgSJEiQIEGCBCFIkCBBggQJEoQgQYIECRIkSBCCBAkSJEiQIEGCECRIkCBBggQJ",
    "QpAgQYIECRIkCEGCBAkSJEiQIEEIEiRIkCBBggQhSJAgQYIECRKEIEGCBAkSJEiQIAQJEiRIkCBB",
    "ghAkSNCfBWZSN3ZEY/gyAAAAAElFTkSuQmCC"
);

use autumn_natter_core::attach::Incoming;
use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

// Wait until a look gives an answer, or until a quarter of an hour has passed.
fn wait_for<T>(what: &str, mut look: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(900);
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(Instant::now() < until, "waited too long for {}", what);
        sleep(Duration::from_millis(500));
    }
}

#[test]
fn a_picture_hung_on_a_message_comes_back_as_words() {
    let root = PathBuf::from("..").canonicalize().expect("the sandbox root");
    let data = root.join("uidata/live-attach");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    config::set_data_dir(data);

    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("the testing model")
        .display()
        .to_string();
    let mut ms = ModelSettings {
        vision: true,
        ctx_size: 4096,
        max_tokens: 60,
        ..Default::default()
    };
    // The CPU only: the list of flags is fitted to the devices of the machine,
    // and a short list is padded with enabled ones.
    ms.gpu_enabled = vec![false; 8];
    // The testing box has no list of devices to speak of, so the two words of
    // the engine are added by hand. The card of the box is full of another
    // model, and a server left to itself would fall over trying to use it.
    ms.extra_args = "--device none -ngl 0".to_string();
    let mut cfg = Config {
        llama_dir: root.join("llamacpp/build/bin").display().to_string(),
        models_dir: root.join("testing_model").display().to_string(),
        model_id: model.clone(),
        ..Default::default()
    };
    cfg.model_settings.insert(model, ms);

    let engine = Engine::start(cfg);
    // The server needs its time with nine billion numbers.
    wait_for("the model to stand ready", || {
        let s = engine.snapshot();
        (!s.generating && !s.busy && !s.model_name.is_empty()).then_some(())
    });

    engine.new_chat();
    engine.prompt(
        "What colour is this picture? Answer in one short sentence.".to_string(),
        vec![Incoming {
            name: "orange-swatch.png".to_string(),
            mime: "image/png".to_string(),
            data: ORANGE.to_string(),
            text: String::new(),
        }],
    );
    // The worker first lets the reader's message stand in the thread, then
    // wakes the server, and at last writes the answer as its own message.
    wait_for("the reader's message to stand in the thread", || {
        engine
            .snapshot()
            .messages
            .iter()
            .any(|m| m.role == "You")
            .then_some(())
    });
    wait_for("the answer to stand in the thread", || {
        let s = engine.snapshot();
        if s.messages.iter().any(|m| m.role == "Assistant") {
            return Some(());
        }
        if s.status.contains("cannot") || s.status.contains("failed") {
            panic!("the engine said: {}", s.status);
        }
        None
    });
    let snap = wait_for("the answer to end", || {
        let s = engine.snapshot();
        (!s.generating).then_some(s)
    });
    println!("status: {}", snap.status);
    let user = snap
        .messages
        .iter().rfind(|m| m.role == "You")
        .expect("the reader's message");
    assert_eq!(user.attachments.len(), 1, "the picture did not hang on the message");
    assert_eq!(user.attachments[0].name, "orange-swatch.png");
    assert!(user.attachments[0].image, "the picture is not marked as one");
    let answer = snap
        .messages
        .iter().rfind(|m| m.role == "Assistant")
        .expect("the model's answer");
    println!("the model said: {}", answer.body.trim());
    assert!(
        answer.body.to_lowercase().contains("orange"),
        "the model did not see the colour: {}",
        answer.body
    );
}
