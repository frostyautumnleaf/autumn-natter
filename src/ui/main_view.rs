// The main window: the top bar, the carousel of chats, the row of previews, the
// first screen, and the box where a message is written.
//
// The window is drawn in layers over a ground of its own size. A place that more
// than one layer needs comes from Shape, so the chat card and the box below it
// always stand in line.

use iced::widget::{button, container, mouse_area, row, space, text_editor};
use iced::{border, window, Background, Element, Length, Theme, Alignment};
use iced::widget::{Column, Row, Stack, pin};

use crate::engine::Snap;
use crate::ui_app::{App, Card, Menu, Msg};
use super::message_view;
use super::palette::{faded, mix, Pal};
use super::parts::{self, word, word_cut, words, Elem};
use super::shape::Shape;

// A layer of the window, stood at a place of it. A pin gives what stands in it
// the room that is left from its place to the far edge of the layer. So the
// size of a layer is its place added to the size of the thing that stands in
// it, and never that size alone. A layer sized that way alone has no room
// left, and what stands in it is squeezed to nothing.
fn shelf<'a>(
    what: impl Into<Element<'a, Msg>>,
    x: f32,
    y: f32,
    wide: f32,
    tall: f32,
) -> Element<'a, Msg> {
    pin(what)
        .x(x)
        .y(y)
        .width(x + wide)
        .height(y + tall)
        .into()
}

