// The main window: the top bar, the carousel of chats, the row of previews, the
// first screen, and the box where a message is written.
//
// The window is drawn in layers over a ground of its own size. A place that more
// than one layer needs comes from Shape, so the chat card and the box below it
// always stand in line.

use iced::widget::{button, container, mouse_area, row, scrollable, space, text_editor};
use iced::{border, window, Background, Color, Element, Length, Theme, Alignment};
use iced::widget::{Column, Row, Stack, pin};

use crate::engine::Snap;
use crate::ui_app::{App, Card, Menu, Msg};
use super::message_view;
use super::palette::{faded, mix, Pal};
use super::parts::{self, word, word_cut, Elem};
use super::shape::Shape;

// Rough room that words take. A box is made as wide as the longest name of its
// list, and this says how much room the letters need. The number for one letter
// sits a little above what the fonts really need, so a name stays inside.
fn words(body: &str, size: f32) -> f32 {
    (body.chars().count() as f32) * size * 0.60
}

// A flat box of one colour, with a hairline and rounded corners.
fn plate(wide: f32, tall: f32, fill: iced::Color, line: iced::Color, round: f32) -> Elem {
    container(space())
        .width(Length::Fixed(wide))
        .height(Length::Fixed(tall))
        .style(move |_| container::Style {
            background: Some(Background::Color(fill)),
            border: border::rounded(round).color(line).width(if line.a > 0.0 { 1.0 } else { 0.0 }),
            ..container::Style::default()
        })
        .into()
}

// The whole window, at the size it has now.
pub fn draw<'a>(app: &'a App, window_id: window::Id, pal: Pal) -> Element<'a, Msg> {
    let shape = app.shape();
    let wide = shape.width;
    let tall = shape.height;
    let mut layers = Stack::new();

    // The ground, so no corner of the window is left without a colour.
    layers = layers.push(plate(wide, tall, pal.bg, transparent(), 0.0));

    // The top bar, with the hairline under it.
    layers = layers.push(
        pin(top_bar(app, pal, &shape))
            .x(0.0)
            .y(0.0)
            .width(wide)
            .height(shape.bar),
    );

    // The stage: the carousel, the washes, and the row of previews.
    layers = layers.push(
        pin(stage(app, pal, &shape))
            .x(0.0)
            .y(shape.stage_top)
            .width(wide)
            .height(shape.stage_height),
    );

    // The mark of the program and the line for this hour, while no chat is
    // open. They stand above the box and never over a chat.
    if app.greet {
        layers = layers.push(
            pin(greeting(app, pal, &shape))
                .x(0.0)
                .y((shape.dock_y - 178.0).max(0.0))
                .width(wide)
                .height(156.0),
        );
    }

    // The status line and the box for a new message.
    layers = layers.push(
        pin(dock(app, pal, &shape))
            .x(shape.card_x)
            .y(shape.dock_y)
            .width(shape.card_width)
            .height(shape.dock_height),
    );

    // One list of the top bar, when it is open. It stands under the bar, at the
    // left, where the two selector boxes stand.
    if app.menu != Menu::None {
        let tall_list = menu_height(app).min(tall - shape.bar - 20.0);
        layers = layers.push(
            pin(menu(app, pal))
                .x(12.0)
                .y(shape.bar + 6.0)
                .width(420.0)
                .height(tall_list),
        );
    }

    // The help bubble stands over everything else, and takes no click.
    parts::tip_over(
        app.tip.as_ref(),
        app.pointer_at(window_id),
        wide,
        tall,
        layers.into(),
        pal,
    )
}

// A colour that shows nothing. It stands where a hairline is not wanted.
fn transparent() -> iced::Color {
    iced::Color::TRANSPARENT
}

// Rough height of a list of the top bar, so the layer under it has a size.
fn menu_height(app: &App) -> f32 {
    match app.menu {
        Menu::Model => 42.0 * (app.snap.models.len() as f32).max(1.0) + 16.0,
        Menu::Repository => 38.0 * (app.snap.repositories.len() as f32 + 2.0) + 16.0,
        Menu::None => 0.0,
    }
    .min(420.0)
}

