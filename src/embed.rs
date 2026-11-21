// The embedding model: what may be had, how it is downloaded, and the process that turns text
// into vectors.
//
// Retrieval needs a model that counts words rather than one that talks. `Embedding Gemma 2` is
// such a model. It is had as a GGUF file from Hugging Face, like the chat models, with one
// difference: the program fetches it by itself. The file is small next to a chat model, it is
// wanted by one feature only, and a user who never turns RAG on should not have to look for it.
// Nothing is downloaded until it is asked for in the settings panel.
//
// The files stay in `embeddings/` inside the data folder. The vision projector (the `mmproj`
// file) is optional: it lets the same model count the contents of an image. It may be added at
// any time afterwards, and when it is in the folder beside the model it is used without being
// asked for again.
//
// The vectors come from a second llama-server, started with `--embeddings` on a port of its own.
// It starts when it is wanted and stops after five minutes without a request, so a chat that
// searches now and then does not hold a model in memory in between. It is never the server the
// answers come from: the two hold different models, and one search must not push the chat model
// out of the memory of the computer.
//
// Only the minimal set of arguments is given to the server. Pooling comes from the model file,
// and the vectors are normalised here, so cosine similarity between them is the dot product and
// no argument that changes its name between versions of llama.cpp is needed.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde_json::{json, Value};

use crate::config;
use crate::proglog;
use crate::util;

// The choices made when nothing was chosen: the model of the program, its biggest quantization,
// and the vision projector with it.
pub const DEFAULT_MODEL: &str = "embeddinggemma-2";
pub const DEFAULT_QUANT: &str = "BF16";
pub const DEFAULT_MMPROJ: &str = "BF16";

// The word that means the vision projector is not wanted. It is a choice in the settings, and
// not the absence of one, so a user who does not count images is not asked again.
pub const NO_MMPROJ: &str = "none";

// One quantization of one model: the word the user reads, the file it is in, and about how big
// it is. The size is only for the label, so it is written as it is written on the site the file
// comes from.
pub struct Quant {
    pub label: &'static str,
    pub file: &'static str,
    pub size: &'static str,
}

// One embedding model that may be had, with its quantizations and its projectors.
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    // The repository on Hugging Face the files come from.
    pub repo: &'static str,
    // What the model is, for the line beside its name in the settings.
    pub note: &'static str,
    pub quants: &'static [Quant],
    pub mmprojs: &'static [Quant],
}

// The models that may be had. Only one is offered today; the list is a list so a second model
// costs a few lines here and nothing anywhere else.
const MODELS: &[Model] = &[Model {
    id: DEFAULT_MODEL,
    name: "Embedding Gemma 2",
    repo: "unsloth/embeddinggemma-2-GGUF",
    note: "Counts the words of a page, and with its vision projector the contents of a picture.",
    quants: &[
        Quant {
            label: "BF16",
            file: "embeddinggemma-2-BF16.gguf",
            size: "558 MB",
        },
        Quant {
            label: "F16",
            file: "embeddinggemma-2-F16.gguf",
            size: "558 MB",
        },
        Quant {
            label: "Q8_0",
            file: "embeddinggemma-2-Q8_0.gguf",
            size: "310 MB",
        },
        Quant {
            label: "Q6_K_XL",
            file: "embeddinggemma-2-UD-Q6_K_XL.gguf",
            size: "249 MB",
        },
        Quant {
            label: "Q5_K_XL",
            file: "embeddinggemma-2-UD-Q5_K_XL.gguf",
            size: "210 MB",
        },
        Quant {
            label: "Q4_K_XL",
            file: "embeddinggemma-2-UD-Q4_K_XL.gguf",
            size: "176 MB",
        },
    ],
    mmprojs: &[
        Quant {
            label: "BF16",
            file: "mmproj-BF16.gguf",
            size: "982 MB",
        },
        Quant {
            label: "F16",
            file: "mmproj-F16.gguf",
            size: "981 MB",
        },
        Quant {
            label: "Q8_0",
            file: "mmproj-Q8_0.gguf",
            size: "555 MB",
        },
    ],
}];

// How long the server may sit without a request before it is stopped, in seconds. Long enough
// that a batch of files is one start of it, and short enough that a model is not held for the
// sake of a search that already happened.
const IDLE_SECS: u64 = 300;

