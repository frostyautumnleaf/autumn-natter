// The colours of the window. Light mode is the default, and the settings window
// switches the whole program to dark mode.
//
// The five autumn colours are the only accents. Every other colour is a warm
// neutral, so the window never needs a colour that is not on this list.

use iced::Color;

// One colour channel, back to the part of a byte it came from.
fn channel(value: u32) -> f32 {
    (value & 255) as f32 / 255.0
}

// A colour from six hexadecimal digits, as the palette list writes them.
fn hex(value: u32) -> Color {
    Color::from_rgb(channel(value >> 16), channel(value >> 8), channel(value))
}

// A colour with a set amount of it left out, so the colour behind it shows
// through.
pub fn faded(ink: Color, amount: f32) -> Color {
    Color { a: amount, ..ink }
}

// Two colours mixed together. Zero keeps the first one, one keeps the second.
pub fn mix(one: Color, other: Color, share: f32) -> Color {
    let left = 1.0 - share;
    Color {
        r: one.r * left + other.r * share,
        g: one.g * left + other.g * share,
        b: one.b * left + other.b * share,
        a: one.a * left + other.a * share,
    }
}

// Every colour one drawing step needs. It is made once for a frame, from the
// one flag that says which palette is in use. It is small enough to copy.
// The palette is copied by value, so a part can keep its colours without
// borrowing from the window that drew it.
#[derive(Clone, Copy)]
pub struct Pal {
    // Window and chat background.
    pub bg: Color,
    // Raised surfaces: cards, popups, the composer box.
    pub surface: Color,
    // Secondary surface: hover, selected rows, blocks of words.
    pub surface_alt: Color,
    // Hairlines and borders.
    pub line: Color,
    // Hairlines that must be seen against a surface of their own.
    pub line_strong: Color,
    // Main text colour.
    pub text: Color,
    // Secondary text colour.
    pub text_dim: Color,
    // Soft autumn tint for selected items.
    pub tint: Color,
    // Text drawn on top of an accent colour.
    pub on_accent: Color,
    // Hover colour for buttons and rows.
    pub hover: Color,

    // Autumn accents.
    pub accent: Color,
    pub accent_warm: Color,
    pub accent_strong: Color,
    pub accent_leaf: Color,
    pub accent_deep: Color,

    // The bubble of the user, with a hairline of its own, so a bubble keeps its
    // shape on a card of its own. The words of the model stand plain on the
    // card, so they need no colour of their own.
    pub bubble_user: Color,
    pub bubble_user_line: Color,
    // Panels for words of a special kind: a command, its output, the thinking.
    pub code_bg: Color,
    pub think_bg: Color,
    // Colour for a failed tool run.
    pub danger: Color,
}

impl Pal {
    // The palette of one mode. True gives the dark colours.
    pub fn new(dark: bool) -> Pal {
        Pal {
            bg: if dark { hex(0x141414) } else { hex(0xFAF2E6) },
            surface: if dark { hex(0x1B1B1B) } else { hex(0xFFF9F0) },
            surface_alt: if dark { hex(0x242424) } else { hex(0xF3E7D5) },
            line: if dark { hex(0x342F2A) } else { hex(0xE6D7C2) },
            line_strong: if dark { hex(0x463F37) } else { hex(0xD6C0A2) },
            text: if dark { hex(0xF2EBE2) } else { hex(0x231A11) },
            text_dim: if dark { hex(0xA4998C) } else { hex(0x7B6A57) },
            tint: if dark { hex(0x2C1D10) } else { hex(0xFBE6D1) },
            on_accent: hex(0xFAF2E6),
            hover: if dark { hex(0x2A2A2A) } else { hex(0xF8ECD9) },

            accent: hex(0xED7401),
            accent_warm: hex(0xDF6101),
            accent_strong: hex(0xC53C01),
            accent_leaf: hex(0xAC4422),
            accent_deep: hex(0xAF2600),

            bubble_user: if dark { hex(0x2B1C10) } else { hex(0xFBE6D1) },
            bubble_user_line: if dark { hex(0x4A3626) } else { hex(0xF0D2B0) },
            code_bg: if dark { hex(0x201D1A) } else { hex(0xF6ECDC) },
            think_bg: if dark { hex(0x221E1B) } else { hex(0xF5EAD9) },
            danger: hex(0xAF2600),
        }
    }
}