// The bar at the top of the window. The wide parts have a limit that follows
// the width of the window, and the chat name takes what is left. Nothing in
// this bar may push another part out of the window.
fn top_bar(app: &App, pal: Pal, shape: &Shape) -> Elem {
    let snap = &app.snap;
    let mut line = Row::new().spacing(10.0).align_y(Alignment::Center);

    // The mark of the program, and its name while there is room.
    let mark_wide = if shape.tight {
        28.0
    } else {
        38.0 + words("Autumn Natter", 13.5)
    };
    line = line.push(mark_of_the_program(mark_wide, pal));

    // The model box. It is as wide as the longest name of the list, with the
    // kind of the model behind it, so a name is never cut down.
    let kind_room = if snap.model_kind.is_empty() {
        0.0
    } else {
        words(snap.model_kind.as_str(), 11.0) + 45.0
    };
    let model_wide =
        (86.0 + words(longest_model(snap).as_str(), 13.0) + kind_room).clamp(118.0, shape.model_cap);
    line = line.push(model_box(app, snap, pal, model_wide));

    // The repository box, when the chat of the middle belongs to a repository.
    if snap.in_repository {
        let repo_wide = (66.0 + words(longest_repository(snap).as_str(), 13.0))
            .clamp(104.0, shape.repo_cap);
        line = line.push(repository_box(snap, pal, repo_wide));
    }

    // The two marks that turn the carousel, with the place of this chat between
    // them.
    line = line.push(roll_box(snap, pal, shape));

    // The name of the chat. It is the chief word of this bar, so it takes the
    // room that is left.
    line = line.push(
        container(word_cut(
            snap.chat_title.clone(),
            pal.text,
            13.5,
            false,
            40.0,
        ))
        .width(Length::Fill)
        .height(Length::Fixed(34.0)),
    );

    // The tokens in use, with a thin bar under the words.
    line = line.push(context_box(snap, pal, shape));

    container(line)
        .width(Length::Fixed(shape.width))
        .height(Length::Fixed(shape.bar))
        .padding([0.0, 12.0])
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.bg)),
            ..container::Style::default()
        })
        .into()
}

// The mark of the program, with the name of the program beside it while there
// is room.
fn mark_of_the_program(wide: f32, pal: Pal) -> Elem {
    let mut line = Row::new().spacing(10.0).align_y(Alignment::Center);
    line = line.push(
        container(parts::mark(pal.accent))
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0)),
    );
    if wide > 40.0 {
        line = line.push(word("Autumn Natter", pal.text, 13.5));
    }
    container(line)
        .width(Length::Fixed(wide))
        .height(Length::Fixed(34.0))
        .into()
}

// The box that shows which model is in use. One click brings its list up.
fn model_box(app: &App, snap: &Snap, pal: Pal, wide: f32) -> Elem {
    let open = app.menu == Menu::Model;
    let line = if open { pal.accent } else { pal.line };
    let ink = if snap.model_name.is_empty() {
        pal.text_dim
    } else {
        pal.text
    };
    let mut inside = Row::new().spacing(8.0).align_y(Alignment::Center);
    inside = inside.push(
        container(parts::glyph("model", pal.accent_warm, 15.0))
            .width(Length::Fixed(15.0))
            .height(Length::Fixed(15.0)),
    );
    inside = inside.push(
        container(word_cut(
            if snap.model_name.is_empty() {
                String::from("No model")
            } else {
                snap.model_name.clone()
            },
            ink,
            13.0,
            false,
            (wide - 78.0).max(40.0),
        ))
        .width(Length::Fill),
    );
    if !snap.model_kind.is_empty() {
        inside = inside.push(pill(snap.model_kind.as_str(), pal));
    }
    inside = inside.push(
        container(parts::glyph("down", pal.text_dim, 13.0))
            .width(Length::Fixed(13.0))
            .height(Length::Fixed(13.0)),
    );
    box_button(
        inside,
        Length::Fixed(wide),
        Length::Fixed(34.0),
        pal.surface,
        line,
        9.0,
        Some(Msg::MenuOpen(Menu::Model)),
        pal,
    )
}

