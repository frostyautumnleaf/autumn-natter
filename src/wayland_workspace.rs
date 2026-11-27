//! A compositor for the agent's apps, with no connection to the host input seat.

use crate::{desktop::DisplayInfo, util::Watch};
use image::RgbaImage;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Seek, Write};
use std::os::fd::AsFd;
use std::os::unix::{fs::DirBuilderExt, net::UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use wayland_client::{
    delegate_noop,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_keyboard, wl_pointer, wl_registry, wl_seat},
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;
const MAX_SESSIONS: usize = 4;
// Keep the standard shortcuts mapped while reserving multimedia keys for text.
const FIRST_TEXT_KEYCODE: u32 = 160;
const TEXT_KEYS: usize = 40;
static WORKSPACES: OnceLock<Mutex<HashMap<String, Workspace>>> = OnceLock::new();

fn workspaces() -> &'static Mutex<HashMap<String, Workspace>> {
    WORKSPACES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn is_open(chat: &str) -> bool {
    workspaces()
        .lock()
        .is_ok_and(|held| held.contains_key(chat))
}

pub fn open(chat: &str, watch: &Watch) -> Result<bool, String> {
    let mut held = workspaces()
        .lock()
        .map_err(|_| "Agent workspaces are busy.")?;
    if let Some(workspace) = held.get_mut(chat) {
        if workspace.process.try_wait().map_err(problem)?.is_none() {
            return Ok(false);
        }
        held.remove(chat);
    }
    if held.len() >= MAX_SESSIONS {
        return Err(String::from(
            "Close another agent workspace before opening one.",
        ));
    }
    held.insert(chat.to_owned(), Workspace::start(watch)?);
    Ok(true)
}

pub fn close(chat: &str) {
    if let Ok(mut held) = workspaces().lock() {
        held.remove(chat);
    }
}

pub fn close_all() {
    if let Some(workspaces) = WORKSPACES.get() {
        if let Ok(mut held) = workspaces.lock() {
            held.clear();
        }
    }
}

pub fn with_workspace<T>(
    chat: &str,
    action: impl FnOnce(&mut Workspace) -> Result<T, String>,
) -> Result<T, String> {
    let mut held = workspaces()
        .lock()
        .map_err(|_| "Agent workspaces are busy.")?;
    let workspace = held
        .get_mut(chat)
        .ok_or("The agent workspace is closed. Take a fresh screenshot to reopen it.")?;
    action(workspace)
}

pub fn display() -> DisplayInfo {
    DisplayInfo {
        id: 1,
        x: 0,
        y: 0,
        width: WIDTH,
        height: HEIGHT,
        primary: true,
    }
}

struct RuntimeFolder(PathBuf);

impl RuntimeFolder {
    fn start() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("autumn-agent-{}", crate::store::new_id()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(problem)?;
        Ok(Self(path))
    }
}

impl Drop for RuntimeFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub struct Workspace {
    process: Child,
    apps: Vec<Child>,
    folder: RuntimeFolder,
    socket: PathBuf,
    connection: Option<Connection>,
    pointer: Option<ZwlrVirtualPointerV1>,
    keyboard: Option<ZwpVirtualKeyboardV1>,
    cursor: RgbaImage,
    position: (i32, i32),
    started: Instant,
}

impl Workspace {
    fn start(watch: &Watch) -> Result<Self, String> {
        let folder = RuntimeFolder::start()?;
        let config = folder.0.join("sway.conf");
        fs::write(&config, format!("xwayland disable\noutput * mode {WIDTH}x{HEIGHT} scale 1\ndefault_border none\nfocus_follows_mouse no\n")).map_err(problem)?;
        let log = fs::File::create(folder.0.join("sway.log")).map_err(problem)?;
        let process = Command::new("sway")
            .arg("--unsupported-gpu")
            .arg("--config")
            .arg(config)
            .env("XDG_RUNTIME_DIR", &folder.0)
            .env("WLR_BACKENDS", "headless")
            .env("WLR_HEADLESS_OUTPUTS", "1")
            .env("WLR_RENDERER", "pixman")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("WAYLAND_SOCKET")
            .env_remove("DISPLAY")
            .env_remove("SWAYSOCK")
            .env_remove("I3SOCK")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .map_err(|error| missing("sway", error))?;
        let mut workspace = Self {
            process,
            apps: Vec::new(),
            folder,
            socket: PathBuf::new(),
            connection: None,
            pointer: None,
            keyboard: None,
            cursor: RgbaImage::new(0, 0),
            position: ((WIDTH / 2) as i32, (HEIGHT / 2) as i32),
            started: Instant::now(),
        };
        let (socket, connection) = workspace.wait_for_compositor(watch)?;
        let (globals, queue) = registry_queue_init::<Registry>(&connection).map_err(problem)?;
        let qh = queue.handle();
        let manager: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=1, ()).map_err(problem)?;
        workspace.pointer = Some(manager.create_virtual_pointer(None, &qh, ()));
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).map_err(problem)?;
        let manager: ZwpVirtualKeyboardManagerV1 = globals.bind(&qh, 1..=1, ()).map_err(problem)?;
        workspace.keyboard = Some(manager.create_virtual_keyboard(&seat, &qh, ()));
        workspace.connection = Some(connection);
        workspace.socket = socket;
        workspace.set_keymap(&[])?;
        let artwork = crate::agent_pointer::artwork().map_err(problem)?;
        workspace.cursor = RgbaImage::from_fn(artwork.width(), artwork.height(), |x, y| {
            let pixel = artwork.pixel(x, y).unwrap().demultiply();
            image::Rgba([pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()])
        });
        workspace.move_to(workspace.position.0, workspace.position.1)?;
        Ok(workspace)
    }

    fn wait_for_compositor(&mut self, watch: &Watch) -> Result<(PathBuf, Connection), String> {
        loop {
            if watch.expired() || self.started.elapsed() >= Duration::from_secs(5) {
                return Err(String::from(
                    "The agent Wayland workspace did not start in time.",
                ));
            }
            if self.process.try_wait().map_err(problem)?.is_some() {
                let log = fs::read_to_string(self.folder.0.join("sway.log")).unwrap_or_default();
                let lines: Vec<_> = log.lines().rev().take(3).collect();
                return Err(format!(
                    "Sway could not start the agent workspace: {}",
                    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
                ));
            }
            for item in fs::read_dir(&self.folder.0).map_err(problem)?.flatten() {
                let name = item.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("wayland-") && !name.ends_with(".lock") {
                    let path = item.path();
                    if let Ok(socket) = UnixStream::connect(&path) {
                        return Ok((path, Connection::from_socket(socket).map_err(problem)?));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn command(&self, program: impl AsRef<Path>) -> Command {
        let mut command = Command::new(program.as_ref());
        command
            .env("WAYLAND_DISPLAY", &self.socket)
            .env("XDG_RUNTIME_DIR", &self.folder.0)
            .env("XDG_SESSION_TYPE", "wayland")
            .env_remove("WAYLAND_SOCKET")
            .env_remove("DISPLAY")
            .env_remove("SWAYSOCK")
            .env_remove("I3SOCK")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    pub fn capture(&self, watch: &Watch) -> Result<RgbaImage, String> {
        let path = self.folder.0.join("screen.png");
        let _ = fs::remove_file(&path);
        let child = self
            .command("grim")
            .arg(&path)
            .spawn()
            .map_err(|error| missing("grim", error))?;
        wait_for_capture(child, watch)?;
        let result = image::open(&path).map_err(problem);
        let _ = fs::remove_file(path);
        let mut shot = result?.to_rgba8();
        if shot.dimensions() != (WIDTH, HEIGHT) {
            return Err(String::from(
                "The agent workspace display resolution changed. Close and reopen it.",
            ));
        }
        self.paint_cursor(&mut shot);
        Ok(shot)
    }

    fn paint_cursor(&self, screen: &mut RgbaImage) {
        let (x, y) = (i64::from(self.position.0), i64::from(self.position.1));
        let title =
            image::imageops::crop_imm(&self.cursor, 0, 0, self.cursor.width(), 25).to_image();
        let arrow = image::imageops::crop_imm(
            &self.cursor,
            0,
            25,
            self.cursor.width(),
            self.cursor.height() - 25,
        )
        .to_image();
        let label_x = (x - 3).clamp(0, i64::from(WIDTH - title.width()));
        let label_y = if y >= 25 {
            y - 25
        } else {
            y + i64::from(arrow.height())
        };
        image::imageops::overlay(screen, &title, label_x, label_y);
        image::imageops::overlay(screen, &arrow, x - 3, y);
    }

    pub fn launch(&mut self, program: &str, watch: &Watch) -> Result<(), String> {
        if watch.expired() {
            return Err(String::from("The workspace action was stopped."));
        }
        self.apps
            .retain_mut(|child| child.try_wait().is_ok_and(|status| status.is_none()));
        let mut child = self
            .command(program)
            .env("GDK_BACKEND", "wayland")
            .env("QT_QPA_PLATFORM", "wayland")
            .spawn()
            .map_err(problem)?;
        // Allow the first frame to map before the tool captures the workspace.
        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            if watch.expired() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(String::from("The workspace action was stopped."));
            }
            match child.try_wait().map_err(problem)? {
                Some(status) if status.success() => return Ok(()),
                Some(status) => return Err(format!("Could not open {program}: {status}.")),
                None if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                None => break,
            }
        }
        self.apps.push(child);
        Ok(())
    }

    pub fn type_text(&self, text: &str, submit: bool, watch: &Watch) -> Result<(), String> {
        let mut text = text.chars();
        loop {
            let symbols: Vec<_> = text.by_ref().take(TEXT_KEYS).collect();
            if symbols.is_empty() {
                break;
            }
            self.set_keymap(&symbols)?;
            for index in 0..symbols.len() {
                // Wayland input codes are eight below the corresponding XKB codes.
                self.send_key(FIRST_TEXT_KEYCODE + index as u32 - 8, 0, watch)?;
            }
        }
        if submit {
            self.press("enter", watch)?;
        }
        Ok(())
    }

    pub fn press(&self, keys: &str, watch: &Watch) -> Result<(), String> {
        let parts: Vec<_> = keys.split('+').collect();
        let (key, modifiers) = parts.split_last().ok_or("Give a key name.")?;
        let mut mask = 0;
        for modifier in modifiers {
            mask |= match *modifier {
                "shift" => 1 << 0,
                "ctrl" | "control" => 1 << 2,
                "alt" => 1 << 3,
                "super" | "win" | "meta" => 1 << 6,
                _ => return Err(String::from("Unknown keyboard modifier.")),
            };
        }
        let code = standard_keycode(key).ok_or("Unknown workspace key.")?;
        self.send_key(code, mask, watch)
    }

    fn keyboard(&self) -> Result<&ZwpVirtualKeyboardV1, String> {
        self.keyboard
            .as_ref()
            .ok_or_else(|| String::from("The workspace keyboard is unavailable."))
    }

    fn set_keymap(&self, symbols: &[char]) -> Result<(), String> {
        let mut keys = String::new();
        for (index, symbol) in symbols.iter().enumerate() {
            let code = FIRST_TEXT_KEYCODE + index as u32;
            keys.push_str(&format!(
                "replace key <I{code}> {{ [ U{:04X} ] }};\n",
                u32::from(*symbol)
            ));
        }
        let keymap = format!(
            "xkb_keymap {{\n\
             xkb_keycodes {{ include \"evdev+aliases(qwerty)\" }};\n\
             xkb_types {{ include \"complete\" }};\n\
             xkb_compatibility {{ include \"complete\" }};\n\
             xkb_symbols {{ include \"pc+us\" {keys} }};\n\
             }};\0"
        );
        let path = self.folder.0.join("keyboard.xkb");
        let mut file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(problem)?;
        fs::remove_file(path).map_err(problem)?;
        file.write_all(keymap.as_bytes()).map_err(problem)?;
        file.rewind().map_err(problem)?;
        self.keyboard()?.keymap(
            wl_keyboard::KeymapFormat::XkbV1.into(),
            file.as_fd(),
            keymap.len() as u32,
        );
        self.sync()
    }

    fn send_key(&self, code: u32, modifiers: u32, watch: &Watch) -> Result<(), String> {
        if watch.expired() {
            return Err(String::from("The workspace action was stopped."));
        }
        let keyboard = self.keyboard()?;
        if modifiers != 0 {
            keyboard.modifiers(modifiers, 0, 0, 0);
        }
        keyboard.key(self.time(), code, wl_keyboard::KeyState::Pressed.into());
        keyboard.key(self.time(), code, wl_keyboard::KeyState::Released.into());
        if modifiers != 0 {
            keyboard.modifiers(0, 0, 0, 0);
        }
        self.sync()
    }

    pub fn position(&self) -> (i32, i32) {
        self.position
    }

    pub fn move_to(&mut self, x: i32, y: i32) -> Result<(), String> {
        if x < 0 || y < 0 || x as u32 >= WIDTH || y as u32 >= HEIGHT {
            return Err(String::from(
                "The pointer position is outside the agent workspace.",
            ));
        }
        self.pointer
            .as_ref()
            .ok_or("The workspace pointer is unavailable.")?
            .motion_absolute(self.time(), x as u32, y as u32, WIDTH, HEIGHT);
        self.flush_pointer()?;
        self.position = (x, y);
        Ok(())
    }

    pub fn button(&self, number: u8, pressed: bool) -> Result<(), String> {
        let pointer = self
            .pointer
            .as_ref()
            .ok_or("The workspace pointer is unavailable.")?;
        let time = self.time();
        match number {
            1..=3 => {
                let button = match number {
                    1 => 0x110,
                    2 => 0x112,
                    _ => 0x111,
                };
                let state = if pressed {
                    wl_pointer::ButtonState::Pressed
                } else {
                    wl_pointer::ButtonState::Released
                };
                pointer.button(time, button, state);
            }
            4..=7 if pressed => {
                let axis = if number <= 5 {
                    wl_pointer::Axis::VerticalScroll
                } else {
                    wl_pointer::Axis::HorizontalScroll
                };
                let step = if number == 4 || number == 6 { -1 } else { 1 };
                pointer.axis_source(wl_pointer::AxisSource::Wheel);
                pointer.axis_discrete(time, axis, f64::from(step * 15), step);
            }
            4..=7 => return Ok(()),
            _ => return Err(String::from("Unknown pointer button.")),
        }
        self.flush_pointer()
    }

    fn time(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    fn flush_pointer(&self) -> Result<(), String> {
        self.pointer
            .as_ref()
            .ok_or("The workspace pointer is unavailable.")?
            .frame();
        self.sync()
    }

    fn sync(&self) -> Result<(), String> {
        self.connection
            .as_ref()
            .ok_or("The workspace is disconnected.")?
            .roundtrip()
            .map_err(problem)?;
        Ok(())
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        for app in &mut self.apps {
            let _ = app.kill();
            let _ = app.wait();
        }
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn standard_keycode(key: &str) -> Option<u32> {
    let code = match key {
        "escape" => 1,
        "backspace" => 14,
        "tab" => 15,
        "enter" => 28,
        "space" => 57,
        "f11" => 87,
        "f12" => 88,
        "home" => 102,
        "up" => 103,
        "pageup" => 104,
        "left" => 105,
        "right" => 106,
        "end" => 107,
        "down" => 108,
        "pagedown" => 109,
        "delete" => 111,
        _ => {
            if let Some(function) = key.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
                return (1..=10).contains(&function).then_some(58 + function);
            }
            if key.len() == 1 {
                for (row, start) in [
                    ("1234567890", 2),
                    ("qwertyuiop", 16),
                    ("asdfghjkl", 30),
                    ("zxcvbnm", 44),
                ] {
                    if let Some(offset) = row.find(key) {
                        return Some(start + offset as u32);
                    }
                }
            }
            return None;
        }
    };
    Some(code)
}

fn wait_for_capture(mut child: Child, watch: &Watch) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "Could not capture the Wayland workspace: {status}."
                ))
            }
            Ok(None) if !watch.expired() && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(String::from(
                    "The Wayland workspace capture was stopped or timed out.",
                ));
            }
        }
    }
}

