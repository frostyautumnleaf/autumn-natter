// Exercise the engine against a local llama-server stand-in that rejects oversized requests.
#![cfg(unix)]

use autumn_natter_core::config::{self, Config, ModelSettings};
use autumn_natter_core::{engine::Engine, store};
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WINDOW: u64 = 4096;
const LIMIT: u64 = 3482;
const OUTPUT_SIZE: u64 = 17_000_000;

const SERVER: &str = r#"#!/usr/bin/env python3
import http.server
import json
from pathlib import Path
import sys

root = Path(__file__).parent
port = int(sys.argv[sys.argv.index('--port') + 1])
window = 4096

def tool_result(body):
    return '\n'.join(message['content'] for message in body['messages']
                     if message['role'] == 'tool')

def count(body):
    # A token for each tool-result byte. Include the tool schema and wrappers
    # so that fitting only the result or an unrendered history does not pass.
    return 256 + len(tool_result(body).encode('utf-8')) + len(json.dumps(body.get('tools', []))) // 8

class Handler(http.server.BaseHTTPRequestHandler):
    def send_json(self, result, status=200):
        data = json.dumps(result).encode()
        self.send_response(status)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self.send_json({'status': 'ok'} if self.path == '/health' else {})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path in ('/apply-template', '/tokenize') and not (root / 'sizing').exists():
            self.send_json({}, 404)
            return
        if self.path == '/apply-template':
            # The request sent for sizing must itself be bounded.
            assert len(tool_result(body)) < 100_000
            self.send_json({'prompt': str(count(body))})
            return
        if self.path == '/tokenize':
            self.send_json({'tokens': [1] * int(body['content'])})
            return
        if self.path != '/v1/chat/completions':
            self.send_json({}, 404)
            return

        messages = body['messages']
        result = tool_result(body)
        tokens = count(body)
        if messages[-1].get('content', '').startswith('Write a fixture handoff summary'):
            kind = 'summary'
            delta = {'content': 'The command ran and printed both sentinel lines.'}
        elif result:
            kind = 'result'
            delta = {'content': 'I inspected the shortened output.'}
        elif (root / 'asks.jsonl').exists():
            kind = 'continued'
            delta = {'content': 'The work is complete.'}
        else:
            kind = 'call'
            repeats = int((root / 'output_size').read_text()) // 4
            command = f'python3 -c "print(\'BEGIN-RESULT\'); print(\'a4*7\' * {repeats}); print(\'END-RESULT\')"'
            delta = {'tool_calls': [{'index': 0, 'id': 'call-large-output', 'type': 'function',
                     'function': {'name': 'run_terminal', 'arguments': json.dumps({'command': command})}}]}

        with (root / 'asks.jsonl').open('a') as log:
            log.write(json.dumps({'kind': kind, 'tokens': tokens, 'result': result}) + '\n')

        if tokens >= window or (kind == 'result' and tokens >= int((root / 'limit').read_text())):
            self.send_json({'error': {'message': 'fixture context overflow'}}, 400)
            return
        finish = 'tool_calls' if kind == 'call' else 'stop'
        chunks = [{'choices': [{'delta': delta, 'finish_reason': None}]},
                  {'choices': [{'delta': {}, 'finish_reason': finish}],
                   'usage': {'prompt_tokens': tokens, 'completion_tokens': 0}}]
        data = ''.join('data: ' + json.dumps(chunk) + '\n\n' for chunk in chunks) + 'data: [DONE]\n\n'
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()
        self.wfile.write(data.encode())

http.server.HTTPServer(('127.0.0.1', port), Handler).serve_forever()
"#;

const TOKENIZER: &str = r#"#!/usr/bin/env python3
from pathlib import Path
import sys

# Fill a pipe before consuming stdin to check that neither side deadlocks.
sys.stdout.write("    1 -> 'x'\n" * 20000)
sys.stdout.flush()
data = sys.stdin.buffer.read()
with Path(__file__).with_name('countings.log').open('a') as log:
    log.write(str(len(data)) + '\n')