// A small rounded label, for example GGUF or Safetensors.
fn pill(label: &str, pal: Pal) -> Elem {
    container(word_cut(label.to_string(), pal.accent_strong, 11.0, false, 150.0))
        .padding([3.0, 7.0])
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.tint)),
            border: border::rounded(7.0),
            ..container::Style::default()
        })
        .into()
}

// The box that shows which repository the carousel belongs to.
fn repository_box(snap: &Snap, pal: Pal, wide: f32) -> Elem {
    let mut inside = Row::new().spacing(7.0).align_y(Alignment::Center);
    inside = inside.push(
        container(parts::glyph("folder", pal.accent_warm, 15.0))
            .width(Length::Fixed(15.0))
            .height(Length::Fixed(15.0)),
    );
    inside = inside.push(
        container(word_cut(
            snap.repository_name.clone(),
            pal.text,
            13.0,
            false,
            40.0,
        ))
        .width(Length::Fill),
    );
    inside = inside.push(
        container(parts::glyph("down", pal.text_dim, 13.0))
            .width(Length::Fixed(13.0))
            .height(Length::Fixed(13.0)),
    );
    box_button(
        inside,
        Length::Fixed(wide),
        Length::Fixed(34.0),
        pal.surface,
        pal.line,
        9.0,
        Some(Msg::MenuOpen(Menu::Repository)),
        pal,
    )
}

// A box of the top bar that answers one click. It grows paler under the
// pointer, which is how the reader sees that it can be pressed. Every colour
// comes from the palette, so the list of what a caller hands over is long.
#[allow(clippy::too_many_arguments)]
pub fn box_button(
    content: Row<'static, Msg>,
    wide: Length,
    tall: Length,
    base: iced::Color,
    line: iced::Color,
    round: f32,
    action: Option<Msg>,
    pal: Pal,
) -> Elem {
    let hover = pal.hover;
    let accent = pal.accent_strong;
    button(
        container(content).width(wide).height(tall),
    )
    .padding(0.0)
    .style(move |_theme: &Theme, state| button::Style {
        background: Some(match state {
            button::Status::Hovered | button::Status::Pressed => Background::Color(hover),
            _ => Background::Color(base),
        }),
        text_color: accent,
        border: border::rounded(round).color(line).width(1.0),
        ..button::Style::default()
    })
    .width(wide)
    .height(tall)
    .on_press_maybe(action)
    .into()
}

// The two marks that turn the carousel, with the place of this chat between
// them. They stand together in one box, so it is clear that they belong to the
// carousel and not to the bar.
fn roll_box(snap: &Snap, pal: Pal, shape: &Shape) -> Elem {
    let wide = if shape.tight { 84.0 } else { 118.0 };
    let place = if snap.chat_count < 2 {
        String::new()
    } else if shape.tight {
        format!("{}", snap.chat_index + 1)
    } else {
        format!("{} of {}", snap.chat_index + 1, snap.chat_count)
    };
    let mut line = Row::new().spacing(1.0).align_y(Alignment::Center).padding([0.0, 3.0]);
    line = line.push(parts::mark_button(
        "left",
        if snap.chat_index > 0 {
            Some(Msg::Roll(-1))
        } else {
            None
        },
        pal,
    ));
    line = line.push(
        container(word_cut(place, pal.text_dim, 12.0, true, 40.0)).width(Length::Fill),
    );
    line = line.push(parts::mark_button(
        "right",
        if snap.chat_index + 1 < snap.chat_count {
            Some(Msg::Roll(1))
        } else {
            None
        },
        pal,
    ));
    container(line)
        .width(Length::Fixed(wide))
        .height(Length::Fixed(34.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.surface)),
            border: border::rounded(9.0).color(pal.line).width(1.0),
            ..container::Style::default()
        })
        .into()
}