// The size of the context asked for. One chunk of a source is about 900 characters, so this is
// far more than any one ask needs, and the server is not asked to hold a conversation.
const SERVER_CTX: &str = "4096";

// ------------------------------------------------------------------ catalog

// Every model that may be had, with the state of every file of it.
pub fn models() -> &'static [Model] {
    MODELS
}

// The model one word names. A word that names nothing answers with the model of the program, so
// a settings file that names a model this program does not know is read as the default rather
// than as nothing at all.
pub fn find(id: &str) -> &'static Model {
    MODELS
        .iter()
        .find(|one| one.id == id)
        .unwrap_or(&MODELS[0])
}

// The word of a model that a settings file may hold.
pub fn sanitize_model(id: &str) -> String {
    find(id).id.to_string()
}

// The quantization of a model. A word that is none of them is read as the biggest one.
pub fn sanitize_quant(id: &str, quant: &str) -> String {
    let model = find(id);
    model
        .quants
        .iter()
        .find(|one| one.label == quant)
        .map(|one| one.label.to_string())
        .unwrap_or_else(|| DEFAULT_QUANT.to_string())
}

// The vision projector of a model. The word that means none is a choice and stays, and any
// other word that is not a projector of the model is read as the default one.
pub fn sanitize_mmproj(id: &str, chosen: &str) -> String {
    if chosen == NO_MMPROJ {
        return String::from(NO_MMPROJ);
    }
    let model = find(id);
    model
        .mmprojs
        .iter()
        .find(|one| one.label == chosen)
        .map(|one| one.label.to_string())
        .unwrap_or_else(|| DEFAULT_MMPROJ.to_string())
}

// The file of one quantization, or nothing when the word names none of them.
pub fn file_of(model: &Model, quant: &str) -> Option<&'static str> {
    model
        .quants
        .iter()
        .find(|one| one.label == quant)
        .map(|one| one.file)
}

// The file of one vision projector, or nothing when the word means none or names no projector.
pub fn mmproj_of(model: &Model, chosen: &str) -> Option<&'static str> {
    if chosen == NO_MMPROJ {
        return None;
    }
    model
        .mmprojs
        .iter()
        .find(|one| one.label == chosen)
        .map(|one| one.file)
}

// The address a file is fetched from.
pub fn url(model: &Model, file: &str) -> String {
    format!(
        "https://huggingface.co/{}/resolve/main/{}",
        model.repo, file
    )
}

// ------------------------------------------------------------------- folder

// The folder the embedding files stay in, under a root named by the caller. It is created when
// it is named, because the download has nowhere to write otherwise.
pub fn folder_in(root: &Path) -> PathBuf {
    let place = root.join("embeddings");
    let _ = fs::create_dir_all(&place);
    place
}

// The folder the embedding files of this program stay in.
pub fn dir() -> PathBuf {
    folder_in(&config::paths().data)
}

// The file of a model of one quantization, in a folder named. The file is not asked to be there.
pub fn model_file_in(folder: &Path, model_id: &str, quant: &str) -> Option<PathBuf> {
    let model = find(model_id);
    let file = file_of(model, &sanitize_quant(model_id, quant))?;
    Some(folder.join(file))
}

// The file of the vision projector that was chosen, in a folder named. Nothing when none was
// chosen.
pub fn projector_file_in(folder: &Path, model_id: &str, chosen: &str) -> Option<PathBuf> {
    let model = find(model_id);
    let file = mmproj_of(model, &sanitize_mmproj(model_id, chosen))?;
    Some(folder.join(file))
}

// A projector of the model that is in the folder, whichever quantization it is. The program uses
// a projector it finds: one put there by hand, or one downloaded under another choice in the
// settings, is used either way. The chosen one is looked for first.
pub fn projector_in(folder: &Path, model_id: &str, chosen: &str) -> Option<PathBuf> {
    let wanted = projector_file_in(folder, model_id, chosen);
    if wanted.as_deref().map(Path::is_file).unwrap_or(false) {
        return wanted;
    }
    if chosen == NO_MMPROJ {
        return None;
    }
    find(model_id)
        .mmprojs
        .iter()
        .map(|one| folder.join(one.file))
        .find(|one| one.is_file())
}

// ---------------------------------------------------------------- the state

