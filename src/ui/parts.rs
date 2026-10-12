// The small parts the window is made of. Every part draws itself with the
// palette it is given, so nothing takes the colours of the desktop and nothing
// stands blue in a window of autumn colours.

use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, space, text, text_editor,
    text_input,
};
use iced::widget::{overlay, slider, Stack, pin};
use iced::{alignment, border, font, Background, Color, Element, Font, Length, Theme};

use super::icon;
use super::palette::Pal;
use crate::ui_app::{Msg, Tip};

// What every part of this window gives back.
pub type Elem = Element<'static, Msg>;

// A mark of the program, in one colour and one size. Nothing is drawn when the
// mark is unknown, so a window never falls over over a missing file.
pub fn glyph(name: &str, ink: Color, size: f32) -> Elem {
    icon::glyph(name, ink, size)
        .map(Elem::from)
        .unwrap_or_else(|| word(String::new(), ink, 1.0))
}

// The mark of the program, for the top bar and for the first screen.
pub fn mark(ink: Color) -> Elem {
    icon::glyph_room("mark", ink, 28.0, 28.0)
        .map(Elem::from)
        .unwrap_or_else(|| word(String::new(), ink, 1.0))
}

// A font of heavier words, for the chief word of a row.
pub const BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..Font::DEFAULT
};
// A font of fixed width, for a command and for the output of one.
pub const MONO: Font = Font::MONOSPACE;

// Rough room that words take. A box is made as wide as the longest name of its
// list, and this says how much room the letters need. The number for one letter
// sits a little above what the fonts really need, so a name stays inside.
pub fn words(body: &str, size: f32) -> f32 {
    (body.chars().count() as f32) * size * 0.60
}

// Words in one colour and one size.
pub fn word(body: impl Into<String>, ink: Color, size: f32) -> Element<'static, Msg> {
    text(body.into()).size(size).color(ink).into()
}

// Words that stand out, for a title or a head.
pub fn strong(body: impl Into<String>, ink: Color, size: f32) -> Element<'static, Msg> {
    text(body.into())
        .size(size)
        .color(ink)
        .font(BOLD)
        .into()
}

// Words of fixed width, for a command, its output and a file.
pub fn mono(body: impl Into<String>, ink: Color, size: f32) -> Element<'static, Msg> {
    text(body.into()).size(size).color(ink).font(MONO).into()
}

// One line of words that never starts a second line. It is cut short when it
// does not fit, so a long name stays on the line it stands on. The box it
// stands in must be told to cut, with `clip`.
pub fn word_one(
    body: impl Into<String>,
    ink: Color,
    size: f32,
) -> Element<'static, Msg> {
    text(body.into())
        .size(size)
        .color(ink)
        .wrapping(iced::widget::text::Wrapping::None)
        .into()
}

// Words cut at a given width, centred in it.
pub fn word_cut(
    body: impl Into<String>,
    ink: Color,
    size: f32,
    centre: bool,
    width: f32,
) -> Element<'static, Msg> {
    let line = text(body.into())
        .size(size)
        .color(ink)
        .width(Length::Fixed(width));
    if centre {
        return line.align_x(iced::alignment::Horizontal::Center).into();
    }
    line.into()
}

// One line of words of fixed width that never starts a second line. It is used
// for a file, which must stay on the line it stands on.
pub fn mono_one(body: impl Into<String>, ink: Color, size: f32) -> Element<'static, Msg> {
    text(body.into())
        .size(size)
        .color(ink)
        .font(MONO)
        .wrapping(iced::widget::text::Wrapping::None)
        .into()
}

// A small rounded label, for example GGUF or Safetensors. It stands beside the
// name of a model, in the top bar and in the window of one model.
pub fn pill(label: &str, pal: Pal) -> Element<'static, Msg> {
    container(word(label.to_string(), pal.accent_text, 11.0))
        .padding([3.0, 7.0])
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.tint)),
            border: border::rounded(7.0),
            ..container::Style::default()
        })
        .into()
}