// The tokens in use, with a thin bar under the words. The bar turns dark red
// when the window of the model is nearly full.
fn context_box(snap: &Snap, pal: Pal, shape: &Shape) -> Elem {
    let ratio = if snap.context_total > 0 {
        (snap.context_used as f32 / snap.context_total as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let near_full = ratio > 0.9;
    let phrase = if shape.tight {
        format!(
            "{} / {}",
            crate::util::count(snap.context_used),
            crate::util::count(snap.context_total)
        )
    } else {
        format!(
            "{} of {} tokens",
            crate::util::count(snap.context_used),
            crate::util::count(snap.context_total)
        )
    };
    let cap = if shape.tight { 150.0 } else { 232.0 };
    let wide = (18.0 + words(phrase.as_str(), 11.5)).clamp(120.0, cap);
    let track = wide - 18.0;
    let ink = if near_full { pal.accent_deep } else { pal.text_dim };
    let bar_ink = if near_full { pal.accent_deep } else { pal.accent };
    let edge = if near_full { pal.accent_deep } else { pal.line };

    let filled = container(space())
        .width(Length::Fixed(track * ratio))
        .height(Length::Fixed(3.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(bar_ink)),
            border: border::rounded(2.0),
            ..container::Style::default()
        });
    let rule = container(filled)
        .width(Length::Fixed(track))
        .height(Length::Fixed(3.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.line)),
            border: border::rounded(2.0),
            ..container::Style::default()
        });

    container(
        Column::new()
            .push(word_cut(phrase, ink, 11.5, false, track))
            .push(rule)
            .spacing(5.0),
    )
    .width(Length::Fixed(wide))
    .height(Length::Fixed(34.0))
    .padding([5.0, 9.0])
    .style(move |_| container::Style {
        background: Some(Background::Color(pal.surface)),
        border: border::rounded(9.0).color(edge).width(1.0),
        ..container::Style::default()
    })
    .into()
}

// The place of the model in use inside the list of models. Nothing is given
// back when no model is in use, because there is then nothing to set.
fn selected_model(app: &App) -> Option<usize> {
    if app.snap.model_id.is_empty() {
        return None;
    }
    app.snap
        .models
        .iter()
        .position(|model| model.id() == app.snap.model_id)
}

// The widest model name of the list. The model box takes this room.
fn longest_model(snap: &Snap) -> String {
    let mut best = String::new();
    for model in &snap.models {
        if model.name.chars().count() > best.chars().count() {
            best = model.name.clone();
        }
    }
    best
}

// The widest repository name of the list. The repository box takes this room.
fn longest_repository(snap: &Snap) -> String {
    let mut best = String::new();
    for repo in &snap.repositories {
        if repo.name.chars().count() > best.chars().count() {
            best = repo.name.clone();
        }
    }
    best
}

