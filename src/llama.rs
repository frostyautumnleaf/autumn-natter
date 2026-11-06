// The llama-server process, and the OpenAI compatible calls to it.
//
// Autumn Natter never loads a model by itself. It starts llama-server as a
// child process and talks to it over HTTP on the loopback address only.

use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::config::ModelSettings;
use crate::gpu::Gpu;
use crate::util;

// KV cache types llama.cpp accepts for -ctk and -ctv.
pub const KV_TYPES: &[&str] = &[
    "f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1",
];

// Speculative decoding methods llama.cpp accepts for --spec-type.
pub const SPEC_TYPES: &[&str] = &[
    "none",
    "draft-simple",
    "draft-eagle3",
    "draft-mtp",
    "draft-dflash",
    "draft-dspark",
    "ngram-simple",
    "ngram-map-k",
    "ngram-map-k4v",
    "ngram-mod",
    "ngram-cache",
];

// Model load modes shown in the user interface. The value handed to llama.cpp
// is the last one, because directio is called dio there.
pub const LOAD_MODES: &[&str] = &["none", "mmap", "mlock", "mmap+mlock", "directio"];

fn load_mode_value(index: usize) -> &'static str {
    match index {
        0 => "none",
        2 => "mlock",
        3 => "mmap+mlock",
        4 => "dio",
        _ => "mmap",
    }
}

// A model file the user can pick.
#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub path: PathBuf,
    // Name without the folder and without the file ending.
    pub name: String,
    // GGUF or Safetensors.
    pub kind: &'static str,
}

impl ModelInfo {
    // The id of a model is its full path. It is stable across runs.
    pub fn id(&self) -> String {
        self.path.display().to_string()
    }
}

// Look for model files in a directory tree. Hidden folders are skipped. A model
// that was split into parts stands once in the list, and a model that stands in
// a folder of its own is named for that folder.
pub fn list_models(dir: &Path) -> Vec<ModelInfo> {
    let mut found: Vec<ModelInfo> = Vec::new();
    walk(dir, 0, 3, &mut found);
    let mut out = named_models(dir, found);
    out.sort_by_key(|m| m.name.to_lowercase());
    out
}

// The mark of a split at the end of the name of a model file:
// `-00001-of-00033`, or the same with underscores. It gives the name the model
// has without the mark, the number of this part, and the number of parts in
// all. A name with no mark gives nothing, so a model of eight billion parameters
// called `llama-3-8b` keeps its name.
fn shard_of(stem: &str) -> Option<(String, usize, usize)> {
    let (before, total) = stem.rsplit_once(['-', '_'])?;
    let total: usize = total.parse().ok()?;
    let (before_of, word) = before.rsplit_once(['-', '_'])?;
    if !word.eq_ignore_ascii_case("of") {
        return None;
    }
    let (base, number) = before_of.rsplit_once(['-', '_'])?;
    let number: usize = number.parse().ok()?;
    // A name that is nothing but the mark of a split names no model.
    if base.is_empty() || number == 0 {
        return None;
    }
    Some((base.to_string(), number, total))
}

// One part of a split, or a whole model, with the name it stands in the list
// under. The parts of one model are gathered under the folder they stand in and
// their name without the mark of the split; the first of them is kept, for the
// rest hang off it and are no choice of their own. A model that stands in a
// folder of its own inside the folder of models is named for that folder, which
// is the name the user gave it when the files were put there.
fn named_models(root: &Path, found: Vec<ModelInfo>) -> Vec<ModelInfo> {
    // One entry per model: the folder, the name without the mark of a split,
    // the number of the part kept, and the record the list is drawn from.
    let mut kept: Vec<(PathBuf, String, usize, ModelInfo)> = Vec::new();
    for model in found {
        let stem = model.name.clone();
        let (base, number) = match shard_of(&stem) {
            Some((base, number, _total)) => (base, number),
            None => (stem, 1usize),
        };
        let dir = model
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| root.to_path_buf());
        match kept
            .iter_mut()
            .find(|(d, b, _, _)| *d == dir && *b == base)
        {
            // The lowest number of the split is the part that opens the rest.
            Some(slot) if number < slot.2 => *slot = (dir, base, number, model),
            Some(_) => {}
            None => kept.push((dir, base, number, model)),
        }
    }
    // How many models stand in each folder, looked up before the folders are
    // given away one by one.
    let mut counts: Vec<(PathBuf, usize)> = Vec::new();
    for (dir, _, _, _) in kept.iter() {
        match counts.iter_mut().find(|(d, _)| d == dir) {
            Some((_, n)) => *n += 1,
            None => counts.push((dir.clone(), 1)),
        }
    }
    let count_at = |dir: &Path| -> usize {
        counts
            .iter()
            .find(|(d, _)| d == dir)
            .map(|(_, n)| *n)
            .unwrap_or(1)
    };
    let mut out: Vec<ModelInfo> = Vec::with_capacity(kept.len());
    for (dir, base, _number, mut model) in kept {
        if dir != root {
            let folder = dir
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            // The folder of one model names that model. A folder of more than
            // one cannot name them all alike, so the name of the file is kept
            // beside it.
            model.name = match (folder.is_empty(), count_at(&dir)) {
                (true, _) => base,
                (false, 1) => folder,
                (false, _) => format!("{folder} ({base})"),
            };
        } else {
            model.name = base;
        }
        out.push(model);
    }
    out
}

// The name a projector of a split model answers to: the name of the model
// without the mark of the split.
fn shard_base(stem: &str) -> String {
    match shard_of(stem) {
        Some((base, _number, _total)) => base,
        None => stem.to_string(),
    }
}

fn walk(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<ModelInfo>) {
    if depth > max_depth {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            walk(&path, depth + 1, max_depth, out);
            continue;
        }
        let lower = name.to_lowercase();
        let kind = if lower.ends_with(".gguf") {
            "GGUF"
        } else if lower.ends_with(".safetensors") {
            "Safetensors"
        } else {
            continue;
        };
        // A vision projector file is not a model on its own.
        if lower.contains("mmproj") {
            continue;
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or(name);
        out.push(ModelInfo {
            path,
            name: stem,
            kind,
        });
    }
}

// Find the vision projector that belongs to a model. It is looked for where the
// files of the model stand, which is a folder of its own when the model has one,
// and not in the folder of models the user pointed at.
pub fn find_mmproj(model: &Path) -> Option<PathBuf> {
    let dir = model.parent()?;
    let stem = model.file_stem()?.to_string_lossy().to_lowercase();
    // The mark of a split is no part of the name a projector answers to.
    let base = shard_base(&stem).to_lowercase();
    let entries = fs::read_dir(dir).ok()?;
    let mut fallback: Option<PathBuf> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.contains("mmproj") {
            continue;
        }
        if !name.ends_with(".gguf") {
            continue;
        }
        let pstem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| name.clone());
        // A projector that was split too is read from its first part.
        if let Some((_pbase, number, _total)) = shard_of(&pstem) {
            if number != 1 {
                continue;
            }
        }
        // Prefer the projector named after the model, with the mark of a split
        // on either side taken away.
        let pbase = shard_base(&pstem);
        if pbase.starts_with(&base) || base.starts_with(&pbase) || name.starts_with(&stem) {
            return Some(path);
        }
        if fallback.is_none() {
            fallback = Some(path);
        }
    }
    fallback
}

// The running llama-server process.
pub struct Server {
    child: Child,
    // Address of the health and chat endpoints of this run.
    pub url: String,
    // Tokens the current chat can hold.
    pub ctx_total: u64,
    pub model_id: String,
    // Notes for the user, for example a missing vision projector.
    pub notes: Vec<String>,
}

impl Server {
    // Stop the child process. The call is safe more than once.
    pub fn stop(&mut self) {
        util::kill_pid(self.child.id());
        let _ = self.child.wait();
    }