// A round mark. It lights while the machine works, and sleeps when it is quiet.
pub fn dot(lit: bool, size: f32, on: Color, off: Color) -> Element<'static, Msg> {
    let ink = if lit { on } else { off };
    container(space())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_| container::Style {
            background: Some(Background::Color(ink)),
            border: border::rounded(size / 2.0),
            ..container::Style::default()
        })
        .into()
}

// A line one hair thick across the whole room it is given. It is put between two
// parts of a list, so a line of words that does not belong to the list is seen
// apart.
pub fn hair_across(ink: Color) -> Element<'static, Msg> {
    container(space())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(ink)),
            ..container::Style::default()
        })
        .into()
}

// One line of a list that comes down from a box of the top bar, and the same
// shape in a list of a window of settings. The line in use stands warmer, with a
// mark before it.
pub fn entry(label: String, live: bool, on: Msg, pal: Pal) -> Element<'static, Msg> {
    let content = row![
        dot(live, 6.0, pal.accent, pal.line),
        container(word_one(label, if live { pal.accent_text } else { pal.text }, 12.5))
            .width(Length::Fill)
            .clip(true),
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center);
    press(
        container(content)
            .width(Length::Fill)
            .padding([6.0, 8.0]),
        if live { pal.tint } else { pal.surface },
        Color::TRANSPARENT,
        8.0,
        0.0,
        pal.text,
        Some(on),
        pal,
    )
}

// The rail of a scroll bar, in the colours of the window. The rail of the
// widget set is grey and thick, so it is drawn here instead.
pub fn rail(pal: Pal) -> scrollable::Rail {
    scrollable::Rail {
        // The track of the rail is left out, so only the short bar that shows
        // how far the list has moved stands in the window.
        background: None,
        border: border::rounded(2.0),
        scroller: scrollable::Scroller {
            // A little of the surface is mixed in, so the bar does not stand out
            // of the window more than it needs to.
            background: Background::Color(super::palette::mix(pal.text_dim, pal.surface, 0.35)),
            border: border::rounded(2.0),
        },
    }
}

// A list that is longer than the room it has. The rail is thin and takes the
// colours of the window, so no grey bar of the widget set stands in a form.
pub fn scroll<'a>(
    body: impl Into<Element<'a, Msg>> + 'a,
    tall: Length,
    pal: Pal,
) -> Element<'a, Msg> {
    scrollable(body)
        .direction(iced::widget::scrollable::Direction::Vertical(
            iced::widget::scrollable::Scrollbar::new()
                .width(6.0)
                .scroller_width(4.0)
                .margin(3.0),
        ))
        .height(tall)
        .style(move |_theme: &Theme, _state| scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail(pal),
            horizontal_rail: rail(pal),
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: Background::Color(pal.surface_alt),
                border: border::rounded(6.0).color(pal.line).width(1.0),
                shadow: iced::Shadow::default(),
                icon: pal.text_dim,
            },
        })
        .into()
}

// A line one hair thick. Its place is always set by hand, because a line with
// no place of its own falls in the middle of the words next to it.
pub fn hair(width: f32, ink: Color) -> Element<'static, Msg> {
    container(space())
        .width(Length::Fixed(width))
        .height(Length::Fixed(1.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(ink)),
            ..container::Style::default()
        })
        .into()
}

// Room of a set size, to push parts apart.
pub fn room(size: f32) -> Element<'static, Msg> {
    container(space())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

// Room of a set size in one direction only.
pub fn room_across(width: f32, height: f32) -> Element<'static, Msg> {
    container(space())
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}