// The middle of the window: the chat that is in use, and the row of small
// previews while the Alt key put it up. The chats beside the one in use are not
// drawn here. They are seen in the row of previews, and the two marks of the top
// bar turn the carousel, so the window keeps the room of one wide card.
fn stage<'a>(app: &'a App, pal: Pal, shape: &Shape) -> Element<'a, Msg> {
    let snap = &app.snap;
    let wide = shape.width;
    let tall = shape.stage_height;
    let mut layers = Stack::new();

    if tall > 4.0 {
        layers = layers.push(cards(app, pal, shape));
    }

    // The soft backdrop of the row of previews, its count, its caption, the
    // cards, and the place where the pointer moves along the row.
    if snap.overview {
        layers = layers.push(pin(
            container(space())
                .width(Length::Fixed(wide))
                .height(Length::Fixed(tall))
                .style(move |_| container::Style {
                    background: Some(Background::Color(faded(pal.bg, 0.94))),
                    ..container::Style::default()
                }),
        )
        .x(0.0)
        .y(0.0)
        .width(wide)
        .height(tall));

        let count = if snap.chats.is_empty() {
            String::from("This place holds no chat yet")
        } else {
            format!(
                "{} {} in {}",
                snap.chats.len(),
                if snap.chats.len() == 1 { "chat" } else { "chats" },
                if snap.in_repository {
                    snap.repository_name.as_str()
                } else {
                    "the loose chats of this machine"
                }
            )
        };
        layers = layers.push(
            pin(
                container(word_cut(
                    count,
                    pal.text_dim,
                    12.0,
                    true,
                    (wide - 80.0).max(60.0),
                ))
                .width(Length::Fixed(wide - 80.0)),
            )
            .x(40.0)
            .y((shape.prev_y - 32.0).max(0.0))
            .width(wide - 80.0)
            .height(20.0),
        );
        layers = layers.push(
            pin(
                container(
                    container(word(
                        "Click a card to open it. Drag, the arrows, A and D or the wheel move along the row. Return opens the chat in the middle. Escape or Alt puts the row away.",
                        pal.text_dim,
                        12.0,
                    ))
                    .width(Length::Fixed((wide - 120.0).max(120.0))),
                )
                .width(Length::Fixed(wide - 120.0)),
            )
            .x(60.0)
            .y(shape.prev_y + shape.prev_height + 22.0)
            .width(wide - 120.0)
            .height(40.0),
        );
        layers = layers.push(pin(previews(app, pal, shape))
            .x(0.0)
            .y(0.0)
            .width(wide)
            .height(tall));
        layers = layers.push(pin(row_place(shape))
            .x(0.0)
            .y(0.0)
            .width(wide)
            .height(tall));
    }

    container(layers)
        .width(Length::Fixed(wide))
        .height(Length::Fixed(tall))
        .into()
}

// The chat that is in use. Only that one chat is drawn, so a place with a
// hundred chats costs the same as a place with three. The card grows from the
// top down while a chat is opened, which is what `turn` says.
fn cards<'a>(app: &'a App, pal: Pal, shape: &Shape) -> Element<'a, Msg> {
    let snap = &app.snap;
    let tall = if snap.home {
        0.0
    } else {
        shape.stage_height * app.turn
    };
    if tall <= 1.0 || snap.chats.is_empty() {
        return space()
            .width(Length::Fixed(shape.width))
            .height(Length::Fixed(shape.stage_height))
            .into();
    }
    let body: Element<'a, Msg> = container(message_view::list(
        &app.rows,
        (shape.card_width - 28.0).max(60.0),
        (tall - 24.0).max(20.0),
        snap.generating,
        app.pulse,
        app.hovered.as_deref(),
        pal,
    ))
    .width(Length::Fixed(shape.card_width))
    .height(Length::Fixed(tall))
    .padding([12.0, 14.0])
    .into();
    let mut layers = Stack::new();
    layers = layers.push(
        pin(card_frame(body, shape.card_width, tall, pal))
            .x(shape.card_x)
            .y(0.0)
            .width(shape.card_width)
            .height(tall),
    );
    container(layers)
        .width(Length::Fixed(shape.width))
        .height(Length::Fixed(shape.stage_height))
        .into()
}

// The card of the chat in use. Its edge stands a hair stronger than the edge of
// a box, so the chat is seen apart from the window it stands on.
fn card_frame<'a>(body: Element<'a, Msg>, wide: f32, tall: f32, pal: Pal) -> Element<'a, Msg> {
    container(body)
        .width(Length::Fixed(wide))
        .height(Length::Fixed(tall))
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.surface)),
            border: border::rounded(16.0).color(pal.line_strong).width(1.0),
            ..container::Style::default()
        })
        .into()
}

// The row of small previews. Only seven are drawn, about the chat in use, so a
// place with many chats stays light to draw.
fn previews(app: &App, pal: Pal, shape: &Shape) -> Elem {
    let mut layers = Stack::new();
    let scale = 1.0 + (1.0 - app.open_t) * 0.5;
    for place in 0..7i64 {
        let index = app.snap.chat_index as i64 + place - 3;
        if index < 0 || index as usize >= app.cards.len() {
            continue;
        }
        let wide = shape.prev_width * scale;
        let high = shape.prev_height * scale;
        let x = shape.prev_x(index as f32, app.snap.chat_index, app.spin)
            + shape.prev_width * (1.0 - scale) / 2.0;
        let y = shape.prev_y + shape.prev_height * (1.0 - scale) / 2.0;
        layers = layers.push(
            pin(preview(
                &app.cards[index as usize],
                index as usize == app.snap.chat_index,
                wide,
                high,
                pal,
            ))
            .x(x)
            .y(y)
            .width(wide)
            .height(high),
        );
    }
    container(layers)
        .width(Length::Fixed(shape.width))
        .height(Length::Fixed(shape.stage_height))
        .into()
}