// What the settings panel and the badge need to know: which model and quantization are chosen,
// which files are in the folder and how big they are, whether the server of the embeddings runs
// now, and how the download that may be going on is going.
pub fn state(model_id: &str, quant: &str, chosen_mmproj: &str) -> Value {
    let model = find(model_id);
    let folder = dir();
    let chosen = sanitize_quant(model_id, quant);
    let projector = sanitize_mmproj(model_id, chosen_mmproj);
    let line = |one: &Quant| {
        let place = folder.join(one.file);
        json!({
            "label": one.label,
            "file": one.file,
            "size": one.size,
            "installed": place.is_file(),
            "bytes": file_bytes(&place),
        })
    };
    let model_file = file_of(model, &chosen).map(|one| folder.join(one));
    let used_projector = projector_in(&folder, model_id, &projector);
    json!({
        "ok": true,
        "model": model.id,
        "name": model.name,
        "note": model.note,
        "quant": chosen,
        "mmproj": projector,
        "models": [models_value(model)],
        "quants": model.quants.iter().map(line).collect::<Vec<_>>(),
        "mmprojs": model.mmprojs.iter().map(line).collect::<Vec<_>>(),
        "installed": model_file.as_deref().map(Path::is_file).unwrap_or(false),
        "path": model_file.as_ref().map(|one| one.display().to_string()).unwrap_or_default(),
        "projector_used": used_projector.map(|one| one.display().to_string()).unwrap_or_default(),
        "server": server_state(),
        "job": job_value(),
        "dir": folder.display().to_string(),
    })
}

// The one model of the catalog, in the form the selects of the settings panel are built from.
fn models_value(model: &Model) -> Value {
    json!({
        "id": model.id,
        "name": model.name,
        "note": model.note,
        "repo": model.repo,
        "quants": model.quants.iter().map(|one| json!({
            "label": one.label, "size": one.size, "default": one.label == DEFAULT_QUANT,
        })).collect::<Vec<_>>(),
        "mmprojs": model.mmprojs.iter().map(|one| json!({
            "label": one.label, "size": one.size, "default": one.label == DEFAULT_MMPROJ,
        })).collect::<Vec<_>>(),
    })
}

// The size of a file that is there, and 0 when it is not.
fn file_bytes(place: &Path) -> u64 {
    fs::metadata(place).map(|one| one.len()).unwrap_or(0)
}

// ---------------------------------------------------------------- download

// What is being downloaded, and how far it has come. One download at a time: two files of one
// model come one after another, and a user who asks for a second thing while one is going is
// answered with the reason.
#[derive(Clone, Debug, Default)]
pub struct Job {
    pub active: bool,
    // "model", "projector" or "done".
    pub stage: String,
    // The file that is coming down now.
    pub file: String,
    pub done: u64,
    pub total: u64,
    // A word for the user when the download left off.
    pub note: String,
    // True when the download stopped with a reason.
    pub failed: bool,
    // Set when the user asked for the download to be given up.
    pub cancel: bool,
}

static JOB: OnceLock<Mutex<Job>> = OnceLock::new();

fn job() -> &'static Mutex<Job> {
    JOB.get_or_init(|| Mutex::new(Job::default()))
}

// The job as the page reads it.
pub fn job_value() -> Value {
    let held = job().lock().unwrap_or_else(|gone| gone.into_inner());
    json!({
        "active": held.active,
        "stage": held.stage,
        "file": held.file,
        "done": held.done,
        "total": held.total,
        "note": held.note,
        "failed": held.failed,
        "percent": if held.total > 0 {
            ((held.done as f64 / held.total as f64) * 100.0).round() as i64
        } else { 0 },
    })
}

// True while a download is going on.
pub fn busy() -> bool {
    job().lock().unwrap_or_else(|gone| gone.into_inner()).active
}

// Ask a download that is going on to stop. The file that was coming down is thrown away: half a
// model file is of no use to anybody.
pub fn cancel() {
    let mut held = job().lock().unwrap_or_else(|gone| gone.into_inner());
    if held.active {
        held.cancel = true;
    }
}

