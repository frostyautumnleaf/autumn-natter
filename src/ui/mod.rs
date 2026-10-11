// The desktop window, written with Iced.
//
// src/ui_app.rs holds the state, the messages and the update step. The files
// in this folder draw that state. Nothing here waits for the model. Every part
// reads a snapshot of the engine, and every click sends a message back.

pub mod dialog_view;
pub mod icon;
pub mod main_view;
pub mod message_view;
pub mod palette;
pub mod parts;
pub mod shape;