// One small card of the row that the Alt key brings up.
fn preview(card: &Card, middle: bool, wide: f32, tall: f32, pal: Pal) -> Elem {
    let edge = if middle { pal.accent } else { pal.line };
    let ground = if middle {
        pal.surface
    } else {
        mix(pal.surface, pal.bg, 0.3)
    };
    let room = (wide - 28.0).max(40.0);
    let mut layers = Stack::new();
    layers = layers.push(
        pin(word_cut(card.title.clone(), pal.text, 13.5, false, room))
            .x(14.0)
            .y(18.0)
            .width(room)
            .height(20.0),
    );
    let body_room = (tall - 112.0).max(24.0);
    layers = layers.push(
        pin(container(word(card.preview.clone(), pal.text_dim, 12.0)).width(Length::Fixed(room)))
            .x(14.0)
            .y(42.0)
            .width(room)
            .height(body_room),
    );
    layers = layers.push(
        pin(word_cut(
            card.repo.clone(),
            pal.accent_leaf,
            11.0,
            false,
            room,
        ))
        .x(14.0)
        .y((tall - 62.0).max(4.0))
        .width(room)
        .height(16.0),
    );
    layers = layers.push(
        pin(parts::hair(room, pal.line))
            .x(14.0)
            .y((tall - 44.0).max(4.0))
            .width(room)
            .height(1.0),
    );
    layers = layers.push(
        pin(word_cut(card.when.clone(), pal.text_dim, 11.0, false, room))
            .x(14.0)
            .y((tall - 30.0).max(4.0))
            .width(room)
            .height(16.0),
    );
    container(layers)
        .width(Length::Fixed(wide))
        .height(Length::Fixed(tall))
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(ground)),
            border: border::rounded(14.0).color(edge).width(if middle { 2.0 } else { 1.0 }),
            ..container::Style::default()
        })
        .into()
}

// The place where the pointer moves the row of previews. A press and a release
// without a move opens the preview it stood on.
fn row_place(shape: &Shape) -> Elem {
    mouse_area(
        container(space())
            .width(Length::Fixed(shape.width))
            .height(Length::Fixed(shape.stage_height)),
    )
    .interaction(iced::mouse::Interaction::Pointer)
    .on_move(|where_| Msg::RowAt(where_.x))
    .on_press(Msg::RowGrab)
    .on_release(Msg::RowRelease)
    .on_scroll(|step| Msg::RowWheel(slide(step)))
    .into()
}

// How far one turn of the wheel moves the row. A turn of lines is turned into
// pixels, because the row moves in pixels.
fn slide(step: iced::mouse::ScrollDelta) -> f32 {
    match step {
        iced::mouse::ScrollDelta::Lines { x, y } => (x + y) * 24.0,
        iced::mouse::ScrollDelta::Pixels { x, y } => x + y,
    }
}

// The first screen: the mark of the program, a line for this hour, and one line
// that says where the words stay.
fn greeting(app: &App, pal: Pal, shape: &Shape) -> Elem {
    let mut lines = Column::new();
    lines = lines.push(
        container(parts::glyph("mark", pal.accent, 64.0))
            .width(Length::Fixed(shape.width))
            .center_x(Length::Shrink),
    );
    lines = lines.push(
        container(word(app.snap.greeting.clone(), pal.text_dim, 16.0))
            .width(Length::Fixed((shape.width - 120.0).max(200.0)))
            .center_x(Length::Shrink),
    );
    lines = lines.push(
        container(word(
            String::from("Every word stays on this machine."),
            faded(pal.text_dim, 0.75),
            11.5,
        ))
        .width(Length::Fixed((shape.width - 120.0).max(200.0)))
        .center_x(Length::Shrink),
    );
    container(lines.spacing(14))
        .width(Length::Fixed(shape.width))
        .height(Length::Fixed(156.0))
        .into()
}

