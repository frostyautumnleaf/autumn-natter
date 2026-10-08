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
    // chat may call the terminal tool.
    pub fn start(
        binary: &Path,
        model: &ModelInfo,
        settings: &ModelSettings,
        gpus: &[Gpu],
        use_tools: bool,
        thinking_mode: &str,
    ) -> Result<Server, String> {
        let port = util::free_port().ok_or("no free port on this machine")?;
        let mut notes = Vec::new();
        let mut args = build_args(binary, model, settings, gpus, port, use_tools, thinking_mode, &mut notes);

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
                Err(e) => {
                    return Err(format!(
                        "cannot start {}: {}",
                        binary.display(),
                        e
                    ))
                }
            };
            let mut server = Server {
                child,
                url: format!("http://127.0.0.1:{}", port),
                ctx_total: settings.ctx_size as u64,
                model_id: model.id(),
                thinking_mode: thinking_mode.to_string(),
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
                            "the model has no usable chat template, so the terminal tool is off"
                                .to_string(),
                        );
                        args = build_args(binary, model, settings, gpus, port, false, thinking_mode, &mut notes);
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
    let total: u64 = gpus.iter().filter(|g| g.is_offload).map(|g| g.memory_mb).sum();
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

    // Vision projector.
    if settings.vision {
        match find_mmproj(&model.path) {
            Some(path) => {
                args.push("--mmproj".into());
                args.push(path.display().to_string());
            }
            None => notes.push(format!(
                "no vision projector next to {}",
                model.name
            )),
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
    // The user stopped the answer.
    pub stopped: bool,
    // A transport problem, or the message from the server.
    pub error: Option<String>,
}

// Send one chat request and stream the answer. on_text gets every piece of new
// text as it arrives, so the window can show it.
pub fn stream_chat(
    base_url: &str,
    body: &Value,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(&str),
    on_reasoning: &mut dyn FnMut(&str),
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
            answer.error = Some(format!("cannot reach the model: {}", other));
            return answer;
        }
    };

    let mut reader = response.into_reader();
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
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
                        if !apply_event(payload.trim(), &mut answer, on_text, on_reasoning) {
                            // The stream told us it is finished.
                            pending.clear();
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                if !answer.stopped {
                    answer.error = Some(format!("the stream stopped: {}", e));
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
        return format!("the model server answered with status {}", code);
    }
    format!("{} (status {})", detail, code)
}