// A box that can be pressed. The pointer makes it a little warmer, and holding
// it down makes it warmer still. Every colour of the part comes from the
// palette, so the list of what a caller must hand over is long.
#[allow(clippy::too_many_arguments)]
pub fn press<'a>(
    content: impl Into<Element<'a, Msg>>,
    fill: Color,
    line: Color,
    radius: f32,
    thick: f32,
    ink: Color,
    on: Option<Msg>,
    pal: Pal,
) -> Element<'a, Msg> {
    let warm = super::palette::mix(fill, pal.hover, 0.55);
    // The button takes no padding of its own. The box handed to it carries the
    // room its words need, and padding here would be counted twice.
    button(content)
        .padding(0.0)
        .style(move |_theme: &Theme, state| {
            let made = match state {
                button::Status::Pressed => warm,
                button::Status::Hovered => warm,
                _ => fill,
            };
            button::Style {
                background: Some(Background::Color(made)),
                text_color: ink,
                border: border::rounded(radius).color(line).width(thick),
                ..button::Style::default()
            }
        })
        .on_press_maybe(on)
        .into()
}

// A mark with a word beside it, as the three marks of the chat box stand. A mark
// that cannot be used stays dim and does nothing.
pub fn badge(
    body: &str,
    mark: &str,
    active: bool,
    live: bool,
    on: Option<Msg>,
    pal: Pal,
) -> Element<'static, Msg> {
    let ink = if !live {
        pal.text_dim
    } else if active {
        pal.accent_text
    } else {
        pal.text_dim
    };
    let mut line = row![].spacing(6).align_y(alignment::Vertical::Center);
    if live {
        line = line.push(glyph(mark, ink, 14.0));
    }
    line = line.push(word(body.to_string(), ink, 12.0));
    let box_fill = if active && live { pal.tint } else { pal.surface };
    let edge = if active && live { pal.accent } else { pal.line };
    press(
        container(line).padding([4.0, 10.0]),
        box_fill,
        edge,
        13.0,
        1.0,
        ink,
        on,
        pal,
    )
}

// A square of one mark. It is the plain shape for the two arrows of the
// carousel and for the mark of the settings.
pub fn mark_button(name: &str, on: Option<Msg>, pal: Pal) -> Element<'static, Msg> {
    let live = on.is_some();
    // The mark stands in the middle of its square. `center_x` and `center_y`
    // are not used here, because they also set the size of the box, and a box
    // of `Fill` would stretch the mark across the whole row.
    let inside = container(glyph(name, if live { pal.text } else { pal.text_dim }, 30.0))
        .width(Length::Fixed(30.0))
        .height(Length::Fixed(30.0))
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center);
    press(
        inside,
        pal.surface,
        Color::TRANSPARENT,
        8.0,
        0.0,
        pal.text,
        on,
        pal,
    )
}

// A button with a word of its own. It is the plain shape for Create, Save and
// Close.
pub fn word_button(
    body: &str,
    firm: bool,
    on: Msg,
    pal: Pal,
) -> Element<'static, Msg> {
    let quiet = if firm { pal.accent } else { pal.surface };
    let near = if firm { pal.accent_warm } else { pal.hover };
    let ink = if firm { pal.on_accent } else { pal.text };
    // The words keep their own height: room above and below them, and no fixed
    // height that could cut the letters.
    let label = container(word(body.to_string(), ink, 13.0)).padding([7.0, 14.0]);
    let _ = near;
    press(
        label,
        quiet,
        if firm {
            Color::TRANSPARENT
        } else {
            pal.line
        },
        8.0,
        if firm { 0.0 } else { 1.0 },
        ink,
        Some(on),
        pal,
    )
}

// A field of one line of words, with a hint for what stands in it.
pub fn field(
    value: &str,
    hint: &str,
    on: impl Fn(String) -> Msg + 'static,
    pal: Pal,
) -> Element<'static, Msg> {
    let ink = pal.text;
    let dim = pal.text_dim;
    let ground = pal.surface;
    let line = pal.line;
    let edge = pal.accent;
    text_input(hint, value)
        .on_input(on)
        .padding([6.0, 10.0])
        .size(13.0)
        .style(move |_theme: &Theme, state| {
            let held = matches!(state, text_input::Status::Focused { .. });
            text_input::Style {
                background: Background::Color(ground),
                border: border::rounded(8.0)
                    .color(if held { edge } else { line })
                    .width(1.0),
                icon: dim,
                placeholder: dim,
                value: ink,
                selection: edge,
            }
        })
        .into()
}