// The dock: the line of what the machine is doing, and the box where a message
// is written. It stands in the middle of the window while no chat is open, and
// at the bottom of the window when one is.
fn dock<'a>(app: &'a App, pal: Pal, shape: &Shape) -> Element<'a, Msg> {
    let mut line = Row::new();
    line = line.push(parts::dot(
        app.snap.generating || app.snap.busy,
        8.0,
        pal.accent,
        pal.text_dim,
    ));
    let said = if app.snap.generating {
        pal.accent_strong
    } else {
        pal.text_dim
    };
    line = line.push(
        container(word(app.snap.status.clone(), said, 12.0)).width(Length::Fill),
    );
    if !shape.tight {
        line = line.push(word(hint_of(app), faded(pal.text_dim, 0.8), 11.0));
    }
    line = line.push(parts::press(
        container(
            row![
                parts::glyph("plus", pal.accent_strong, 13.0),
                word(String::from("New chat"), pal.accent_strong, 12.0),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        ),
        pal.surface,
        pal.line,
        13.0,
        1.0,
        pal.accent_strong,
        Some(Msg::NewChat),
        pal,
    ));
    line = line.push(parts::mark_button("settings", Some(Msg::Settings), pal));
    let mut parts = Column::new();
    parts = parts.push(container(line.spacing(8).align_y(Alignment::Center)).height(30.0));
    parts = parts.push(composer_box(app, pal, shape));
    container(parts.spacing(8)).into()
}

// The box a message is written in. Enter sends, and Shift and Enter starts a
// new line. The keys are read before the box sees them, so a half written
// message is never lost.
fn composer_box<'a>(app: &'a App, pal: Pal, shape: &Shape) -> Element<'a, Msg> {
    let wide = shape.card_width;
    let tall = 118.0;
    let live = app.snap.generating || !app.draft_text.is_empty();
    // The row of previews takes every key while it stands in front, so a letter
    // cannot be written into the box behind it.
    let row_up = app.snap.overview;
    let draft = text_editor(&app.draft)
        .id(app.draft_id.clone())
        .placeholder(if app.snap.generating {
            String::from("Wait for this answer, or press Escape to stop it.")
        } else {
            String::from("Write a message. Enter sends it, and Shift and Enter starts a new line.")
        })
        .height(Length::Fixed(tall - 58.0))
        .padding(0.0)
        .size(13.5)
        .on_action(Msg::Draft)
        .key_binding(move |press: iced::widget::text_editor::KeyPress| {
            use iced::keyboard::{key::Named, Key};
            if row_up {
                // Nothing is written, and nothing is sent. Escape, Alt and
                // Return are answered by the window itself.
                return Some(iced::widget::text_editor::Binding::Sequence(vec![]));
            }
            if !matches!(press.key, Key::Named(Named::Enter)) {
                return None;
            }
            if press.modifiers.shift() {
                return Some(iced::widget::text_editor::Binding::Insert('\n'));
            }
            Some(iced::widget::text_editor::Binding::Custom(Msg::Send))
        })
        .style(move |_: &Theme, _| iced::widget::text_editor::Style {
            background: Background::Color(pal.surface),
            border: border::rounded(0.0).width(0.0).color(pal.surface),
            placeholder: faded(pal.text_dim, 0.75),
            value: pal.text,
            selection: pal.accent,
        });

    // The marks at the foot of the box.
    let think_name = match app.snap.thinking_mode.as_str() {
        "auto" => String::from("think: auto"),
        "off" => String::from("think: off"),
        "think" => String::from("think: on"),
        other => format!("think: {other}"),
    };
    let marks = Row::new()
        .spacing(8.0)
        .align_y(Alignment::Center)
        .push(parts::badge(
            "Code",
            "code",
            app.snap.code_enabled,
            true,
            Some(Msg::Code(!app.snap.code_enabled)),
            pal,
        ))
        .push(parts::badge(
            "Sandbox",
            "shield",
            app.snap.sandbox_enabled,
            app.snap.sandbox_available,
            Some(Msg::Sandbox(!app.snap.sandbox_enabled)),
            pal,
        ))
        .push(parts::badge(
            if app.snap.thinking_available {
                think_name.as_str()
            } else {
                "No thinking modes"
            },
            "think",
            app.snap.thinking_available && app.snap.thinking_mode != "auto",
            app.snap.thinking_available,
            Some(Msg::Thinking),
            pal,
        ));

    // The round mark. It is always there, dull while there is nothing to send,
    // and it turns into the stop mark while an answer comes in.
    let send_fill = if app.snap.generating {
        pal.accent_strong
    } else if live {
        pal.accent
    } else {
        mix(pal.accent, pal.surface, 0.6)
    };
    let send = parts::press(
        container(parts::glyph(
            if app.snap.generating { "stop" } else { "send" },
            pal.on_accent,
            16.0,
        ))
        .width(Length::Fixed(34.0))
        .height(Length::Fixed(34.0))
        .center_x(Length::Fixed(34.0))
        .center_y(Length::Fixed(34.0)),
        send_fill,
        Color::TRANSPARENT,
        17.0,
        0.0,
        pal.on_accent,
        Some(if app.snap.generating {
            Msg::Cancel
        } else {
            Msg::Send
        }),
        pal,
    );

    let mut layers = Stack::new();
    layers = layers.push(pin(draft).x(16.0).y(14.0).width(wide - 32.0).height(tall - 58.0));
    layers = layers.push(pin(marks).x(10.0).y(tall - 38.0));
    layers = layers.push(pin(send).x(wide - 46.0).y(tall - 46.0));
    container(layers)
        .width(Length::Fixed(wide))
        .height(Length::Fixed(tall))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(pal.surface)),
            border: border::rounded(14.0).color(pal.line).width(1.0),
            ..container::Style::default()
        })
        .into()
}