    // True while the process is still running.
    pub fn alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) => false,
            // The handle is gone, so treat the server as dead.
            Err(_) => false,
        }
    }


    // Start llama-server and wait until it answers.
    //
    // The thinking mode, the choice of keeping the thinking, the tools of the
    // chat and the jail of the sandbox stand on none of the flags of the
    // server: they ride on every ask instead (see `thinking_kwargs`), so a
    // change of any of them leaves this process, and the model it loaded, be.
    // What a server is started with is the model and the settings of the Model
    // tab, and nothing else.
    //
    // `keep_talks` names the folder the server writes the talks it has worked
    // through into, and reads them back from -- see `slot_save_flags`. It is
    // none when the user has switched the keeping off, and nothing of a talk
    // is then kept anywhere.
    pub fn start(
        binary: &Path,
        model: &ModelInfo,
        settings: &ModelSettings,
        gpus: &[Gpu],
        keep_talks: Option<&Path>,
    ) -> Result<Server, String> {
        let port = util::free_port().ok_or("No free port on this machine")?;
        let mut notes = Vec::new();
        // The folder has to stand before it is named to the server, for the
        // server throws out an argument that names no folder of the disk, and
        // the keeping of every talk would go missing with it. A folder that
        // cannot be made is told to the user; the server starts none the
        // less, only without the keeping.
        let keeping = match keep_talks {
            Some(dir) => match fs::create_dir_all(dir) {
                Ok(()) => slot_save_flags(dir),
                Err(problem) => {
                    notes.push(format!(
                        "The talks the model has read through cannot be kept: the folder of the program could not be made ({})",
                        problem
                    ));
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        // Every start asks for the chat template of the model. Three things of
        // a chat are read out of that template and nothing else: the tools the
        // chat offers, the thinking mode, and the keeping of the thinking. A
        // model whose template will not stand is started again without it, and
        // such a server reads no tool call and no thinking flag.
        let mut template = true;
        let mut args = build_args(
            binary,
            model,
            settings,
            gpus,
            port,
            template,
            &mut notes,
        );
        args.extend(keeping.iter().cloned());

        // One log for this start of the server, under the logs of the program.
        // The Logs tab of the sheet of settings reads it, and the ten newest of
        // them, so a start that went wrong can still be read after the next one.
        let log_place = crate::logs::start(crate::logs::Kind::Server);
        let mut attempt = 0;
        loop {
            attempt += 1;
            let log = open_log(log_place.as_deref());
            let mut command = Command::new(binary);
            command.args(&args).stdin(Stdio::null());
            if graphs_of_a_spread_model(gpus) {
                command.env("GGML_CUDA_DISABLE_GRAPHS", "1");
            }
            match log {
                Some(file) => {
                    // Both output pipes go to the same log file.
                    if let Ok(copy) = file.try_clone() {
                        command.stderr(Stdio::from(copy));
                    }
                    command.stdout(Stdio::from(file));
                }
                None => {
                    command.stdout(Stdio::null());
                    command.stderr(Stdio::null());
                }
            }
            let child = match command.spawn() {
                Ok(child) => child,
                Err(e) => return Err(format!("Cannot start {}: {}", binary.display(), e)),
            };
            let mut server = Server {
                child,
                url: format!("http://127.0.0.1:{}", port),
                ctx_total: settings.ctx_size as u64,
                model_id: model.id(),
                notes: notes.clone(),
            };
            match server.wait_ready(log_place.as_deref()) {
                Ok(props) => {
                    if let Some(n) = props
                        .get("default_generation_settings")
                        .and_then(|d| d.get("n_ctx"))
                        .and_then(|v| v.as_u64())
                    {
                        if n > 0 {
                            server.ctx_total = n;
                        }
                    }
                    return Ok(server);
                }
                Err(problem) => {
                    // A model whose chat template will not stand cannot carry
                    // the tools of a chat, a thinking mode, or the keeping of
                    // the thinking. One retry without the template tells the
                    // user less and still works.
                    if attempt == 1 && template && problem.exited {
                        notes.push(
                            "This model has no usable chat template, so no tool and no thinking mode is offered"
                                .to_string(),
                        );
                        template = false;
                        args = build_args(
                            binary,
                            model,
                            settings,
                            gpus,
                            port,
                            template,
                            &mut notes,
                        );
                        // The retry is a whole new command line, so the folder
                        // of the kept talks is named to it again.
                        args.extend(keeping.iter().cloned());
                        server.stop_now();
                        continue;
                    }
                    server.stop_now();
                    let mut text = problem.text;
                    if !problem.output.is_empty() {
                        text.push('\n');
                        text.push_str(&problem.output);
                    }
                    return Err(text);
                }
            }
        }
    }

    fn stop_now(&mut self) {
        util::kill_pid(self.child.id());
        let _ = self.child.wait();
    }

    // Wait for the health endpoint. The load of a big model can take minutes.
    // The log of this start is read back when the wait fails, so the words the
    // server gave are with the words of the fault.
    fn wait_ready(&mut self, log_place: Option<&Path>) -> Result<Value, Fail> {
        let deadline = Instant::now() + Duration::from_secs(900);
        // The status of the last health call, for the error text. Every round
        // of the loop sets it before the text is made.
        let mut last_status: Option<u16>;
        loop {
            // A child that left early never becomes ready.
            if let Ok(Some(_)) = self.child.try_wait() {
                return Err(Fail {
                    text: "llama-server stopped before it was ready".to_string(),
                    output: read_tail(log_place, 1200),
                    exited: true,
                });
            }
            match get_json(&format!("{}/health", self.url)) {
                Ok(_) => {
                    // The model is loaded. Read the global properties.
                    let props = get_json(&format!("{}/props", self.url)).unwrap_or(json!({}));
                    return Ok(props);
                }
                Err(status) => {
                    last_status = Some(status);
                }
            }
            if Instant::now() > deadline {
                return Err(Fail {
                    text: format!(
                        "llama-server did not become ready in time (last status {})",
                        last_status.unwrap_or(0)
                    ),
                    output: read_tail(log_place, 1200),
                    exited: false,
                });
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}

// Why the start failed.
struct Fail {
    text: String,
    output: String,
    exited: bool,
}

// Open the log file of this start. The file was made when the start was asked
// for, and it is written from the front again when the server is started a
// second time in the same run of the program.
fn open_log(place: Option<&Path>) -> Option<fs::File> {
    let place = place?;
    fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(place)
        .ok()
}

// Read the end of a text file, for an error message. No place at all, and no
// file at all, give back nothing.
fn read_tail(path: Option<&Path>, max_bytes: usize) -> String {
    let text = match path {
        Some(place) => fs::read_to_string(place).unwrap_or_default(),
        None => String::new(),
    };
    if text.len() <= max_bytes {
        return text;
    }
    let mut idx = text.len() - max_bytes;
    // Keep whole characters only.
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    text[idx..].to_string()
}

// A model spread over more than one card has its graphs made on each of them,
// and a graph of one card does not stand with the memory of another: such a
// server is told to make no graphs at all, which costs a little speed and
// saves the start.
pub fn graphs_of_a_spread_model(gpus: &[Gpu]) -> bool {
    gpus.iter().filter(|g| g.enabled && g.is_offload).count() > 1
}

// Build the command line for llama-server. The server takes many knobs,
// so the call is a flat list.
//
// `template` says whether the chat template of the model is asked for with
// `--jinja`. It is asked for on every start, for the three things a chat may
// change between one answer and the next are all read out of that template and
// are sent with the ask instead of with the command line: the tools of the
// chat, the thinking mode, and the keeping of the thinking of the earlier
// turns. `thinking_kwargs` is the other half of that.
pub fn build_args(
    _binary: &Path,
    model: &ModelInfo,
    settings: &ModelSettings,
    gpus: &[Gpu],
    port: u16,
    template: bool,
    notes: &mut Vec<String>,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--model".into(),
        model.path.display().to_string(),
        "--alias".into(),
        model.name.clone(),
        // The server must not answer on the network.
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--no-webui".into(),
        "--props".into(),
        "--slots".into(),
        // The log of the server is read in the Logs tab of the sheet of
        // settings, so it holds everything the server has to say: every level of
        // the log, the time before every line, and no colours, for a file holds
        // no colours. These three stand here and not in the settings of one
        // model, for a log that hides nothing is what a fault is read out of.
        "-v".into(),
        "--log-timestamps".into(),
        "--log-colors".into(),
        "off".into(),
        "-c".into(),
        settings.ctx_size.to_string(),
        "-ctk".into(),
        KV_TYPES[clamp(settings.kv_index, KV_TYPES.len())].to_string(),
        "-ctv".into(),
        KV_TYPES[clamp(settings.kv_index, KV_TYPES.len())].to_string(),
        "-np".into(),
        settings.parallel.max(1).to_string(),
        "--batch-size".into(),
        settings.batch_size.max(1).to_string(),
        "--ubatch-size".into(),
        settings.ubatch_size.max(1).to_string(),
        "-lm".into(),
        load_mode_value(clamp(settings.load_index, LOAD_MODES.len())).to_string(),
        // Fast attention stands on for every start, and has no setting of its
        // own.
        "-fa".into(),
        "on".into(),
    ];

    // Devices. llama.cpp has no tensor parallel switch, so the device list
    // does that work: every enabled device, or only the first one.
    let offload: Vec<&Gpu> = gpus.iter().filter(|g| g.enabled && g.is_offload).collect();
    let any_offload = gpus.iter().any(|g| g.is_offload);
    if any_offload {
        if offload.is_empty() {
            args.push("--device".into());
            args.push("none".into());
            args.push("-ngl".into());
            args.push("0".into());
        } else {
            let names: Vec<String> = if settings.tensor_parallel {
                offload.iter().map(|g| g.device.clone()).collect()
            } else {
                vec![offload[0].device.clone()]
            };
            args.push("--device".into());
            args.push(names.join(","));
            // The whole of the layers go to the cards, which is what putting a
            // model on a card means. llama.cpp reads any negative number as the
            // whole of the layers, and -1 is its own way of saying it; a great
            // number of layers said the same thing in the end, but read as a
            // count the model was over, and left the server nothing to say about
            // the layers of a model that would not fit.
            args.push("-ngl".into());
            args.push("-1".into());
        }
    }

    // A model spread over more than one card has its graphs made on each card,
    // and a graph of one card does not stand with the memory of another, which
    // the server answers by failing to make the graph at all. Such a server is
    // told to make no graphs, which costs a little speed and saves the start.
    // There is no word for this on the command line, so it is carried in the
    // touch of the server instead. See `graphs_of_a_spread_model`.

    // The fit of the model into the cards is asked for whenever the user holds
    // any of a card back, and needs no setting of its own. The server is
    // told the room of the talk the chat was given, and how much of every
    // card is to stand over: the share the user set
    // is what the model may work in, and what is left of a card is the margin.
    // A margin never falls below the least a server needs to stand up in, which
    // is what a slider at the whole of the cards comes to -- for the whole of
    // the cards would else ask for a margin of nothing at all.
    // The least a server may be left standing over: below this there is no
    // room for the talk, and a model loaded with such a margin would have
    // nowhere to put it.
    const LEAST_FIT_TARGET: u64 = 256;
    let share = crate::config::sanitize_vram_budget(settings.vram_budget);
    let total: u64 = gpus
        .iter()
        .filter(|g| g.is_offload)
        .map(|g| g.memory_mb)
        .sum();
    if total > 0 && share < 100.0 {
        args.push("--fit".into());
        args.push("on".into());
        args.push("--fit-ctx".into());
        args.push(settings.ctx_size.to_string());
        let margins: Vec<String> = gpus
            .iter()
            .filter(|g| g.is_offload)
            .map(|g| {
                let left = g.memory_mb as f64 * (100.0 - share) / 100.0;
                let margin = (left.round() as u64).max(LEAST_FIT_TARGET);
                margin.min(g.memory_mb).to_string()
            })
            .collect();
        args.push("--fit-target".into());
        args.push(margins.join(","));
    }

    // Sampling of one answer. The values are sent on every start, so the
    // defaults of this program stand over the defaults of the server.
    args.push("--temp".into());
    args.push(settings.temperature.to_string());
    args.push("--top-p".into());
    args.push(settings.top_p.to_string());
    args.push("--top-k".into());
    args.push(settings.top_k.to_string());
    args.push("--min-p".into());
    args.push(settings.min_p.to_string());
    args.push("--repeat-penalty".into());
    args.push(settings.repeat_penalty.to_string());
    args.push("--presence-penalty".into());
    args.push(settings.presence_penalty.to_string());
    // -1 leaves the answer without a limit of tokens.
    args.push("--n-predict".into());
    args.push(settings.max_tokens.to_string());
    // -1 makes the server draw a new seed on every start.
    args.push("--seed".into());
    args.push(settings.seed.to_string());

    // The room for context checkpoints, and the ceiling of the talk memory.
    // 0 turns the checkpoints off, and for the ceiling 0 turns the cache off
    // while -1 lifts the limit.
    args.push("--ctx-checkpoints".into());
    args.push(settings.ctx_checkpoints.to_string());
    args.push("--cache-ram".into());
    args.push(settings.cache_ram.to_string());

    // Prompt caching. The server keeps the work of the prompt and reuses it
    // on the next turn. The flag is sent in one form or the other, so the
    // switch of the window always reaches the server.
    if settings.cache_prompt {
        args.push("--cache-prompt".into());
    } else {
        args.push("--no-cache-prompt".into());
    }

    // A talk that overflows the room. By default the server shifts the old
    // words out and keeps going. With the switch on, the shift stays away,
    // and the answer stops at the edge of the room.
    if settings.no_context_shift {
        args.push("--no-context-shift".into());
    }

    // Vision projector.
    if settings.vision {
        match find_mmproj(&model.path) {
            Some(path) => {
                args.push("--mmproj".into());
                args.push(path.display().to_string());
            }
            None => notes.push(format!("No vision projector file next to {}", model.name)),
        }
        // The room one image may take. 0 keeps the value inside the model
        // file, so the flag stays away.
        if settings.image_min_tokens > 0 {
            args.push("--image-min-tokens".into());
            args.push(settings.image_min_tokens.to_string());
        }
        if settings.image_max_tokens > 0 {
            args.push("--image-max-tokens".into());
            args.push(settings.image_max_tokens.to_string());
        }
        // The projector works on the card, by default. With the switch on, it
        // works on the CPU, and the card stays free for the model.
        if settings.mmproj_cpu {
            args.push("--no-mmproj-offload".into());
        }
    }

    // Reasoning budget.
    if settings.reasoning_budget != -1 {
        args.push("--reasoning-budget".into());
        args.push(settings.reasoning_budget.to_string());
    }
    if !settings.reasoning_message.trim().is_empty() {
        args.push("--reasoning-budget-message".into());
        args.push(settings.reasoning_message.clone());
    }

    // Thinking stands on no flag of the server. The mode of the chat in hand,
    // and the choice of keeping the thinking of the earlier turns, are words
    // for the chat template and are sent with every ask -- see
    // `thinking_kwargs` -- so a badge moved between two answers neither changes
    // this command line nor sends the model away with it. The reasoning budget
    // of the Model tab is a setting of the server and stays here.

    // Speculative decoding.
    let spec = SPEC_TYPES[clamp(settings.spec_index, SPEC_TYPES.len())];
    if spec != "none" {
        args.push("--spec-type".into());
        args.push(spec.to_string());
        args.push("--spec-draft-n-max".into());
        args.push(settings.draft_tokens.max(1).to_string());
        if !settings.draft_model.trim().is_empty() {
            args.push("--spec-draft-model".into());
            args.push(settings.draft_model.clone());
        }
    }

    // The chat template of the model, asked for with `--jinja`. The tools of a
    // chat, the thinking mode and the keeping of the thinking are all read out
    // of that template, so the flag is asked for on every start and no longer
    // stands over by itself for each of the two asks that used to push it.
    if template {
        args.push("--jinja".into());
    }

    // A custom chat template replaces the one embedded in the model.
    let custom = settings.chat_template.trim().to_string();
    if !custom.is_empty() {
        args.push("--chat-template".into());
        args.push(custom);
    }

    // Free form arguments of the user come last, so they win.
    for part in util::split_args(&settings.extra_args) {
        args.push(part);
    }
    args
}

// What one ask tells the chat template about thinking, as the json of
// `chat_template_kwargs`. Nothing of the thinking stands on the command line
// of the server, so a badge moved between two answers costs no load of a
// model: the word for it goes out with the ask instead.
//
// The server reads these words over the ones it was started with, so a keyword
// named here holds for this ask and for this ask only. A mode this program
// does not know -- the word `auto` of a chat kept from an older version, for
// example -- sends nothing, which leaves the model to its own ways.
//
// `enable_thinking` and `reasoning_effort` are the two words llama.cpp has for
// a long time. `preserve_reasoning` is the word of a newer one: the server
// takes it from the same place as the flag `--reasoning-preserve` of the
// command line, and puts the four words behind it the same way. Those four are
// named here as well, for a server old enough to know only the words of the
// template itself then reads the same choice, and a template that names none
// of them takes no heed of any.
pub fn thinking_kwargs(mode: &str, preserve: Option<bool>) -> Value {
    let mut made: serde_json::Map<String, Value> = serde_json::Map::new();
    match mode {
        "off" => {
            made.insert(String::from("enable_thinking"), json!(false));
        }
        "on" => {
            made.insert(String::from("enable_thinking"), json!(true));
        }
        "low" | "medium" | "high" | "xhigh" => {
            made.insert(String::from("enable_thinking"), json!(true));
            made.insert(String::from("reasoning_effort"), json!(mode));
        }
        _ => {}
    }
    if let Some(kept) = preserve {
        made.insert(String::from("preserve_reasoning"), json!(kept));
        made.insert(String::from("preserve_thinking"), json!(kept));
        made.insert(String::from("clear_thinking"), json!(!kept));
        made.insert(String::from("truncate_history_thinking"), json!(!kept));
        made.insert(String::from("drop_thinking"), json!(!kept));
    }
    if made.is_empty() {
        return Value::Null;
    }
    Value::Object(made)
}

// The arguments that hand the server the folder it writes the talks it has
// worked through into, and reads them back from. The folder must stand
// already: the server throws out an argument that names no folder of the disk,
// which is why `Server::start` makes the folder before the arguments are put
// together.
//
// A server started without these arguments knows nothing of the keeping. It
// answers a call of `slot_state` with words saying that it does not support
// the action, and the caller takes that as a talk that was not kept.
pub fn slot_save_flags(dir: &Path) -> Vec<String> {
    vec![
        "--slot-save-path".into(),
        dir.display().to_string(),
    ]
}

fn clamp(index: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    index.min(len - 1)
}

// Tell the server what to do with the talk that one of its slots has worked
// its way through. `action` is "save", which copies the worked-through state
// of the slot into a file of the folder from `--slot-save-path`, or "restore",
// which lays the state from such a file in the slot again, so that the next
// ask of that slot is read on from where the talk left off instead of from its
// first word.
//
// Nothing here shortens or lays out that file: the talk is gigabytes, and it
// stands in its short shape except for the moment the server is writing it or
// reading it. `write_talk` and `read_talk` in the engine are the two places
// that ask for the file, and they do the shortening round this call.
//
// The name of the file goes in the body of the call, and the server looks for
// it in that folder and nowhere else: a name with a folder mark in it, or with
// a point of the disk, is thrown out by the server. `context_name` in the
// engine makes names that always pass.
//
// The number given back is the bigness of the file in bytes. Both calls answer
// with a fault when there is nothing to be done: a slot with no talk worked
// through in it, a file that is not there, or a file that does not fit the
// room of the slot. None of those is a hurt -- the talk is then simply read
// through again, which is what would have happened without any keeping -- so
// the engine takes a fault quietly and says nothing of it.
pub fn slot_state(
    base_url: &str,
    action: &str,
    slot: usize,
    filename: &str,
) -> Result<u64, String> {
    let url = format!("{}/slots/{}?action={}", base_url, slot, action);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        // A whole talk is copied to, or off, the disk. A long talk on a slow
        // disk takes longer than the small calls of the endpoints, so the wait
        // here is a long one. Nothing else is asked of the server meanwhile.
        .timeout_read(Duration::from_secs(600))
        .build();
    let body = json!({ "filename": filename });
    match agent.post(&url).send_json(&body) {
        Ok(response) => {
            let mut text = String::new();
            let _ = response.into_reader().read_to_string(&mut text);
            let told = serde_json::from_str::<Value>(&text).map_err(|_| {
                format!(
                    "The server answered the {} of a talk with words that could not be read",
                    action
                )
            })?;
            Ok(told
                .get("n_bytes")
                .and_then(Value::as_u64)
                .unwrap_or(0))
        }
        Err(ureq::Error::Status(code, response)) => {
            let mut text = String::new();
            let _ = response.into_reader().read_to_string(&mut text);
            Err(server_message(code, &text))
        }
        Err(other) => Err(format!("Cannot reach the model: {}", other)),
    }
}

// Ask one endpoint on the loopback address. The error carries the status code.
pub fn get_json(url: &str) -> Result<Value, u16> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(10))
        .build();
    match agent.get(url).call() {
        Ok(response) => {
            let mut text = String::new();
            let _ = response.into_reader().read_to_string(&mut text);
            serde_json::from_str(&text).map_err(|_| 0)
        }
        Err(ureq::Error::Status(code, _)) => Err(code),
        Err(_) => Err(0),
    }
}

// One tool call the model asked for.
#[derive(Clone, Debug, Default)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

// How far the server is with the words it was sent. The server sends these
// numbers while it works the prompt through, before it has a word to give
// back. `total` counts every token of the prompt, and `cache` the tokens the
// server kept from an earlier turn and does not work through again.
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub total: u64,
    pub cache: u64,
    pub processed: u64,
    pub time_ms: u64,
}

impl Progress {
    // How fast the words that were sent are being worked through, in tokens of
    // the talk to the second. The server gives the tokens it has done with and
    // the clock it took to do them, so the speed is read from those two. The
    // tokens kept from before are left out, for they cost no work. A piece that
    // names no time gives nought, and the window counts the clocks itself.
    pub fn per_second(&self) -> f64 {
        if self.time_ms == 0 {
            return 0.0;
        }
        let cache = self.cache.min(self.processed);
        let done = self.processed.saturating_sub(cache);
        if done == 0 {
            return 0.0;
        }
        done as f64 / (self.time_ms as f64 / 1000.0)
    }

    // The part of the work that is done, between 0.0 and 1.0. The tokens the
    // server kept from before are left out of both sides, for they cost
    // nothing and would make the work look finished before it began.
    pub fn part(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let cache = self.cache.min(self.total);
        let room = self.total - cache;
        if room == 0 {
            // Every token was kept from before, so there is nothing to do.
            return 1.0;
        }
        let done = self.processed.min(self.total).saturating_sub(cache);
        (done as f64 / room as f64).clamp(0.0, 1.0)
    }
}

// How full the room of the talk is so far, as the server says it. The server
// is asked to give these numbers on every piece of the answer, and not only at
// the end of it, so the meter of the context can move while the model writes.
// `prompt` counts the words of the talk the server has worked through, `cache`
// the words it kept from an earlier turn, and `written` the words the model has
// given back.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    pub prompt: u64,
    pub cache: u64,
    pub written: u64,
    // The two speeds the server measures, given on the same piece of the answer
    // as the counts above. `prompt_per_second` is how fast the words that were
    // sent are worked through, and `decode_per_second` how fast the model is
    // writing. A server that says nothing of speed leaves both at nought, and the
    // window then counts the clocks itself.
    pub prompt_per_second: f64,
    pub decode_per_second: f64,
}

