// The browser page of the remote mode, and the state that the page reads.
//
// The page is one HTML file with its CSS and JavaScript inside. It is put into the program when
// the program is built, so a browser needs nothing from the internet. The page requests the
// state with a short poll, and it sends the actions of the user with a POST call.

use serde_json::{json, Value};

use crate::config::{self, Config, ModelSettings};
use crate::engine::Snap;
use crate::llama;
use crate::planning;
use crate::store;
use crate::util;

// The page itself. See web/page.html.
pub const PAGE: &str = include_str!("../web/page.html");

// The state as JSON. The page sends the number it already has (since). When nothing changed,
// the answer stays small.
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

    // The summarisations of the open chat. The heading in the conversation and the timeline at
    // the right of the chat are made of these: the number of the summarisation, what became of
    // it, and a little of what it reports.
    let handoffs: Vec<Value> = snap
        .handoffs
        .iter()
        .map(|one| {
            json!({
                // The message that holds the summarisation. The heading is there in the
                // conversation, and a node is tied to it.
                "id": one.id,
                // "Summarised #N" is written from this.
                "number": one.number,
                // "writing", "done", or "owed" for a summary that was never written and can be
                // started again.
                "state": one.state,
                "when": store::format_day_clock(one.when),
                "before": one.before,
                "after": one.after,
                "snippet": one.snippet,
                "why": one.why,
                // Which of the two asks the summary came from: the prompt of the settings, or
                // the short prompt asked again when the context window filled under the first.
                // The settings panel that shows the whole summary reports which of the two is
                // here.
                "short": one.short,
            })
        })
        .collect();
    let messages: Vec<Value> = snap
        .messages
        .iter()
        .map(|m| {
            // The blocks of the attachments need the name and the type of each file, in the
            // order the user hung them. The text of a file and the bytes of an image are not
            // here: the page requests one of them by its id when it draws the block.
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
                // The id lets the page keep the rows it has and write again only the text that
                // changed.
                "id": m.id,
                "role": m.role,
                "body": m.body,
                "attachments": attachments,
                // The thinking of the model, when the model has any.
                "reasoning": m.reasoning,
                // How long that thinking took, in milliseconds.
                "reasoning_ms": m.reasoning_ms,
                // Tool outcomes and the current card title in corporate display.
                "work": &m.work,
                "work_head": &m.work_head,
                "tool": m.tool,
                // Which tool this run was: the page puts the marker and the text of that tool
                // on the header of the block.
                "tool_name": m.tool_name,
                "mono": m.mono,
                "failed": m.failed,
                // A call the context window filled under. The block of it reports what the tool
                // was for, and this reports the page that the writing of it was stopped rather
                // than the running of it failed.
                "cancelled": m.cancelled,
                // An answer of the model that never finished of its own, because the user
                // stopped it.
                "stopped": m.stopped,
                // True on the one answer of the model that the page may propose to go on with:
                // the newest saying of the model, while the chat is at rest. The answer may
                // have been stopped by the user, or ended by the model itself on an
                // end-of-sequence token or the token limit.
                "can_continue": m.id == snap.continue_id,
                // The files a run of the download tool proposed to the user. The page draws a
                // block for each of them at the end of the answer that made the proposal, and
                // only once the model has stopped.
                "downloads": m.downloads.iter().map(|one| json!({
                    "name": one.name,
                    "extension": one.extension,
                    "kind": one.kind,
                    "path": one.path,
                    "size": one.size,
                })).collect::<Vec<Value>>(),
                // The files one run of the display tool put in the window. The page draws a
                // block with one button, and the button opens the window of a file over these
                // files, in this order.
                "displayed": m.displayed.iter().map(|one| json!({
                    "name": one.name,
                    "extension": one.extension,
                    "shape": one.shape,
                    "path": one.path,
                    "size": one.size,
                })).collect::<Vec<Value>>(),
                // The next steps one run of the suggestion tool proposed. The page draws a
                // button for each of them at the end of the answer that made the proposal, and
                // only once the model has stopped writing. A press sends the prompt as a
                // message of the user.
                "suggestions": m.suggestions.iter().map(|one| json!({
                    "label": one.label,
                    "prompt": one.prompt,
                })).collect::<Vec<Value>>(),
                // The steps of the work one run of the steps tool showed. The page draws one
                // block at the end of the answer that made the call, above the other blocks an
                // answer ends with, and shows one step at a time.
                "steps": m.steps.as_ref().map(|one| json!({
                    "title": one.title,
                    "steps": one.steps.iter().map(|step| json!({
                        "label": step.label,
                        "content": step.content,
                    })).collect::<Vec<Value>>(),
                })),
                "when": store::format_clock(m.when),
                // A summarisation carries the two numbers of the space it was for, and the
                // day as well as the hour, for the card of it is read long after the
                // conversation was cut.
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
    // The calls of a tool the model is writing right now. The text of a command is those
    // extracted from the call so far, for the call is seldom finished.
    let live_tools: Vec<Value> = snap
        .live_tools
        .iter()
        .map(|one| {
            json!({
                "name": one.name,
                "command": one.command,
                "purpose": one.purpose,
                "target": one.target,
                // The model has moved on from this call, so its text is all in and the block is
                // no longer being written.
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
            // Whether the model stands loaded in a run right now, which is not the same question
            // as which model the settings name: between the sending away of a model and the next
            // message the settings still name one and nothing of it is in the memory.
            "loaded": snap.model_loaded,
        },
        "models": models,
        "repository": { "id": snap.repository_id, "name": snap.repository_name },
        "repositories": repositories,
        "chat": {
            "id": snap.chat_id,
            "title": snap.chat_title,
            // Planning mode. The heading over the conversation and the border of the chat
            // composer come from these two: one reports the chat is planning now, and the other
            // that a plan was let go and may still be read.
            "planning": snap.planning,
            "planned": snap.planned,
        },
        // The commands a user may write in the chat composer. The list of them pops up when the
        // marker of a command is typed, and each carries what it is for.
        "commands": planning::list(),
        // The first screen: the chat composer on its own, with a line for this hour. `Create
        // new chat` in the side bar goes back to this screen.
        "home": snap.home,
        "greeting": snap.greeting,
        // The number the lists of the side bar are read again on. A chat that came, went,
        // moved, was renamed, or was opened moves it, so the page requests the lists when they
        // changed and not on every look.
        "structure_rev": snap.structure_rev,
        "messages": messages,
        // The summarisations of the open chat, oldest first. The line at the right of the chat
        // is made of these, and of nothing else.
        "handoffs": handoffs,
        // Compaction. How the size of the context is kept short, and whether the line of the
        // summarisations is beside the chat.
        "compaction": {
            "type": snap.chat_compaction,
            "by": snap.chat_compact_by,
            "percent": snap.chat_compact_percent,
            "reserve": snap.chat_compact_reserve,
            "prompt": snap.chat_compact_prompt,
            // The prompt of the second ask, the one that leans towards a short summary when the
            // first ran to the end of the space.
            "prompt_short": snap.chat_compact_prompt_short,
            "timeline": snap.chat_timeline,
        },
        // Text the model wrote since the last message was stored.
        "live": snap.live_text,
        // True while the program is writing a handoff summary. The text of a summary is never
        // shown, so the page draws no live answer while this remains: the heading of the
        // summarisation and the status line say that the program is at work.
        "summarising": snap.summarising,
        // The calls the model is writing at this moment, in the order it writes them. The
        // window shows them as blocks of a command being written, so the user sees the text of
        // it and does not wait for the run.
        "live_tools": live_tools,
        // Tool outcomes and the current card title in corporate display.
        "live_work": snap.live_work,
        "live_work_head": snap.live_work_head,
        // The question the model has put to the user and is waiting on, when it has put one.
        // The popup of the request is above the chat composer, and the user answers it there:
        // by choosing one of the answers the model proposed, or by writing an answer of their
        // own.
        "ask": snap.ask.as_ref().map(|one| json!({
            "id": one.id,
            "question": one.question,
            "choices": one.choices,
            "critical": one.critical,
            // The seconds left to answer, or nothing when the question waits until it is
            // answered.
            "left": one.left(),
            "seconds": one.seconds,
            // A questionnaire of planning mode carries its questions, and the marker that shows
            // it is one: it is in the conversation itself, the user chooses what they like of
            // every question, and nothing goes until they send all of it.
            "plan": one.plan,
            "items": one.items.iter().map(|item| json!({
                "question": item.prompt,
                "choices": item.choices,
                "many": item.many,
            })).collect::<Vec<_>>(),
        })),
        // The call of a tool that is running right now, with all of the time it was given and
        // the seconds it has left. The header of the block of the call wears the clock and the
        // two markers of a running call out of this. A call that was given no time to return
        // within carries nothing in either number, and wears neither clock nor marker.
        "running": snap.running.as_ref().map(|one| json!({
            "id": one.id,
            "timeout": one.timeout,
            "left": one.left,
        })),
        // The messages waiting their turn in this chat. The list is kept by the program and
        // belongs to the chat, so a page that is shut and opened again finds the rows as they
        // were left, and the messages go out when the answer each of them waited for is over
        // with no page watching it. The next message to go out is at the start of `items`; the
        // rows are written newest first, which is the other way about. The programmed messages
        // are a list of their own, under the same header, and each carries the text it waits for
        // so that its row can say what it is waiting for. A row left out of this JSON is a row
        // the page never draws: the programmed messages once stood in that state, added to the
        // list by the engine and unseen by the user.
        "queue": snap.queue.as_ref().map(|one| {
            let row = |one: &crate::engine::QueuedRow| json!({
                "id": one.id,
                "text": one.text,
                "files": one.files,
                "matches": one.matches,
            });
            json!({
                "chat": one.chat,
                "held": one.held,
                "items": one.items.iter().map(row).collect::<Vec<Value>>(),
                "planned": one.planned.iter().map(row).collect::<Vec<Value>>(),
            })
        }),
        // The thinking that comes with a live answer, how long it lasted and whether it has
        // stopped. The page writes the last two on the header of the panel.
        "reasoning": snap.live_reasoning,
        "reasoning_ms": snap.live_thought_ms,
        "reasoning_done": snap.live_thought_done,
        // How far the server is with the text it was sent. The meter above the chat composer is
        // made of these three: the number, whether it remains, and what the program is working
        // on. The page puts the text of the last one together, as it does for the names of the
        // thinking modes.
        "progress": {
            "percent": snap.progress,
            "active": snap.progress_on,
            // The two speeds of the work, in tokens of the conversation to the second, as the
            // server measures them. The page shows the one of the two that belongs to the work
            // being done now: the reading of the text that was sent while the meter remains,
            // and the writing after it. Both are given to a tenth, so the number does not
            // shiver as it moves.
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
            // The modes the badge proposes, for the drop-down of the badge.
            "modes": &snap.thinking_modes,
            // Whether the thinking of the turns before the last answer goes back to the model,
            // and whether this model presents the choice.
            "preserve": snap.thinking_preserve,
            "preserve_available": snap.thinking_preserve_available,
        },
        "context": {
            "used": util::count(snap.context_used),
            "total": util::count(snap.context_total),
            "ratio": ratio(snap),
            // Where the summarisation of the handoff begins, as a part of the whole space,
            // so the page has only to put its line at that share of the meter. Zero reports
            // there is nothing to marker.
            "cut": cut_ratio(snap),
        },
        "generating": snap.generating,
        // The chats that are being answered, by their id. More than one chat may be answered at
        // once when the model was started with space for that many answers, so the side bar
        // wears a marker on every row that is at work and not on the open chat alone.
        "working": snap.working,
        "busy": snap.busy,
        "code": snap.code_enabled,
        // The lines of the drop-down of the badge of the tools. The badge itself is the switch
        // over all of them, so a tool that is switched on here is proposed only while the badge
        // is on. The map is made from the same list the window of the program reads, so a line
        // cannot be in one and not the other: a line the page does not read is drawn as switched
        // on, for a switch nothing reports is taken for on, and a click on it then sends it off.
        "tool_switches": tool_switches_value(&snap.tool_switches),
        "search": snap.search_enabled,
        // Retrieval: the badge of the chat, and what the `.sources` folder of its working
        // directory holds. The badge is drawn from these: a folder with nothing in it, or files
        // that are stored but not counted, is shown on the badge rather than left for a search to
        // report.
        "rag": {
            "enabled": snap.rag.enabled,
            "files": snap.rag.files,
            "chunks": snap.rag.chunks,
            "pending": snap.rag.pending,
            "ready": snap.rag.ready,
            "vision": snap.rag.vision,
        },
        "sandbox": snap.sandbox_enabled,
        "sandbox_available": snap.sandbox_available,
        "sandbox_note": snap.sandbox_note,
        // The three settings of the sandbox, for the menu of the badge. All three remain off,
        // and each is opened on its own line of the menu. The servers of the user's own, for
        // the menu of the MCP badge and for the MCP tab of the settings. The text of the code
        // of a server of the program itself are kept out of here, for this is requested about
        // six times a second: the settings panel requests them by a call of its own.
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
        // The theme of the pages: `light`, `dark`, `silver`, `corporate` or `neon`. The word of
        // the theme travels beside the switch of the two that was before the third, so a page
        // written for the two of them still reads a theme.
        "theme": snap.theme,
        // Whether that theme is drawn in its pixel-art form.
        "theme_pixel": snap.theme_pixel,
        // Chatting. How the page shows a thinking panel, a tool panel and a block of code.
        "chatting": {
            "collapse_thinking": snap.chat_collapse_thinking,
            "grow_thinking": snap.chat_grow_thinking,
            "tools_in_thinking": snap.chat_tools_in_thinking,
            "collapse_tools": snap.chat_collapse_tools,
            "date_new_chat": snap.chat_date_new_chat,
            "highlight": snap.chat_highlight,
            "bold_markdown": snap.chat_bold_markdown,
            // The colours the model requests: painted on the text while this is on, and read as
            // plain text while it remains off.
            "colour_text": snap.chat_colour_text,
            "seamless": snap.chat_seamless,
            // The corporate way of showing the work. The page hides the thoughts and the calls
            // when it remains, and draws one block for an answer instead.
            "corporate": snap.chat_display == config::CORPORATE_DISPLAY,
            // True once the first-run screen has been removed through.
            "welcomed": snap.welcomed,
            "silent_retries": snap.chat_silent_retries,
            // The line of the user's own messages in the left gutter beside the sidebar.
            "userline": snap.chat_userline,
            // Whether the background of the chat is a faint grid. Only the page draws it.
            "grid": snap.chat_grid,
            // What falls behind the text of the chat. The page draws it, and stops again when
            // the drawing is turned off.
            "background": &snap.chat_background,
            // Whether the rain splashes where it meets the chat composer box and the blocks of
            // the chat. Only the page draws it.
            "rain_splash": snap.chat_rain_splash,
            // Whether the cursor uncovers a field of random digits in the background of the
            // chat. Only the page draws them.
            "cursor_digits": snap.chat_cursor_digits,
            // Whether a message you sent is carried into the chat with a little movement. The
            // movement is all on the screen, so the engine only reports that the switch
            // remains.
            "send_anim": snap.chat_send_anim,
            // Whether the conversation a chat worked through is kept in a file, so that it does
            // not have to be read through again. The settings panel shows the switch; the
            // keeping itself is all behind the request, and the page has nothing else to do
            // with it.
            "context_files": snap.chat_context_files,
            // The same for the compressed form of those files: the settings panel shows the
            // switch, and the page does nothing else with it.
            "context_compression": snap.chat_context_compression,
            "paste_limit": snap.chat_paste_limit,
            "width": snap.chat_width,
            // How long the model waits for an answer to a question it put to the user: the
            // short wait of an ordinary question, and the wait of one the model cannot go on
            // without.
            "ask_seconds": snap.ask_seconds,
            "ask_critical_seconds": snap.ask_critical_seconds,
        },
        // The program directory data. The window of settings reads it out.
        "data_dir": snap.data_dir,
        // The folder the contexts the model has processed are in.
        "contexts_dir": snap.contexts_dir,
    })
}

// The lines of the badge of the tools as the page reads them: one entry for every line of the
// drop-down, from `store::ToolSwitches::words`, which is the list the window of the program is
// given too. The order of the lines is the order of the drop-down.
fn tool_switches_value(switches: &store::ToolSwitches) -> Value {
    let mut held = serde_json::Map::new();
    for (part, on) in switches.words() {
        held.insert(part.to_string(), json!(on));
    }
    Value::Object(held)
}

// Part of the context meter, between 0 and 1.
fn ratio(snap: &Snap) -> f64 {
    if snap.context_total == 0 {
        return 0.0;
    }
    (snap.context_used as f64 / snap.context_total as f64).clamp(0.0, 1.0)
}

// Where the summarisation begins, as a part of the whole directory. Zero reports that nothing
// is at the place: the summarisation is off, or the space is not known.
fn cut_ratio(snap: &Snap) -> f64 {
    if snap.context_total == 0 || snap.context_cut == 0 || snap.context_cut >= snap.context_total {
        return 0.0;
    }
    (snap.context_cut as f64 / snap.context_total as f64).clamp(0.0, 1.0)
}

// A speed of the work, to a tenth of a token to the second. The server measures the two speeds
// over all of the work it has done, and reports them again on every piece of the answer; the
// tenth keeps the number the page shows from shivering under the user. A speed that is nothing,
// or that makes no sense, reads as nothing, and the page then keeps the number away.
fn rate(per: f64) -> f64 {
    if !per.is_finite() || per <= 0.0 {
        return 0.0;
    }
    (per * 10.0).round() / 10.0
}

// The lists the side bar is made of: every repository with the chats that are in it, the chats
// that belong to no repository, and the chats the user put away. The page requests these when a
// list changed, and not with every look at the state, for the chats are read from the disk and
// can be many.
//
// The chats of a group remain newest opened first, which is how the engine puts them. The page
// splits each group into the pinned chats, five chats, and what is left behind a `Show more`.
pub fn lists_value(engine: &crate::engine::Engine) -> Value {
    let open = engine.snapshot().chat_id;
    let lists = engine.lists();
    let line = |chat: &store::Chat| {
        json!({
            "id": chat.meta.id,
            "title": chat.meta.title,
            "repo": chat.meta.repo_id,
            // The line of the group the chat is in, in text the user of the pinned group can
            // use.
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
// The calls of the settings panel. The desktop window makes them through the bridge of Tauri,
// and the page of the remote mode makes them through the small server; both come here, so the
// two pages cannot drift apart.

// The settings of the program, for the Program tab of the settings panel.
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

// The lines of the pre-written prompts, for the Program tab of the settings: the id, the name
// and the short account of each. The text of a prompt is not in these lines. The page requests
// the text of one prompt when the user opens it to read, by the same call with the id of the
// prompt in it.
pub fn prompts_value(id: &str) -> Value {
    if id.is_empty() {
        return json!({ "lines": crate::prompts::lines() });
    }
    match crate::prompts::words(id) {
        Some(words) => json!({
            "id": id,
            "name": crate::prompts::find(id).map(|p| p.name).unwrap_or(id),
            "words": words,
        }),
        None => refused("No prompt of that name"),
    }
}

// Every repository on disk, most recently used first. The settings panel shows all of them,
// while the top bar shows five.
pub fn repositories_value(engine: &crate::engine::Engine) -> Value {
    let repos = engine.repositories();
    Value::Array(
        repos
            .iter()
            .map(|repo| json!({ "id": repo.id, "name": repo.name }))
            .collect(),
    )
}

// Everything the Model tab needs: the settings of one model, the devices of the computer, and
// the three lists of choices.
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
        "specs": llama::spec_names(),
        // The forms of the settings that the user has named. They come with the settings of the
        // model, for the tab that shows the one shows the other, and a preset is nothing but
        // settings with a name was before them.
        "presets": engine
            .model_presets()
            .into_iter()
            .map(|(name, held)| {
                json!({
                    "name": name,
                    "settings": serde_json::to_value(&held).unwrap_or_else(|_| json!({})),
                })
            })
            .collect::<Vec<_>>(),
    })
}

// Save the settings of one model. A running server is stopped, because it was started with the
// old values.
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

// The command line the next answer will use, read again whenever a field of the Model tab
// moves. Nothing is started: only the arguments are built. The answer is one line of text, and
// the notes under it.
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
        true,
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
    // The thinking is no part of the command line: it rides on every ask. The line of the
    // current chat reports what the next ask will carry, so that the settings panel shows all
    // of what the model is given.
    let thought = llama::thinking_kwargs(
        snap.thinking_mode.as_str(),
        snap.thinking_preserve_available.then_some(snap.thinking_preserve),
    );
    if !thought.is_null() {
        notes.push(format!(
            "Every ask carries chat_template_kwargs: {}",
            thought
        ));
    }
    for note in notes {
        line.push('\n');
        line.push_str(&note);
    }
    Value::String(line.trim_end().to_string())
}

// ------------------------------------------------------------------ the logs
//
// The logs, for the Logs tab of the settings panel: the output of llama-server, and the log the
// program writes of its own work. They are requested on their own and never with the state, for
// one log of a server that was set to say everything is far too big to read six times a second.
// The type of the log comes with the request, so the two pages of the tab ask the same way.

// Every log of one type that is kept, newest first, with the folder they are in and the version
// of the program that wrote them. The last two are of use when something is wrong: one reports
// where to look, and the other reports what ran.
pub fn logs_value(kind: crate::logs::Kind) -> Value {
    json!({
        // Which type of log this list is, in the one word the page asked with. The page knows
        // the type it requested, but the answer of a request that returned after the tab had
        // moved on to the other type is deleted by it.
        "kind": kind.word(),
        "dir": crate::logs::dir().display().to_string(),
        "version": env!("CARGO_PKG_VERSION"),
        // How many logs of one type are kept. The text of the tab reports that older ones was
        // deleted.
        "kept": crate::logs::MOST_LOGS as u64,
        "logs": crate::logs::list(kind)
            .iter()
            .map(|one| json!({
                // The name of the file, which the page asks with.
                "id": one.name,
                // The date and the time the log is at, and its size in text.
                "when": one.at,
                "size": one.size,
                "bytes": one.bytes,
                // True for the log this run of the program is writing, which is the one the
                // page reads again by itself.
                "current": one.current,
            }))
            .collect::<Vec<_>>(),
    })
}

// One log, from its end. A name that is not the name of a log of this type of this program
// returns nothing, so the page cannot request a file of the computer.
pub fn log_value(kind: crate::logs::Kind, id: &str, most: Option<u64>) -> Value {
    // The bytes the page requested, with the ceiling of the core over them. No request at all
    // means the most that may be read.
    let want = most.unwrap_or(crate::logs::GREATEST_READ as u64);
    let cap = want.clamp(1, crate::logs::GREATEST_READ as u64) as usize;
    match crate::logs::read(kind, id, cap) {
        Some(read) => json!({
            "id": id,
            // Which type of log this text belongs to.
            "kind": kind.word(),
            "text": read.text,
            // The whole size of the log, and true when its front was left behind because the
            // log is longer than what is read.
            "bytes": read.bytes,
            "cut": read.cut,
            "read": crate::logs::size_text(read.text.len() as u64),
        }),
        None => refused("That log is not here"),
    }
}

// The servers of the user's own, whole: what the MCP tab of the settings is filled from. The
// line that starts a server, the address of it, and the text of a server of the program itself
// all come here, and nowhere else, for the tab is opened now and then and the state is
// requested always. One file the user was proposed, for the window that has no web server of
// its own. The name, the type of the bytes, and the bytes as a data URL: the window hangs the
// data URL on a link and the file drops, which is what the button of a block does over http as
// well. A name that leaves the working directory of the open chat answers with nothing, as it does on
// the path of the remote page.
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
        None => refused("That file is not here, or it is not in this chat"),
    }
}