fn missing(program: &str, error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        format!("The agent workspace needs {program}. Install sway and grim, then try again.")
    } else {
        problem(error)
    }
}

fn problem(error: impl std::fmt::Display) -> String {
    format!("Agent Wayland workspace: {error}")
}

struct Registry;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Registry {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(Registry: ignore ZwlrVirtualPointerManagerV1);
delegate_noop!(Registry: ignore ZwlrVirtualPointerV1);
delegate_noop!(Registry: ignore wl_seat::WlSeat);
delegate_noop!(Registry: ignore ZwpVirtualKeyboardManagerV1);
delegate_noop!(Registry: ignore ZwpVirtualKeyboardV1);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentic::computer_run;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    #[ignore = "requires Sway, grim, and GTK 3 Python bindings"]
    fn computer_use_controls_an_app_on_the_private_wayland_display() {
        let folder = RuntimeFolder::start().unwrap();
        let events = folder.0.join("events");
        let app = folder.0.join("workspace-test-app");
        fs::write(&app, format!(r#"#!/usr/bin/python3
import gi
import os
gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, Gdk

def record(message):
    with open({events:?}, 'a') as log:
        print(message, file=log)

window = Gtk.Window(title='Independent Agent Pointer')
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
entry = Gtk.Entry()
entry.connect('changed', lambda item: record('text:' + item.get_text()))
entry.connect('activate', lambda _item: record('activated'))
area = Gtk.EventBox()
area.set_name('workspace-content')
area.add(Gtk.Label(label='Private Wayland display'))
provider = Gtk.CssProvider()
provider.load_from_data(b'#workspace-content {{ background: #1a476f; color: white; }}')
area.get_style_context().add_provider(provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
area.add_events(Gdk.EventMask.BUTTON_PRESS_MASK | Gdk.EventMask.BUTTON_RELEASE_MASK | Gdk.EventMask.POINTER_MOTION_MASK | Gdk.EventMask.SCROLL_MASK)
area.connect('button-press-event', lambda _widget, event: record('down:' + str(int(event.x))))
area.connect('button-release-event', lambda _widget, event: record('up:' + str(int(event.x))))
area.connect('scroll-event', lambda *_args: record('scroll'))
box.pack_start(entry, False, False, 0)
box.pack_start(area, True, True, 0)
window.add(box)
window.show_all()
entry.grab_focus()
record('display:' + os.environ['WAYLAND_DISPLAY'])
Gtk.main()
"#, events = events.to_str().unwrap())).unwrap();
        fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).unwrap();

        struct Close(String);
        impl Drop for Close {
            fn drop(&mut self) {
                crate::agentic::close_computer_workspace(&self.0);
            }
        }
        let chat = Close(crate::store::new_id());
        let run = |action: serde_json::Value| {
            let result = computer_run(
                &action.to_string(),
                &chat.0,
                crate::store::ComputerTarget::Workspace,
                true,
                &Watch::new(Duration::from_secs(10)),
            );
            assert!(result.report.ok, "{}: {}", action, result.report.output);
            result
        };
        let opened = run(json!({"action":"screenshot"}));
        assert!(opened
            .report
            .output
            .contains("Agent Wayland workspace screenshot"));
        run(json!({"action":"open_app", "name": app}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !events.exists() {
            assert!(
                Instant::now() < deadline,
                "the fixture never connected to the workspace"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
        let socket = with_workspace(&chat.0, |workspace| Ok(workspace.socket.clone())).unwrap();
        assert!(fs::read_to_string(&events)
            .unwrap()
            .contains(&format!("display:{}", socket.display())));
        assert_ne!(
            Some(socket.into_os_string()),
            std::env::var_os("WAYLAND_DISPLAY")
        );
        run(json!({"action":"click", "x":200, "y":220}));
        run(json!({"action":"drag", "x":300, "y":240, "to_x":460, "to_y":246}));
        run(json!({"action":"scroll", "x":480, "y":240, "direction":"down", "amount":1}));
        run(json!({"action":"type", "text":"Independent ✓"}));
        run(json!({"action":"press", "key":"backspace"}));
        run(json!({"action":"press", "key":"ctrl+a"}));
        run(json!({"action":"type", "text":"Agent workspace ready"}));
        run(json!({"action":"press", "key":"enter"}));
        let result = run(json!({"action":"cursor"}));
        assert!(result.report.output.contains("x=480, y=240"));
        let records = fs::read_to_string(&events).unwrap();
        assert!(records.contains("down:200"), "{records}");
        assert!(
            records.contains("down:300") && records.contains("up:460"),
            "{records}"
        );
        assert!(records.contains("scroll"), "{records}");
        assert!(records.contains("text:Independent ✓"), "{records}");
        assert!(records.contains("text:Independent \n"), "{records}");
        assert!(
            records.ends_with("text:Agent workspace ready\nactivated\n"),
            "{records}"
        );
        let image =
            with_workspace(&chat.0, |workspace| workspace.capture(&Watch::unbounded())).unwrap();
        assert!(image::imageops::crop_imm(&image, 477, 215, 52, 20)
            .to_image()
            .pixels()
            .any(|pixel| *pixel == image::Rgba([255, 152, 46, 255])));
        assert!(image::imageops::crop_imm(&image, 485, 222, 33, 10)
            .to_image()
            .pixels()
            .any(|pixel| pixel[0] < 90 && pixel[1] < 70));
        if let Some(path) = std::env::var_os("AUTUMN_WAYLAND_WORKSPACE_TEST_IMAGE") {
            image.save(path).unwrap();
        }
        crate::agentic::close_computer_workspace(&chat.0);
        assert!(!is_open(&chat.0));
    }
}
