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
    // Tame the web view before GTK opens a window. See the notes below.
    calm_the_web_view_on_wayland();

    // The engine is the same one the remote mode drives. It is started before
    // the window, so the first draw already has the models and the chats.
    let engine = Engine::start(cfg);

    let app = tauri::Builder::default()
        .manage(engine)
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::action,
            commands::get_attachment,
            commands::get_config,
            commands::save_config,
            commands::list_repositories,
            commands::get_model_settings,
            commands::save_model_settings,
            commands::preview_command
        ])
        // The window stands in the settings file, but it is built here, and the
        // file says so with "create": false. Two switches the file cannot hold
        // go with it. The file does say "dragDropEnabled": false, which keeps
        // Tauri from eating a file let go over the window: with that handler on
        // the page never sees a drop, and the chat box could never take a file.
        // The switch below is the leave to read the clipboard of the machine,
        // which the web view of the system keeps from the page until it is
        // asked for. Without it a Ctrl+V of a file came to nothing in the
        // window, though it worked in the browser page of the remote mode.
        .setup(|app| {
            let windows = app.config().app.windows.clone();
            let Some(shape) = windows.first() else {
                return Err("no window in the settings of the program".into());
            };
            tauri::WebviewWindowBuilder::from_config(app.handle(), shape)?
                .enable_clipboard_access()
                .build()?;
            Ok(())
        })
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

// WebKitGTK draws with a GPU path called the DMABUF renderer by default. On
// Wayland it asks the compositor to share buffers in a way that some of them
// refuse. The compositor then closes the connection, and GDK ends the run with
// "Error 71 (Protocol error) dispatching to Wayland display". Turning that path
// off makes WebKitGTK draw through shared memory instead, which every
// compositor accepts. It costs a little speed, and it is the fix the compositors
// and NVIDIA drivers ask for. A few compositors also want accelerated
// compositing off, so both are set. Nothing is changed when the user already
// chose a value, and X11 is left alone.
#[cfg(target_os = "linux")]
fn calm_the_web_view_on_wayland() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }
    for (key, value) in [
        ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
        ("WEBKIT_DISABLE_COMPOSITING_MODE", "1"),
    ] {
        if std::env::var_os(key).is_none() {
            // SAFETY: the call runs before any thread starts and before GTK
            // reads the environment, so no other code looks at these keys.
            unsafe {
                std::env::set_var(key, value);
            }
        }
    }
}

// The web view of the other systems does not have this problem.
#[cfg(not(target_os = "linux"))]
fn calm_the_web_view_on_wayland() {}