// Start the download of the chosen model and, when one was chosen and it is not there already,
// of its vision projector. The call returns at once: the files come down in a thread of their
// own, and the page reads how they are going through `job_value`.
pub fn install(model_id: &str, quant: &str, chosen_mmproj: &str) -> Result<(), String> {
    let model = find(model_id);
    let chosen = sanitize_quant(model_id, quant);
    let projector = sanitize_mmproj(model_id, chosen_mmproj);
    let file = file_of(model, &chosen).ok_or("No such quantization")?.to_string();
    let extra = mmproj_of(model, &projector).map(|one| one.to_string());
    {
        let mut held = job().lock().unwrap_or_else(|gone| gone.into_inner());
        if held.active {
            return Err(String::from("A download is going on already"));
        }
        held.active = true;
        held.cancel = false;
        held.failed = false;
        held.stage = String::from("model");
        held.file = file.clone();
        held.done = 0;
        held.total = 0;
        held.note = String::new();
    }
    let id = model.id.to_string();
    std::thread::Builder::new()
        .name("an-embed-download".to_string())
        .spawn(move || {
            let made = fetch_files(&id, &file, extra.as_deref());
            let mut held = job().lock().unwrap_or_else(|gone| gone.into_inner());
            held.active = false;
            match made {
                Ok(words) => {
                    held.stage = String::from("done");
                    held.note = words;
                    held.failed = false;
                }
                Err(problem) => {
                    held.stage = String::from("failed");
                    held.note = problem;
                    held.failed = true;
                }
            }
            proglog::note(
                proglog::FOLDERS,
                &format!("embedding model: {}", held.note),
            );
        })
        .map_err(|why| format!("The download could not be started: {}", why))?;
    Ok(())
}

// Bring the files down. The model first, because without it nothing else of this works, and the
// projector after it. A file that is already in the folder is not fetched again, which is what
// the user wants in every case the settings panel can put them in: the model is asked for once,
// and the projector may be added a month later.
fn fetch_files(model_id: &str, file: &str, extra: Option<&str>) -> Result<String, String> {
    let model = find(model_id);
    let mut said: Vec<String> = Vec::new();
    let folder = dir();
    if folder.join(file).is_file() {
        said.push(String::from("the model file is there already"));
    } else {
        download(model, file, "model")?;
        said.push(String::from("the model file is in place"));
    }
    if let Some(extra) = extra {
        if folder.join(extra).is_file() {
            said.push(String::from("the vision projector is there already"));
        } else {
            download(model, extra, "projector")?;
            said.push(String::from("the vision projector is in place"));
        }
    }
    Ok(said.join(", and "))
}

// One file, with its progress on the line of the job. The bytes are written to a second file and
// moved into place at the end, so an interrupted download never leaves a short model file that
// looks like a whole one.
fn download(model: &Model, file: &str, stage: &str) -> Result<(), String> {
    let address = url(model, file);
    let folder = dir();
    let place = folder.join(file);
    let part = folder.join(format!("{}.part", file));
    let _ = fs::remove_file(&part);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        // The read is a piece of a big file at a time, so this is the wait for one piece and not
        // for the whole file.
        .timeout_read(Duration::from_secs(60))
        .redirects(10)
        .build();
    let response = agent
        .get(&address)
        .call()
        .map_err(|why| format!("{} could not be asked for: {}", file, first_words(&why.to_string(), 160)))?;
    let total = response
        .header("content-length")
        .and_then(|one| one.parse::<u64>().ok())
        .unwrap_or(0);
    {
        let mut held = job().lock().unwrap_or_else(|gone| gone.into_inner());
        held.stage = stage.to_string();
        held.file = file.to_string();
        held.total = total;
        held.done = 0;
    }
    let made = fs::File::create(&part).map_err(|why| {
        format!(
            "{} cannot be written into: {}",
            folder.display(),
            why
        )
    });
    let mut out = match made {
        Ok(handle) => handle,
        Err(problem) => return Err(problem),
    };
    let mut stream = response.into_reader();
    let mut piece = [0u8; 64 * 1024];
    let mut done: u64 = 0;
    // The moment the line of the job was written last. A file of half a gigabyte is fetched in
    // thousands of pieces, and the job is only worth writing a few times a second.
    let mut told = Instant::now();
    loop {
        if cancelled() {
            drop(out);
            let _ = fs::remove_file(&part);
            return Err(String::from("the download was given up"));
        }
        match stream.read(&mut piece) {
            Ok(0) => break,
            Ok(got) => {
                out.write_all(&piece[..got])
                    .map_err(|why| format!("{} could not be written: {}", file, why))?;
                done += got as u64;
                if told.elapsed() >= Duration::from_millis(400) {
                    let mut held = job().lock().unwrap_or_else(|gone| gone.into_inner());
                    held.done = done;
                    told = Instant::now();
                }
            }
            Err(why) => {
                drop(out);
                let _ = fs::remove_file(&part);
                return Err(format!("{} came to an end: {}", file, why));
            }
        }
    }
    drop(out);
    fs::rename(&part, &place)
        .map_err(|why| format!("{} could not be put in place: {}", file, why))?;
    let mut held = job().lock().unwrap_or_else(|gone| gone.into_inner());
    held.done = done;
    held.total = done.max(total);
    Ok(())
}