// A field that holds a number. Words that are not a number are left out, so the
// value the program holds is always a real one. The number is kept between the
// two ends it is given, which are the ends the server takes.
pub fn number_field(
    value: i64,
    low: i64,
    top: i64,
    on: impl Fn(i64) -> Msg + 'static,
    pal: Pal,
) -> Element<'static, Msg> {
    field(
        &value.to_string(),
        "",
        move |words: String| {
            let clean: String = words
                .chars()
                .filter(|one| one.is_ascii_digit() || *one == '-')
                .collect();
            let made = clean.parse::<i64>().unwrap_or(low);
            on(made.clamp(low, top))
        },
        pal,
    )
}

// A box of several lines of words, for the extra arguments and for the chat
// template.
pub fn area<'a>(
    held: &'a iced::widget::text_editor::Content,
    hint: &'a str,
    height: f32,
    on: impl Fn(iced::widget::text_editor::Action) -> Msg + 'static,
    fixed: bool,
    pal: Pal,
) -> Element<'a, Msg> {
    let ink = pal.text;
    let dim = pal.text_dim;
    let ground = pal.surface;
    let line = pal.line;
    let edge = pal.accent;
    let body = if fixed { MONO } else { Font::DEFAULT };
    text_editor(held)
        .placeholder(hint)
        .height(Length::Fixed(height))
        .on_action(on)
        .padding([8.0, 10.0])
        .size(if fixed { 12.0 } else { 13.0 })
        .font(body)
        .style(move |_theme: &Theme, state| {
            let held = matches!(state, text_editor::Status::Focused { .. });
            text_editor::Style {
                background: Background::Color(ground),
                border: border::rounded(8.0)
                    .color(if held { edge } else { line })
                    .width(1.0),
                placeholder: dim,
                value: ink,
                selection: edge,
            }
        })
        .into()
}

// A list of choices that opens when it is pressed. The number of the choice is
// what the program wants; the words are what the reader sees.
#[derive(Clone, PartialEq)]
pub struct Choice {
    pub number: usize,
    pub label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, sink: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        sink.write_str(&self.label)
    }
}

// The choices of a list, numbered.
pub fn choices(names: &[&'static str]) -> Vec<Choice> {
    names
        .iter()
        .enumerate()
        .map(|(number, name)| Choice {
            number,
            label: (*name).to_string(),
        })
        .collect()
}

// A box that holds one choice of a list.
pub fn chooser<'a>(
    options: &'a [Choice],
    index: usize,
    on: impl Fn(usize) -> Msg + 'static,
    pal: Pal,
) -> Element<'a, Msg> {
    if options.is_empty() {
        // A list with nothing in it is drawn as an empty box, so the window does
        // not fall over when a machine has no choice to offer.
        return container(space())
            .width(Length::Fill)
            .height(Length::Fixed(30.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(pal.surface)),
                border: border::rounded(8.0).color(pal.line).width(1.0),
                ..container::Style::default()
            })
            .into();
    }
    let picked = index.min(options.len() - 1);
    let ink = pal.text;
    let dim = pal.text_dim;
    let ground = pal.surface;
    let line = pal.line;
    let edge = pal.accent;
    let tint = pal.tint;
    pick_list(options, Some(&options[picked]), move |chosen: Choice| on(chosen.number))
        .text_size(13.0)
        .padding([6.0, 10.0])
        .menu_height(Length::Fixed(240.0))
        .style(move |_theme: &Theme, _state| pick_list::Style {
            text_color: ink,
            placeholder_color: dim,
            handle_color: dim,
            background: Background::Color(ground),
            border: border::rounded(8.0).color(line).width(1.0),
        })
        .menu_style(move |_theme: &Theme| overlay::menu::Style {
            background: Background::Color(ground),
            border: border::rounded(10.0).color(edge).width(1.0),
            text_color: ink,
            selected_text_color: pal.accent_strong,
            selected_background: Background::Color(tint),
            shadow: iced::Shadow {
                color: iced::Color {
                    a: 0.18,
                    ..iced::Color::BLACK
                },
                offset: iced::Vector::new(0.0, 3.0),
                blur_radius: 12.0,
            },
        })
        .into()
}