impl Context {
    // The words the talk takes up: every word that was sent, whether the server
    // worked it through anew or kept it from before, and every word the model
    // has written. The two sides of the words sent do not stand together, so
    // all three numbers are wanted.
    pub fn used(&self) -> u64 {
        self.prompt + self.cache + self.written
    }

    // The words the server was sent, and none of the words it has written back: the
    // words it worked through and the words it kept from an earlier turn. This is the
    // room the talk took when the ask went out. The words sent stand in the talk
    // whichever way the ask ends, and it is the one number of it the server speaks
    // that is still true of the talk when an answer was stopped before its end.
    pub fn sent(&self) -> u64 {
        self.prompt + self.cache
    }
}

// What one streamed answer gave.
#[derive(Clone, Debug, Default)]
pub struct Answer {
    pub text: String,
    // The model's reasoning, streamed separately from the answer.
    pub reasoning: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    // The last word the server said about the prompt it was working through.
    pub progress: Progress,
    // The last word the server said about the room of the talk. A piece of no words
    // says nothing of the talk and is passed over in silence, as `on_context` is, so
    // this stands at nought for an ask that never said anything of the room.
    // `Answer::sent`, read out of it, is the number the meter of the context keeps
    // when an ask speaks no number of the talk at its end.
    pub context: Context,
    // The user stopped the answer.
    pub stopped: bool,
    // The program stopped the answer itself, because the talk came to the room it
    // was given. The words of the answer, and of a tool call that was being
    // written, are kept up to the moment of the cut: the model is still holding
    // its pen in the middle of a word, and what it had written so far is all the
    // program has of the thought.
    pub interrupted: bool,
    // A transport problem, or the message from the server.
    pub error: Option<String>,
}