// True when the user asked for the download to be given up.
fn cancelled() -> bool {
    job().lock().unwrap_or_else(|gone| gone.into_inner()).cancel
}

// ------------------------------------------------------------------ runner

// The embedding server of this run. One at a time: it holds one model, and a second one would
// hold the same file twice.
struct Holder {
    child: Child,
    url: String,
    // The model and projector this server loaded. Another choice means a new server.
    key: String,
    last: Instant,
}

static RUNNER: OnceLock<Mutex<Option<Holder>>> = OnceLock::new();

fn runner_slot() -> &'static Mutex<Option<Holder>> {
    RUNNER.get_or_init(|| Mutex::new(None))
}

// Stop the embedding server. Nothing happens when none runs. The program calls this when it is
// asked to let go of the model, and a run that ends in another way leaves the child with the
// process it came from.
pub fn stop() {
    let mut held = runner_slot().lock().unwrap_or_else(|gone| gone.into_inner());
    if let Some(one) = held.as_mut() {
        util::kill_pid(one.child.id());
        let _ = one.child.wait();
    }
    *held = None;
}

// The state of the server, for the settings panel.
pub fn server_state() -> Value {
    let held = runner_slot().lock().unwrap_or_else(|gone| gone.into_inner());
    match held.as_ref() {
        Some(one) => json!({ "running": true, "url": one.url }),
        None => json!({ "running": false, "url": "" }),
    }
}

// True when a server is running with the model and projector the settings ask for.
pub fn running_with(model_id: &str, quant: &str, chosen_mmproj: &str) -> bool {
    let key = key_of(model_id, quant, chosen_mmproj);
    let held = runner_slot().lock().unwrap_or_else(|gone| gone.into_inner());
    held.as_ref().map(|one| one.key == key).unwrap_or(false)
}

// The words that name one server: the model file and the projector file, as they are on the
// disk. They are taken from the settings, so a change of either is a new server.
fn key_of(model_id: &str, quant: &str, chosen_mmproj: &str) -> String {
    let folder = dir();
    let model = model_file_in(&folder, model_id, quant)
        .map(|one| one.display().to_string())
        .unwrap_or_default();
    let projector = projector_in(&folder, model_id, &sanitize_mmproj(model_id, chosen_mmproj))
        .map(|one| one.display().to_string())
        .unwrap_or_default();
    format!("{}|{}", model, projector)
}

// Ask for the address of the server, starting it when it is not up or when the settings ask for
// another file. The lock is held across the start, so two chats that search at the same moment
// load one model and not two.
fn server_for(
    model_id: &str,
    quant: &str,
    chosen_mmproj: &str,
    binary: &Path,
) -> Result<String, String> {
    let mut held = runner_slot().lock().unwrap_or_else(|gone| gone.into_inner());
    let key = key_of(model_id, quant, chosen_mmproj);
    if let Some(one) = held.as_mut() {
        let idle = one.last.elapsed() > Duration::from_secs(IDLE_SECS);
        if one.key == key && !idle && one.child.try_wait().map(|one| one.is_none()).unwrap_or(false)
        {
            one.last = Instant::now();
            return Ok(one.url.clone());
        }
        util::kill_pid(one.child.id());
        let _ = one.child.wait();
        *held = None;
    }
    let folder = dir();
    let model = model_file_in(&folder, model_id, quant)
        .filter(|one| one.is_file())
        .ok_or("No embedding model is installed. Choose one in the settings and install it.")?;
    let projector = projector_in(&folder, model_id, &sanitize_mmproj(model_id, chosen_mmproj));
    let started = start(&model, projector.as_deref(), binary, key.clone())?;
    let url = started.url.clone();
    *held = Some(started);
    Ok(url)
}

