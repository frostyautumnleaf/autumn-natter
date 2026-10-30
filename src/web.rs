// The browser page of the remote mode, and the state that the page reads.
//
// The page is one HTML file with its CSS and JavaScript inside. It is put into
// the program when the program is built, so a browser needs nothing from the
// internet. The page asks for the state with a short poll, and it sends the
// actions of the user with a POST call.

use serde_json::{json, Value};

use crate::config::{Config, ModelSettings};
use crate::engine::Snap;
use crate::llama;
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

    // The mark of a chapter is not a message of the talk: it stands beside the
    // chat as a node on the chapter line, so it is kept out of the words.
    let chapters: Vec<Value> = snap
        .chapters
        .iter()
        .map(|one| {
            json!({
                "id": one.id,
                "name": one.name,
                // The chapter begins after this message. Empty for the chapter
                // at the very top of the chat.
                "after": one.after,
            })
        })
        .collect();
    // The summarisations of the open chat. The heading in the talk and the
    // timeline at the right of the chat are made of these: the number of the
    // summarisation, what became of it, and a little of what it says.
    let handoffs: Vec<Value> = snap
        .handoffs
        .iter()
        .map(|one| {
            json!({
                // The message that holds the summarisation. The heading stands
                // there in the talk, and a node is tied to it.
                "id": one.id,
                // "Summarised #N" is written from this.
                "number": one.number,
                // "writing", "done", or "owed" for a summary that was never
                // written and can be started again.
                "state": one.state,
                "when": store::format_day_clock(one.when),
                "before": one.before,
                "after": one.after,
                "snippet": one.snippet,
                "why": one.why,
            })
        })
        .collect();
    let messages: Vec<Value> = snap
        .messages
        .iter()
        .map(|m| {
            // The blocks of the attachments need the name and the kind of
            // each file, in the order the reader hung them. The words of a
            // file and the bytes of a picture do not stand here: the page
            // asks for one of them by its id when it draws the block.
            let attachments: Vec<Value> = m
                .attachments
                .iter()
                .map(|a| {
                    json!({
                        "id": a.id,
                        "name": a.name,
                        "mime": a.mime,
                        "image": a.image,
                    })
                })
                .collect();
            json!({
                // The id lets the page keep the rows it has and write again
                // only the words that changed.
                "id": m.id,
                "role": m.role,
                "body": m.body,
                "attachments": attachments,
                // The thinking of the model, when the model has any.
                "reasoning": m.reasoning,
                // How long that thinking took, in milliseconds.
                "reasoning_ms": m.reasoning_ms,
                "tool": m.tool,
                // Which tool this run was: the page puts the mark and the words
                // of that tool on the head of the block.
                "tool_name": m.tool_name,
                "mono": m.mono,
                "failed": m.failed,
                // A call the room gave out under. The block of it says what the
                // tool was for, and this tells the page that the writing of it was
                // stopped rather than the running of it failed.
                "cancelled": m.cancelled,
                // An answer of the model that never came to an end of its own,
                // because the reader stopped it. The page puts the mark of going on
                // with it under that answer alone.
                "stopped": m.stopped,
                // The files a run of the download tool offered to the reader. The
                // page draws a block for each of them at the end of the answer
                // that made the offer, and only once the model has stopped.
                "downloads": m.downloads.iter().map(|one| json!({
                    "name": one.name,
                    "extension": one.extension,
                    "kind": one.kind,
                    "path": one.path,
                    "size": one.size,
                })).collect::<Vec<Value>>(),
                "when": store::format_clock(m.when),
                // A summarisation carries the two numbers of the room it stood
                // for, and the day as well as the hour, for the card of it is
                // read long after the talk was cut.
                "context_before": m.context_before,
                "context_after": m.context_after,
                "when_full": if m.role == "Summary" {
                    store::format_day_clock(m.when)
                } else {
                    String::new()
                },
            })
        })
        .collect();
    // The calls of a tool the model is writing right now. The words of a
    // command are those read out of the call so far, for the call is seldom
    // finished. A mark of a chapter is never in this list, as it is never shown.
    let live_tools: Vec<Value> = snap
        .live_tools
        .iter()
        .map(|one| {
            json!({
                "name": one.name,
                "command": one.command,
                "purpose": one.purpose,
                "target": one.target,
                // The model has moved on from this call, so its words are all
                // in and the block is no longer being written.
                "done": one.done,
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
        // The first screen: the chat box on its own, with a line for this hour.
        // `Create new chat` in the side bar goes back to this screen.
        "home": snap.home,
        "greeting": snap.greeting,
        // The number the lists of the side bar are read again on. A chat that
        // came, went, moved, was renamed, or was opened moves it, so the page
        // asks for the lists when they changed and not on every look.
        "structure_rev": snap.structure_rev,
        "messages": messages,
        // The chapters the model marked, in the order it marked them. The line
        // beside the chat is made of these, and of nothing else.
        "chapters": chapters,
        // The summarisations of the open chat, oldest first. The line at the
        // right of the chat is made of these, and of nothing else.
        "handoffs": handoffs,
        // Compaction. How the room of the talk is kept short, and whether the
        // line of the summarisations stands beside the chat.
        "compaction": {
            "type": snap.chat_compaction,
            "by": snap.chat_compact_by,
            "percent": snap.chat_compact_percent,
            "reserve": snap.chat_compact_reserve,
            "prompt": snap.chat_compact_prompt,
            // The prompt of the second ask, the one that leans towards a short
            // summary when the first ran to the end of the room.
            "prompt_short": snap.chat_compact_prompt_short,
            "timeline": snap.chat_timeline,
        },
        // Text the model wrote since the last message was stored.
        "live": snap.live_text,
        // True while the program is writing a handoff summary. The words of a
        // summary are never shown, so the page draws no live answer while this
        // stands: the heading of the summarisation and the status line say that
        // the program is at work.
        "summarising": snap.summarising,
        // The calls the model is writing at this moment, in the order it writes
        // them. The window shows them as blocks of a command being written, so
        // the reader sees the words of it and does not wait for the run.
        "live_tools": live_tools,
        // The thinking that comes with a live answer, how long it lasted and
        // whether it has stopped. The page writes the last two on the head of
        // the panel.
        "reasoning": snap.live_reasoning,
        "reasoning_ms": snap.live_thought_ms,
        "reasoning_done": snap.live_thought_done,
        // How far the server is with the words it was sent. The gauge above the
        // chat box is made of these three: the number, whether it stands, and
        // what the program is working on. The page puts the words of the last
        // one together, as it does for the names of the thinking modes.
        "progress": {
            "percent": snap.progress,
            "active": snap.progress_on,
            // The two speeds of the work, in tokens of the talk to the second, as
            // the server measures them. The page shows the one of the two that
            // belongs to the work being done now: the reading of the words that
            // were sent while the gauge stands, and the writing after it. Both
            // are given to a tenth, so the number does not shiver as it moves.
            "reading": rate(snap.prompt_rate),
            "writing": rate(snap.answer_rate),
            "what": if snap.progress_images > 1 {
                "images"
            } else if snap.progress_images == 1 {
                "image"
            } else {
                "text"
            },
        },
        // The thinking mode of the open chat, and whether this model has one.
        "thinking": {
            "mode": &snap.thinking_mode,
            "available": snap.thinking_available,
            // The modes the badge offers, for the drop-down of the badge.
            "modes": &snap.thinking_modes,
            // Whether the thinking of the turns before the last answer goes
            // back to the model, and whether this model offers the choice.
            "preserve": snap.thinking_preserve,
            "preserve_available": snap.thinking_preserve_available,
        },
        "context": {
            "used": util::count(snap.context_used),
            "total": util::count(snap.context_total),
            "ratio": ratio(snap),
        },
        "generating": snap.generating,
        "busy": snap.busy,
        "code": snap.code_enabled,
        // The lines of the drop-down of the badge of the tools. The badge itself
        // is the switch over all of them, so a tool that is switched on here is
        // offered only while the badge stands on.
        "tool_switches": {
            "commands": snap.tool_switches.commands,
            "files": snap.tool_switches.files,
            "picture": snap.tool_switches.picture,
            "hand_out": snap.tool_switches.hand_out,
            "ask": snap.tool_switches.ask,
        },
        "search": snap.search_enabled,
        "sandbox": snap.sandbox_enabled,
        "sandbox_available": snap.sandbox_available,
        "sandbox_note": snap.sandbox_note,
        // The three settings of the sandbox, for the menu of the badge. All three
        // stand off, and each is opened on its own line of the menu.
        // The servers of the reader's own, for the menu of the MCP badge and
        // for the MCP tab of the settings. The words of the code of a server of
        // the program itself are kept out of here, for this is asked for about
        // six times a second: the sheet asks for them by a call of its own.
        "mcp": {
            "servers": snap.mcp.iter().map(|one| json!({
                "id": one.id,
                "name": one.name,
                "kind": one.kind,
                "icon": one.icon,
                "shown": one.shown,
                "on": one.on,
                "tools": one.tools,
                "fault": one.fault,
            })).collect::<Vec<Value>>(),
        },
        "sandbox_network": snap.sandbox_access.network,
        "sandbox_pip": snap.sandbox_access.pip,
        "sandbox_fetch": snap.sandbox_access.fetch,
        "status": snap.status,
        "dark": snap.dark_mode,
        // Chatting. How the page shows a thinking panel, a tool panel and a
        // block of code.
        "chatting": {
            "collapse_thinking": snap.chat_collapse_thinking,
            "grow_thinking": snap.chat_grow_thinking,
            "tools_in_thinking": snap.chat_tools_in_thinking,
            "collapse_tools": snap.chat_collapse_tools,
            "date_new_chat": snap.chat_date_new_chat,
            "highlight": snap.chat_highlight,
            "chapters": snap.chat_chapters,
            "paste_limit": snap.chat_paste_limit,
            "width": snap.chat_width,
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

// A speed of the work, to a tenth of a token to the second. The server measures
// the two speeds over the whole of the work it has done, and says them again on
// every piece of the answer; the tenth keeps the number the page shows from
// shivering under the reader. A speed that is nothing, or that makes no sense,
// reads as nothing, and the page then keeps the number away.
fn rate(per: f64) -> f64 {
    if !per.is_finite() || per <= 0.0 {
        return 0.0;
    }
    (per * 10.0).round() / 10.0
}

// The lists the side bar is made of: every repository with the chats that stand
// in it, the chats that belong to no repository, and the chats the reader put
// away. The page asks for these when a list changed, and not with every look at
// the state, for the chats are read from the disk and can be many.
//
// The chats of a group stand newest opened first, which is how the engine puts
// them. The page splits each group into the pinned chats, five chats, and what
// is left behind a `Show more`.
pub fn lists_value(engine: &crate::engine::Engine) -> Value {
    let open = engine.snapshot().chat_id;
    let lists = engine.lists();
    let line = |chat: &store::Chat| {
        json!({
            "id": chat.meta.id,
            "title": chat.meta.title,
            "repo": chat.meta.repo_id,
            // The line of the group the chat stands in, in words the reader of
            // the pinned group can use.
            "when": store::format_when(chat.meta.updated),
            "pinned": chat.meta.pinned,
            "archived": chat.meta.archived,
            "active": chat.meta.id == open,
            "messages": chat.messages.len(),
            "preview": store::preview(&chat.messages),
        })
    };
    json!({
        "repositories": lists
            .repositories
            .iter()
            .map(|(repo, chats)| json!({
                "id": repo.id,
                "name": repo.name,
                "pinned": repo.pinned,
                "chats": Value::Array(chats.iter().map(line).collect()),
            }))
            .collect::<Vec<_>>(),
        "loose": Value::Array(lists.loose.iter().map(line).collect()),
        "archived": Value::Array(lists.archived.iter().map(line).collect()),
    })
}

// ------------------------------------------------------------------ settings
//
// The calls of the sheet of settings. The desktop window makes them through the
// bridge of Tauri, and the page of the remote mode makes them through the small
// server; both come here, so the two faces cannot drift apart.

// The settings of the program, for the Program tab of the sheet.
pub fn config_value(engine: &crate::engine::Engine) -> Value {
    serde_json::to_value(engine.config()).unwrap_or_else(|_| json!({}))
}

// Store new settings, and refresh what depends on them.
pub fn save_config(engine: &crate::engine::Engine, config: Value) -> Value {
    match serde_json::from_value::<Config>(config) {
        Ok(cfg) => {
            engine.save_settings(cfg);
            done()
        }
        Err(problem) => refused(&format!(
            "The settings could not be read: {}",
            problem
        )),
    }
}

// Every repository on disk, most recently used first. The sheet shows all of
// them, while the top bar shows five.
pub fn repositories_value(engine: &crate::engine::Engine) -> Value {
    let repos = engine.repositories();
    Value::Array(
        repos
            .iter()
            .map(|repo| json!({ "id": repo.id, "name": repo.name }))
            .collect(),
    )
}

// Everything the Model tab needs: the settings of one model, the devices of the
// machine, and the three lists of choices.
pub fn model_settings_value(engine: &crate::engine::Engine, id: &str) -> Value {
    let snap = engine.snapshot();
    let (settings, gpus) = engine.model_settings(id);
    let (name, kind) = snap
        .models
        .iter()
        .find(|model| model.id() == id)
        .map(|model| (model.name.clone(), model.kind.to_string()))
        .unwrap_or_default();
    json!({
        "id": id,
        "name": name,
        "kind": kind,
        "settings": serde_json::to_value(&settings).unwrap_or_else(|_| json!({})),
        "gpus": gpus
            .iter()
            .map(|card| json!({
                "device": card.device,
                "name": card.name,
                "memory_mb": card.memory_mb,
                "enabled": card.enabled,
                "is_offload": card.is_offload,
                "memory_text": card.memory_text(),
            }))
            .collect::<Vec<_>>(),
        "kinds": llama::KV_TYPES.to_vec(),
        "loads": llama::LOAD_MODES.to_vec(),
        "specs": llama::SPEC_TYPES.to_vec(),
    })
}

// Save the settings of one model. A running server is stopped, because it was
// started with the old values.
pub fn save_model_settings(engine: &crate::engine::Engine, id: &str, settings: Value) -> Value {
    match serde_json::from_value::<ModelSettings>(settings) {
        Ok(made) => {
            engine.save_model_settings(id.to_string(), made);
            done()
        }
        Err(problem) => refused(&format!(
            "The model settings could not be read: {}",
            problem
        )),
    }
}

// The command line the next answer will use, read again whenever a field of the
// Model tab moves. Nothing is started: only the arguments are built. The answer
// is one line of words, and the notes under it.
pub fn preview_line(engine: &crate::engine::Engine, id: &str, settings: Value) -> Value {
    let snap = engine.snapshot();
    let Some(model) = snap.models.iter().find(|m| m.id() == id) else {
        return Value::String(String::new());
    };
    let Ok(held) = serde_json::from_value::<ModelSettings>(settings) else {
        return Value::String(String::new());
    };
    // The switches of the form win over the switches the model was saved with.
    let (_, devices) = engine.model_settings(id);
    let mut cards = devices;
    for (place, on) in held.gpu_enabled.iter().enumerate() {
        if let Some(card) = cards.get_mut(place) {
            card.enabled = *on;
        }
    }
    let mut notes = Vec::new();
    let args = llama::build_args(
        std::path::Path::new(""),
        model,
        &held,
        &cards,
        0,
        snap.code_enabled,
        snap.thinking_mode.as_str(),
        // The line shown in the sheet is the line of the chat in hand, so the
        // choice of keeping the thinking of the earlier turns stands in it too.
        snap.thinking_preserve_available.then_some(snap.thinking_preserve),
        &mut notes,
    );
    let mut line = String::from("llama-server ");
    for arg in args {
        if arg.contains(' ') {
            line.push_str(&format!("\"{arg}\""));
        } else {
            line.push_str(&arg);
        }
        line.push(' ');
    }
    for note in notes {
        line.push('\n');
        line.push_str(&note);
    }
    Value::String(line.trim_end().to_string())
}

// ------------------------------------------------------------------ the logs
//
// The logs of llama.cpp, for the Logs tab of the sheet of settings. They are
// asked for on their own and never with the state, for one log of a server that
// was told to say everything is far too big to read six times a second.

// Every log that is kept, newest first, with the folder they stand in and the
// version of the program that wrote them. The last two are of use when
// something is wrong: one says where to look, and the other says what ran.
pub fn logs_value() -> Value {
    json!({
        "dir": crate::logs::dir().display().to_string(),
        "version": env!("CARGO_PKG_VERSION"),
        // How many logs are kept. The words of the tab say that older ones were
        // thrown away.
        "kept": crate::logs::MOST_LOGS as u64,
        "logs": crate::logs::list()
            .iter()
            .map(|one| json!({
                // The name of the file, which the page asks with.
                "id": one.name,
                // The day and the hour the log stands at, and its size in words.
                "when": one.at,
                "size": one.size,
                "bytes": one.bytes,
                // True for the log this run of the program is writing, which is
                // the one the page reads again by itself.
                "current": one.current,
            }))
            .collect::<Vec<_>>(),
    })
}

// One log, from its end. A name that is not the name of a log of this program
// gives back nothing, so the page cannot ask for a file of the machine.
pub fn log_value(id: &str, most: Option<u64>) -> Value {
    // The bytes the page asked for, with the ceiling of the core over them. No
    // ask at all means the most that may be read.
    let want = most.unwrap_or(crate::logs::GREATEST_READ as u64);
    let cap = want.clamp(1, crate::logs::GREATEST_READ as u64) as usize;
    match crate::logs::read(id, cap) {
        Some(read) => json!({
            "id": id,
            "text": read.text,
            // The whole size of the log, and true when its front was left
            // behind because the log is longer than what is read.
            "bytes": read.bytes,
            "cut": read.cut,
            "read": crate::logs::size_text(read.text.len() as u64),
        }),
        None => refused("That log is not here"),
    }
}

// The servers of the reader's own, whole: what the MCP tab of the settings is
// filled from. The line that starts a server, the address of it, and the words
// of a server of the program itself all come here, and nowhere else, for the tab
// is opened now and then and the state is asked for always.
// One file the reader was offered, for the window that has no web server of its
// own. The name, the kind of the bytes, and the bytes as a data URL: the window
// hangs the data URL on a link and the file comes down, which is what the button
// of a block does over http as well. A name that leaves the room of the open chat
// answers with nothing, as it does on the road of the remote page.
pub fn gift_value(engine: &crate::engine::Engine, asked: &str) -> Value {
    match engine.gift(asked) {
        Some((name, kind, bytes)) => {
            let size = bytes.len();
            json!({
                "ok": true,
                "name": name,
                "mime": kind,
                "data": format!("data:{};base64,{}", kind, util::b64(&bytes)),
                "size": size,
            })
        }
        None => refused("That file is not here, or it does not stand in this chat"),
    }
}

// Several files the reader was offered, wound into one archive for the same
// window. The archive is made in the memory and is never written out, so taking
// the whole of an offer leaves no file of its own behind.
pub fn gift_zip_value(engine: &crate::engine::Engine, paths: &[String]) -> Value {
    match engine.gift_zip(paths) {
        Some((name, bytes)) => {
            let size = bytes.len();
            json!({
                "ok": true,
                "name": name,
                "mime": "application/zip",
                "data": format!("data:application/zip;base64,{}", util::b64(&bytes)),
                "size": size,
            })
        }
        None => refused("None of those files is here any more"),
    }
}

pub fn mcp_value(engine: &crate::engine::Engine) -> Value {
    let held = engine.mcp_servers();
    let known: Vec<Value> = held
        .iter()
        .map(|server| {
            json!({
                "id": server.id,
                "name": server.name,
                "kind": server.kind(),
                "command": server.command,
                "url": server.url,
                "code": server.code,
                "icon": server.icon,
                "shown": server.shown,
                "slug": server.slug,
            })
        })
        .collect();
    json!({
        "ok": true,
        "servers": known,
        // The icons the sheet offers, so the sheet and the program agree on how
        // many there are. The drawing of them stands in the page.
        "icons": crate::mcp::ICON_COUNT,
        // The Python the servers of the program itself are run with. The sheet
        // says what it found, so a machine without Python is known about.
        "python": crate::mcp::python(),
        // The words a server of the program itself begins with. The sheet puts
        // them in the field of the code when the kind is moved to that one, so
        // the example and the rules for an AI stand in one place, in the file
        // that holds the runner they describe.
        "template": crate::mcp::TEMPLATE,
    })
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
            // The files the reader hung on the message, in the order the
            // reader hung them. A message of files and no words is a real
            // message, so only a message of nothing at all is refused.
            let files = crate::attach::parse_incoming(
                value.get("attachments").cloned().unwrap_or(serde_json::Value::Null),
            );
            if text.trim().is_empty() && files.is_empty() {
                return refused("Nothing was written");
            }
            engine.prompt(text, files);
            done()
        }
        "/api/cancel" | "cancel" => {
            engine.cancel();
            done()
        }
        // Take one message out of the talk, with everything that came after it.
        // The page asks under a message, when the pointer goes over it.
        "/api/delete_message" | "delete_message" => {
            engine.remove_message(text_field(&value, "id"));
            done()
        }
        // Write the answer of a message again. The talk is cut back to the message
        // the reader is going on from, and the model is asked again from there.
        "/api/retry_message" | "retry_message" => {
            engine.retry_message(text_field(&value, "id"));
            done()
        }
        // Go on with an answer the reader stopped while the model was writing it.
        "/api/continue_message" | "continue_message" => {
            engine.continue_message(text_field(&value, "id"));
            done()
        }
        // Start the handoff summary of this chat again. The button for it stands
        // on the heading of a summarisation that was never written.
        "/api/handoff" | "handoff" => {
            engine.restart_handoff();
            done()
        }
        "/api/code" | "code" => {
            engine.set_code(flag(&value, "on"));
            done()
        }
        // One line of the drop-down of the badge of the tools. The line that was
        // clicked says which tool it switched, under `part`.
        "/api/tool_switch" | "tool_switch" => {
            let part = text_field(&value, "part");
            if part.is_empty() {
                return refused("That is not one of the tools of the program");
            }
            engine.set_tool_switch(&part, flag(&value, "on"));
            done()
        }
        "/api/search" | "search" => {
            engine.set_search(flag(&value, "on"));
            done()
        }
        "/api/sandbox" | "sandbox" => {
            engine.set_sandbox(flag(&value, "on"));
            done()
        }
        // One of the three settings of the sandbox. The line of the menu that was
        // clicked says which of them, under `part`.
        "/api/sandbox_setting" | "sandbox_setting" => {
            let Some(setting) = crate::engine::SandboxSetting::from_name(&text_field(&value, "part"))
            else {
                return refused("That is not one of the settings of the sandbox");
            };
            engine.set_sandbox_setting(setting, flag(&value, "on"));
            done()
        }
        // One server of the reader's own, switched on or off for this chat. The
        // menu of the badge sends the id of the server it drew.
        "/api/mcp_on" | "mcp_on" => {
            engine.set_mcp(&text_field(&value, "id"), flag(&value, "on"));
            done()
        }
        // The MCP tab of the settings: add a server, write one over, take one
        // away, move it in or out of the menu, or ask it again what it has.
        "/api/mcp" | "mcp" => {
            let op = text_field(&value, "op");
            match op.as_str() {
                "save" => {
                    let given: crate::mcp::Server = serde_json::from_value(
                        value.get("server").cloned().unwrap_or(json!({})),
                    )
                    .unwrap_or_default();
                    match engine.mcp_save(given) {
                        Ok(_) => done(),
                        Err(problem) => refused(&problem),
                    }
                }
                "remove" => match engine.mcp_remove(&text_field(&value, "id")) {
                    Ok(_) => done(),
                    Err(problem) => refused(&problem),
                },
                "shown" => {
                    engine.mcp_shown(&text_field(&value, "id"), flag(&value, "on"));
                    done()
                }
                "look" => {
                    engine.mcp_look(&text_field(&value, "id"));
                    done()
                }
                other => refused(&format!("Nothing is done by \"{}\"", other)),
            }
        }
        "/api/thinking" | "thinking" => {
            engine.set_thinking(&text_field(&value, "mode"));
            done()
        }
        "/api/preserve" | "preserve" => {
            engine.set_preserve(flag(&value, "on"));
            done()
        }
        "/api/new_chat" | "new_chat" => {
            engine.new_chat();
            done()
        }
        "/api/open_chat" | "open_chat" => {
            engine.open_chat(text_field(&value, "id"));
            done()
        }
        // The lists of the side bar. They do not come with the state, for the
        // chats are read from the disk and the state is read six times a second.
        "/api/lists" | "lists" => lists_value(engine),
        // The work of the side bar on one chat, by its id: another name, the
        // group it stands in, the place it belongs to, and taking it away.
        "/api/rename_chat" | "rename_chat" => {
            engine.rename_chat(text_field(&value, "id"), text_field(&value, "title"));
            done()
        }
        "/api/pin_chat" | "pin_chat" => {
            engine.set_chat_pinned(text_field(&value, "id"), flag(&value, "on"));
            done()
        }
        "/api/archive_chat" | "archive_chat" => {
            engine.set_chat_archived(text_field(&value, "id"), flag(&value, "on"));
            done()
        }
        "/api/move_chat" | "move_chat" => {
            engine.move_chat(text_field(&value, "id"), text_field(&value, "repo"));
            done()
        }
        "/api/delete_chat" | "delete_chat" => {
            engine.remove_chat(text_field(&value, "id"));
            done()
        }
        // The work of the side bar on one repository: another name, and the
        // group it stands in.
        "/api/rename_repo" | "rename_repo" => {
            engine.rename_repository(text_field(&value, "id"), text_field(&value, "name"));
            done()
        }
        "/api/pin_repo" | "pin_repo" => {
            engine.set_repository_pinned(text_field(&value, "id"), flag(&value, "on"));
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
        // The sheet of settings. The window of the program makes these calls
        // through its bridge, and the page of the remote mode through these
        // routes; both come to the same three functions above.
        "/api/config" | "config" => save_config(
            engine,
            value.get("config").cloned().unwrap_or(Value::Null),
        ),
        "/api/model_settings" | "model_settings" => save_model_settings(
            engine,
            &text_field(&value, "id"),
            value.get("settings").cloned().unwrap_or(Value::Null),
        ),
        "/api/preview_command" | "preview_command" => preview_line(
            engine,
            &text_field(&value, "id"),
            value.get("settings").cloned().unwrap_or(Value::Null),
        ),
        other => refused(&format!("The caller may not use {}", other)),
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
