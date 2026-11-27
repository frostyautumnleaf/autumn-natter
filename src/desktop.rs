//! Desktop capture backends used by Computer use and the native browser panel.
//!
//! Linux capture and input use X11 or the desktop portals, depending on the active session.

use image::RgbaImage;

#[derive(Clone, Copy, Debug)]
pub struct DisplayInfo {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

pub struct CapturedArea {
    pub image: RgbaImage,
    pub x: i32,
    pub y: i32,
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{CapturedArea, DisplayInfo};
    use ashpd::desktop::{
        remote_desktop::{DeviceType, KeyState, RemoteDesktop},
        screencast::{CursorMode, SourceType},
        screenshot::Screenshot,
        PersistMode, Session,
    };
    use image::RgbaImage;
    use std::sync::{Mutex, OnceLock};
    use x11rb::connection::Connection;
    use x11rb::protocol::randr;
    use x11rb::protocol::xproto::{
        AtomEnum, ConnectionExt as XprotoConnectionExt, ImageFormat, ImageOrder,
    };
    use x11rb::rust_connection::RustConnection;

    pub fn is_wayland() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE")
                .is_ok_and(|session| session.eq_ignore_ascii_case("wayland"))
    }

    fn ensure_x11() -> Result<(), String> {
        if std::env::var_os("DISPLAY").is_none() {
            return Err(String::from(
                "Computer use needs an X11 desktop session; DISPLAY is not set.",
            ));
        }
        Ok(())
    }

    fn connect() -> Result<(RustConnection, usize), String> {
        ensure_x11()?;
        x11rb::connect(None)
            .map_err(|problem| format!("Could not connect to the X11 display: {problem}"))
    }