// Start llama-server with `--embeddings` and wait until it answers. Only the words the server
// cannot do without are given to it, so a build of llama.cpp older than the last few months
// still takes the command line.
fn start(model: &Path, projector: Option<&Path>, binary: &Path, key: String) -> Result<Holder, String> {
    let port = util::free_port().ok_or("No free port on this machine")?;
    let folder = dir();
    let log_place = folder.join("embedding-server.log");
    let log = fs::File::create(&log_place).ok();
    let mut command = Command::new(binary);
    command
        .arg("--model")
        .arg(model)
        .arg("--embeddings")
        .arg("--alias")
        .arg("embeddings")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("-c")
        .arg(SERVER_CTX)
        .arg("--no-webui");
    if let Some(extra) = projector {
        command.arg("--mmproj").arg(extra);
    }
    command
        .stdin(Stdio::null())
        .stdout(match log {
            Some(handle) => Stdio::from(handle),
            None => Stdio::null(),
        })
        // The error text of a server that will not start is worth reading, so it is written into
        // the log of the start rather than thrown away.
        .stderr(Stdio::inherit());
    let child = command
        .spawn()
        .map_err(|why| format!("The embedding server could not be started: {}", why))?;
    let url = format!("http://127.0.0.1:{}", port);
    let mut holder = Holder {
        child,
        url: url.clone(),
        key,
        last: Instant::now(),
    };
    if let Err(problem) = wait_ready(&mut holder, &log_place) {
        util::kill_pid(holder.child.id());
        let _ = holder.child.wait();
        return Err(problem);
    }
    Ok(holder)
}

// Wait for the health endpoint. The load of a model of half a gigabyte is a few seconds on a
// cold disk and less than that on a warm one.
fn wait_ready(holder: &mut Holder, log_place: &Path) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        if let Ok(Some(_)) = holder.child.try_wait() {
            return Err(format!(
                "The embedding server stopped before it was ready. Read {}",
                log_place.display()
            ));
        }
        if crate::llama::get_json(&format!("{}/health", holder.url)).is_ok() {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!(
                "The embedding server did not answer in time. Read {}",
                log_place.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

// The vectors of a list of texts, in the order they were given. The two forms the endpoint may
// answer with are both read: the OpenAI form first, and the plain form of llama-server after it.
fn embed_at(url: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        // A batch of a few hundred chunks takes a few seconds on a model of this size.
        .timeout_read(Duration::from_secs(300))
        .build();
    let body = json!({ "input": texts });
    let first = format!("{}/v1/embeddings", url);
    let made = agent.post(&first).send_json(&body);
    if let Ok(response) = made {
        let mut text = String::new();
        let _ = response.into_reader().read_to_string(&mut text);
        if let Some(made) = openai_words(&text, texts.len()) {
            return Ok(made);
        }
    }
    let plain = format!("{}/embedding", url);
    let body = json!({ "content": texts });
    let made = agent
        .post(&plain)
        .send_json(&body)
        .map_err(|why| format!("The embedding server could not be asked: {}", first_words(&why.to_string(), 160)))?;
    let mut text = String::new();
    let _ = made.into_reader().read_to_string(&mut text);
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| String::from("The embedding server answered with text that could not be read"))?;
    let rows = value
        .as_array()
        .ok_or("The embedding server answered with neither a list of vectors nor anything like one")?;
    let mut out: Vec<Vec<f32>> = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(numbers(row)?);
    }
    Ok(normalise(out))
}

// The OpenAI form of the answer: `{"data": [{"embedding": [...]}, ...]}`. Nothing when the text
// is not of that form, which is when the other form is tried.
fn openai_words(text: &str, wanted: usize) -> Option<Vec<Vec<f32>>> {
    let value: Value = serde_json::from_str(text).ok()?;
    let rows = value.get("data")?.as_array()?;
    if rows.len() != wanted {
        return None;
    }
    let mut out: Vec<Vec<f32>> = Vec::with_capacity(rows.len());
    for row in rows {
        let made = row.get("embedding")?;
        out.push(numbers(made).ok()?);
    }
    Some(normalise(out))
}