// A line the user sets a number on by sliding a knob. The number may not leave
// the two ends, and it moves whole steps at a time.
pub fn line(
    value: i64,
    low: i64,
    top: i64,
    step: i64,
    on: impl Fn(f64) -> Msg + 'static,
    pal: Pal,
) -> Element<'static, Msg> {
    let held = (value as f64).clamp(low as f64, top as f64);
    slider(
        low as f64..=top as f64,
        held,
        move |fresh: f64| on(fresh),
    )
    .step(step.max(1) as f64)
    .style(move |_theme: &Theme, _state| slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(pal.line),
                Background::Color(pal.accent),
            ),
            width: 4.0,
            border: border::rounded(2.0),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 7.0 },
            background: Background::Color(pal.accent_strong),
            border_width: 1.0,
            border_color: pal.on_accent,
        },
    })
    .into()
}

// A switch of two places. It is drawn here, because a switch of the widget set
// takes the accent colour of the desktop. The knob stands at the left when the
// switch is off, and at the right when it is on, so the place of the knob says
// what the colour says.
pub fn switch(on: bool, live: bool, change: Option<Msg>, pal: Pal) -> Element<'static, Msg> {
    let knob = container(space())
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(if live {
                pal.on_accent
            } else {
                pal.text_dim
            })),
            border: border::rounded(8.0),
            ..container::Style::default()
        });
    // The track keeps the size it is given. `align_right` and `align_left` set
    // the width of the box as well, so the width handed to them is the width of
    // the track. A `Fill` there would stretch the track over the whole window.
    let track = if on {
        container(knob)
            .height(Length::Fixed(22.0))
            .padding(3.0)
            .align_right(Length::Fixed(40.0))
    } else {
        container(knob)
            .height(Length::Fixed(22.0))
            .padding(3.0)
            .align_left(Length::Fixed(40.0))
    };
    let track = track
        .style(move |_| container::Style {
            background: Some(Background::Color(if on {
                pal.accent
            } else {
                pal.surface_alt
            })),
            border: border::rounded(11.0).color(pal.line).width(1.0),
            ..container::Style::default()
        });
    match change {
        None => container(track).into(),
        Some(step) => mouse_area(track)
            .on_press(step)
            .interaction(iced::mouse::Interaction::Pointer)
            .into(),
    }
}

// Three marks that light one after the other while the machine works.
pub fn busy(working: bool, step: usize, pal: Pal) -> Element<'static, Msg> {
    let ink = pal.accent;
    let mut line = row![].spacing(3);
    for place in 0..3 {
        let lit = working && place == step;
        line = line.push(
            container(space())
                .width(Length::Fixed(4.0))
                .height(Length::Fixed(4.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(super::palette::faded(
                        ink,
                        if lit { 1.0 } else { 0.28 },
                    ))),
                    border: border::rounded(2.0),
                    ..container::Style::default()
                }),
        );
    }
    line.into()
}

