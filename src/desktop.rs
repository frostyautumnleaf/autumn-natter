//! Desktop capture backends used by Computer use and the native browser panel.
//!
//! Linux follows Unsloth's supported X11 path. Keeping Wayland capture out of the default
//! dependency graph avoids pulling PipeWire's native development headers into every Linux build.

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
    use image::RgbaImage;
    use x11rb::connection::Connection;
    use x11rb::protocol::randr;
    use x11rb::protocol::xproto::{
        AtomEnum, ConnectionExt as XprotoConnectionExt, ImageFormat, ImageOrder,
    };
    use x11rb::rust_connection::RustConnection;

    fn ensure_x11() -> Result<(), String> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE")
                .is_ok_and(|session| session.eq_ignore_ascii_case("wayland"))
        {
            return Err(String::from(
                "Computer use on Linux requires an X11 (Xorg) session; Wayland is not supported.",
            ));
        }
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

    pub fn displays() -> Result<Vec<DisplayInfo>, String> {
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

    fn display_by_id(id: u32) -> Result<DisplayInfo, String> {
        displays()?
            .into_iter()
            .find(|display| display.id == id)
            .ok_or_else(|| String::from("The selected display is no longer available."))
    }

    pub fn capture_display(id: u32) -> Result<(DisplayInfo, RgbaImage), String> {
        let (connection, screen_number) = connect()?;
        let display = display_by_id(id)?;
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

    pub fn capture_application_window(_pid: u32, _title: &str) -> Result<CapturedArea, String> {
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

    pub fn focused_window() -> Option<(String, String)> {
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
            return Err(String::from("The display region is too large to capture safely."));
        }
        let x = i16::try_from(x)
            .map_err(|_| String::from("The display x position is out of range."))?;
        let y = i16::try_from(y)
            .map_err(|_| String::from("The display y position is out of range."))?;
        let width = u16::try_from(width)
            .map_err(|_| String::from("The display width is out of range."))?;
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
