// Autumn Natter. A chat program for models that run on this machine.
//
// The program starts llama-server as a child process and talks to it over the
// OpenAI compatible HTTP interface. It does not load a model by itself.
//
// Run it with --help to see the arguments.

mod config;
mod engine;
mod remote;
mod gpu;
mod llama;
mod store;
mod tools;
mod ui_app;
mod upnp;
mod util;
mod web;

use std::path::PathBuf;
use std::process::ExitCode;

// What the user asked for on the command line.
struct Args {
    // Headless run with a browser server.
    remote: bool,
    // Port for the browser server.
    port: Option<u16>,
    // Another place for the program data.
    data_dir: Option<PathBuf>,
    help: bool,
    version: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("autumn-natter: {}", problem);
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    if args.help {
        print_help();
        return Ok(());
    }
    if args.version {
        println!("autumn-natter {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // The data folder must be known before anything reads a path.
    if let Some(dir) = args.data_dir {
        config::set_data_dir(dir);
    }
    config::ensure_dirs().map_err(|e| format!("cannot create the data folder: {}", e))?;

    let mut cfg = config::load();
    if let Some(port) = args.port {
        cfg.remote_port = port;
    }

    if args.remote {
        // The remote run has no window. It serves a page for other computers.
        return remote::start(cfg);
    }
    ui_app::run(cfg)
}

// Read the command line. Unknown arguments are an error, so a spelling mistake
// is not ignored.
fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        remote: false,
        port: None,
        data_dir: None,
        help: false,
        version: false,
    };
    let mut items: Vec<String> = std::env::args().skip(1).collect();
    let mut index = 0;
    while index < items.len() {
        let item = items[index].clone();
        index += 1;
        match item.as_str() {
            "--remote" | "-r" => args.remote = true,
            "--help" | "-h" => args.help = true,
            "--version" | "-V" => args.version = true,
            "--port" | "-p" => {
                let Some(value) = items.get(index) else {
                    return Err("--port needs a number".to_string());
                };
                index += 1;
                args.port = Some(
                    value
                        .parse::<u16>()
                        .map_err(|_| format!("{} is not a port number", value))?,
                );
            }
            "--data-dir" => {
                let Some(value) = items.get(index) else {
                    return Err("--data-dir needs a folder".to_string());
                };
                index += 1;
                args.data_dir = Some(PathBuf::from(value));
            }
            other => {
                return Err(format!(
                    "unknown argument {}. Run with --help for the list",
                    other
                ))
            }
        }
    }
    // The list is only read once, so drop the parts that were used.
    items.clear();
    Ok(args)
}

fn print_help() {
    print!(
        "Autumn Natter {} - chat with a local model through llama.cpp

Usage:
  autumn-natter [arguments]

Arguments:
  -r, --remote        Start without a window and serve a page for other
                      computers on the local network.
  -p, --port PORT     Port for the remote page. The settings file holds the
                      port when this is not given.
      --data-dir DIR  Keep the program data in another folder.
  -h, --help          Show this text.
  -V, --version       Show the version.

The settings live in the autumn_natter folder of the program data of the
operating system. Set the folders for llama-server and for the models in the
settings window.
"
    , env!("CARGO_PKG_VERSION"));
}