// One vector of numbers.
fn numbers(value: &Value) -> Result<Vec<f32>, String> {
    let row = value
        .as_array()
        .ok_or("The embedding server gave something that is not a list of numbers")?;
    if row.is_empty() {
        return Err(String::from("The embedding server gave no numbers at all"));
    }
    let mut out: Vec<f32> = Vec::with_capacity(row.len());
    for one in row {
        let number = one
            .as_f64()
            .ok_or("The embedding server gave something that is not a number")?;
        out.push(number as f32);
    }
    Ok(out)
}

// Make every vector of a list the length one. Cosine similarity between two of them is then
// their dot product, and a source that holds a hundred times as much text as another is not
// ranked above it for that reason.
pub fn normalise(vectors: Vec<Vec<f32>>) -> Vec<Vec<f32>> {
    vectors
        .into_iter()
        .map(|one| {
            let sum: f32 = one.iter().map(|word| word * word).sum();
            if sum <= 0.0 {
                return one;
            }
            let root = sum.sqrt();
            one.into_iter().map(|word| word / root).collect()
        })
        .collect()
}

// The first words of a reason, for a text the user reads.
fn first_words(words: &str, most: usize) -> String {
    let cut = words.chars().take(most).collect::<String>();
    if cut.chars().count() < words.chars().count() {
        format!("{}...", cut)
    } else {
        cut
    }
}

// The embedder the settings ask for. Nothing is started here: the server is only started when a
// text is actually counted.
pub fn server(binary: &Path, model_id: &str, quant: &str, chosen_mmproj: &str) -> Arc<dyn crate::rag::Embedder> {
    Arc::new(Runner {
        binary: binary.to_path_buf(),
        model: sanitize_model(model_id),
        quant: sanitize_quant(model_id, quant),
        mmproj: sanitize_mmproj(model_id, chosen_mmproj),
    })
}

// One embedding server, asked for by the settings of one run.
struct Runner {
    binary: PathBuf,
    model: String,
    quant: String,
    mmproj: String,
}

