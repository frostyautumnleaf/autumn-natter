// The calls the window makes into the engine.
//
// The page of the window asks for the state with a short poll, in the same way
// as the browser page of the remote mode, and it sends the actions of the user
// as named calls. Every command is async and takes the engine by reference, so
// Tauri runs it on a worker thread and a slow answer never stops the window
// from drawing. A command that takes a reference must return a Result, so each
// one does. None of them fail on purpose: a bad value answers with a small
// "not ok" record, and the window reads the state again.

use serde_json::{json, Value};
use tauri::State;

use autumn_natter_core::config::{Config, ModelSettings};
use autumn_natter_core::engine::Engine;
use autumn_natter_core::{llama, web};

// The state as JSON. The page sends the number it already has (since). When
// nothing changed, the answer stays small.
#[tauri::command]
pub async fn get_state(engine: State<'_, Engine>, since: Option<u64>) -> Result<Value, String> {
    let snap = engine.snapshot();
    Ok(web::state(&snap, since))
}

// One action of the user. The name is the same as the route of the remote page,
// so the two share the mapping in web::action.
#[tauri::command]
pub async fn action(
    engine: State<'_, Engine>,
    path: String,
    body: Option<Value>,
) -> Result<Value, String> {
    let text = body.map(|value| value.to_string()).unwrap_or_default();
    Ok(web::action(&engine, &path, &text))
}

// One attachment of a message, for the block the window draws. The page
// asks by the id the state gave with the message, and gets back the name,
// the kind, the data URL of a picture, or the words of a file.
#[tauri::command]
pub async fn get_attachment(engine: State<'_, Engine>, id: String) -> Result<Value, String> {
    match engine.attachment(&id) {
        Some(record) => Ok(json!({
            "name": record.name,
            "mime": record.mime,
            "data": record.data,
            "text": record.text,
        })),
        None => Err("that attachment is not here".to_string()),
    }
}

// The settings of the program, for the window of settings.
#[tauri::command]
pub async fn get_config(engine: State<'_, Engine>) -> Result<Value, String> {
    Ok(serde_json::to_value(engine.config()).unwrap_or_else(|_| json!({})))
}

// Store new settings, and refresh what depends on them.
#[tauri::command]
pub async fn save_config(engine: State<'_, Engine>, config: Value) -> Result<Value, String> {
    match serde_json::from_value::<Config>(config) {
        Ok(cfg) => {
            engine.save_settings(cfg);
            Ok(web::done())
        }
        Err(problem) => Ok(web::refused(&format!(
            "the settings could not be read: {}",
            problem
        ))),
    }
}

// Every repository on disk, most recently used first. The window of settings
// shows all of them, while the top bar shows five.
#[tauri::command]
pub async fn list_repositories(engine: State<'_, Engine>) -> Result<Value, String> {
    let repos = engine.repositories();
    Ok(Value::Array(
        repos
            .iter()
            .map(|repo| json!({ "id": repo.id, "name": repo.name }))
            .collect(),
    ))
}

// Everything the window of one model needs: the settings it holds, the devices
// of the machine, and the three lists of choices.
#[tauri::command]
pub async fn get_model_settings(engine: State<'_, Engine>, id: String) -> Result<Value, String> {
    let snap = engine.snapshot();
    let (settings, gpus) = engine.model_settings(&id);
    let (name, kind) = snap
        .models
        .iter()
        .find(|model| model.id() == id)
        .map(|model| (model.name.clone(), model.kind.to_string()))
        .unwrap_or_default();
    Ok(json!({
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
    }))
}

// Save the settings of one model. A running server is stopped, because it was
// started with the old values.
#[tauri::command]
pub async fn save_model_settings(
    engine: State<'_, Engine>,
    id: String,
    settings: Value,
) -> Result<Value, String> {
    match serde_json::from_value::<ModelSettings>(settings) {
        Ok(made) => {
            engine.save_model_settings(id, made);
            Ok(web::done())
        }
        Err(problem) => Ok(web::refused(&format!(
            "the model settings could not be read: {}",
            problem
        ))),
    }
}

// The command line the next answer will use, read again whenever a field of the
// window of one model moves. Nothing is started: only the arguments are built.
#[tauri::command]
pub async fn preview_command(
    engine: State<'_, Engine>,
    id: String,
    settings: Value,
) -> Result<String, String> {
    let snap = engine.snapshot();
    let Some(model) = snap.models.iter().find(|m| m.id() == id) else {
        return Ok(String::new());
    };
    let Ok(held) = serde_json::from_value::<ModelSettings>(settings) else {
        return Ok(String::new());
    };
    // The switches of the form win over the switches the model was saved with.
    let (_, devices) = engine.model_settings(&id);
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
    Ok(line.trim_end().to_string())
}