impl Answer {
    // The room of the words the server was sent on this ask, as the server spoke it.
    // It speaks the same room in two places: on every piece of the answer, in the
    // timings of the talk, and on the progress of the words being worked through, as
    // the whole of the prompt. Either can be all there is, for the progress of the
    // prompt is spoken before the first word of the talk is counted, and the timings
    // go on saying the room while the model writes and stop with the answer. The
    // larger of the two is the room, since the two count the selfsame words.
    //
    // This is what the meter of the context stands on when an ask speaks no number
    // at its end: see `meter_after` in the engine.
    pub fn sent(&self) -> u64 {
        self.context.sent().max(self.progress.total)
    }
}

// Send one chat request and stream the answer. on_text gets every piece of new
// text as it arrives, so the window can show it. on_progress gets the state of
// the prompt work, as the server reports it while the prompt goes through.
// on_context gets how full the room of the talk is, as the server reports it on
// every piece of the answer. on_tool gets the calls of the model as they are
// written, so the user can see the words of a command as the model puts them
// down. The stream has a hand for everything the window shows at once, so the
// call is a flat list of them.
//
// Two hands stop the stream. `cancel` is the user's, and `interrupt` is the
// program's own: it is put up when the talk has come to the room the program
// meant to keep, and the stream is cut off at the next piece of the answer. The
// one cannot wait for the other, for a model writing a tool call bigger than the
// room it has writes until the room is gone, and the room being gone is what the
// program was trying to avoid. Nothing of what came before the cut is lost: the
// words of the answer and the words of the unfinished call stand in the answer
// that is given back, with `interrupted` said of them.
#[allow(clippy::too_many_arguments)]
pub fn stream_chat(
    base_url: &str,
    body: &Value,
    cancel: &AtomicBool,
    interrupt: &AtomicBool,
    on_text: &mut dyn FnMut(&str),
    on_reasoning: &mut dyn FnMut(&str),
    on_progress: &mut dyn FnMut(Progress),
    on_context: &mut dyn FnMut(Context),
    on_tool: &mut dyn FnMut(&[ToolCall]),
) -> Answer {
    let mut answer = Answer::default();
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        // A long prompt can keep the server busy for many minutes.
        .timeout_read(Duration::from_secs(1800))
        .build();
    let url = format!("{}/v1/chat/completions", base_url);
    let response = match agent.post(&url).send_json(body) {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let mut text = String::new();
            let _ = response.into_reader().read_to_string(&mut text);
            answer.error = Some(server_message(code, &text));
            return answer;
        }
        Err(other) => {
            answer.error = Some(format!("Cannot reach the model: {}", other));
            return answer;
        }
    };

    let mut stream = response.into_reader();
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    // The pieces of one read are worked through one after another, and the room of
    // the talk may come to an end under any one of them. So the program's hand is
    // looked at after every piece, and not only after every read: a whole tool
    // call can come in one read, and the call has to be cut off at the piece the
    // room gave out under, which is the piece the talk was spoken of in.
    'answer: loop {
        if cancel.load(Ordering::Relaxed) {
            answer.stopped = true;
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                pending.extend_from_slice(&chunk[..n]);
                while let Some(position) = pending.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = pending.drain(..=position).collect();
                    let text = String::from_utf8_lossy(&line);
                    let text = text.trim_end_matches(['\r', '\n']);
                    if let Some(payload) = text.strip_prefix("data:") {
                        if !apply_event(
                            payload.trim(),
                            &mut answer,
                            on_text,
                            on_reasoning,
                            on_progress,
                            on_context,
                            on_tool,
                        ) {
                            // The stream told us it is finished.
                            pending.clear();
                            break;
                        }
                        // The room gave out under this piece. The words of it are
                        // already in the answer, and the rest of the stream goes
                        // with the connection. A piece that brought the word the
                        // model ends an answer with is not cut at all: the answer
                        // came to its end of itself, and the last thing a stream
                        // speaks is often the number that moves the meter, which
                        // would else be read as a call broken in the middle.
                        if answer.finish.is_empty() && interrupt.load(Ordering::Relaxed) {
                            answer.interrupted = true;
                            break 'answer;
                        }
                    }
                }
            }
            Err(e) => {
                if !answer.stopped {
                    answer.error = Some(format!("The stream stopped: {}", e));
                }
                break;
            }
        }
    }
    answer
}