// A part with a help bubble: a question mark stands at its edge while the
// pointer is over it, and the bubble comes when the pointer stays on the mark.
pub fn tipped<'a>(
    part: impl Into<Element<'a, Msg>>,
    note: &str,
    pal: Pal,
) -> Element<'a, Msg> {
    if note.is_empty() {
        return part.into();
    }
    let note = note.to_string();
    let mark = icon::glyph("tip", pal.accent_strong, 15.0)
        .map(Element::from)
        .unwrap_or_else(|| word(String::new(), pal.text_dim, 1.0));
    let asked = mouse_area(mark)
        .on_enter(Msg::TipOn(note))
        .on_exit(Msg::TipOff)
        .interaction(iced::mouse::Interaction::Help);
    iced::widget::hover(
        part.into(),
        container(asked)
            .align_left(Length::Fill)
            .align_bottom(Length::Fill),
    )
}

// The help bubble, laid over a window. It comes when the pointer has stayed
// still on the mark of a control, and it stands beside the pointer, so the
// reader sees which control the words belong to. A control at the right edge of
// the window asks for the bubble on the left of the pointer. The bubble is not
// drawn at all before its wait is over.
pub fn tip_over<'a>(
    tip: Option<&Tip>,
    pointer: Option<(f32, f32)>,
    wide: f32,
    tall: f32,
    below: Element<'a, Msg>,
    pal: Pal,
) -> Element<'a, Msg> {
    let Some(held) = tip.filter(|wanted| wanted.shown()) else {
        return below;
    };
    let bubble_wide = 268.0f32.min((wide - 12.0).max(140.0));
    // A window that has not seen the pointer yet takes the bubble under its
    // top bar, which is where the marks of the window stand.
    let (x, y) = pointer.unwrap_or((wide - 24.0, 14.0));
    // The bubble stands to the right of the pointer, and is pulled back inside
    // the window when there is no room for the whole of it there.
    let want = x + 12.0;
    let place_x = want.max(6.0).min((wide - bubble_wide - 6.0).max(6.0));
    let place_y = (y + 16.0).max(6.0).min((tall - 70.0).max(6.0));
    Stack::with_capacity(2)
        .push(below)
        .push(pin(bubble(&held.note, wide, pal)).x(place_x).y(place_y))
        .into()
}

// The help bubble itself. The window draws it last, over the rest, and sets its
// place. Words that do not fit go under the line before them.
pub fn bubble(note: &str, room_width: f32, pal: Pal) -> Element<'static, Msg> {
    let wide = 268.0f32.min((room_width - 12.0).max(140.0));
    container(container(word(note.to_string(), pal.text, 12.0)).width(Length::Fixed(wide - 18.0)))
        .padding([9.0, 9.0])
        .width(Length::Fixed(wide))
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.surface_alt)),
            border: border::rounded(6.0).color(pal.line).width(1.0),
            ..container::Style::default()
        })
        .into()
}

// The head of a block of fields.
pub fn section(title: &str, pal: Pal) -> Element<'static, Msg> {
    column![
        row![
            container(space())
                .width(Length::Fixed(3.0))
                .height(Length::Fixed(14.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(pal.accent)),
                    border: border::rounded(2.0),
                    ..container::Style::default()
                }),
            room(7.0),
            strong(title.to_string(), pal.text, 13.0),
        ]
        .spacing(0)
        .align_y(alignment::Vertical::Center),
        hair(1.0, Color::TRANSPARENT),
    ]
    .spacing(4)
    .into()
}

// One line of a settings window: the name of the field, and the field.
pub fn line_of<'a>(
    name: &str,
    note: &str,
    control: impl Into<Element<'a, Msg>>,
    pal: Pal,
) -> Element<'a, Msg> {
    // The name keeps a room of its own and no height of its own, because a name
    // that needs two lines must have the room of two lines.
    let named = container(strong(name.to_string(), pal.text, 13.0)).width(Length::Fixed(190.0));
    // The room of the control is its own. `center_y` is not used, because it
    // sets the height of the box as well, and a tall control would be cut.
    row![
        tipped(named, note, pal),
        container(control)
            .width(Length::Fill)
            .align_y(alignment::Vertical::Center),
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center)
    .into()
}
