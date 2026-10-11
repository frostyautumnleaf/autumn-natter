// The words of a chat. This file draws the list of messages, one block of
// rendered markdown, the thinking of the model, and one run of the terminal tool.
//
// The marks of markdown are taken out in src/markdown.rs, so what stands here is
// always words a reader can read. The kind of a block decides its shape.

use iced::widget::{column, container, row, rule, scrollable, space};
use iced::{alignment, border, Background, Color, Element, Length, Padding, Theme};

use super::palette::Pal;
use super::parts;
use crate::markdown::Block;
use crate::ui_app::Msg;

// One message as the window shows it. The blocks hold the words of the message
// read as markdown. They are made once, when a message came in or grew.
pub struct Row {
    pub id: String,
    pub role: String,
    pub body: String,
    // The first line of the command, for the head of the tool panel. It is empty
    // for a message that is not a tool run.
    pub cmd: String,
    pub tool: String,
    pub mono: bool,
    pub failed: bool,
    pub when: String,
    pub blocks: Vec<Block>,
    // The reasoning of the model, shown in a block of its own.
    pub reasoning: String,
}

impl Row {
    // True for the words of the user.
    fn mine(&self) -> bool {
        self.role == "You"
    }
    // True for a line from the program, which stands with no bubble.
    fn note(&self) -> bool {
        self.role == "Note"
    }
    // True for a run of the terminal tool.
    fn tool(&self) -> bool {
        self.role == "Tool"
    }
}

// The list of messages of one chat, with a scroll bar of its own in the colours
// of the window.
pub fn list<'a>(
    rows: &'a [Row],
    width: f32,
    height: f32,
    generating: bool,
    pulse: usize,
    hovered: Option<&str>,
    pal: Pal,
) -> Element<'a, Msg> {
    let room = width - 14.0;
    let mut body: iced::widget::Column<'a, Msg> = column![].spacing(0);
    for row_in in rows {
        let lit = hovered == Some(row_in.id.as_str());
        body = body.push(one(row_in, room, generating, pulse, lit, pal));
    }
    scrollable(body)
        .direction(iced::widget::scrollable::Direction::Vertical(
            iced::widget::scrollable::Scrollbar::new()
                .width(4.0)
                .scroller_width(4.0)
                .margin(2.0),
        ))
        .height(Length::Fixed(height))
        .width(Length::Fixed(width))
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

// The rail of the scroll bar.
fn rail(pal: Pal) -> scrollable::Rail {
    scrollable::Rail {
        background: Some(Background::Color(pal.line)),
        border: border::rounded(2.0),
        scroller: scrollable::Scroller {
            background: Background::Color(pal.text_dim),
            border: border::rounded(2.0),
        },
    }
}

// One message. The words of the user stand in a bubble of their own colour; the
// words of the model stand on the card, because the bubble of the user is the
// mark of the user. A line from the program stands in the middle, with no bubble.
fn one<'a>(
    item: &'a Row,
    width: f32,
    generating: bool,
    pulse: usize,
    lit: bool,
    pal: Pal,
) -> Element<'a, Msg> {
    if item.note() {
        let ink = if item.failed {
            pal.danger
        } else {
            pal.text_dim
        };
        return container(parts::word_cut(
            item.body.clone(),
            ink,
            12.0,
            true,
            (width - 68.0).max(60.0),
        ))
        .width(Length::Fixed(width))
        .padding([8.0, 34.0])
        .into();
    }
    // The widths stand on the width of the list, which the caller gives in.
    let mine = item.mine();
    let field = if mine {
        (width * 0.55).max(180.0)
    } else {
        (width * 0.75).max(200.0)
    };
    let words = content(item, field - if mine { 28.0 } else { 0.0 }, generating, pulse, pal);
    let bubble = container(words)
        .width(Length::Fixed(field))
        .padding(if mine { [12.0, 14.0] } else { [0.0, 0.0] })
        .style(move |_| container::Style {
            background: Some(Background::Color(if mine {
                pal.bubble_user
            } else {
                Color::TRANSPARENT
            })),
            border: if mine {
                border::rounded(corner(mine)).color(pal.bubble_user_line).width(1.0)
            } else {
                border::rounded(0.0)
            },
            ..container::Style::default()
        });
    // The time of a message stands in the open space beside the bubble, so it
    // never pushes the words about. It comes up when the pointer is over the
    // message.
    let clock_width = (width - field - 22.0).max(46.0);
    let clock = parts::word_cut(item.when.clone(), pal.text_dim, 11.0, !mine, clock_width);
    let clock = if lit {
        container(clock)
    } else {
        container(parts::room(0.0))
    };
    let line = if mine {
        row![clock, bubble].spacing(14)
    } else {
        row![bubble, clock].spacing(14)
    };
    // The row watches the pointer only. Nothing in a message can be pressed, so
    // the watch takes no click from anywhere.
    iced::widget::mouse_area(container(line).width(Length::Fixed(width)).padding(Padding { top: 6.0, right: 0.0, bottom: 12.0, left: 0.0 }))
        .on_enter(Msg::RowLit(item.id.clone()))
        .on_exit(Msg::RowDim)
        .into()
}

