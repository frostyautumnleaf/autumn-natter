// The picture marks of the window. Every mark is put into the program when it
// is built, so the window needs nothing from disk and from the network.
//
// The files hold black lines on a transparent ground. The colour is written
// into the text of the file when a mark is asked for. One file can then serve
// every colour of the palette, which the drawing of the window needs often.

use iced::widget::svg;
use iced::{Color, Length};

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

// The black that stands in every mark of this program.
const BLACK: &str = "#000";

// The text of one mark file. A name that is not on this list has no mark.
fn raw(name: &str) -> Option<&'static str> {
    Some(match name {
        "check" => include_str!("../../assets/icons/check.svg"),
        "chat" => include_str!("../../assets/icons/chat.svg"),
        "chevron-down" => include_str!("../../assets/icons/chevron-down.svg"),
        "code" => include_str!("../../assets/icons/code.svg"),
        "down" => include_str!("../../assets/icons/down.svg"),
        "folder" => include_str!("../../assets/icons/folder.svg"),
        "gpu" => include_str!("../../assets/icons/gpu.svg"),
        "left" => include_str!("../../assets/icons/left.svg"),
        "mark" => include_str!("../../assets/icons/autumn-natter.svg"),
        "model" => include_str!("../../assets/icons/model.svg"),
        "plus" => include_str!("../../assets/icons/plus.svg"),
        "right" => include_str!("../../assets/icons/right.svg"),
        "send" => include_str!("../../assets/icons/send.svg"),
        "settings" => include_str!("../../assets/icons/settings.svg"),
        "shield" => include_str!("../../assets/icons/shield.svg"),
        "snowflake" => include_str!("../../assets/icons/snowflake.svg"),
        "stop" => include_str!("../../assets/icons/stop.svg"),
        "terminal" => include_str!("../../assets/icons/terminal.svg"),
        "think" => include_str!("../../assets/icons/think.svg"),
        "tip" => include_str!("../../assets/icons/tooltip.svg"),
        "up" => include_str!("../../assets/icons/up.svg"),
        "warning" => include_str!("../../assets/icons/warning.svg"),
        _ => return None,
    })
}

// A colour as the six digits a mark file wants.
fn digits(ink: Color) -> String {
    let part = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
    format!("#{:02X}{:02X}{:02X}", part(ink.r), part(ink.g), part(ink.b))
}

// A mark in one colour. A frame asks for the same marks again and again, so the
// handles that were made are kept and used again.
pub fn handle(name: &str, ink: Color) -> Option<svg::Handle> {
    let tinted = raw(name)?.replace(BLACK, &digits(ink));
    static HELD: OnceLock<Mutex<HashMap<String, svg::Handle>>> = OnceLock::new();
    let held = HELD.get_or_init(Mutex::default);
    let Ok(mut kept) = held.lock() else {
        return Some(svg::Handle::from_memory(Cow::Owned(tinted.into_bytes())));
    };
    if let Some(found) = kept.get(&tinted) {
        return Some(found.clone());
    }
    let bytes = tinted.clone().into_bytes();
    let made = svg::Handle::from_memory(Cow::Owned(bytes));
    kept.insert(tinted, made.clone());
    Some(made)
}

// A mark at one size, ready to stand in a row or a column.
pub fn glyph(name: &str, ink: Color, size: f32) -> Option<svg::Svg<'static>> {
    let made = svg::Svg::new(handle(name, ink)?)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size));
    Some(made)
}

// A mark whose two sides are set apart, for a file that is not square.
pub fn glyph_room(
    name: &str,
    ink: Color,
    width: f32,
    height: f32,
) -> Option<svg::Svg<'static>> {
    let made = svg::Svg::new(handle(name, ink)?)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height));
    Some(made)
}
