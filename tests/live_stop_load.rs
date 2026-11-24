// A live check that stopping an answer while its model is being loaded stops the load, not
// for the repository.
//
// The load of a model is part of the turn it was begun for. A model of many parts is minutes
// of waiting, and the user who stops the answer in the middle of them means the whole of the
// turn: the load with it. What this check stands against is a stop that sets a flag and no
// more - the child goes on reading the model in, the meter of the state stays at work, and
// the line says nothing of what became of the request.
//
// The server here is not llama.cpp but `probe/slowload/llama-server`: it opens its port and
// answers the health ask with 503 for the whole of its life, which is the shape llama.cpp
// has while a model is coming in and never loses. The turn therefore waits at that ask until
// it is stopped, and the check has the process id of the child and the pace of the asks
// written beside the script to look at.
//
// What the check is of:
//
//   * the turn is seen to be at the load of the model, with the child running and the health
//     of it being asked about;
//   * a stop asked for then brings the turn to its end in a fraction of a second, and not
//     when the reading of the model ends, which is never;
//   * the child is gone with it, so nothing is left loading a model no answer is waiting for;
//   * the line of state and a note in the chat say that the load was stopped, rather than
//     that the model failed to start;
//   * the message of the user is still in the chat, and no answer of a model was written into
//     it;
//   * the program is left idle, so the waiting lists of the chats and the settings that may
//     not move during an answer are not held back by a turn that ended.
//
//     cargo test -p autumn-natter-core --test live_stop_load -- --nocapture --test-threads=1

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

// The longest the wait for the turn to be seen at the load of its model. The scripted server
// answers at once, so the state of the load is come to quickly.
const LOADING: Duration = Duration::from_secs(30);
// The longest a stop is given to bring the turn to its end. The flag an answer is stopped
// with is asked about every round of the wait for the server, and a round is three tenths of
// a second. A stop that is not honoured at all is not come to within this either: the wait
// for a server that never becomes ready is given fifteen minutes.
const STOP: Duration = Duration::from_secs(15);

// Wait until a look gives an answer, or until the bound of the wait is out.
fn wait_for<T>(what: &str, limit: Duration, mut look: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + limit;
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(
            Instant::now() < until,
            "waited {} seconds for {}",
            limit.as_secs(),
            what
        );
        sleep(Duration::from_millis(50));
    }
}

// The process id the scripted server wrote down for itself. Nothing is given back before the
// server has started, and the file is read again every look.
fn child_pid(scratch: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(scratch.join("started.json")).ok()?;
    let told: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    told.get("pid")?.as_u64().map(|one| one as u32)
}

// Whether a process of that id is on the machine.
fn running(pid: u32) -> bool {
    Path::new(format!("/proc/{}", pid).as_str()).exists()
}

// How many times the health of the scripted server has been asked about. The pace of the asks
// is the pace the program waits at, so the number stands still only when the child that was
// asked is gone.
fn asks(scratch: &Path) -> usize {
    std::fs::read_to_string(scratch.join("health.log"))
        .unwrap_or_default()
        .lines()
        .count()
}

// The program, with its model pointed at the scripted server that never becomes ready.
fn stand_up() -> (Engine, PathBuf) {
    let root = PathBuf::from("..").canonicalize().expect("the root of the work");
    let scratch = root.join("probe/slowload");
    let data = root.join("uidata/live-stop-load");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).expect("a place for the program data");
    // What the last run of the check left behind is taken out of the way: the check reads the
    // words of the server it started, and of no other.
    let _ = std::fs::remove_file(scratch.join("started.json"));
    let _ = std::fs::remove_file(scratch.join("health.log"));
    config::set_data_dir(data);

    // The model file is only wanted so the program has a model to choose. The scripted server
    // reads nothing of it.
    let model = root
        .join("testing_model/Qwen3.5-9B-Q4_K_M.gguf")
        .canonicalize()
        .expect("a model file to choose")
        .display()
        .to_string();
    let ms = ModelSettings {
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
    (Engine::start(cfg), scratch)
}

#[test]
fn stopping_an_answer_while_the_model_loads_stops_the_load() {
    let (engine, scratch) = stand_up();
    engine.new_chat();
    engine.prompt(
        "Say something of the weather, in one line.".to_string(),
        Vec::new(),
    );

    // The turn is at the load of the model, and the child it is waiting for is running.
    let pid = wait_for("the load of the model to be waited at", LOADING, || {
        let snap = engine.snapshot();
        if !(snap.generating && snap.status.contains("loading")) {
            return None;
        }
        child_pid(&scratch).filter(|one| running(*one))
    });
    let asked = wait_for("the health of the server to be asked about", LOADING, || {
        let told = asks(&scratch);
        (told >= 3).then(|| {
            println!("the health of the server was asked about {} times", told);
            told
        })
    });
    println!(
        "the model is loading: the line says {:?}, the child is {}, the health has been asked \
         about {} times",
        engine.snapshot().status,
        pid,
        asked
    );

    // The stop the user asks for, taken at the moment the load is going on.
    let stopped = Instant::now();
    engine.cancel();

    let snap = wait_for("the turn to come to its end", STOP, || {
        let one = engine.snapshot();
        (!one.generating && !one.busy).then_some(one)
    });
    let took = stopped.elapsed();
    println!(
        "the turn came to its end {} ms after the stop. The line said {:?} and the program is \
         busy: {}",
        took.as_millis(),
        snap.status,
        snap.busy
    );
    assert!(
        took < STOP,
        "the turn was not come to its end within the {} seconds a stop is given",
        STOP.as_secs()
    );

    // What the user is told. The load was stopped, and it is said so: a line about a model
    // that did not start sends them looking for a fault of the machine.
    assert!(
        snap.status.contains("Stopped before the model loaded"),
        "the line stated: {:?}",
        snap.status
    );
    assert!(
        snap.messages.iter().any(|one| one.role == "Note"
            && one.body.contains("The load of the model was stopped")),
        "nothing in the chat says what became of the load: {:?}",
        snap.messages
            .iter()
            .map(|one| format!("{}: {}", one.role, one.body.chars().take(60).collect::<String>()))
            .collect::<Vec<_>>()
    );

    // The message of the user is where it was, and no answer of a model was written into the
    // chat. Stopping a load takes nothing of the conversation away.
    assert!(
        snap.messages
            .iter()
            .any(|one| one.role == "You" && one.body.contains("weather")),
        "the message of the user did not stay in the chat"
    );
    assert!(
        !snap.messages.iter().any(|one| one.role == "Assistant"),
        "an answer of a model was written into the chat that never answered"
    );

    // The child is gone with the turn it was loading for, and the asking about its health
    // went with it.
    wait_for("the child of the load to be gone", STOP, || {
        (!running(pid)).then_some(())
    });
    let at_rest = asks(&scratch);
    sleep(Duration::from_millis(1200));
    assert_eq!(
        asks(&scratch),
        at_rest,
        "the health of a server that no longer runs is still being asked about"
    );
    println!(
        "the child was gone {} ms after the stop and the health was asked no more ({} asks in \
         all)",
        stopped.elapsed().as_millis(),
        at_rest
    );

    // Nothing of the program is left held at work by a turn that ended in the middle of its
    // load.
    let rest = engine.snapshot();
    assert!(
        !rest.generating && !rest.busy,
        "the program is left busy after a stopped load: generating {} busy {}",
        rest.generating,
        rest.busy
    );
    engine.shutdown();
}