// The corner of a bubble. The corner that looks inwards is cut less than the
// others, which shows at a glance who spoke.
fn corner(mine: bool) -> border::Radius {
    if mine {
        // One corner stays square, so the bubble points at the reader.
        border::radius(14.0).top_right(4.0)
    } else {
        border::radius(0.0)
    }
}

// The words inside a bubble: the blocks of the message, the thinking of the
// model, or one run of the terminal tool.
fn content<'a>(
    item: &'a Row,
    width: f32,
    generating: bool,
    pulse: usize,
    pal: Pal,
) -> Element<'a, Msg> {
    if item.tool() && !item.tool.is_empty() {
        return tool_panel(item, width, pulse, pal);
    }
    let ink = if item.failed {
        pal.danger
    } else {
        pal.text
    };
    let mut stack = column![].spacing(8);
    if !item.reasoning.is_empty() {
        stack = stack.push(thinking(
            &item.reasoning,
            generating && item.role == "Assistant",
            width,
            pulse,
            pal,
        ));
    }
    let last = item.blocks.len().saturating_sub(1);
    let mut words = column![].spacing(6);
    for (place, block) in item.blocks.iter().enumerate() {
        let tail = generating && place == last && item.role == "Assistant";
        words = words.push(block_of(block, ink, width, item.mono, tail, pal));
    }
    stack = stack.push(words);
    stack.into()
}

// One block of rendered markdown. The kind of the block decides the shape.
pub fn block_of(
    block: &Block,
    ink: Color,
    width: f32,
    fixed: bool,
    tail: bool,
    pal: Pal,
) -> Element<'static, Msg> {
    match block.kind.as_str() {
        "heading" => {
            // The heading of the first level stands a little larger than the
            // headings below it.
            let high = if block.level <= 1 { 15.0 } else { 14.0 };
            row![
                bar_of(3.0, pal.accent),
                container(parts::strong(block.content.clone(), ink, high))
                    .width(Length::Fixed((width - 11.0).max(40.0))),
            ]
            .spacing(11)
            .into()
        }
        "list" => row![
            bar_of(2.0, pal.line_strong),
            container(parts::word(block.content.clone(), ink, 13.5))
                .width(Length::Fixed((width - 11.0).max(40.0))),
        ]
        .spacing(11)
        .into(),
        "code" => container(
            container(parts::mono(block.content.clone(), pal.text, 12.5))
                .width(Length::Fixed((width - 22.0).max(40.0))),
        )
        .padding([9.0, 11.0])
        .width(Length::Fixed(width))
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.code_bg)),
            border: border::rounded(8.0).color(pal.line).width(1.0),
            ..container::Style::default()
        })
        .into(),
        _ => {
            let line = if fixed {
                parts::mono(block.content.clone(), ink, 12.5)
            } else {
                parts::word(block.content.clone(), ink, 13.5)
            };
            // The last words of an answer that is still coming in stand a little
            // paler, so a new word does not pop in harshly.
            let _ = tail;
            container(line)
                .width(Length::Fixed(width.max(40.0)))
                .into()
        }
    }
}