impl crate::rag::Embedder for Runner {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let url = server_for(&self.model, &self.quant, &self.mmproj, &self.binary)?;
        embed_at(&url, texts)
    }

    fn ready(&self) -> bool {
        let folder = dir();
        model_file_in(&folder, &self.model, &self.quant)
            .map(|one| one.is_file())
            .unwrap_or(false)
    }

    fn vision(&self) -> bool {
        projector_in(&dir(), &self.model, &self.mmproj).is_some()
    }

    // The vector of one picture. The counting endpoint of llama.cpp takes a picture as base64
    // beside a text, and answers with the vector of the two together: the name of the file goes
    // into the text beside it, so a question that names a source finds the picture of it as well
    // as a page of writing.
    fn embed_image(&self, name: &str, bytes: &[u8]) -> Result<Vec<f32>, String> {
        if bytes.is_empty() {
            return Err(String::from("the picture has no bytes in it"));
        }
        let url = server_for(&self.model, &self.quant, &self.mmproj, &self.binary)?;
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(300))
            .build();
        let body = json!({
            "content": format!("image {}", name),
            "image": [B64.encode(bytes)],
        });
        let made = agent
            .post(&format!("{}/embedding", url))
            .send_json(&body)
            .map_err(|why| {
                format!(
                    "the counting server would not take the picture: {}",
                    first_words(&why.to_string(), 160)
                )
            })?;
        let mut text = String::new();
        let _ = made.into_reader().read_to_string(&mut text);
        let value: Value = serde_json::from_str(&text).map_err(|_| {
            String::from("the counting server answered the picture with text that could not be read")
        })?;
        // The plain endpoint answers with a list of vectors. A server that answers with the OpenAI
        // shape is read the same way, so one reader is enough for both.
        let first = if let Some(rows) = value.as_array() {
            rows.first().cloned().unwrap_or(Value::Null)
        } else {
            value
                .get("data")
                .and_then(|one| one.as_array())
                .and_then(|rows| rows.first())
                .and_then(|one| one.get("embedding"))
                .cloned()
                .unwrap_or(Value::Null)
        };
        let mut made = numbers(&first).map(|one| normalise(vec![one]))?;
        made.pop().filter(|one| !one.is_empty()).ok_or_else(|| {
            String::from("the counting server gave no vector for the picture")
        })
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    // The catalog answers to every word the settings panel may send, and the words that name
    // nothing are read as the defaults.
    #[test]
    fn the_catalog_reads_every_word_it_may_be_asked() {
        let model = find(DEFAULT_MODEL);
        assert_eq!(model.id, "embeddinggemma-2");
        assert_eq!(model.quants[0].file, "embeddinggemma-2-BF16.gguf");
        assert_eq!(model.mmprojs[0].file, "mmproj-BF16.gguf");
        // A word that names no model is the default model, and a quantization that is none of
        // the list is the biggest one.
        assert_eq!(sanitize_model("nothing at all"), DEFAULT_MODEL);
        assert_eq!(sanitize_quant(DEFAULT_MODEL, "BF16"), "BF16");
        assert_eq!(sanitize_quant(DEFAULT_MODEL, "Q4_K_XL"), "Q4_K_XL");
        assert_eq!(sanitize_quant(DEFAULT_MODEL, "made up"), DEFAULT_QUANT);
        assert_eq!(sanitize_mmproj(DEFAULT_MODEL, NO_MMPROJ), NO_MMPROJ);
        assert_eq!(sanitize_mmproj(DEFAULT_MODEL, "Q8_0"), "Q8_0");
        assert_eq!(sanitize_mmproj(DEFAULT_MODEL, "made up"), DEFAULT_MMPROJ);
        assert_eq!(
            file_of(model, "Q4_K_XL"),
            Some("embeddinggemma-2-UD-Q4_K_XL.gguf")
        );
        assert_eq!(mmproj_of(model, NO_MMPROJ), None);
        assert_eq!(url(model, "mmproj-F16.gguf"), "https://huggingface.co/unsloth/embeddinggemma-2-GGUF/resolve/main/mmproj-F16.gguf");
    }

    // The file of the vision projector that is in the folder is found whatever the choice in the
    // settings was, and a choice of none finds nothing.
    #[test]
    fn a_projector_in_the_folder_is_found() {
        let root = std::env::temp_dir().join("an-embed-projector");
        let _ = fs::remove_dir_all(&root);
        let folder = folder_in(&root);
        assert!(folder.is_dir());
        assert_eq!(projector_in(&folder, DEFAULT_MODEL, "BF16"), None);
        fs::write(folder.join("mmproj-Q8_0.gguf"), b"x").expect("the file is written");
        let found = projector_in(&folder, DEFAULT_MODEL, "BF16");
        assert_eq!(found, Some(folder.join("mmproj-Q8_0.gguf")));
        // The chosen file is looked for first, so a folder that holds two projectors answers with
        // the one the user chose.
        fs::write(folder.join("mmproj-F16.gguf"), b"x").expect("the file is written");
        assert_eq!(
            projector_in(&folder, DEFAULT_MODEL, "F16"),
            Some(folder.join("mmproj-F16.gguf"))
        );
        // A choice of none means none, even when the folder holds a projector.
        assert_eq!(projector_in(&folder, DEFAULT_MODEL, NO_MMPROJ), None);
        let _ = fs::remove_dir_all(&root);
    }

    // Two vectors of the same direction come to the same length, and the length is one.
    #[test]
    fn a_vector_is_made_the_length_one() {
        let made = normalise(vec![vec![3.0f32, 4.0]]);
        assert_eq!(made.len(), 1);
        let near = (made[0][0] - 0.6).abs() < 1e-6 && (made[0][1] - 0.8).abs() < 1e-6;
        assert!(near, "the vector is 0.6 and 0.8");
        // A vector of nothing is left as it is, for there is no length to divide by.
        let none = normalise(vec![vec![0.0f32, 0.0]]);
        assert_eq!(none[0], vec![0.0f32, 0.0]);
    }

    // Both of the forms the endpoint may answer with are read.
    #[test]
    fn the_answer_of_the_server_is_read_in_either_form() {
        let made = openai_words(
            r#"{"data":[{"embedding":[1.0,0.0]},{"embedding":[0.0,2.0]}]}"#,
            2,
        )
        .expect("the OpenAI form is read");
        assert_eq!(made.len(), 2);
        assert!((made[1][1] - 1.0).abs() < 1e-6, "the second vector is normalised");
        assert!(openai_words(r#"{"nonsense":1}"#, 2).is_none());
        // The plain form of llama-server is an array of arrays, and it is read by `embed_at`.
        let plain: Value = serde_json::from_str("[[1.0, 0.0]]").expect("a list of vectors");
        assert_eq!(numbers(&plain[0]).expect("one vector").len(), 2);
    }
}
