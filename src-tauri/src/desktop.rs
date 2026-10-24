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
            commands::preview_command,
            commands::get_logs,
            commands::get_log,
            commands::take_files,
            commands::clipboard_take
        ])
        // The window stands in the settings file, but it is built here, and the
        // file says so with "create": false. Two things the file cannot hold go
        // with it. The leave below is the leave to read the clipboard of the
        // machine, which the web view of the system keeps from the page until it
        // is asked for. And on this system the window watches a file let go over
        // it itself, in watch_the_drop below, for the web view of the system
        // never brings such a drop down to the page.
        .setup(|app| {
            let windows = app.config().app.windows.clone();
            let Some(shape) = windows.first() else {
                return Err("No window in the settings of the program".into());
            };
            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), shape)?
                .enable_clipboard_access()
                .build()?;
            watch_the_drop(&window);
            Ok(())
        })
        .build(tauri::generate_context!())
        .map_err(|problem| format!("The window could not be opened: {}", problem))?;

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

// ---------------------------------------------------------------- a file let go
//
// The settings file says "dragDropEnabled": false, which keeps Tauri and its
// web view wrapper out of the way of a drop. On the macOS and Windows builds
// that is enough, for their web views bring the drop down to the page and the
// page takes the files itself. On this system it is not enough in either
// direction: WebKitGTK does not bring a file let go down to the page at all
// (the web view keeps the list of the kinds of a drop to a handful of kinds of
// words, and a file is none of them: bugs.webkit.org/show_bug.cgi?id=265857),
// and the watcher that Tauri has of its own for a drop reports one only when the
// file is let go after the drag has gone away and before the loop of the window
// rests again, which under Wayland it is not (tauri-apps/wry#1256). So the
// window watches the drop itself, on the very widget the web view watches it on,
// and tells the page of it.
//
// The page is told three things, and nothing of the file itself: that a drag
// stands over the window (for the box to say so), that the drag has gone away,
// and that a file was let go with the places of the files. The files are then
// read by take_files in commands.rs, which is the same reader the clipboard of
// the machine uses.

// Word to the page of the window: "over", "away", or "down" with the places of
// the files that were let go.
#[cfg(target_os = "linux")]
fn tell(handle: &tauri::AppHandle, kind: &str, paths: &[String]) {
    use tauri::Emitter;
    let _ = handle.emit(
        "window-drop",
        serde_json::json!({ "kind": kind, "paths": paths }),
    );
}

// What the window knows about the drag that stands over it now.
#[cfg(target_os = "linux")]
#[derive(Default)]
struct Drag {
    // The places of the files of this drag, as soon as they are known. The web
    // view asks the drag for its data while it is still moving, so they often
    // stand here before anything is let go.
    paths: Vec<String>,
    // True when a file was let go and its places were not known yet. They are
    // of use the moment they come.
    asked: bool,
    // True while a drag stands over the window.
    over: bool,
}

// What the window knows about the drag, looked at by the handler that needs it.
// The lock is never kept across two of the handlers, and a handler that fell
// over in the middle of it leaves nothing behind that the next one could not
// make sense of.
#[cfg(target_os = "linux")]
fn watching(
    drag: &std::sync::Arc<std::sync::Mutex<Drag>>,
) -> std::sync::MutexGuard<'_, Drag> {
    drag.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// The window watches a file let go over it. Every handler here only listens:
// none of them says it handled the drop, so the web view is left to its own
// work with the drag, and the page keeps its words and its marks.
#[cfg(target_os = "linux")]
fn watch_the_drop(window: &tauri::WebviewWindow) {
    use gtk::prelude::*;

    let handle = window.app_handle().clone();
    let drag: std::sync::Arc<std::sync::Mutex<Drag>> = std::sync::Arc::default();

    // The closure runs on the thread of the window, which is the thread GTK is
    // the work of, and that is where its signals may be listened to.
    let listened = window.with_webview(move |place| {
        let webview = place.inner();

        // The places of the files, from the moment the drag gives them up.
        {
            let drag = drag.clone();
            let handle = handle.clone();
            webview.connect_drag_data_received(
                move |_view, _context, _x, _y, held, _info, _time| {
                    // A drag carries many kinds of itself. Only the list of
                    // places is of interest here, and nothing else answers with
                    // any places at all.
                    let made = place_paths(held);
                    if made.is_empty() {
                        return;
                    }
                    let taken = {
                        let mut now = watching(&drag);
                        now.paths = made;
                        if now.asked {
                            now.asked = false;
                            Some(std::mem::take(&mut now.paths))
                        } else {
                            None
                        }
                    };
                    if let Some(files) = taken {
                        tell(&handle, "down", &files);
                    }
                },
            );
        }

        // The file is let go. The places are either already here, or they come
        // of their own in a moment.
        {
            let drag = drag.clone();
            let handle = handle.clone();
            webview.connect_drag_drop(move |_view, _context, _x, _y, _time| {
                let taken = {
                    let mut now = watching(&drag);
                    now.over = false;
                    if now.paths.is_empty() {
                        now.asked = true;
                        None
                    } else {
                        Some(std::mem::take(&mut now.paths))
                    }
                };
                tell(&handle, "away", &[]);
                if let Some(files) = taken {
                    tell(&handle, "down", &files);
                    return false;
                }
                // Should the drag give nothing up after all, the window forgets
                // it asked, rather than answer a later drag with this one.
                let later = drag.clone();
                gtk::glib::timeout_add_local_once(
                    std::time::Duration::from_millis(700),
                    move || {
                        let mut now = watching(&later);
                        if now.asked {
                            now.asked = false;
                            now.paths.clear();
                        }
                    },
                );
                false
            });
        }

        // A drag stands over the window, which the chat box says so.
        {
            let drag = drag.clone();
            let handle = handle.clone();
            webview.connect_drag_motion(move |_view, _context, _x, _y, _time| {
                let came = {
                    let mut now = watching(&drag);
                    if now.over {
                        false
                    } else {
                        now.over = true;
                        true
                    }
                };
                if came {
                    tell(&handle, "over", &[]);
                }
                false
            });
        }

        // The drag has gone away. The places are kept until the run of the loop
        // is through, for under Wayland a file is let go after the drag has
        // gone away, and the places of that file are the places of this drag.
        {
            let drag = drag.clone();
            let handle = handle.clone();
            webview.connect_drag_leave(move |_view, _context, _time| {
                let gone = {
                    let mut now = watching(&drag);
                    if now.over {
                        now.over = false;
                        true
                    } else {
                        false
                    }
                };
                if gone {
                    tell(&handle, "away", &[]);
                }
                let later = drag.clone();
                gtk::glib::idle_add_local_once(move || {
                    let mut now = watching(&later);
                    if !now.asked {
                        now.paths.clear();
                    }
                });
            });
        }
    });
    if let Err(problem) = listened {
        // Nothing was listened to, so the window takes no file by drag. The
        // page is none the wiser, and says nothing of it.
        eprintln!("autumn-natter: the window could not watch a file let go: {problem}");
    }
}

// The places of the files a drag carries, from the data of the drag. A drag of
// words, or of a picture with no place behind it, gives no places.
#[cfg(target_os = "linux")]
fn place_paths(held: &gtk::SelectionData) -> Vec<String> {
    held.uris()
        .iter()
        .filter_map(|uri| crate::commands::uri_to_place(uri.as_str()))
        .collect()
}

// The other systems bring a file let go down to the page, which takes it there.
#[cfg(not(target_os = "linux"))]
fn watch_the_drop(_window: &tauri::WebviewWindow) {}