// The bar of colour that stands at the left of a heading or of a list. It is as
// tall as the words beside it.
fn bar_of(thick: f32, ink: Color) -> Element<'static, Msg> {
    container(space())
        .width(Length::Fixed(thick))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(ink)),
            border: border::rounded(thick / 2.0),
            ..container::Style::default()
        })
        .into()
}

// What the model thought before it answered. It is set apart by a bar of colour
// and by softer words.
pub fn thinking(
    body: &str,
    live: bool,
    width: f32,
    pulse: usize,
    pal: Pal,
) -> Element<'static, Msg> {
    let head = row![
        parts::strong("Thinking", pal.accent_leaf, 11.0),
        if live {
            parts::busy(true, pulse, pal)
        } else {
            parts::room(0.0)
        },
    ]
    .spacing(6)
    .align_y(alignment::Vertical::Center);
    let inside = column![
        head,
        container(parts::word(body.to_string(), pal.text_dim, 12.0)).width(Length::Fill),
    ]
    .spacing(6);
    let body = row![
        bar_of(3.0, pal.accent_leaf),
        container(inside).padding([8.0, 10.0]).width(Length::Fill),
    ]
    .spacing(0)
    .width(Length::Fixed(width));
    container(body)
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.think_bg)),
            border: border::rounded(10.0),
            ..container::Style::default()
        })
        .into()
}

// One run of the terminal tool. The head says which command runs, or has run.
// The body keeps the command and its output under a thin rule of colour.
fn tool_panel<'a>(item: &'a Row, width: f32, pulse: usize, pal: Pal) -> Element<'a, Msg> {
    let running = item.tool == "running ...";
    let ink = if item.failed {
        pal.danger
    } else {
        pal.text
    };
    let head = row![
        if running {
            parts::busy(true, pulse, pal)
        } else {
            icon_head(item.failed, pal)
        },
        parts::strong(
            format!(
                "{}{}",
                if running { "Using tool: " } else { "Used tool: " },
                item.cmd
            ),
            ink,
            12.0,
        ),
    ]
    .spacing(9)
    .align_y(alignment::Vertical::Center);
    let body = row![
        bar_of(2.0, pal.line_strong),
        column![
            parts::word("command", pal.text_dim, 11.0),
            parts::mono(item.body.clone(), ink, 12.0),
            parts::room_across(0.0, 4.0),
            parts::word("output", pal.text_dim, 11.0),
            parts::mono(
                item.tool.clone(),
                if running {
                    pal.text_dim
                } else if item.failed {
                    pal.danger
                } else {
                    pal.text
                },
                12.0
            ),
        ]
        .spacing(2)
        .width(Length::Fill),
    ]
    .spacing(13);
    let rule = rule::horizontal(1.0).style(move |_| iced::widget::rule::Style {
        color: pal.line,
        radius: 1.0.into(),
        fill_mode: iced::widget::rule::FillMode::Full,
        snap: false,
    });
    container(
        column![
            container(head).padding([7.0, 12.0]),
            rule,
            // The room below the words holds the last line of the body, which a
            // box sized from its words does not count.
            container(body).padding(Padding {
                top: 7.0,
                right: 12.0,
                bottom: 30.0,
                left: 12.0,
            }),
        ]
        .spacing(0),
    )
    .width(Length::Fixed(width))
    .style(move |_| container::Style {
            background: Some(Background::Color(pal.code_bg)),
            border: if item.failed {
                border::rounded(10.0).color(pal.danger).width(1.0)
            } else {
                border::rounded(10.0)
            },
            ..container::Style::default()
        })
    .into()
}

// The mark at the head of a tool run that has finished.
fn icon_head(failed: bool, pal: Pal) -> Element<'static, Msg> {
    let ink = if failed { pal.danger } else { pal.text_dim };
    super::icon::glyph("terminal", ink, 13.0)
        .map(Element::from)
        .unwrap_or_else(|| parts::room(0.0))
}
