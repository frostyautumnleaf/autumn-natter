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
                "role": m.role,
                "body": m.body,
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