    fn displays_x11() -> Result<Vec<DisplayInfo>, String> {
        let (connection, screen_number) = connect()?;
        let screen = connection
            .setup()
            .roots
            .get(screen_number)
            .ok_or_else(|| String::from("The X11 display has no screen."))?;
        let monitors = randr::get_monitors(&connection, screen.root, true)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.monitors)
            .unwrap_or_default();
        let displays = monitors
            .into_iter()
            .filter(|monitor| monitor.width > 0 && monitor.height > 0)
            .enumerate()
            .map(|(index, monitor)| DisplayInfo {
                id: index as u32 + 1,
                x: i32::from(monitor.x),
                y: i32::from(monitor.y),
                width: u32::from(monitor.width),
                height: u32::from(monitor.height),
                primary: monitor.primary,
            })
            .collect::<Vec<_>>();
        if !displays.is_empty() {
            return Ok(displays);
        }
        Ok(vec![DisplayInfo {
            id: 1,
            x: 0,
            y: 0,
            width: u32::from(screen.width_in_pixels),
            height: u32::from(screen.height_in_pixels),
            primary: true,
        }])
    }

    fn display_by_id_x11(id: u32) -> Result<DisplayInfo, String> {
        displays_x11()?
            .into_iter()
            .find(|display| display.id == id)
            .ok_or_else(|| String::from("The selected display is no longer available."))
    }

    fn capture_display_x11(id: u32) -> Result<(DisplayInfo, RgbaImage), String> {
        let (connection, screen_number) = connect()?;
        let display = display_by_id_x11(id)?;
        let image = capture_region(
            &connection,
            screen_number,
            display.x,
            display.y,
            display.width,
            display.height,
        )?;
        Ok((display, image))
    }

    fn capture_application_window_x11(_pid: u32, _title: &str) -> Result<CapturedArea, String> {
        let (connection, screen_number) = connect()?;
        let screen = connection
            .setup()
            .roots
            .get(screen_number)
            .ok_or_else(|| String::from("The X11 display has no screen."))?;
        let image = capture_region(
            &connection,
            screen_number,
            0,
            0,
            u32::from(screen.width_in_pixels),
            u32::from(screen.height_in_pixels),
        )?;
        Ok(CapturedArea { image, x: 0, y: 0 })
    }

    fn focused_window_x11() -> Option<(String, String)> {
        let (connection, screen_number) = connect().ok()?;
        let root = connection.setup().roots.get(screen_number)?.root;
        let active_atom = connection
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let window = connection
            .get_property(false, root, active_atom, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?
            .value32()?
            .next()?;
        let title = property_text(&connection, window, b"_NET_WM_NAME")
            .or_else(|| property_text(&connection, window, b"WM_NAME"))
            .unwrap_or_default();
        let class = property_bytes(&connection, window, b"WM_CLASS")
            .map(|value| {
                value
                    .split(|byte| *byte == 0)
                    .filter(|part| !part.is_empty())
                    .last()
                    .map(String::from_utf8_lossy)
                    .map(|part| part.into_owned())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        Some((class, title))
    }

    fn property_bytes(connection: &RustConnection, window: u32, name: &[u8]) -> Option<Vec<u8>> {
        let atom = connection.intern_atom(false, name).ok()?.reply().ok()?.atom;
        connection
            .get_property(false, window, atom, AtomEnum::ANY, 0, 4096)
            .ok()?
            .reply()
            .ok()
            .map(|reply| reply.value)
    }

    fn property_text(connection: &RustConnection, window: u32, name: &[u8]) -> Option<String> {
        property_bytes(connection, window, name).map(|value| {
            String::from_utf8_lossy(&value)
                .chars()
                .filter(|character| !character.is_control())
                .take(300)
                .collect()
        })
    }

    fn capture_region(
        connection: &RustConnection,
        screen_number: usize,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, String> {
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 24_000_000 {
            return Err(String::from(
                "The display region is too large to capture safely.",
            ));
        }
        let x = i16::try_from(x)
            .map_err(|_| String::from("The display x position is out of range."))?;
        let y = i16::try_from(y)
            .map_err(|_| String::from("The display y position is out of range."))?;
        let width =
            u16::try_from(width).map_err(|_| String::from("The display width is out of range."))?;
        let height = u16::try_from(height)
            .map_err(|_| String::from("The display height is out of range."))?;
        let screen = connection
            .setup()
            .roots
            .get(screen_number)
            .ok_or_else(|| String::from("The X11 display has no screen."))?;
        let reply = connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                screen.root,
                x,
                y,
                width,
                height,
                u32::MAX,
            )
            .map_err(|problem| format!("Could not request an X11 screenshot: {problem}"))?
            .reply()
            .map_err(|problem| format!("Could not capture the X11 screen: {problem}"))?;
        let format = connection
            .setup()
            .pixmap_formats
            .iter()
            .find(|format| format.depth == screen.root_depth)
            .ok_or_else(|| String::from("The X11 screen pixel format is unavailable."))?;
        let visual = screen
            .allowed_depths
            .iter()
            .flat_map(|depth| depth.visuals.iter())
            .find(|visual| visual.visual_id == screen.root_visual)
            .ok_or_else(|| String::from("The X11 screen colour format is unavailable."))?;
        let bytes_per_pixel = usize::from(format.bits_per_pixel).div_ceil(8);
        let scanline_pad = usize::from(format.scanline_pad).max(8);
        let row_bytes = usize::from(width)
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| String::from("The X11 screenshot size is invalid."))?;
        let stride = row_bytes
            .checked_add(scanline_pad / 8 - 1)
            .map(|value| value / (scanline_pad / 8) * (scanline_pad / 8))
            .ok_or_else(|| String::from("The X11 screenshot row is invalid."))?;
        if reply.data.len() < stride.saturating_mul(usize::from(height)) {
            return Err(String::from("The X11 screenshot data was incomplete."));
        }
        let mut rgba = vec![0; usize::from(width) * usize::from(height) * 4];
        for row in 0..usize::from(height) {
            for column in 0..usize::from(width) {
                let start = row * stride + column * bytes_per_pixel;
                let pixel_bytes = &reply.data[start..start + bytes_per_pixel];
                let pixel = if connection.setup().image_byte_order == ImageOrder::LSB_FIRST {
                    pixel_bytes
                        .iter()
                        .enumerate()
                        .fold(0u32, |value, (index, byte)| {
                            value | (u32::from(*byte) << (index * 8))
                        })
                } else {
                    pixel_bytes
                        .iter()
                        .fold(0u32, |value, byte| (value << 8) | u32::from(*byte))
                };
                let target = (row * usize::from(width) + column) * 4;
                rgba[target] = channel(pixel, visual.red_mask);
                rgba[target + 1] = channel(pixel, visual.green_mask);
                rgba[target + 2] = channel(pixel, visual.blue_mask);
                rgba[target + 3] = 255;
            }
        }
        RgbaImage::from_raw(u32::from(width), u32::from(height), rgba)
            .ok_or_else(|| String::from("The X11 screenshot pixels could not be decoded."))
    }

    fn channel(pixel: u32, mask: u32) -> u8 {
        if mask == 0 {
            return 0;
        }
        let shift = mask.trailing_zeros();
        let maximum = mask >> shift;
        (((pixel & mask) >> shift) * 255 / maximum) as u8
    }

    #[derive(Clone, Copy)]
    struct PortalStream {
        node: u32,
        display: DisplayInfo,
    }

    struct PortalDesktop {
        remote: RemoteDesktop<'static>,
        session: Session<'static, RemoteDesktop<'static>>,
        streams: Vec<PortalStream>,
        last_used: std::time::Instant,
    }

    static WAYLAND_PORTAL: OnceLock<Mutex<Option<PortalDesktop>>> = OnceLock::new();
    impl PortalDesktop {
        fn start() -> Result<Self, String> {
            let (remote, session, streams) = futures::executor::block_on(async {
                let remote = RemoteDesktop::new().await.map_err(portal_error)?;
                let screencast = ashpd::desktop::screencast::Screencast::new()
                    .await
                    .map_err(portal_error)?;
                let session = remote.create_session().await.map_err(portal_error)?;
                remote
                    .select_devices(
                        &session,
                        DeviceType::Keyboard | DeviceType::Pointer,
                        None,
                        PersistMode::DoNot,
                    )
                    .await
                    .map_err(portal_error)?
                    .response()
                    .map_err(portal_error)?;
                // ScreenCast rejects persistence when selecting sources for a RemoteDesktop session.
                screencast
                    .select_sources(
                        &session,
                        CursorMode::Embedded,
                        SourceType::Monitor.into(),
                        true,
                        None,
                        PersistMode::DoNot,
                    )
                    .await
                    .map_err(portal_error)?
                    .response()
                    .map_err(portal_error)?;
                let selected = remote
                    .start(&session, None)
                    .await
                    .map_err(portal_error)?
                    .response()
                    .map_err(portal_error)?;
                let streams = selected
                    .streams()
                    .ok_or_else(|| String::from("The screen sharing portal returned no displays."))?
                    .iter()
                    .filter_map(|stream| {
                        let (x, y) = stream.position().unwrap_or((0, 0));
                        let (width, height) = stream.size()?;
                        if width <= 0 || height <= 0 {
                            return None;
                        }
                        Some(PortalStream {
                            node: stream.pipe_wire_node_id(),
                            display: DisplayInfo {
                                id: 0,
                                x,
                                y,
                                width: width as u32,
                                height: height as u32,
                                primary: false,
                            },
                        })
                    })
                    .enumerate()
                    .map(|(index, mut stream)| {
                        stream.display.id = index as u32 + 1;
                        stream.display.primary = index == 0;
                        stream
                    })
                    .collect::<Vec<_>>();
                if streams.is_empty() {
                    return Err(String::from(
                        "The screen sharing portal returned no usable display sizes.",
                    ));
                }
                Ok::<_, String>((remote, session, streams))
            })?;
            Ok(Self {
                remote,
                session,
                streams,
                last_used: std::time::Instant::now(),
            })
        }

        fn active_stream(&self, display: u32) -> Result<PortalStream, String> {
            self.streams
                .iter()
                .find(|stream| stream.display.id == display)
                .copied()
                .ok_or_else(|| String::from("The selected display is no longer shared."))
        }
    }

    fn portal_error(problem: impl std::fmt::Display) -> String {
        format!("The Wayland desktop portal could not complete computer use: {problem}")
    }

    fn with_portal<T>(
        action: impl FnOnce(&mut PortalDesktop) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut held = WAYLAND_PORTAL
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| String::from("The Wayland desktop portal is busy."))?;
        if held
            .as_ref()
            .is_some_and(|portal| portal.last_used.elapsed() > std::time::Duration::from_secs(300))
        {
            *held = None;
        }
        if held.is_none() {
            *held = Some(PortalDesktop::start()?);
        }
        let portal = held.as_mut().expect("portal was initialized above");
        let result = action(portal);
        portal.last_used = std::time::Instant::now();
        if result.is_err() {
            *held = None;
        }
        result
    }

    fn portal_screenshot() -> Result<RgbaImage, String> {
        let response = futures::executor::block_on(async {
            Screenshot::request()
                .interactive(false)
                .send()
                .await
                .map_err(portal_error)?
                .response()
                .map_err(portal_error)
        })?;
        let path = response.uri().to_file_path().map_err(|_| {
            String::from("The Wayland screenshot portal returned a non-file image.")
        })?;
        let bytes = std::fs::read(path)
            .map_err(|problem| format!("The Wayland screenshot could not be read: {problem}"))?;
        image::load_from_memory(&bytes)
            .map(|image| image.to_rgba8())
            .map_err(|problem| format!("The Wayland screenshot could not be decoded: {problem}"))
    }

    fn wayland_displays() -> Result<Vec<DisplayInfo>, String> {
        with_portal(|portal| Ok(portal.streams.iter().map(|stream| stream.display).collect()))
    }

    fn wayland_capture_display(id: u32) -> Result<(DisplayInfo, RgbaImage), String> {
        let (display, streams) = with_portal(|portal| {
            let selected = portal.active_stream(id)?.display;
            Ok((
                selected,
                portal
                    .streams
                    .iter()
                    .map(|stream| stream.display)
                    .collect::<Vec<_>>(),
            ))
        })?;
        let image = portal_screenshot()?;
        if streams.len() <= 1
            || (image.width() == display.width && image.height() == display.height)
        {
            return Ok((display, image));
        }
        let min_x = streams.iter().map(|stream| stream.x).min().unwrap_or(0);
        let min_y = streams.iter().map(|stream| stream.y).min().unwrap_or(0);
        let max_x = streams
            .iter()
            .map(|stream| stream.x.saturating_add(stream.width as i32))
            .max()
            .unwrap_or(display.width as i32);
        let max_y = streams
            .iter()
            .map(|stream| stream.y.saturating_add(stream.height as i32))
            .max()
            .unwrap_or(display.height as i32);
        let logical_width = max_x.saturating_sub(min_x).max(1) as f64;
        let logical_height = max_y.saturating_sub(min_y).max(1) as f64;
        let scale_x = f64::from(image.width()) / logical_width;
        let scale_y = f64::from(image.height()) / logical_height;
        let left = ((display.x - min_x) as f64 * scale_x).round().max(0.0) as u32;
        let top = ((display.y - min_y) as f64 * scale_y).round().max(0.0) as u32;
        let width = (f64::from(display.width) * scale_x).round().max(1.0) as u32;
        let height = (f64::from(display.height) * scale_y).round().max(1.0) as u32;
        if left.saturating_add(width) > image.width() || top.saturating_add(height) > image.height()
        {
            return Err(String::from("The Wayland screenshot size did not match the selected display. Take a fresh screenshot."));
        }
        Ok((
            display,
            image::imageops::crop_imm(&image, left, top, width, height).to_image(),
        ))
    }

    pub fn displays() -> Result<Vec<DisplayInfo>, String> {
        if is_wayland() {
            wayland_displays()
        } else {
            displays_x11()
        }
    }

    pub fn capture_display(id: u32) -> Result<(DisplayInfo, RgbaImage), String> {
        if is_wayland() {
            wayland_capture_display(id)
        } else {
            capture_display_x11(id)
        }
    }

    pub fn capture_application_window(pid: u32, title: &str) -> Result<CapturedArea, String> {
        if is_wayland() {
            let (monitor, image) = wayland_capture_display(1)?;
            Ok(CapturedArea {
                image,
                x: monitor.x,
                y: monitor.y,
            })
        } else {
            capture_application_window_x11(pid, title)
        }
    }

    pub fn focused_window() -> Option<(String, String)> {
        if is_wayland() {
            None
        } else {
            focused_window_x11()
        }
    }

    pub fn wayland_pointer(display: u32, x: i32, y: i32) -> Result<(), String> {
        with_portal(|portal| {
            let stream = portal.active_stream(display)?;
            futures::executor::block_on(portal.remote.notify_pointer_motion_absolute(
                &portal.session,
                stream.node,
                f64::from(x - stream.display.x),
                f64::from(y - stream.display.y),
            ))
            .map_err(portal_error)
        })
    }

    pub fn wayland_button(button: u32, pressed: bool) -> Result<(), String> {
        with_portal(|portal| {
            futures::executor::block_on(portal.remote.notify_pointer_button(
                &portal.session,
                button as i32,
                if pressed {
                    KeyState::Pressed
                } else {
                    KeyState::Released
                },
            ))
            .map_err(portal_error)
        })
    }

    pub fn wayland_key(keysym: u32, pressed: bool) -> Result<(), String> {
        with_portal(|portal| {
            futures::executor::block_on(portal.remote.notify_keyboard_keysym(
                &portal.session,
                keysym as i32,
                if pressed {
                    KeyState::Pressed
                } else {
                    KeyState::Released
                },
            ))
            .map_err(portal_error)
        })
    }

    pub fn wayland_scroll(horizontal: i32, vertical: i32) -> Result<(), String> {
        use ashpd::desktop::remote_desktop::Axis;
        with_portal(|portal| {
            futures::executor::block_on(async {
                if vertical != 0 {
                    portal
                        .remote
                        .notify_pointer_axis_discrete(&portal.session, Axis::Vertical, vertical)
                        .await
                        .map_err(portal_error)?;
                }
                if horizontal != 0 {
                    portal
                        .remote
                        .notify_pointer_axis_discrete(&portal.session, Axis::Horizontal, horizontal)
                        .await
                        .map_err(portal_error)?;
                }
                Ok(())
            })
        })
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::{CapturedArea, DisplayInfo};
    use image::RgbaImage;

    pub fn displays() -> Result<Vec<DisplayInfo>, String> {
        xcap::Monitor::all()
            .map_err(|problem| format!("Could not read displays: {problem}"))?
            .into_iter()
            .map(|monitor| {
                Ok(DisplayInfo {
                    id: monitor.id().map_err(|problem| problem.to_string())?,
                    x: monitor.x().map_err(|problem| problem.to_string())?,
                    y: monitor.y().map_err(|problem| problem.to_string())?,
                    width: monitor.width().map_err(|problem| problem.to_string())?,
                    height: monitor.height().map_err(|problem| problem.to_string())?,
                    primary: monitor.is_primary().unwrap_or(false),
                })
            })
            .collect()
    }

    pub fn capture_display(id: u32) -> Result<(DisplayInfo, RgbaImage), String> {
        let monitor = xcap::Monitor::all()
            .map_err(|problem| format!("Screen capture is unavailable: {problem}"))?
            .into_iter()
            .find(|monitor| monitor.id().ok() == Some(id))
            .ok_or_else(|| String::from("The selected display is no longer available."))?;
        let info = DisplayInfo {
            id,
            x: monitor.x().map_err(|problem| problem.to_string())?,
            y: monitor.y().map_err(|problem| problem.to_string())?,
            width: monitor.width().map_err(|problem| problem.to_string())?,
            height: monitor.height().map_err(|problem| problem.to_string())?,
            primary: monitor.is_primary().unwrap_or(false),
        };
        let image = monitor
            .capture_image()
            .map_err(|problem| format!("The screen could not be captured: {problem}"))?;
        Ok((info, image))
    }

    pub fn capture_application_window(pid: u32, title: &str) -> Result<CapturedArea, String> {
        let window = xcap::Window::all()
            .map_err(|problem| format!("Could not list open windows: {problem}"))?
            .into_iter()
            .find(|window| {
                window.pid().ok() == Some(pid) && window.title().ok().as_deref() == Some(title)
            })
            .ok_or_else(|| {
                String::from("The application window is not visible to the screenshot service.")
            })?;
        let x = window.x().map_err(|problem| problem.to_string())?;
        let y = window.y().map_err(|problem| problem.to_string())?;
        let image = window
            .capture_image()
            .map_err(|problem| format!("Could not capture the application window: {problem}"))?;
        Ok(CapturedArea { image, x, y })
    }

    pub fn focused_window() -> Option<(String, String)> {
        xcap::Window::all()
            .ok()?
            .into_iter()
            .find(|window| window.is_focused().unwrap_or(false))
            .map(|window| {
                (
                    window.app_name().unwrap_or_default(),
                    window.title().unwrap_or_default(),
                )
            })
    }
}

pub fn displays() -> Result<Vec<DisplayInfo>, String> {
    platform::displays()
}

pub fn capture_display(id: u32) -> Result<(DisplayInfo, RgbaImage), String> {
    platform::capture_display(id)
}

pub fn capture_application_window(pid: u32, title: &str) -> Result<CapturedArea, String> {
    platform::capture_application_window(pid, title)
}

pub fn focused_window() -> Option<(String, String)> {
    platform::focused_window()
}

#[cfg(target_os = "linux")]
pub fn is_wayland() -> bool {
    platform::is_wayland()
}

#[cfg(target_os = "linux")]
pub fn wayland_pointer(display: u32, x: i32, y: i32) -> Result<(), String> {
    platform::wayland_pointer(display, x, y)
}

#[cfg(target_os = "linux")]
pub fn wayland_button(button: u32, pressed: bool) -> Result<(), String> {
    platform::wayland_button(button, pressed)
}

#[cfg(target_os = "linux")]
pub fn wayland_key(keysym: u32, pressed: bool) -> Result<(), String> {
    platform::wayland_key(keysym, pressed)
}

#[cfg(target_os = "linux")]
pub fn wayland_scroll(horizontal: i32, vertical: i32) -> Result<(), String> {
    platform::wayland_scroll(horizontal, vertical)
}