// The words of a thing the server said. Words stand as one string, or as a list
// of parts, of which the words of a part ride under `text` and a picture of one
// under `image_url`: a list of pictures alone says nothing at all, and is said
// so. Nothing is given when the thing is neither of those shapes, so a silence
// is told from a saying by the caller.
fn said_words(said: &Value) -> Option<String> {
    match said {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let mut made = String::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    made.push_str(text);
                }
            }
            Some(made)
        }
        _ => None,
    }
}

// Read one SSE payload. False means the stream is over.
fn apply_event(
    payload: &str,
    answer: &mut Answer,
    on_text: &mut dyn FnMut(&str),
    on_reasoning: &mut dyn FnMut(&str),
    on_progress: &mut dyn FnMut(Progress),
    on_context: &mut dyn FnMut(Context),
    on_tool: &mut dyn FnMut(&[ToolCall]),
) -> bool {
    if payload == "[DONE]" {
        return false;
    }
    if payload.is_empty() {
        return true;
    }
    let value: Value = match serde_json::from_str(payload) {
        Ok(value) => value,
        Err(_) => return true,
    };
    if let Some(usage) = value.get("usage") {
        answer.prompt_tokens = usage
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(answer.prompt_tokens);
        answer.completion_tokens = usage
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(answer.completion_tokens);
    }
    // The server is asked for the state of the prompt work (return_progress).
    // Every number of it is given again in one go, so the last one seen is the
    // one that stands.
    if let Some(made) = value.get("prompt_progress") {
        let number = |name: &str| made.get(name).and_then(|v| v.as_u64()).unwrap_or(0);
        answer.progress = Progress {
            total: number("total"),
            cache: number("cache"),
            processed: number("processed"),
            time_ms: number("time_ms"),
        };
        on_progress(answer.progress);
    }
    // The room of the talk, as the server says it on this piece. The server is
    // asked for the numbers of every piece (timings_per_token), so the meter of
    // the context moves during the answer. A piece of no words at all says
    // nothing of the talk, so it is passed over in silence.
    if let Some(made) = value.get("timings") {
        let number = |name: &str| made.get(name).and_then(|v| v.as_u64()).unwrap_or(0);
        let speed = |name: &str| made.get(name).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let told = Context {
            prompt: number("prompt_n"),
            cache: number("cache_n"),
            written: number("predicted_n"),
            prompt_per_second: speed("prompt_per_second"),
            decode_per_second: speed("predicted_per_second"),
        };
        if told.used() > 0 {
            // Every number of it is given again in one go, as it is for the progress
            // of the prompt above, so the last piece that said anything of the talk
            // is the piece that stands when the ask is over.
            answer.context = told;
            on_context(told);
        }
    }
    let Some(choice) = value.get("choices").and_then(|c| c.get(0)) else {
        return true;
    };
    if let Some(finish) = choice.get("finish_reason").and_then(|f| f.as_str()) {
        answer.finish = finish.to_string();
    }
    let delta = choice.get("delta").unwrap_or(&Value::Null);
    // The words of an answer come as one string, or as a list of parts of which
    // the words and the pictures are mixed: a server of the OpenAI shape may
    // send either, and the parts of a picture carry no words to read. Both are
    // read, for a program that reads only the string sees an answer of parts as
    // a turn with nothing in it and asks the model again over its own answer.
    if let Some(content) = said_words(delta.get("content").unwrap_or(&Value::Null)) {
        if !content.is_empty() {
            answer.text.push_str(&content);
            on_text(&content);
        }
    }
    // Reasoning models stream their thinking in a field of its own. The name of
    // that field is not settled between the servers: llama.cpp gives back
    // `reasoning_content`, and a server of the same shape gives the same words
    // under `reasoning`. Both are read, and the first of them that holds words
    // is the thinking of this piece, for a program that sees neither reads the
    // turn as an empty one and goes on asking the model over words it had
    // already written.
    if let Some(reasoning) = delta
        .get("reasoning_content")
        .and_then(said_words)
        .filter(|said| !said.is_empty())
        .or_else(|| delta.get("reasoning").and_then(|v| v.as_str()).map(String::from))
    {
        if !reasoning.is_empty() {
            answer.reasoning.push_str(&reasoning);
            on_reasoning(&reasoning);
        }
    }
    // Tool calls arrive in pieces. They are joined by their index.
    if let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) {
        // The names and the words of a call are put again below, so the whole
        // of what is written so far is given back, and not only the piece.
        let wrote = !calls.is_empty();
        for item in calls {
            let index = item
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(answer.tool_calls.len() as u64) as usize;
            while answer.tool_calls.len() <= index {
                answer.tool_calls.push(ToolCall::default());
            }
            let target = &mut answer.tool_calls[index];
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                if !id.is_empty() {
                    target.id.push_str(id);
                }
            }
            if let Some(name) = item
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
            {
                target.name.push_str(name);
            }
            if let Some(args) = item
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str())
            {
                target.arguments.push_str(args);
            }
        }
        if wrote {
            on_tool(&answer.tool_calls);
        }
    }
    true
}

