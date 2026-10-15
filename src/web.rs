// The browser page of the remote mode, and the state that the page reads.
//
// The page is one HTML file with its CSS and JavaScript inside. It is put into
// the program when the program is built, so a browser needs nothing from the
// internet. The page asks for the state with a short poll, and it sends the
// actions of the user with a POST call.

use serde_json::{json, Value};

use crate::engine::Snap;
use crate::store;
use crate::util;

// The page itself. See web/page.html.
pub const PAGE: &str = include_str!("../web/page.html");

// The state as JSON. The page sends the number it already has (since). When
// nothing changed, the answer stays small.
pub fn state(snap: &Snap, since: Option<u64>) -> Value {
    if since == Some(snap.rev) {
        return json!({ "unchanged": true, "rev": snap.rev });
    }
    let models: Vec<Value> = snap
        .models
        .iter()
        .map(|m| json!({ "id": m.id(), "name": m.name, "kind": m.kind }))
        .collect();
    let repositories: Vec<Value> = snap
        .repositories
        .iter()
        .map(|r| json!({ "id": r.id, "name": r.name }))
        .collect();
    let chats: Vec<Value> = snap
        .chats
        .iter()
        .map(|c| {
            json!({
                "id": c.meta.id,
                "title": c.meta.title,
                "repo": repo_name(snap, &c.meta.repo_id),
                "when": store::format_when(c.meta.updated),
                "active": c.meta.id == snap.chat_id,
                "preview": store::preview(&c.messages),
            })
        })
        .collect();
    let messages: Vec<Value> = snap
        .messages
        .iter()
        .map(|m| {
            json!({
                // The id lets the page keep the rows it has and write again
                // only the words that changed.
                "id": m.id,
                "role": m.role,
                "body": m.body,
                // The thinking of the model, when the model has any.
                "reasoning": m.reasoning,
                // How long that thinking took, in milliseconds.
                "reasoning_ms": m.reasoning_ms,
                "tool": m.tool,
                "mono": m.mono,
                "failed": m.failed,
                "when": store::format_clock(m.when),
            })
        })
        .collect();
    json!({
        "rev": snap.rev,
        "unchanged": false,
        "model": {
            "id": snap.model_id,
            "name": if snap.model_name.is_empty() { "no model" } else { &snap.model_name },
            "kind": snap.model_kind,
        },
        "models": models,
        "repository": { "id": snap.repository_id, "name": snap.repository_name },
        "repositories": repositories,
        "chat": { "id": snap.chat_id, "title": snap.chat_title },
        "chats": chats,
        // The first screen: the chat box on its own, with a line for this hour.
        "home": snap.home,
        "greeting": snap.greeting,
        "overview": snap.overview,
        // Where the carousel stands, and whether the open chat is blank.
        "carousel": {
            "index": snap.chat_index,
            "count": snap.chat_count,
            "blank": snap.blank_chat,
        },
        "messages": messages,
        // Text the model wrote since the last message was stored.
        "live": snap.live_text,
        // The thinking that comes with a live answer, how long it lasted and
        // whether it has stopped. The page writes the last two on the head of
        // the panel.
        "reasoning": snap.live_reasoning,
        "reasoning_ms": snap.live_thought_ms,
        "reasoning_done": snap.live_thought_done,
        // The thinking mode of the open chat, and whether this model has one.
        "thinking": {
            "mode": &snap.thinking_mode,
            "available": snap.thinking_available,
        },
        "context": {
            "used": util::count(snap.context_used),
            "total": util::count(snap.context_total),
            "ratio": ratio(snap),
        },
        "generating": snap.generating,
        "busy": snap.busy,
        "code": snap.code_enabled,
        "sandbox": snap.sandbox_enabled,
        "sandbox_available": snap.sandbox_available,
        "sandbox_note": snap.sandbox_note,
        "status": snap.status,
        "dark": snap.dark_mode,
        // Chatting. How the page shows a thinking panel, a tool panel and a
        // block of code.
        "chatting": {
            "collapse_thinking": snap.chat_collapse_thinking,
            "collapse_tools": snap.chat_collapse_tools,
            "highlight": snap.chat_highlight,
        },
        // The folder of the program data. The window of settings reads it out.
        "data_dir": snap.data_dir,
    })
}

// Part of the context meter, between 0 and 1.
fn ratio(snap: &Snap) -> f64 {
    if snap.context_total == 0 {
        return 0.0;
    }
    (snap.context_used as f64 / snap.context_total as f64).clamp(0.0, 1.0)
}

// Name of a repository, for the chat list. A chat with no repository gives an
// empty string, and the page then shows nothing there.
fn repo_name<'a>(snap: &'a Snap, id: &str) -> &'a str {
    if id.is_empty() {
        return "";
    }
    snap.repositories
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.name.as_str())
        .unwrap_or("")
}

// Answer after an action worked. The page reads the new state right away.
pub fn done() -> Value {
    json!({ "ok": true })
}

// Answer for a call that cannot be made.
pub fn refused(text: &str) -> Value {
    json!({ "ok": false, "status": text })
}

// One call of the user interface. The page of the remote mode and the buttons
// of the desktop window send the same names, so the two go through here. Every
// call is answered with a small JSON record, and the caller then reads the
// state again.
pub fn action(engine: &crate::engine::Engine, path: &str, body: &str) -> Value {
    let value = serde_json::from_str::<Value>(body).unwrap_or_else(|_| json!({}));
    match path {
        "/api/send" | "send" => {
            let text = text_field(&value, "text");
            if text.trim().is_empty() {
                return refused("nothing was written");
            }
            engine.prompt(text);
            done()
        }
        "/api/cancel" | "cancel" => {
            engine.cancel();
            done()
        }
        "/api/code" | "code" => {
            engine.set_code(flag(&value, "on"));
            done()
        }
        "/api/sandbox" | "sandbox" => {
            engine.set_sandbox(flag(&value, "on"));
            done()
        }
        "/api/thinking" | "thinking" => {
            engine.cycle_thinking();
            done()
        }
        "/api/new_chat" | "new_chat" => {
            engine.new_chat();
            done()
        }
        // Turn the carousel of chats. Minus one shows the chat at the left.
        "/api/roll" | "roll" => {
            let step = value
                .get("step")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                .clamp(-50, 50) as i32;
            engine.roll(step);
            done()
        }
        "/api/open_chat" | "open_chat" => {
            engine.open_chat(text_field(&value, "id"));
            done()
        }
        "/api/overview" | "overview" => {
            engine.set_overview(flag(&value, "on"));
            done()
        }
        "/api/select_model" | "select_model" => {
            engine.select_model(text_field(&value, "id"));
            done()
        }
        "/api/select_repository" | "select_repository" => {
            engine.select_repository(text_field(&value, "id"));
            done()
        }
        "/api/create_repository" | "create_repository" => {
            engine.create_repository(text_field(&value, "name"));
            done()
        }
        "/api/dark" | "dark" => {
            engine.set_dark_mode(flag(&value, "on"));
            done()
        }
        other => refused(&format!("the caller may not use {}", other)),
    }
}

// One text of the JSON body. A missing text gives an empty string.
fn text_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

// One switch of the JSON body. A missing switch is off.
fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}