// The room of a stack of layers. The first thing put into a stack gives the
// stack its size, and a layer that stands outside that size is squeezed to
// nothing. So a stack of layers starts with an empty room of its whole size.
fn room_of(wide: f32, tall: f32) -> Elem {
    space()
        .width(Length::Fixed(wide))
        .height(Length::Fixed(tall))
        .into()
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
    layers = layers.push(shelf(
        top_bar(app, pal, &shape),
        0.0,
        0.0,
        wide,
        shape.bar,
    ));

    // The stage: the carousel, the washes, and the row of previews.
    layers = layers.push(shelf(
        stage(app, pal, &shape),
        0.0,
        shape.stage_top,
        wide,
        shape.stage_height,
    ));

    // The mark of the program and the line for this hour, while no chat is
    // open. They stand above the box and never over a chat.
    if app.greet {
        layers = layers.push(shelf(
            greeting(app, pal, &shape),
            0.0,
            shape.greet_y,
            wide,
            shape.greet_height,
        ));
    }

    // The status line and the box for a new message.
    layers = layers.push(shelf(
        dock(app, pal, &shape),
        shape.card_x,
        shape.dock_y,
        shape.card_width,
        shape.dock_height,
    ));

    // One list of the top bar, when it is open. It stands under the box that
    // brought it up, so the list and its box stand in line.
    if app.menu != Menu::None {
        let x = menu_place(app, &shape);
        let tall_list = menu_height(app).min(tall - shape.bar - 20.0);
        let wide_list = menu_width(app, &shape).min((wide - x - 12.0).max(160.0));
        layers = layers.push(shelf(
            menu(app, pal),
            x,
            shape.bar + 6.0,
            wide_list,
            tall_list,
        ));
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
    // One line of the list, with the hair of room at its head and foot. The
    // list of models ends with a line and with the entry of the settings, so
    // two lines more are counted.
    let line = 31.0;
    match app.menu {
        Menu::Model => {
            let mut rows = app.snap.models.len() as f32;
            if selected_model(app).is_some() {
                rows += 2.0;
            }
            line * rows.max(1.0) + 14.0
        }
        Menu::Repository => line * (app.snap.repositories.len() as f32 + 1.0).max(1.0) + 14.0,
        Menu::None => 0.0,
    }
    .min(420.0)
}

// The place where a box of the top bar stands. The list of the box stands
// under it, so the two are made from the same words.
fn menu_place(app: &App, shape: &Shape) -> f32 {
    let snap = &app.snap;
    let mark_wide = if shape.tight {
        28.0
    } else {
        38.0 + words("Autumn Natter", 13.5)
    };
    let mut x = 12.0 + mark_wide + 10.0;
    if app.menu == Menu::Repository {
        let kind_room = if snap.model_kind.is_empty() {
            0.0
        } else {
            words(snap.model_kind.as_str(), 11.0) + 45.0
        };
        let model_wide = (86.0 + words(longest_model(snap).as_str(), 13.0) + kind_room)
            .clamp(118.0, shape.model_cap);
        x += model_wide + 10.0;
    }
    x
}

// How wide a list of the top bar must be. It is as wide as the widest line it
// holds, so no name is cut, and it never grows past the window.
fn menu_width(app: &App, shape: &Shape) -> f32 {
    let mut longest = 0.0f32;
    match app.menu {
        Menu::Model => {
            for model in &app.snap.models {
                let line = format!("{} ({})", model.name, model.kind);
                longest = longest.max(words(line.as_str(), 12.5));
            }
            longest = longest.max(words("Settings of this model", 12.5));
            (longest + 62.0).clamp(240.0, 420.0).min(shape.width - 24.0)
        }
        Menu::Repository => {
            for repo in &app.snap.repositories {
                longest = longest.max(words(repo.name.as_str(), 12.5));
            }
            longest = longest.max(words("No place", 12.5));
            (longest + 62.0).clamp(220.0, 400.0).min(shape.width - 24.0)
        }
        Menu::None => 240.0,
    }
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
    let model_wide = (86.0 + words(longest_model(snap).as_str(), 13.0) + kind_room) .clamp(118.0, shape.model_cap);
    line = line.push(model_box(app, snap, pal, model_wide));

    // The repository box, when the chat of the middle belongs to a repository.
    if snap.in_repository {
        let repo_wide = (66.0 + words(longest_repository(snap).as_str(), 13.0))
            .clamp(104.0, shape.repo_cap);
        line = line.push(repository_box(snap, pal, repo_wide));
    }

    // The name of the chat. It is the chief word of this bar, so it takes the
    // room that is left.
    line = line.push(
        container(parts::word_one(snap.chat_title.clone(), pal.text, 13.5))
            .width(Length::Fill)
            .height(Length::Fixed(34.0))
            .clip(true),
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
    let named = if snap.model_name.is_empty() {
        String::from("No model")
    } else {
        snap.model_name.clone()
    };
    // The name keeps a room of its own, so the mark of the kind of file stands
    // close behind it. The box as a whole stays as wide as the longest name of
    // the list, so it does not move when another model is picked.
    inside = inside.push(
        container(parts::word_one(named, ink, 13.0))
            .width(Length::Fixed((words(snap.model_name.as_str(), 13.0) + 8.0).max(40.0)))
            .clip(true),
    );
    if !snap.model_kind.is_empty() {
        inside = inside.push(parts::pill(snap.model_kind.as_str(), pal));
    }
    // The room between the mark of the kind and the mark of the list takes what
    // is left, so the two marks keep their places at the ends.
    inside = inside.push(
        container(space())
            .width(Length::Fill)
            .height(Length::Fixed(1.0)),
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
        line,
        9.0,
        Some(Msg::MenuOpen(Menu::Model)),
        pal,
    )
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
        container(parts::word_one(
            snap.repository_name.clone(),
            pal.text,
            13.0,
        ))
        .width(Length::Fill)
        .clip(true),
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
    // The words stand a little in from the edge of the box, so the marks at
    // the ends do not touch the hairline.
    button(
        container(content).width(wide).height(tall).padding([0.0, 10.0]),
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

    // The chat in use is put away while the row of previews stands in front of
    // it, so no word of the chat shows behind a preview card.
    if tall > 4.0 && !snap.overview {
        layers = layers.push(cards(app, pal, shape));
    }

    // The ground of the row of previews, its count, its caption, the cards, and
    // the place where the pointer moves along the row.
    if snap.overview {
        layers = layers.push(shelf(
            container(space())
                .width(Length::Fixed(wide))
                .height(Length::Fixed(tall))
                .style(move |_| container::Style {
                    background: Some(Background::Color(pal.bg)),
                    ..container::Style::default()
                }),
            0.0,
            0.0,
            wide,
            tall,
        ));

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
        layers = layers.push(shelf(
            container(word_cut(
                count,
                pal.text_dim,
                12.0,
                true,
                (wide - 80.0).max(60.0),
            ))
            .width(Length::Fixed(wide - 80.0)),
            40.0,
            (shape.prev_y - 32.0).max(0.0),
            wide - 80.0,
            20.0,
        ));
        layers = layers.push(shelf(
            container(
                container(word(
                    "Click a card to open it. Drag, the arrows, A and D or the wheel move along the row. Return opens the chat in the middle. Escape or Alt puts the row away.",
                    pal.text_dim,
                    12.0,
                ))
                .width(Length::Fixed((wide - 120.0).max(120.0))),
            )
            .width(Length::Fixed(wide - 120.0)),
            60.0,
            shape.prev_y + shape.prev_height + 22.0,
            wide - 120.0,
            40.0,
        ));
        layers = layers.push(shelf(previews(app, pal, shape), 0.0, 0.0, wide, tall));
        layers = layers.push(shelf(row_place(shape), 0.0, 0.0, wide, tall));
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
    layers = layers.push(shelf(
        card_frame(body, shape.card_width, tall, pal),
        shape.card_x,
        0.0,
        shape.card_width,
        tall,
    ));
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
    layers = layers.push(room_of(shape.width, shape.stage_height));
    // The row comes up from a smaller size, and the middle card stands a little
    // larger than the cards beside it.
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
        layers = layers.push(shelf(
            preview(
                &app.cards[index as usize],
                index as usize == app.snap.chat_index,
                wide,
                high,
                pal,
            ),
            x,
            y,
            wide,
            high,
        ));
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
    layers = layers.push(room_of(wide, tall));
    layers = layers.push(shelf(
        word_cut(card.title.clone(), pal.text, 13.5, false, room),
        14.0,
        18.0,
        room,
        20.0,
    ));
    let body_room = (tall - 112.0).max(24.0);
    layers = layers.push(shelf(
        container(word(card.preview.clone(), pal.text_dim, 12.0)).width(Length::Fixed(room)),
        14.0,
        42.0,
        room,
        body_room,
    ));
    layers = layers.push(shelf(
        word_cut(card.repo.clone(), pal.accent_leaf, 11.0, false, room),
        14.0,
        (tall - 62.0).max(4.0),
        room,
        16.0,
    ));
    layers = layers.push(shelf(
        parts::hair(room, pal.line),
        14.0,
        (tall - 44.0).max(4.0),
        room,
        1.0,
    ));
    layers = layers.push(shelf(
        word_cut(card.when.clone(), pal.text_dim, 11.0, false, room),
        14.0,
        (tall - 30.0).max(4.0),
        room,
        16.0,
    ));
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
    // The three lines stand in the middle of the window. `center_x` is not
    // used, because it sets the width of the box as well.
    lines = lines.push(
        container(parts::glyph("mark", pal.accent, 62.0))
            .width(Length::Fixed(shape.width))
            .align_x(iced::alignment::Horizontal::Center),
    );
    lines = lines.push(
        container(word(app.snap.greeting.clone(), pal.text_dim, 16.0))
            .width(Length::Fixed(shape.width))
            .align_x(iced::alignment::Horizontal::Center),
    );
    lines = lines.push(
        container(word(
            String::from("Every word stays on this machine."),
            faded(pal.text_dim, 0.75),
            11.5,
        ))
        .width(Length::Fixed(shape.width))
        .align_x(iced::alignment::Horizontal::Center),
    );
    container(lines.spacing(13))
        .width(Length::Fixed(shape.width))
        .height(Length::Fixed(shape.greet_height))
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
        pal.accent_text
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
                parts::glyph("plus", pal.accent_text, 13.0),
                word(String::from("New chat"), pal.accent_text, 12.0),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding([4.0, 11.0]),
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
    container(parts.spacing(8))
        .width(Length::Fixed(shape.card_width))
        .height(Length::Fixed(shape.dock_height))
        .into()
}

// The box a message is written in. Enter sends, and Shift and Enter starts a
// new line. The keys are read before the box sees them, so a half written
// message is never lost.
fn composer_box<'a>(app: &'a App, pal: Pal, shape: &Shape) -> Element<'a, Msg> {
    let wide = shape.card_width;
    let tall = 118.0;
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
            // The window answers Escape, so the box keeps the keys and the
            // caret stays where it was.
            if matches!(press.key, Key::Named(Named::Escape)) {
                return Some(iced::widget::text_editor::Binding::Sequence(vec![]));
            }
            if matches!(press.key, Key::Named(Named::Enter)) {
                if press.modifiers.shift() {
                    return Some(iced::widget::text_editor::Binding::Insert('\n'));
                }
                return Some(iced::widget::text_editor::Binding::Custom(Msg::Send));
            }
            // A key the box does not answer for itself is given the binding of
            // the box, so a letter is written and a mark is moved. A key that
            // is given back as nothing is lost, and nothing is written.
            iced::widget::text_editor::Binding::from_key_press(press)
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

    let mut layers = Stack::new();
    layers = layers.push(room_of(wide, tall));
    layers = layers.push(shelf(draft, 16.0, 14.0, wide - 32.0, tall - 58.0));
    layers = layers.push(shelf(marks, 10.0, tall - 38.0, wide - 20.0, 38.0));
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
    container(parts::scroll(lines, Length::Fill, pal))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([6.0, 6.0])
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.surface)),
            border: border::rounded(10.0).color(pal.line_strong).width(1.0),
            shadow: iced::Shadow {
                color: pal.shade,
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 18.0,
            },
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