// Turn the error body of the server into one line of text.
fn server_message(code: u16, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            let error = value.get("error")?;
            let text = match error {
                Value::String(text) => text.clone(),
                other => other.get("message")?.as_str()?.to_string(),
            };
            Some(text)
        })
        .unwrap_or_else(|| body.lines().next().unwrap_or("").to_string());
    if detail.is_empty() {
        return format!("The model server answered with status {}", code);
    }
    format!("{} (status {})", detail, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editfile;
    use std::path::PathBuf;

    // One model file, and no devices, for the argument tests.
    fn model() -> ModelInfo {
        ModelInfo {
            path: PathBuf::from("/models/one.gguf"),
            name: String::from("one"),
            kind: "GGUF",
        }
    }

    // The folder of the kept talks goes to the server as one argument, and the
    // server is told the folder and nothing else: it makes the names of the
    // files inside it by itself.
    #[test]
    fn the_folder_of_the_kept_talks_goes_to_the_server() {
        let flags = slot_save_flags(Path::new("/data/of/the/program/contexts"));
        assert_eq!(
            flags,
            vec![
                String::from("--slot-save-path"),
                String::from("/data/of/the/program/contexts")
            ]
        );
    }

    // The Sampling section sends its values to the server on every start, and
    // the defaults of the program stand in a new set of settings.
    #[test]
    fn the_sampling_values_go_to_the_server() {
        let settings = ModelSettings {
            temperature: 0.2,
            top_p: 0.5,
            top_k: 7,
            min_p: 0.1,
            repeat_penalty: 1.1,
            presence_penalty: 0.2,
            max_tokens: 256,
            seed: 42,
            ..Default::default()
        };
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            true,
            &mut notes,
        );
        let find = |flag: &str| -> String {
            let pos = args.iter().position(|a| a == flag).expect("the flag is there");
            args[pos + 1].clone()
        };
        assert_eq!(find("--temp"), "0.2");
        assert_eq!(find("--top-p"), "0.5");
        assert_eq!(find("--top-k"), "7");
        assert_eq!(find("--min-p"), "0.1");
        assert_eq!(find("--repeat-penalty"), "1.1");
        assert_eq!(find("--presence-penalty"), "0.2");
        assert_eq!(find("--n-predict"), "256");
        assert_eq!(find("--seed"), "42");

        // The defaults of a new set of settings.
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[],
            8080,
            true,
            &mut notes,
        );
        assert_eq!(find_flag(&args, "--temp"), "0.7");
        assert_eq!(find_flag(&args, "--top-p"), "0.95");
        assert_eq!(find_flag(&args, "--top-k"), "20");
        assert_eq!(find_flag(&args, "--min-p"), "0");
        assert_eq!(find_flag(&args, "--repeat-penalty"), "1");
        assert_eq!(find_flag(&args, "--presence-penalty"), "0");
        assert_eq!(find_flag(&args, "--n-predict"), "-1");
        assert_eq!(find_flag(&args, "--seed"), "-1");
    }

    // The chat template of the model is asked for once, and it is asked for on
    // every start. The tools of a chat, the thinking mode and the keeping of
    // the thinking are all read out of that template and go out with the ask,
    // so the flag can no longer stand over for one of them alone -- and a
    // start without it would take all three away from the chat.
    #[test]
    fn the_chat_template_is_asked_for_once() {
        let templated = ModelSettings {
            chat_template: String::from("{% for message in messages %}{{ message.content }}{% endfor %}"),
            ..Default::default()
        };
        // How many times the flag stands on the command line.
        let asked = |settings: &ModelSettings, template: bool| -> usize {
            let mut notes = Vec::new();
            let args = build_args(
                std::path::Path::new(""),
                &model(),
                settings,
                &[],
                8080,
                template,
                &mut notes,
            );
            args.iter().filter(|a| a.as_str() == "--jinja").count()
        };
        let plain = ModelSettings::default();
        // One ask, whichever of the two settings stands behind it.
        assert_eq!(asked(&plain, true), 1);
        assert_eq!(asked(&templated, true), 1);
        // The way the flag is put away again is the retry of a model whose
        // template will not stand, and it is asked for once there too.
        assert_eq!(asked(&plain, false), 0);
        // And the template of the user still goes to the server beside the ask.
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &templated,
            &[],
            8080,
            true,
            &mut notes,
        );
        assert_eq!(
            find_flag(&args, "--chat-template"),
            "{% for message in messages %}{{ message.content }}{% endfor %}"
        );
    }

    // Nothing of the thinking stands on the command line of the server any
    // more, so no choice of a badge moves a loaded model.
    #[test]
    fn no_thinking_choice_moves_the_command_line() {
        let line = || {
            let mut notes = Vec::new();
            build_args(
                std::path::Path::new(""),
                &model(),
                &ModelSettings::default(),
                &[],
                8080,
                true,
                &mut notes,
            )
        };
        let args = line();
        for flag in [
            "--reasoning",
            "--reasoning-effort",
            "--reasoning-preserve",
            "--no-reasoning-preserve",
        ] {
            assert!(!args.iter().any(|a| a == flag), "{} stands in the line", flag);
        }
    }

    // The thinking of an ask goes out with the ask. The switch, the level and
    // the keeping are the three words a chat may move between two answers, and
    // each of them is said to the chat template of that one ask.
    #[test]
    fn the_thinking_of_an_ask_rides_on_the_ask() {
        let off = thinking_kwargs("off", None);
        assert_eq!(off["enable_thinking"], json!(false));
        assert!(off.get("reasoning_effort").is_none());
        assert!(off.get("preserve_reasoning").is_none());

        let on = thinking_kwargs("on", None);
        assert_eq!(on["enable_thinking"], json!(true));
        assert!(on.get("reasoning_effort").is_none());

        let level = thinking_kwargs("medium", None);
        assert_eq!(level["enable_thinking"], json!(true));
        assert_eq!(level["reasoning_effort"], json!("medium"));

        // A mode the program does not know says nothing at all, which is what
        // a chat kept from an older version stood on before.
        assert!(thinking_kwargs("auto", None).is_null());
        assert!(thinking_kwargs("", None).is_null());
    }

    // The choice to keep the thinking of the turns before the last answer is
    // said in the one word of a newer server and in the four words a template
    // names for itself, so that both read the same choice. A model whose
    // template has no switch for it -- the choice of none -- says nothing.
    #[test]
    fn the_kept_thinking_rides_on_the_ask() {
        let held = thinking_kwargs("on", Some(true));
        assert_eq!(held["preserve_reasoning"], json!(true));
        assert_eq!(held["preserve_thinking"], json!(true));
        assert_eq!(held["clear_thinking"], json!(false));
        assert_eq!(held["truncate_history_thinking"], json!(false));
        assert_eq!(held["drop_thinking"], json!(false));

        let dropped = thinking_kwargs("on", Some(false));
        assert_eq!(dropped["preserve_reasoning"], json!(false));
        assert_eq!(dropped["preserve_thinking"], json!(false));
        assert_eq!(dropped["clear_thinking"], json!(true));
        assert_eq!(dropped["truncate_history_thinking"], json!(true));
        assert_eq!(dropped["drop_thinking"], json!(true));

        assert!(thinking_kwargs("on", None).get("preserve_reasoning").is_none());
    }

    // The whole of the layers go to the cards, and llama.cpp is read with its
    // own number for that -- a negative one -- rather than with a count of layers
    // the model cannot be over.
    #[test]
    fn the_layers_go_to_the_cards_as_a_negative_number() {
        let card = Gpu {
            device: String::from("CUDA0"),
            name: String::from("A card"),
            memory_mb: 12288,
            enabled: true,
            is_offload: true,
        };
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            std::slice::from_ref(&card),
            8080,
            true,
            &mut notes,
        );
        assert_eq!(find_flag(&args, "--device"), "CUDA0");
        assert_eq!(find_flag(&args, "-ngl"), "-1");

        // A card the user kept back is no card at all, and the layers stay in the
        // memory of the machine with the model.
        let back = Gpu {
            enabled: false,
            ..card
        };
        let mut notes = Vec::new();
        let held = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[back],
            8080,
            true,
            &mut notes,
        );
        assert_eq!(find_flag(&held, "--device"), "none");
        assert_eq!(find_flag(&held, "-ngl"), "0");
    }

    // The checkpoints and the ceiling of the talk memory go to the server, and
    // the defaults of the program stand in a new set of settings.
    #[test]
    fn the_checkpoints_and_the_cache_ram_go_to_the_server() {
        let settings = ModelSettings {
            ctx_checkpoints: 4,
            cache_ram: -1,
            ..Default::default()
        };
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            true,
            &mut notes,
        );
        assert_eq!(find_flag(&args, "--ctx-checkpoints"), "4");
        assert_eq!(find_flag(&args, "--cache-ram"), "-1");

        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[],
            8080,
            true,
            &mut notes,
        );
        assert_eq!(find_flag(&args, "--ctx-checkpoints"), "32");
        assert_eq!(find_flag(&args, "--cache-ram"), "8192");
    }

    // The prompt cache stands on for a new set of settings, and the switch of
    // the window reaches the server in one form or the other.
    #[test]
    fn the_cache_prompt_flag_follows_the_switch() {
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[],
            8080,
            true,
            &mut notes,
        );
        assert!(args.iter().any(|a| a == "--cache-prompt"));
        assert!(!args.iter().any(|a| a == "--no-cache-prompt"));

        let settings = ModelSettings {
            cache_prompt: false,
            ..Default::default()
        };
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            true,
            &mut notes,
        );
        assert!(!args.iter().any(|a| a == "--cache-prompt"));
        assert!(args.iter().any(|a| a == "--no-cache-prompt"));
    }

    // The room one image may take goes to the server with the vision switch
    // on. With the switch off, or with 0 in a field, the flag stays away, and
    // the value inside the model file stands.
    #[test]
    fn the_image_tokens_go_to_the_server_with_vision() {
        let settings = ModelSettings {
            vision: true,
            image_min_tokens: 256,
            image_max_tokens: 1024,
            ..Default::default()
        };
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            true,
            &mut notes,
        );
        assert_eq!(find_flag(&args, "--image-min-tokens"), "256");
        assert_eq!(find_flag(&args, "--image-max-tokens"), "1024");

        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[],
            8080,
            true,
            &mut notes,
        );
        assert!(!args.iter().any(|a| a == "--image-min-tokens"));
        assert!(!args.iter().any(|a| a == "--image-max-tokens"));

        let settings = ModelSettings {
            vision: true,
            ..Default::default()
        };
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            true,
            &mut notes,
        );
        assert!(!args.iter().any(|a| a == "--image-min-tokens"));
        assert!(!args.iter().any(|a| a == "--image-max-tokens"));
    }

    // Fast attention is sent to the server on every start, with no setting of
    // its own.
    #[test]
    fn fast_attention_is_always_on() {
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[],
            8080,
            true,
            &mut notes,
        );
        assert_eq!(find_flag(&args, "-fa"), "on");
    }

    // The switch for no context shift sends its flag, and the flag stays away
    // for a new set of settings.
    #[test]
    fn the_no_context_shift_flag_follows_the_switch() {
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[],
            8080,
            true,
            &mut notes,
        );
        assert!(!args.iter().any(|a| a == "--no-context-shift"));

        let settings = ModelSettings {
            no_context_shift: true,
            ..Default::default()
        };
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            true,
            &mut notes,
        );
        assert!(args.iter().any(|a| a == "--no-context-shift"));
    }

    // The switch for the CPU projector sends its flag with the vision switch
    // on, and the flag stays away for a new set of settings.
    #[test]
    fn the_cpu_projector_flag_follows_the_switch() {
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[],
            8080,
            true,
            &mut notes,
        );
        assert!(!args.iter().any(|a| a == "--no-mmproj-offload"));

        let settings = ModelSettings {
            vision: true,
            mmproj_cpu: true,
            ..Default::default()
        };
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            true,
            &mut notes,
        );
        assert!(args.iter().any(|a| a == "--no-mmproj-offload"));
    }

    // The server answers a request for the state of the prompt work with a
    // part named prompt_progress, and every number of it is kept.
    #[test]
    fn the_state_of_the_prompt_work_is_read_from_the_stream() {
        let mut answer = Answer::default();
        let mut seen: Vec<u64> = Vec::new();
        let payload = concat!(
            "{\"choices\":[{\"index\":0,\"delta\":{\"content\":null}}],",
            "\"prompt_progress\":{\"total\":215,\"cache\":20,\"processed\":108,\"time_ms\":900}}"
        );
        let going_on = apply_event(
            payload,
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |made: Progress| seen.push(made.processed),
            &mut |_| {},
            &mut |_| {},
        );
        assert!(going_on);
        assert_eq!(seen, vec![108]);
        assert_eq!(answer.progress.total, 215);
        assert_eq!(answer.progress.cache, 20);
        assert_eq!(answer.progress.time_ms, 900);
    }

    // The words of a call of the model come in pieces, about one word at a
    // time. The whole of what is written so far is given back on every piece, so
    // the user can watch a command grow instead of waiting for the end of it.
    #[test]
    fn the_words_of_a_call_are_given_back_as_they_come() {
        let mut answer = Answer::default();
        let mut seen: Vec<String> = Vec::new();
        let pieces = [
            concat!(
                "{\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,",
                "\"id\":\"call-one\",\"type\":\"function\",\"function\":",
                "{\"name\":\"run_terminal\",\"arguments\":\"{\"}}]}}]}"
            ),
            concat!(
                "{\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,",
                "\"function\":{\"arguments\":\"\\\"command\\\":\\\"ls\"}}]}}]}"
            ),
            concat!(
                "{\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,",
                "\"function\":{\"arguments\":\" -la\\\"}\"}}]}}]}"
            ),
        ];
        for piece in pieces {
            apply_event(
                piece,
                &mut answer,
                &mut |_| {},
                &mut |_| {},
                &mut |_| {},
                &mut |_| {},
                &mut |made: &[ToolCall]| seen.push(made[0].arguments.clone()),
            );
        }
        assert_eq!(
            seen,
            vec![
                "{",
                "{\"command\":\"ls",
                "{\"command\":\"ls -la\"}",
            ]
        );
        // The name came with the first piece, and the id with it, so the call is
        // known by the end of it as well as it was before.
        let call = answer.tool_calls.first().expect("one call was written");
        assert_eq!(call.name, "run_terminal");
        assert_eq!(call.id, "call-one");
    }

    // A stream cut off at the limit of the room leaves the words of a call in the
    // middle of them. Those words are all the program has of what the model was
    // put to, so they stand in the answer that is given back. And the answer has
    // spoken no finishing word: that is how the stream knows an answer that was
    // stopped from one that came to its end under the very piece that moved the
    // meter of the talk, which is a number the limit is looked at by.
    #[test]
    fn the_words_of_a_stopped_call_stand_in_the_answer() {
        let mut answer = Answer::default();
        let mut seen: Vec<String> = Vec::new();
        let pieces = [
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-edit","type":"function","function":{"name":"edit_file","arguments":"{\"path\":\"notes.md\""}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":",\"edits\":[{\"old_string\":\"th"}}]}}]}"#,
        ];
        for piece in pieces {
            apply_event(
                piece,
                &mut answer,
                &mut |_| {},
                &mut |_| {},
                &mut |_| {},
                &mut |_| {},
                &mut |made: &[ToolCall]| seen.push(made[0].arguments.clone()),
            );
        }
        // The words of the call stand whole up to the piece the stream stopped at,
        // and they are the words a server could not read as arguments: the string
        // the model was writing was never shut.
        assert_eq!(
            answer.tool_calls[0].arguments,
            r#"{"path":"notes.md","edits":[{"old_string":"th"#
        );
        let given = seen.last().map(String::as_str);
        assert_eq!(given, Some(r#"{"path":"notes.md","edits":[{"old_string":"th"#));
        assert_eq!(answer.tool_calls[0].name, editfile::EDIT_NAME);
        // No finishing word, so a hand over the stream that stops it is the cut
        // off of an answer, and not the end of one.
        assert!(answer.finish.is_empty());
        assert!(!answer.stopped);
        assert!(!answer.interrupted);
        // The word the model ends an answer with is told by the piece that brings
        // it, and no piece after it can be read as a call broken in the middle.
        apply_event(
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#,
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
        );
        assert_eq!(answer.finish, "stop");
    }

    // A call of a file tool, streamed in the order a model likes rather than in
    // the order the shape of the call asks for: the words to put in first, the
    // words to take out after them, and the name of the file last of all. The
    // words the user of the chat is handed move with every piece of the stream,
    // so the block of the call holds the change from the first word of it, and is
    // never left blank while the model is still writing.
    #[test]
    fn a_file_call_streamed_the_other_way_about_shows_its_words_as_they_come() {
        let arguments = concat!(
            r#"{"edits":[{"new_string":"uno","old_string":"one"}],"#,
            r#""path":"notes/todo.txt"}"#,
        );
        // One piece of the stream, as the server sends it. The first piece names
        // the tool and carries the mark that opens the words of the call.
        let piece = |words: &str, named: bool| -> String {
            let mut part = json!({ "index": 0, "function": { "arguments": words } });
            if named {
                part["id"] = json!("call-edit");
                part["type"] = json!("function");
                part["function"]["name"] = json!(editfile::EDIT_NAME);
            }
            json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [part] } }] }).to_string()
        };
        let mut answer = Answer::default();
        // The body of the block and the name of the file in the head, as the
        // user was handed them after every piece.
        let mut read: Vec<(String, String)> = Vec::new();
        let mut told = |made: &[ToolCall]| {
            read.push(editfile::live_params(editfile::EDIT_NAME, &made[0].arguments));
        };
        apply_event(
            &piece("{", true),
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut told,
        );
        // The rest of the words come four characters at a time, which is about
        // what a piece of an answer carries.
        let mut rest = &arguments[1..];
        while !rest.is_empty() {
            let end = rest
                .char_indices()
                .nth(4)
                .map(|(at, _)| at)
                .unwrap_or(rest.len());
            let (head, tail) = rest.split_at(end);
            apply_event(
                &piece(head, false),
                &mut answer,
                &mut |_| {},
                &mut |_| {},
                &mut |_| {},
                &mut |_| {},
                &mut told,
            );
            rest = tail;
        }
        // The whole of the call reads as the whole of any other call does.
        assert_eq!(
            read.last(),
            Some(&(
                String::from("change 1 · what stands now\no one\nchange 1 · what it becomes\nr uno\n"),
                String::from("notes/todo.txt")
            ))
        );
        // The words of the change stood in the block while the file was still
        // unnamed, and they never fell back to nothing after they had begun.
        let words = read
            .iter()
            .position(|(body, _)| !body.is_empty())
            .expect("a word of the change came to the block");
        // The first word the block was given stood under the mark of a change,
        // which is what a call written the other way about has to give: the words
        // to put in are the only words that have come so far. A block that waited
        // for the words to take out would show them first, over a mark.
        assert!(
            read[words].0.starts_with("change 1 · what it becomes\nr "),
            "the block first showed {:?}, and not a word under the mark of a change",
            read[words].0
        );
        let named = read
            .iter()
            .position(|(_, name)| !name.is_empty())
            .expect("the file was named");
        assert!(
            words < named,
            "the change came to the block at {} and the name of the file at {}",
            words,
            named
        );
        for (number, (body, _)) in read.iter().enumerate() {
            assert!(
                number < words || !body.is_empty(),
                "the block went blank again at piece {}",
                number
            );
        }
        // The body of the block moved more than once, so the user watched the
        // change being written instead of waiting for the end of it.
        let moves = read
            .windows(2)
            .filter(|pair| pair[0].0 != pair[1].0)
            .count();
        assert!(
            moves > 3,
            "the body of the block moved {} times over {} pieces",
            moves,
            read.len()
        );
    }

    // A piece of an answer is not a piece of a call, so it says nothing about
    // the calls.
    #[test]
    fn a_piece_of_the_answer_says_nothing_of_the_calls() {
        let mut answer = Answer::default();
        let mut told = 0usize;
        let piece = "{\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Now\"}}]}";
        apply_event(
            piece,
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_| told += 1,
        );
        // A piece that carries a call of nothing is passed over too.
        let empty = "{\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[]}}]}";
        apply_event(
            empty,
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_| told += 1,
        );
        assert_eq!(told, 0);
    }

    // The server is asked for the room the talk takes on every piece, so the
    // meter of the context has numbers to move on while the model writes.
    #[test]
    fn the_room_of_the_talk_is_read_from_the_stream() {
        let mut answer = Answer::default();
        let mut seen: Vec<u64> = Vec::new();
        let payload = concat!(
            "{\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Now\"}}],",
            "\"timings\":{\"cache_n\":14,\"prompt_n\":327,\"predicted_n\":9,\"prompt_ms\":4393.85,",
            "\"prompt_per_second\":26.38,\"predicted_per_second\":5.77}}"
        );
        let going_on = apply_event(
            payload,
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |made: Context| seen.push(made.used()),
            &mut |_| {},
        );
        assert!(going_on);
        // 327 words worked through, 14 kept from an earlier turn, and 9 words
        // written by the model.
        assert_eq!(seen, vec![350]);
    }

    // The room of the words the server was sent stands by itself on the answer, for
    // an ask that speaks no number at its end is weighed on those words and not on
    // the words the model had written when it was stopped. See the meter of the
    // context, and `Answer::context`.
    #[test]
    fn the_words_the_server_was_sent_stand_by_themselves() {
        let mut answer = Answer::default();
        // Nothing has been said of the talk yet, which is how an ask that the server
        // never answered reads.
        assert_eq!(answer.context.used(), 0);
        assert_eq!(answer.context.sent(), 0);
        let piece = concat!(
            "{\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Now\"}}],",
            "\"timings\":{\"cache_n\":14,\"prompt_n\":327,\"predicted_n\":9}}"
        );
        let going_on = apply_event(
            piece,
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_made: Context| {},
            &mut |_| {},
        );
        assert!(going_on);
        // 341 words were sent to the server, 9 had been written back, and the last
        // word of the kind stands on the answer whichever way the ask ends.
        assert_eq!(answer.context.used(), 350);
        assert_eq!(answer.context.sent(), 341);
        // A piece of no words at all says nothing of the talk, so it leaves the
        // number that stands alone.
        let hush = "{\"choices\":[{\"index\":0,\"delta\":{}}],\"timings\":{\"prompt_n\":0}}";
        apply_event(
            hush,
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_made: Context| {},
            &mut |_| {},
        );
        assert_eq!(answer.context.sent(), 341);
        // The answer gives the room of the words it was sent out of the two numbers
        // the server speaks of them, whichever it has.
        assert_eq!(answer.sent(), 341);
        // The progress of the prompt speaks the whole of the words that were sent
        // before a word of the talk is counted, which is all there is to stand on
        // for an answer the user stopped while the server was still working them
        // through.
        let mut early = Answer::default();
        apply_event(
            concat!(
                "{\"prompt_progress\":{\"total\":213797,\"cache\":0,",
                "\"processed\":98304,\"time_ms\":4000}}"
            ),
            &mut early,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_made: Context| {},
            &mut |_| {},
        );
        assert_eq!(early.context.sent(), 0, "nothing was said of the talk itself");
        assert_eq!(early.sent(), 213_797);
    }

    // The two speeds of the work stand on the same piece of the answer as the
    // room of the talk, for the server is asked for the numbers of every piece.
    // The numbers below are as a run of the model really gave them: 817 words of
    // the talk worked through at 26.4 to the second, and 16 words written at 5.8
    // to the second.
    #[test]
    fn the_two_speeds_of_the_work_are_read_from_the_stream() {
        let mut answer = Answer::default();
        let mut told: Option<Context> = None;
        let payload = concat!(
            "{\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Now\"}}],",
            "\"timings\":{\"cache_n\":0,\"prompt_n\":817,\"predicted_n\":16,",
            "\"prompt_ms\":30969.59,\"predicted_ms\":2772.1,",
            "\"prompt_per_second\":26.380714,\"predicted_per_second\":5.771872}}"
        );
        apply_event(
            payload,
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |made: Context| {
                if told.is_none() {
                    told = Some(made);
                }
            },
            &mut |_| {},
        );
        let said = told.expect("the piece carried the room, so it was told");
        assert!((said.prompt_per_second - 26.380714).abs() < 1e-6);
        assert!((said.decode_per_second - 5.771872).abs() < 1e-6);
        // A piece that says nothing of speed leaves both at nought, which is how
        // a server without these numbers reads.
        let mut quiet = Answer::default();
        let mut hush: Option<Context> = None;
        apply_event(
            "{\"choices\":[{\"index\":0,\"delta\":{\"content\":\"A\"}}],\"timings\":{\"prompt_n\":5}}",
            &mut quiet,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |made: Context| {
                if hush.is_none() {
                    hush = Some(made);
                }
            },
            &mut |_| {},
        );
        let still = hush.expect("the piece named a word of the talk");
        assert_eq!((still.prompt_per_second, still.decode_per_second), (0.0, 0.0));
    }

    // The speed of the words that were sent is read from the two numbers of the
    // progress of the prompt: the tokens done with, and the clock they took. The
    // tokens kept from before are no work at all, so they stand on neither side.
    #[test]
    fn the_speed_of_the_prompt_is_read_from_its_progress() {
        // 817 words in 30.967 seconds, which is 26.4 to the second.
        let going = Progress { total: 817, cache: 0, processed: 817, time_ms: 30967 };
        assert!((going.per_second() - 26.383).abs() < 0.01, "{}", going.per_second());
        // Half of the words were kept from before. The server counts the tokens it
        // has come to, the kept ones among them, so the work done in the time is
        // the tokens come to less the tokens kept: 60 come to, 40 kept, and 20
        // done in the second.
        let kept = Progress { total: 100, cache: 40, processed: 60, time_ms: 1000 };
        assert!((kept.per_second() - 20.0).abs() < 1e-9, "{}", kept.per_second());
        // A piece that names no time, or no work, gives no speed. The first piece
        // of a request is of that shape.
        assert_eq!(Progress { total: 817, cache: 0, processed: 0, time_ms: 0 }.per_second(), 0.0);
        assert_eq!(Progress { total: 0, cache: 0, processed: 0, time_ms: 500 }.per_second(), 0.0);
    }

    // A piece with no room in it says nothing of the talk. The first piece of a
    // request stands for the request itself, and the numbers of it are nothing.
    #[test]
    fn a_piece_of_no_words_says_nothing_of_the_talk() {
        let mut answer = Answer::default();
        let mut told = 0usize;
        let empty = concat!(
            "{\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":null}}],",
            "\"timings\":{\"cache_n\":0,\"prompt_n\":0,\"predicted_n\":0}}"
        );
        apply_event(
            empty,
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_| told += 1,
            &mut |_| {},
        );
        assert_eq!(told, 0);
        // A piece with no room of its own in it says nothing either.
        let plain = "{\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Now\"}}]}";
        apply_event(
            plain,
            &mut answer,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
            &mut |_| told += 1,
            &mut |_| {},
        );
        assert_eq!(told, 0);
    }

    // The words of the talk are counted once, whatever the server kept from
    // before. A talk of no words at all takes no room.
    #[test]
    fn the_words_of_the_talk_are_counted_once() {
        assert_eq!(Context::default().used(), 0);
        let fresh = Context {
            prompt: 327,
            cache: 0,
            written: 108,
            ..Context::default()
        };
        assert_eq!(fresh.used(), 435);
        // The same talk, but the server kept part of the words from before.
        let kept = Context {
            prompt: 4,
            cache: 323,
            written: 108,
            ..Context::default()
        };
        assert_eq!(kept.used(), 435);
    }

    // The part of the work that is done leaves out the tokens the server kept
    // from an earlier turn, for those cost nothing.
    #[test]
    fn the_part_of_the_work_leaves_the_kept_tokens_out() {
        let half = Progress {
            total: 200,
            cache: 100,
            processed: 150,
            time_ms: 10,
        };
        assert!((half.part() - 0.5).abs() < 1e-9);
        // Nothing is known yet, so nothing is done.
        assert_eq!(Progress::default().part(), 0.0);
        // Every token was kept from before, so no work is left to do.
        let kept = Progress {
            total: 200,
            cache: 200,
            processed: 200,
            time_ms: 0,
        };
        assert_eq!(kept.part(), 1.0);
        // A part over the whole is read as the whole.
        let over = Progress {
            total: 100,
            cache: 0,
            processed: 140,
            time_ms: 0,
        };
        assert_eq!(over.part(), 1.0);
    }

    // The mark of a split is read out of the name of a file.
    #[test]
    fn the_mark_of_a_split_is_read_from_a_name() {
        let parts = shard_of("Qwen3.8-Flash-Next-AD-5.00bpw-Q5_K_M-M64-00001-of-00033").unwrap();
        assert_eq!(parts.0, "Qwen3.8-Flash-Next-AD-5.00bpw-Q5_K_M-M64");
        assert_eq!(parts.1, 1);
        assert_eq!(parts.2, 33);
        let under = shard_of("model_00002_of_00004").unwrap();
        assert_eq!((under.0.as_str(), under.1, under.2), ("model", 2, 4));
        // A name that is not a split keeps itself.
        assert!(shard_of("llama-3-8b-instruct").is_none());
        assert!(shard_of("Qwen3.5-0.8B-UD-Q6_K_XL").is_none());
        assert!(shard_of("thing-of-00002").is_none());
    }

    // A model split into parts stands once in the list, named for the folder it
    // stands in, and its projector is found beside it.
    #[test]
    fn a_split_model_stands_once_in_the_list() {
        let root = std::env::temp_dir().join(format!(
            "atnatter-models-test-{}-1",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let inner = root.join("Qwen-3.8");
        fs::create_dir_all(&inner).unwrap();
        for n in 1..=33 {
            let name = format!("Qwen3.8-Flash-Next-AD-5.00bpw-Q5_K_M-M64-{n:05}-of-00033.gguf");
            fs::write(inner.join(name), b"x").unwrap();
        }
        fs::write(inner.join("Qwen-3.8-mmproj-f16.gguf"), b"x").unwrap();
        fs::write(root.join("Qwen3.5-0.8B-UD-Q6_K_XL.gguf"), b"x").unwrap();
        fs::write(root.join("Qwen3.5-0.8B-UD-Q6_K_XL-mmproj.gguf"), b"x").unwrap();

        let models = list_models(&root);
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["Qwen-3.8", "Qwen3.5-0.8B-UD-Q6_K_XL"]);
        // The first part of the split stands for the whole of it.
        let split = models.iter().find(|m| m.name == "Qwen-3.8").unwrap();
        assert!(split
            .path
            .to_string_lossy()
            .ends_with("M64-00001-of-00033.gguf"));
        // The projector is read from the folder of the model, where it stands.
        let mmproj = find_mmproj(&split.path).expect("the projector stands with the model");
        assert_eq!(mmproj.file_name().unwrap(), "Qwen-3.8-mmproj-f16.gguf");
        assert_eq!(mmproj.parent().unwrap(), inner);
        let alone = models.iter().find(|m| m.name != "Qwen-3.8").unwrap();
        let other = find_mmproj(&alone.path).expect("the projector stands with the model");
        assert_eq!(other.file_name().unwrap(), "Qwen3.5-0.8B-UD-Q6_K_XL-mmproj.gguf");
        let _ = fs::remove_dir_all(&root);
    }

    // A folder of two models cannot name them both for the folder, so the name
    // of the file is kept beside it.
    #[test]
    fn two_models_in_one_folder_are_told_apart() {
        let root = std::env::temp_dir().join(format!(
            "atnatter-models-test-{}-2",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let inner = root.join("pile");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("first-00001-of-00002.gguf"), b"x").unwrap();
        fs::write(inner.join("second.gguf"), b"x").unwrap();
        let models = list_models(&root);
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["pile (first)", "pile (second)"]);
        let _ = fs::remove_dir_all(&root);
    }

    // The value behind one flag, for the tests.
    fn find_flag(args: &[String], flag: &str) -> String {
        let pos = args.iter().position(|a| a == flag).expect("the flag is there");
        args[pos + 1].clone()
    }
}