// The bytes of one file of a show, for the window over the contents of a file. The window
// requests the file by the name the block carries, and the rule of the working directory follows the
// state of the sandbox of the open chat. The window answers with the bytes, the type of them
// and the name, as the gift answers.
pub fn display_value(engine: &crate::engine::Engine, asked: &str) -> Value {
    match engine.display_of(asked) {
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
        None => refused("That file is not here, or it is not a type the window shows"),
    }
}

// Several files the user was proposed, wound into one archive for the same window. The archive
// is created in the memory and is never written out, so taking all of an propose leaves no file
// of its own behind.
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
        // The icons the settings panel proposes, so the settings panel and the program agree on
        // how many there are. The drawing of them is in the page.
        "icons": crate::mcp::ICON_COUNT,
        // The Python the servers of the program itself are run with. The settings panel reports
        // what it found, so a computer without Python is known about.
        "python": crate::mcp::python(),
        // The text a server of the program itself begins with. The settings panel puts it in
        // the field of the code when the type is moved to that one, so the example and the
        // rules for an AI are in one place, in the file that holds the runner they describe.
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

// One call of the user interface. The page of the remote mode and the buttons of the desktop
// window send the same names, so the two go through here. Every call is answered with a small
// JSON record, and the caller then reads the state again.
pub fn action(engine: &crate::engine::Engine, path: &str, body: &str) -> Value {
    let value = serde_json::from_str::<Value>(body).unwrap_or_else(|_| json!({}));
    match path {
        "/api/send" | "send" => {
            let text = text_field(&value, "text");
            // The files the user hung on the message, in the order the user hung them. A
            // message of files and no text is a real message, so only a message of nothing at
            // all is refused.
            let files = crate::attach::parse_incoming(
                value.get("attachments").cloned().unwrap_or(serde_json::Value::Null),
            );
            if text.trim().is_empty() && files.is_empty() {
                return refused("Nothing was written");
            }
            engine.prompt(text, files);
            done()
        }
        // The waiting list. A message written while the model is writing waits in the program
        // and not in the page: the list belongs to the chat, and the worker sends the messages
        // when the answer each of them waited for is over, whether or not a page is left open
        // to watch it. The rows are read back in the state, so a page opened again finds the
        // list as it was left.
        "/api/queue_add" | "queue_add" => {
            let text = text_field(&value, "text");
            let files = crate::attach::parse_incoming(
                value.get("attachments").cloned().unwrap_or(serde_json::Value::Null),
            );
            match engine.queue_add(text, files) {
                Ok(()) => done(),
                Err(problem) => refused(&problem),
            }
        }
        // A programmed message: the text of the message, and the text it waits for. The message
        // is sent when an answer of this chat holds them, ahead of the messages that were only
        // put in the list. The text is matched without regard to the bigness of the letters.
        "/api/queue_plan" | "queue_plan" => {
            let text = text_field(&value, "text");
            let waits_for = text_field(&value, "match");
            let files = crate::attach::parse_incoming(
                value.get("attachments").cloned().unwrap_or(serde_json::Value::Null),
            );
            match engine.queue_plan(text, files, waits_for) {
                Ok(()) => done(),
                Err(problem) => refused(&problem),
            }
        }
        "/api/queue_remove" | "queue_remove" => {
            let Some(id) = value.get("id").and_then(|one| one.as_u64()) else {
                return refused("No message was named");
            };
            if engine.queue_remove(id) {
                done()
            } else {
                refused("That message is not in the list any more")
            }
        }
        "/api/queue_edit" | "queue_edit" => {
            let Some(id) = value.get("id").and_then(|one| one.as_u64()) else {
                return refused("No message was named");
            };
            let text = text_field(&value, "text");
            if text.trim().is_empty() {
                return refused("Nothing was written");
            }
            if engine.queue_edit(id, text) {
                done()
            } else {
                refused("That message is not in the list any more")
            }
        }
        // The place requested is counted in the order the messages go out in, where 0 is the
        // next to go. The rows of the list are written the other way about, and the page turns
        // that around before asking.
        "/api/queue_move" | "queue_move" => {
            let Some(id) = value.get("id").and_then(|one| one.as_u64()) else {
                return refused("No message was named");
            };
            let Some(at) = value.get("index").and_then(|one| one.as_u64()) else {
                return refused("No place was named");
            };
            if engine.queue_move(id, at as usize) {
                done()
            } else {
                refused("That message is not in the list any more")
            }
        }
        "/api/cancel" | "cancel" => {
            engine.cancel();
            done()
        }
        // The user answered the question the model put with the tool of that name. The id is
        // the one the request carried, so an answer that comes late -- after the question was
        // given up, or after another answer took it -- does nothing at all.
        "/api/ask_answer" | "ask_answer" => {
            let id = text_field(&value, "id");
            let answer = text_field(&value, "answer");
            if answer.trim().is_empty() {
                return refused("Nothing was written");
            }
            if !engine.answer_ask(&id, &answer) {
                return refused("That question is answered already, or is over");
            }
            done()
        }
        // The user sent the answers of a questionnaire of planning mode. Each question carries
        // the answers they chose for it, and the answers are in the order the questions remain.
        // A question the user wrote text for carries that text as its one answer.
        "/api/plan_answers" | "plan_answers" => {
            let id = text_field(&value, "id");
            let mut answers: Vec<Vec<String>> = Vec::new();
            if let Some(list) = value.get("answers").and_then(|made| made.as_array()) {
                for one in list {
                    let mut chosen: Vec<String> = Vec::new();
                    if let Some(words) = one.as_array() {
                        for word in words {
                            if let Some(text) = word.as_str() {
                                if !text.trim().is_empty() {
                                    chosen.push(text.trim().to_string());
                                }
                            }
                        }
                    } else if let Some(text) = one.as_str() {
                        if !text.trim().is_empty() {
                            chosen.push(text.trim().to_string());
                        }
                    }
                    answers.push(chosen);
                }
            }
            if answers.iter().all(|one| one.is_empty()) {
                return refused("Nothing was chosen");
            }
            if !engine.answer_plan_ask(&id, answers) {
                return refused("That questionnaire is answered already, or is over");
            }
            done()
        }
        // The user truncated the call of a tool that is running, from the header of its block.
        // The command that was started is stopped, and everything it had printed by then still
        // goes back to the model.
        "/api/tool_cancel" | "tool_cancel" => {
            engine.cancel_running_tool();
            done()
        }
        // The user gave the call of a tool that is running more time to return within, from the
        // header of its block. Each press adds the same time, and a call may be given more time
        // as often as the user likes.
        "/api/tool_extend" | "tool_extend" => {
            engine.extend_running_tool();
            done()
        }
        // Take one message out of the conversation, with everything that came after it. The
        // page asks under a message, when the pointer goes over it.
        "/api/delete_message" | "delete_message" => {
            engine.remove_message(text_field(&value, "id"));
            done()
        }
        // Take a message of the user back out of the conversation and into the chat composer,
        // with everything that came after it. The page has read the files hung on the message
        // before it asks, so those files are left where they are and go out again with the
        // text.
        "/api/rewrite_message" | "rewrite_message" => {
            engine.rewrite_message(text_field(&value, "id"));
            done()
        }
        // Write the text of an answer again, where they are. The conversation around it does
        // not move, for the user is putting right what the model stated.
        "/api/edit_answer" | "edit_answer" => {
            engine.edit_answer(text_field(&value, "id"), text_field(&value, "body"));
            done()
        }
        // Write the answer of a message again. The conversation is cut back to the message the
        // user is going on from, and the model is asked again from there.
        "/api/retry_message" | "retry_message" => {
            engine.retry_message(text_field(&value, "id"));
            done()
        }
        // Go on with an answer the user stopped while the model was writing it.
        "/api/continue_message" | "continue_message" => {
            engine.continue_message(text_field(&value, "id"));
            done()
        }
        // Start the handoff summary of this chat again. The button for it is on the heading of
        // a summarisation that was never written.
        "/api/handoff" | "handoff" => {
            engine.restart_handoff();
            done()
        }
        "/api/code" | "code" => {
            engine.set_code(flag(&value, "on"));
            done()
        }
        // One line of the drop-down of the badge of the tools. The line that was clicked
        // reports which tool it switched, under `part`.
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
        // Retrieval. The badge of the composer switches it for this chat and for the next one, as
        // every other badge does.
        "/api/rag" | "rag" => {
            engine.set_rag(flag(&value, "on"));
            done()
        }
        // The embedding model of the sources, for the section at the top of the Program tab: what
        // is chosen, what is in the folder, and how a download is going.
        "/api/embed_state" | "embed_state" => engine.embedding_state(),
        // The choice of the model, its quantization and its vision projector, and the download of
        // the files when the user pressed Install. Without `install` the choice alone is written,
        // which is what a change of the selects does.
        "/api/embed_install" | "embed_install" => engine.choose_embedding(
            &text_field(&value, "model"),
            &text_field(&value, "quant"),
            &text_field(&value, "mmproj"),
            true,
        ),
        "/api/embed_choose" | "embed_choose" => engine.choose_embedding(
            &text_field(&value, "model"),
            &text_field(&value, "quant"),
            &text_field(&value, "mmproj"),
            false,
        ),
        // How the counting is to be run: whether the card of the computer may be used at all, the
        // batch of tokens, the threads and the length of one ask. The four words are written into
        // the settings; a server that is running under the old words is left to finish what it is
        // at and started again with the new ones when it is next asked for.
        "/api/embed_run" | "embed_run" => engine.set_embedding_run(&value),
        // The sources of the open chat, for the pop-up of the sources: the files, their state, the
        // folder they are in, and the job that may be going on over them.
        "/api/sources_list" | "sources_list" => engine.sources_value(),
        // A drop of files. The bytes come as data URLs, for that is how a page reads a file it was
        // given, and the writing of them and the counting of their words go on in a thread of
        // their own while the pop-up watches the job.
        "/api/sources_add" | "sources_add" => engine.sources_add(&value),
        // Take sources out of the index. With no path in the call, all of them go; the files
        // themselves stay in the folder.
        "/api/sources_remove" | "sources_remove" => {
            let paths: Vec<String> = value
                .get("paths")
                .and_then(|one| one.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|one| one.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            engine.sources_remove(&paths)
        }
        // Count the files that are stored and not counted. This is the button of the pop-up, and
        // it is what happens by itself after an embedding model is installed.
        "/api/sources_count" | "sources_count" => engine.sources_count(),
        // Give up the job the sources, or the download of the embedding model, are at.
        "/api/sources_cancel" | "sources_cancel" => engine.sources_cancel(),
        "/api/sandbox" | "sandbox" => {
            engine.set_sandbox(flag(&value, "on"));
            done()
        }
        // One of the three settings of the sandbox. The line of the menu that was clicked
        // reports which of them, under `part`.
        "/api/sandbox_setting" | "sandbox_setting" => {
            let Some(setting) = crate::engine::SandboxSetting::from_name(&text_field(&value, "part"))
            else {
                return refused("That is not one of the settings of the sandbox");
            };
            engine.set_sandbox_setting(setting, flag(&value, "on"));
            done()
        }
        // One server of the user's own, switched on or off for this chat. The menu of the badge
        // sends the id of the server it drew.
        "/api/mcp_on" | "mcp_on" => {
            engine.set_mcp(&text_field(&value, "id"), flag(&value, "on"));
            done()
        }
        // The MCP tab of the settings: add a server, write one over, take one away, move it in
        // or out of the menu, or ask it again what it has.
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
        // The presets of the Model tab. A form of the settings is kept under a name of the
        // user's own, or a name is deleted. Loading a preset is nothing of the program's: the
        // page puts the text of one into the fields of the tab, and the user writes them to the
        // model with the Save button, as they would any other text of the tab.
        "/api/model_preset" | "model_preset" => {
            let name = text_field(&value, "name");
            match text_field(&value, "op").as_str() {
                "save" => {
                    let held = value.get("settings").cloned().unwrap_or(Value::Null);
                    match serde_json::from_value::<ModelSettings>(held) {
                        Ok(made) => match engine.model_preset_save(name, made) {
                            Ok(kept) => json!({ "ok": true, "name": kept }),
                            Err(problem) => refused(&problem),
                        },
                        Err(problem) => refused(&format!(
                            "The model settings could not be read: {}",
                            problem
                        )),
                    }
                }
                "remove" => match engine.model_preset_remove(&name) {
                    Ok(_) => done(),
                    Err(problem) => refused(&problem),
                },
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
        // The lists of the side bar. They do not come with the state, for the chats are read
        // from the disk and the state is read six times a second.
        "/api/lists" | "lists" => lists_value(engine),
        // The work of the side bar on one chat, by its id: another name, the group it is in,
        // the place it belongs to, and taking it away.
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
        // The work of the side bar on one repository: another name, and the group it is in.
        "/api/rename_repo" | "rename_repo" => {
            engine.rename_repository(text_field(&value, "id"), text_field(&value, "name"));
            done()
        }
        "/api/pin_repo" | "pin_repo" => {
            engine.set_repository_pinned(text_field(&value, "id"), flag(&value, "on"));
            done()
        }
        // A repository removed whole: the name of it had to be written out in the window that
        // asked, its chats and conversations and tasks go with it, and its directory is moved
        // into the corner of the repositories directory rather than destroyed. See
        // `Engine::remove_repository`.
        "/api/delete_repo" | "delete_repo" => {
            engine.remove_repository(text_field(&value, "id"));
            done()
        }
        "/api/select_model" | "select_model" => {
            engine.select_model(text_field(&value, "id"));
            done()
        }
        // Send the loaded model away, so the memory it holds goes back to the computer. The model
        // of the settings is kept, so the next message loads the same model again. A run that a
        // chat is being answered on is let come to its end first, and the state carries the word
        // that the model is loaded until the run is truly gone.
        "/api/unload_model" | "unload_model" => {
            engine.unload_model();
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
        "/api/theme" | "theme" => {
            engine.set_theme(&text_field(&value, "theme"));
            // The pixel-art form travels in the same call, so a page that changes the theme and
            // the form together writes both in one go. A call that shows nothing of the form
            // leaves it as it is now.
            if let Some(pixel) = value.get("pixel").and_then(Value::as_bool) {
                engine.set_theme_pixel(pixel);
            }
            done()
        }
        // The settings panel. The window of the program makes these calls through its bridge,
        // and the page of the remote mode through these routes; both come to the same three
        // functions above.
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

#[cfg(test)]
mod tests {
    use super::*;

    // Every line of the drop-down of the badge of the tools stands in the state the page reads,
    // and the state says the same of each as the switch of the tools says. The list is taken from
    // `store::ToolSwitches::words`, which is the list the window of the program reads too, so a
    // line added to the tools cannot be left out of the JSON of the page unmarked. It matters
    // that no line is left out: a line the page cannot read is drawn as switched on, for a switch
    // nothing reports is taken for on, and the click that was meant to turn such a line on sends
    // it off - the line of the tasks suite stood in that state for a while, and nothing but the
    // engine had it off.
    #[test]
    fn every_line_of_the_badge_of_the_tools_reaches_the_page() {
        let _guard = config::test_turn();
        config::set_data_dir(std::path::PathBuf::from("uidata/web-tests"));
        let _ = config::ensure_dirs();
        let engine = crate::engine::Engine::start(Config::default());
        let snap = engine.snapshot();
        let held = state(&snap, None)["tool_switches"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let lines = snap.tool_switches.words();
        assert_eq!(
            held.len(),
            lines.len(),
            "the state holds one entry for every line of the drop-down"
        );
        for (part, on) in &lines {
            assert_eq!(
                held.get(*part).and_then(|v| v.as_bool()),
                Some(*on),
                "{part} stands in the state as the switch of the tools has it"
            );
        }
        assert_eq!(
            held.get("tasks").and_then(|v| v.as_bool()),
            Some(false),
            "the tasks suite is off at first, and the state says so rather than saying nothing"
        );
    }

    // The line of the tasks suite is turned on by the post the page sends, and the state tells
    // the page so afterwards. The line was once unturnable: it stood among the tools of the
    // engine but not in the JSON of the state, so the page drew it as switched on and the click
    // that meant to turn it on sent `on: false` - the engine turned it off and said so on the
    // line, and nothing of the suite was ever offered to the model.
    #[test]
    fn the_line_of_the_tasks_suite_is_turned_on_by_a_post_of_the_page() {
        let _guard = config::test_turn();
        config::set_data_dir(std::path::PathBuf::from("uidata/web-tests"));
        let _ = config::ensure_dirs();
        let engine = crate::engine::Engine::start(Config::default());
        engine.new_chat();
        assert!(!engine.snapshot().tool_switches.tasks, "off at first");
        action(&engine, "/api/tool_switch", "{\"part\":\"tasks\",\"on\":true}");
        let snap = engine.snapshot();
        assert!(snap.tool_switches.tasks, "the tasks suite is on");
        assert_eq!(
            state(&snap, None)["tool_switches"]["tasks"].as_bool(),
            Some(true),
            "the page is told that the line is on, so the next click means off and not on again"
        );
        assert!(
            snap.status.contains("Tasks suite"),
            "the line under the chat says what was opened: {}",
            snap.status
        );
        // And back off again, so that the line of the drop-down is a switch of both ways.
        action(&engine, "/api/tool_switch", "{\"part\":\"tasks\",\"on\":false}");
        let shut = engine.snapshot();
        assert!(!shut.tool_switches.tasks, "the tasks suite is off again");
        assert_eq!(
            state(&shut, None)["tool_switches"]["tasks"].as_bool(),
            Some(false),
            "and the page is told that too"
        );
    }

    // A programmed message reaches the page it was written in. The rows of the programmed part
    // of the waiting list are a list of their own in the state, and each row carries the text it
    // waits for: the page draws its section of the waiting block out of these, and a part left
    // out of this JSON is a message the program holds and the user never sees - which is how the
    // programmed message once stood, taken into the list by the engine and drawn nowhere.
    #[test]
    fn the_programmed_messages_of_the_list_reach_the_page() {
        let _guard = config::test_turn();
        config::set_data_dir(std::path::PathBuf::from("uidata/web-tests"));
        let _ = config::ensure_dirs();
        let engine = crate::engine::Engine::start(Config::default());
        engine.new_chat();
        action(
            &engine,
            "/api/queue_plan",
            "{\"text\":\"start the build\",\"match\":\"READY\"}",
        );
        action(
            &engine,
            "/api/queue_add",
            "{\"text\":\"an ordinary message\"}",
        );
        let held = state(&engine.snapshot(), None)["queue"].clone();
        assert_eq!(
            held["planned"].as_array().map(|rows| rows.len()),
            Some(1),
            "the page is told of the programmed row, so its section of the waiting block is drawn"
        );
        assert_eq!(held["planned"][0]["text"].as_str(), Some("start the build"));
        assert_eq!(
            held["planned"][0]["matches"].as_str(),
            Some("READY"),
            "the row says what it waits for, which is the only words a programmed row has to show"
        );
        assert_eq!(
            held["items"].as_array().map(|rows| rows.len()),
            Some(1),
            "the messages that are only waiting their turn ride along as they always did"
        );
        assert_eq!(
            held["items"][0]["matches"].as_str(),
            Some(""),
            "a plain row carries nothing to wait for"
        );
    }

    // The page is told whether the model stands loaded, which is not the same question as which
    // model the settings name: after a model was sent away, and before the next message, the
    // settings still name one and nothing of it is in the memory of the computer. The button that
    // sends the model away shows on this word alone, so a state that said nothing of it would
    // leave the button hidden for ever.
    #[test]
    fn the_page_is_told_whether_the_model_stands_loaded() {
        let _guard = config::test_turn();
        config::set_data_dir(std::path::PathBuf::from("uidata/web-tests"));
        let _ = config::ensure_dirs();
        let engine = crate::engine::Engine::start(Config::default());
        assert_eq!(
            state(&engine.snapshot(), None)["model"]["loaded"].as_bool(),
            Some(false),
            "nothing stands loaded before the first message"
        );
        {
            let mut guard = engine.state.lock().unwrap();
            guard.model_loaded = true;
        }
        assert_eq!(
            state(&engine.snapshot(), None)["model"]["loaded"].as_bool(),
            Some(true),
            "a run that holds the model says so, though no setting moved"
        );
        action(&engine, "/api/unload_model", "{}");
        let mut given_back = false;
        for _ in 0..200 {
            if !engine.snapshot().model_loaded {
                given_back = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(given_back, "the post of the page sends the model away");
        assert_eq!(
            state(&engine.snapshot(), None)["model"]["loaded"].as_bool(),
            Some(false),
            "the page is told the memory is given back, so its button goes off with the model"
        );
    }
}
