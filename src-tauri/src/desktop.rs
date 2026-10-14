// The desktop window of Autumn Natter.
//
// Tauri opens one window and shows the page under frontend/, drawn by the web
// view that the operating system already has. The engine runs on the Rust side,
// behind the calls in commands.rs. The page asks for the state with a short
// poll, so the window and the browser page of the remote mode read the same
// engine and show the same thing.

use autumn_natter_core::config::Config;
use autumn_natter_core::engine::Engine;
use tauri::{Manager, RunEvent};

use crate::commands;

// Open the window and run the event loop until the window is put away.
pub fn run(cfg: Config) -> Result<(), String> {
    // The engine is the same one the remote mode drives. It is started before
    // the window, so the first draw already has the models and the chats.
    let engine = Engine::start(cfg);

    let app = tauri::Builder::default()
        .manage(engine)
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::action,
            commands::get_config,
            commands::save_config,
            commands::list_repositories,
            commands::get_model_settings,
            commands::save_model_settings,
            commands::preview_command
        ])
        .build(tauri::generate_context!())
        .map_err(|problem| format!("the window could not be opened: {}", problem))?;

    app.run(|app_handle, event| {
        // The window is going away, so the model must leave the memory with it.
        // A llama-server left behind would hold the model in the machine.
        match event {
            RunEvent::Exit | RunEvent::ExitRequested { .. } => {
                if let Some(engine) = app_handle.try_state::<Engine>() {
                    engine.shutdown();
                }
            }
            _ => {}
        }
    });
    Ok(())
}
