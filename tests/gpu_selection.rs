// Start a small stand-in for llama-server and inspect the command it receives.
#![cfg(unix)]

use autumn_natter_core::{
    config::{self, ModelSettings},
    gpu::{self, Gpu},
    llama::{self, ModelInfo, Server},
    store,
};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn card(name: &str, memory_mb: u64) -> Gpu {
    Gpu {
        device: name.into(),
        name: name.into(),
        memory_mb,
        enabled: true,
        is_offload: true,
    }
}

#[test]
fn the_server_uses_the_same_gpu_selection_as_the_preview() {
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("python3 is unavailable; the server startup check was skipped");
        return;
    }

    let place = std::env::temp_dir().join(format!("autumn-gpu-selection-{}", store::new_id()));
    std::fs::create_dir_all(&place).unwrap();
    config::set_data_dir(place.join("data"));
    let binary = place.join("llama-server");
    std::fs::write(
        &binary,
        r##"#!/usr/bin/env python3
import http.server
import json
import sys

with open(__file__ + '.json', 'w') as output:
    json.dump(sys.argv[1:], output)

port = int(sys.argv[sys.argv.index('--port') + 1])

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = b'{"status":"ok"}' if self.path == '/health' else b'{}'
        self.send_response(200)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

http.server.HTTPServer(('127.0.0.1', port), Handler).serve_forever()
"##,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let model = ModelInfo {
        path: PathBuf::from("/models/test.gguf"),
        name: "test".into(),
        kind: "GGUF",
    };
    let devices = vec![
        card("CUDA0", 4096),
        card("CUDA1", 8192),
        card("CUDA2", 16384),
    ];
    for (choices, parallel, expected, margin) in [
        (
            vec![true, false, true],
            true,
            "CUDA0,CUDA2",
            Some("410,1638"),
        ),
        (vec![true, false, true], false, "CUDA0", Some("410")),
        (vec![false, true, false], true, "CUDA1", Some("819")),
        (vec![false, false, false], true, "none", None),
        (vec![], true, "CUDA0,CUDA1,CUDA2", Some("410,819,1638")),
    ] {
        let settings = ModelSettings {
            gpu_enabled: choices,
            tensor_parallel: parallel,
            ..Default::default()
        };
        let mut server = Server::start(&binary, &model, &settings, &devices, None, None).unwrap();
        let args: Vec<String> =
            serde_json::from_slice(&std::fs::read(binary.with_extension("json")).unwrap()).unwrap();
        let device = args.iter().position(|arg| arg == "--device").unwrap();
        assert_eq!(args[device + 1], expected);
        let actual_margin = args
            .iter()
            .position(|arg| arg == "--fit-target")
            .map(|index| args[index + 1].as_str());
        assert_eq!(actual_margin, margin);
        if expected == "none" {
            let layers = args.iter().position(|arg| arg == "-ngl").unwrap();
            assert_eq!(args[layers + 1], "0");
        }
        let port: u16 = args[args.iter().position(|arg| arg == "--port").unwrap() + 1]
            .parse()
            .unwrap();
        let selected = gpu::selected(&devices, &settings.gpu_enabled);
        let preview = llama::build_args(
            Path::new(""),
            &model,
            &settings,
            &selected,
            port,
            true,
            &mut Vec::new(),
        );
        assert_eq!(args, preview);
        server.stop();
    }

    std::fs::remove_dir_all(place).unwrap();
}