// The list that comes under the top bar: the models this machine has, or the
// places the chats are kept in.
fn menu(app: &App, pal: Pal) -> Elem {
    let mut lines = Column::new().spacing(2);
    match app.menu {
        Menu::Model => {
            for (place, model) in app.snap.models.iter().enumerate() {
                let live = model.id() == app.snap.model_id;
                let label = format!("{} ({})", model.name, model.kind);
                lines = lines.push(parts::entry(label, live, Msg::PickModel(place), pal));
            }
            // The window of one model is reached from this list and from no
            // other place, so the entry stands at its foot.
            if let Some(place) = selected_model(app) {
                lines = lines.push(parts::hair_across(pal.line));
                lines = lines.push(parts::entry(
                    String::from("Settings of this model"),
                    false,
                    Msg::ModelSettings(place),
                    pal,
                ));
            }
        }
        Menu::Repository => {
            let live = app.snap.repository_id.is_empty();
            lines = lines.push(parts::entry(
                String::from("No place"),
                live,
                Msg::PickRepository(0),
                pal,
            ));
            for (place, repo) in app.snap.repositories.iter().enumerate() {
                let live = repo.id == app.snap.repository_id;
                lines = lines.push(parts::entry(
                    repo.name.clone(),
                    live,
                    Msg::PickRepository(place + 1),
                    pal,
                ));
            }
        }
        Menu::None => {}
    }
    container(scrollable(lines).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([6.0, 6.0])
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.surface)),
            border: border::rounded(10.0).color(pal.line_strong).width(1.0),
            ..container::Style::default()
        })
        .into()
}

// One line that says what the keys do, for the quiet corner of the status row.
fn hint_of(app: &App) -> String {
    if app.snap.generating {
        return String::from("Escape stops the answer.");
    }
    if app.snap.overview {
        return String::from("Escape puts the row away.");
    }
    if app.snap.home {
        return String::from("Alt brings the row of chats up.");
    }
    String::from("Enter sends. Alt brings the row of chats up.")
}