print(f'Total number of tokens: {len(data)}')
"#;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_tool_result_bigger_than_the_space_is_cut_before_it_is_sent() {
    for (handoff, sizing, output_size) in [
        (true, true, OUTPUT_SIZE),
        (false, true, OUTPUT_SIZE),
        (true, false, OUTPUT_SIZE),
        (true, true, 6000),
    ] {
        let scratch =
            Scratch(std::env::temp_dir().join(format!("autumn-big-result-{}", store::new_id())));
        fs::create_dir_all(&scratch.0).unwrap();
        config::set_data_dir(scratch.0.join("data"));
        let limit = if handoff { LIMIT } else { WINDOW };
        executable(&scratch.0.join("llama-server"), SERVER);
        if sizing {
            executable(&scratch.0.join("llama-tokenize"), TOKENIZER);
            fs::write(scratch.0.join("sizing"), "").unwrap();
        }
        fs::write(scratch.0.join("limit"), limit.to_string()).unwrap();
        fs::write(scratch.0.join("output_size"), output_size.to_string()).unwrap();
        let model = scratch.0.join("fixture.gguf");
        fs::write(&model, b"GGUF").unwrap();
        let model = model.display().to_string();
        let dir = scratch.0.display().to_string();
        let mut cfg = Config {
            llama_dir: dir.clone(),
            models_dir: dir,
            model_id: model.clone(),
            chat_compaction: if handoff { "handoff" } else { "off" }.into(),
            chat_compact_prompt: "Write a fixture handoff summary of the work.".into(),
            ..Config::default()
        };
        cfg.model_settings.insert(
            model,
            ModelSettings {
                ctx_size: WINDOW as u32,
                gpu_enabled: vec![false; 8],
                ..ModelSettings::default()
            },
        );
        let engine = Engine::start(cfg);
        engine.new_chat();
        engine.set_code(true);
        engine.set_sandbox(false);
        engine.prompt("Run the synthetic output command.".into(), Vec::new());

        let timeout = Instant::now() + Duration::from_secs(55);
        let asks_path = scratch.0.join("asks.jsonl");
        let snap = loop {
            let snap = engine.snapshot();
            let completed = snap.messages.iter().any(|message| {
                message.role == "Assistant"
                    && message.body
                        == if handoff {
                            "The work is complete."
                        } else {
                            "I inspected the shortened output."
                        }
            });
            if completed && !snap.generating && !snap.busy {
                break snap;
            }
            assert!(
                Instant::now() < timeout,
                "turn stalled (handoff={handoff}, sizing={sizing}, output={output_size}): {}. Summary state: {:?}",
                snap.status,
                snap.messages
                    .iter()
                    .filter(|message| message.role == "Summary")
                    .map(|message| (&message.body, &message.tool))
                    .collect::<Vec<_>>(),
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        let asks: Vec<Value> = fs::read_to_string(asks_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let result = asks
            .iter()
            .find(|ask| ask["kind"] == "result")
            .unwrap_or_else(|| panic!("the tool result never reached the model: {}", snap.status));
        assert!(result["tokens"].as_u64().unwrap() < limit);
        assert!(
            snap.context_used < limit,
            "the meter counted the uncut transcript"
        );
        let told = result["result"].as_str().unwrap();
        assert!(told.starts_with("BEGIN-RESULT"));
        assert!(told.trim_end().ends_with("END-RESULT"));
        assert!(told.contains("characters of it are left out"));

        let stored = snap
            .messages
            .iter()
            .find(|message| message.call_id == "call-large-output")
            .unwrap();
        assert!(stored.tool.len() as u64 > output_size);
        assert!(!stored.cancelled);
        assert!(!stored.tool.contains("characters of it are left out"));
        if sizing {
            assert_eq!(
                fs::read_to_string(scratch.0.join("countings.log"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
        }
        if handoff {
            assert!(
                snap.messages
                    .iter()
                    .any(|message| message.role == "Summary" && !message.body.is_empty()),
                "the handoff remains unwritten: {}",
                snap.status
            );
            let summary = asks.iter().find(|ask| ask["kind"] == "summary").unwrap();
            assert!(summary["tokens"].as_u64().unwrap() <= WINDOW - 512);
            assert!(summary["result"].as_str().unwrap().contains("END-RESULT"));
        } else {
            assert!(!asks.iter().any(|ask| ask["kind"] == "summary"));
        }
        engine.shutdown();
    }
}
