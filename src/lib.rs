// Autumn Natter. A chat program for models that run on this machine.
//
// The program starts llama-server as a child process and talks to it over the
// OpenAI compatible HTTP interface. It does not load a model by itself.
//
// This crate holds everything except the window: the engine, the settings, the
// store on disk, the llama.cpp reader, the terminal tool, the device list, the
// UPnP mapping, the browser page of the remote mode, and the state as JSON.
//
// The desktop window lives in the sibling crate `src-tauri`. It is a Tauri
// window: Rust on the back, and an HTML page shown by the system web view. The
// page and the remote mode read the same state, so both show the same thing.
//
// Run the program with --help to see the arguments.

// The state of the page is one great json! call in web.rs. Every part of it is
// a step of the macro, and the calls of the chatting settings came near the
// limit the compiler goes by. The limit is raised for the crate so the state
// can grow without the compiler stopping over it.
#![recursion_limit = "256"]

pub mod attach;
pub mod config;
pub mod editfile;
pub mod engine;
pub mod gguf;
pub mod gpu;
pub mod greet;
pub mod llama;
pub mod logs;
pub mod markdown;
pub mod mcp;
pub mod remote;
pub mod search;
pub mod store;
pub mod tools;
pub mod upnp;
pub mod util;
pub mod viewimage;
pub mod web;
