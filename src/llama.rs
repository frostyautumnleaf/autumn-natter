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

// Look for model files in a directory tree. Hidden folders are skipped.
pub fn list_models(dir: &Path) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = Vec::new();
    walk(dir, 0, 3, &mut out);
    out.sort_by_key(|m| m.name.to_lowercase());
    out
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

// Find the vision projector that belongs to a model.
pub fn find_mmproj(model: &Path) -> Option<PathBuf> {
    let dir = model.parent()?;
    let stem = model.file_stem()?.to_string_lossy().to_lowercase();
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
        // Prefer the projector named after the model.
        if name.starts_with(&stem) {
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
    // The thinking mode the server was started with. A change needs a restart.
    pub thinking_mode: String,
    // Whether the server was started to keep the thinking of the turns before
    // the last answer. Nothing is kept when the template has no switch for it.
    pub preserve_thinking: Option<bool>,
    // Whether the server was started with tool support. A tool coming needs a
    // restart, for the server reads the chat template once and one started
    // without the flag reads no tool call. A tool going needs nothing: such a
    // server answers a chat that asks for none as plain words.
    pub tools_on: bool,
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

    fn log_file() -> Option<fs::File> {
        let path = crate::config::paths().data.join("server.log");
        fs::File::create(path).ok()
    }

    // Start llama-server and wait until it answers. use_tools is true when the
    // chat may call a tool of any kind: a command, or a mark of a chapter.
    pub fn start(
        binary: &Path,
        model: &ModelInfo,
        settings: &ModelSettings,
        gpus: &[Gpu],
        use_tools: bool,
        thinking_mode: &str,
        preserve_thinking: Option<bool>,
    ) -> Result<Server, String> {
        let port = util::free_port().ok_or("No free port on this machine")?;
        let mut notes = Vec::new();
        let mut args = build_args(
            binary,
            model,
            settings,
            gpus,
            port,
            use_tools,
            thinking_mode,
            preserve_thinking,
            &mut notes,
        );

        let mut attempt = 0;
        loop {
            attempt += 1;
            let log = Self::log_file();
            let mut command = Command::new(binary);
            command.args(&args).stdin(Stdio::null());
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
                thinking_mode: thinking_mode.to_string(),
                preserve_thinking,
                tools_on: use_tools,
                notes: notes.clone(),
            };
            match server.wait_ready() {
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
                    // A model without a chat template cannot use --jinja. One
                    // retry without the tool support tells the user less and
                    // still works.
                    if attempt == 1 && use_tools && problem.exited {
                        notes.push(
                            "This model has no usable chat template, so no tool is offered"
                                .to_string(),
                        );
                        args = build_args(
                            binary,
                            model,
                            settings,
                            gpus,
                            port,
                            false,
                            thinking_mode,
                            preserve_thinking,
                            &mut notes,
                        );
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
    fn wait_ready(&mut self) -> Result<Value, Fail> {
        let deadline = Instant::now() + Duration::from_secs(900);
        // The status of the last health call, for the error text. Every round
        // of the loop sets it before the text is made.
        let mut last_status: Option<u16>;
        loop {
            // A child that left early never becomes ready.
            if let Ok(Some(_)) = self.child.try_wait() {
                return Err(Fail {
                    text: "llama-server stopped before it was ready".to_string(),
                    output: read_tail(&crate::config::paths().data.join("server.log"), 1200),
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
                    output: read_tail(&crate::config::paths().data.join("server.log"), 1200),
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

// Read the end of a text file, for an error message.
fn read_tail(path: &Path, max_bytes: usize) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
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

// Build the command line for llama-server. The server takes many knobs,
// so the call is a flat list.
#[allow(clippy::too_many_arguments)]
pub fn build_args(
    _binary: &Path,
    model: &ModelInfo,
    settings: &ModelSettings,
    gpus: &[Gpu],
    port: u16,
    use_tools: bool,
    thinking_mode: &str,
    preserve_thinking: Option<bool>,
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
            args.push("-ngl".into());
            args.push("999".into());
        }
    }

    // A VRAM budget below the total makes llama.cpp fit the model into it.
    let total: u64 = gpus
        .iter()
        .filter(|g| g.is_offload)
        .map(|g| g.memory_mb)
        .sum();
    if settings.vram_budget > 0.0 && total > 0 && (settings.vram_budget as u64) < total {
        args.push("--fit".into());
        args.push("on".into());
        let margins: Vec<String> = gpus
            .iter()
            .filter(|g| g.is_offload)
            .map(|g| {
                let share = g.memory_mb as f64 * (settings.vram_budget / total as f64);
                let used = share.min(g.memory_mb as f64);
                let margin = (g.memory_mb as f64 - used).max(32.0) as u64;
                margin.to_string()
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

    // Thinking mode from the chat badge.
    match thinking_mode {
        "off" => {
            args.push("--reasoning".into());
            args.push("off".into());
        }
        "on" => {
            args.push("--reasoning".into());
            args.push("on".into());
        }
        "low" | "medium" | "high" | "xhigh" => {
            args.push("--reasoning".into());
            args.push("on".into());
            args.push("--reasoning-effort".into());
            args.push(thinking_mode.to_string());
        }
        _ => {}
    }

    // The thinking of the turns before the last answer. llama.cpp keeps it on
    // its own for a template that can, so the flag stays away when the template
    // of the model has no switch for it. The two flags of llama.cpp are the
    // plain way to say the same thing; the older spelling, a template keyword
    // of the json of --chat-template-kwargs, is now looked at and pointed at
    // these two.
    match preserve_thinking {
        Some(true) => args.push("--reasoning-preserve".into()),
        Some(false) => args.push("--no-reasoning-preserve".into()),
        None => {}
    }

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

    // Tool support needs the chat template of the model.
    if use_tools {
        args.push("--jinja".into());
    }

    // A custom chat template replaces the one embedded in the model.
    if !settings.chat_template.trim().is_empty() {
        args.push("--jinja".into());
        args.push("--chat-template".into());
        args.push(settings.chat_template.trim().to_string());
    }

    // Free form arguments of the user come last, so they win.
    for part in util::split_args(&settings.extra_args) {
        args.push(part);
    }
    args
}

fn clamp(index: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    index.min(len - 1)
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

// Send one chat request and stream the answer. on_text gets every piece of new
// text as it arrives, so the window can show it. on_progress gets the state of
// the prompt work, as the server reports it while the prompt goes through.
// on_context gets how full the room of the talk is, as the server reports it on
// every piece of the answer. on_tool gets the calls of the model as they are
// written, so the reader can see the words of a command as the model puts them
// down. The stream has a hand for everything the window shows at once, so the
// call is a flat list of them.
//
// Two hands stop the stream. `cancel` is the reader's, and `interrupt` is the
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

    let mut reader = response.into_reader();
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
        match reader.read(&mut chunk) {
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
    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            answer.text.push_str(content);
            on_text(content);
        }
    }
    // Reasoning models stream their thinking in a separate field.
    if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
        if !reasoning.is_empty() {
            answer.reasoning.push_str(reasoning);
            on_reasoning(reasoning);
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

    // The Sampling section sends its values to the server on every start, and
    // the defaults of the program stand in a new set of settings.
    #[test]
    fn the_sampling_values_go_to_the_server() {
        let mut settings = ModelSettings::default();
        settings.temperature = 0.2;
        settings.top_p = 0.5;
        settings.top_k = 7;
        settings.min_p = 0.1;
        settings.repeat_penalty = 1.1;
        settings.presence_penalty = 0.2;
        settings.max_tokens = 256;
        settings.seed = 42;
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            false,
            "off",
            None,
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
            false,
            "off",
            None,
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

    // The checkpoints and the ceiling of the talk memory go to the server, and
    // the defaults of the program stand in a new set of settings.
    #[test]
    fn the_checkpoints_and_the_cache_ram_go_to_the_server() {
        let mut settings = ModelSettings::default();
        settings.ctx_checkpoints = 4;
        settings.cache_ram = -1;
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            false,
            "off",
            None,
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
            false,
            "off",
            None,
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
            false,
            "off",
            None,
            &mut notes,
        );
        assert!(args.iter().any(|a| a == "--cache-prompt"));
        assert!(!args.iter().any(|a| a == "--no-cache-prompt"));

        let mut settings = ModelSettings::default();
        settings.cache_prompt = false;
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            false,
            "off",
            None,
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
        let mut settings = ModelSettings::default();
        settings.vision = true;
        settings.image_min_tokens = 256;
        settings.image_max_tokens = 1024;
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            false,
            "off",
            None,
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
            false,
            "off",
            None,
            &mut notes,
        );
        assert!(!args.iter().any(|a| a == "--image-min-tokens"));
        assert!(!args.iter().any(|a| a == "--image-max-tokens"));

        let mut settings = ModelSettings::default();
        settings.vision = true;
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            false,
            "off",
            None,
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
            false,
            "off",
            None,
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
            false,
            "off",
            None,
            &mut notes,
        );
        assert!(!args.iter().any(|a| a == "--no-context-shift"));

        let mut settings = ModelSettings::default();
        settings.no_context_shift = true;
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            false,
            "off",
            None,
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
            false,
            "off",
            None,
            &mut notes,
        );
        assert!(!args.iter().any(|a| a == "--no-mmproj-offload"));

        let mut settings = ModelSettings::default();
        settings.vision = true;
        settings.mmproj_cpu = true;
        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &settings,
            &[],
            8080,
            false,
            "off",
            None,
            &mut notes,
        );
        assert!(args.iter().any(|a| a == "--no-mmproj-offload"));
    }

    // The choice to keep the thinking of the turns before the last answer sends
    // one of the two flags of llama.cpp for it. A model whose template has no
    // switch for it gets neither, and the server then does as it likes.
    #[test]
    fn the_kept_thinking_sends_its_flag() {
        let holds = |args: &Vec<String>, flag: &str| args.iter().any(|a| a == flag);

        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[],
            8080,
            false,
            "on",
            None,
            &mut notes,
        );
        assert!(!holds(&args, "--reasoning-preserve"));
        assert!(!holds(&args, "--no-reasoning-preserve"));

        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[],
            8080,
            false,
            "on",
            Some(true),
            &mut notes,
        );
        assert!(holds(&args, "--reasoning-preserve"));
        assert!(!holds(&args, "--no-reasoning-preserve"));

        let mut notes = Vec::new();
        let args = build_args(
            std::path::Path::new(""),
            &model(),
            &ModelSettings::default(),
            &[],
            8080,
            false,
            "off",
            Some(false),
            &mut notes,
        );
        assert!(holds(&args, "--no-reasoning-preserve"));
        assert!(!holds(&args, "--reasoning-preserve"));
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
    // the reader can watch a command grow instead of waiting for the end of it.
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
    // words the reader of the chat is handed move with every piece of the stream,
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
        // reader was handed them after every piece.
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
                String::from("one\n> uno"),
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
            read[words].0.starts_with("> "),
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
        // The body of the block moved more than once, so the reader watched the
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

    // The value behind one flag, for the tests.
    fn find_flag(args: &[String], flag: &str) -> String {
        let pos = args.iter().position(|a| a == flag).expect("the flag is there");
        args[pos + 1].clone()
    }
}
