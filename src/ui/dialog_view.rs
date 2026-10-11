// The two windows of settings: one for the program, and one for a model.
//
// Each window holds a form. The form is filled in when the window opens and is
// read back when the user presses Save. A field that is left alone keeps the
// value it had, so a half written field cannot be missed.

use iced::widget::{column, container, row, scrollable, space};
use iced::{alignment, border, window, Element, Length, Padding};

use crate::ui::palette::Pal;
use crate::ui::parts::{self, line_of};
use crate::ui_app::{App, ModelForm, Msg, SettingsForm};

// The body of a settings window, with its buttons under it. A help bubble of
// this window stands over the whole of it, next to the pointer.
fn shell<'a>(
    app: &App,
    which: window::Id,
    body: Element<'a, Msg>,
    above: Element<'a, Msg>,
    pal: Pal,
) -> Element<'a, Msg> {
    let foot = row![
        parts::word_button("Save", true, Msg::Save, pal),
        parts::word_button("Close", false, Msg::Close, pal),
    ]
    .spacing(8)
    .align_y(alignment::Vertical::Center);
    let made: Element<'a, Msg> = column![
        above,
        container(scrollable(body).height(Length::Fill)).width(Length::Fill),
        container(foot)
            .width(Length::Fill)
            .padding(Padding { top: 10.0, right: 22.0, bottom: 14.0, left: 22.0 }),
    ]
    .spacing(10)
    .padding(Padding { top: 16.0, right: 0.0, bottom: 0.0, left: 0.0 })
    .into();
    let (wide, tall) = app.room_of(which);
    parts::tip_over(app.tip.as_ref(), app.pointer_at(which), wide, tall, made, pal)
}

// A window of settings that has no form to show. It is put away in the step of
// the message that took the form away, so one frame of this may be seen.
pub fn blank(pal: Pal) -> parts::Elem {
    container(space())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(pal.bg)),
            ..container::Style::default()
        })
        .into()
}

// The window with the settings of the program.
pub fn program(app: &App, which: window::Id, form: &SettingsForm, pal: Pal) -> parts::Elem {
    // Every repository on disk stands here. The bar at the top of the main
    // window shows the five most recent ones, so this list is the way to a
    // place that was not used lately.
    let mut places = column![].spacing(2);
    places = places.push(parts::entry(
        String::from("No repository"),
        app.snap.repository_id.is_empty(),
        Msg::PickRepository(0),
        pal,
    ));
    // The list is read from the engine, because the snapshot of the main window
    // holds only the five places the top bar shows.
    let repos = app.engine.repositories();
    for (place, repo) in repos.iter().enumerate() {
        places = places.push(parts::entry(
            repo.name.clone(),
            repo.id == app.snap.repository_id,
            Msg::PickRepository(place + 1),
            pal,
        ));
    }
    let list = container(
        container(scrollable(places).height(Length::Fill))
            .width(Length::Fill)
            .padding([3.0, 3.0]),
    )
    .width(Length::Fill)
    .height(Length::Fixed(124.0))
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(pal.surface)),
        border: border::rounded(8.0).color(pal.line).width(1.0),
        ..container::Style::default()
    });

    let body = column![
        parts::section("Places", pal),
        // The folder the chats and the settings live in. It cannot be changed
        // here, but the reader must be able to find it.
        line_of(
            "Data directory",
            "Where Autumn Natter keeps the settings, the chats and the data of the repositories.",
            parts::word(app.snap.data_dir.clone(), pal.text_dim, 12.0),
            pal
        ),
        line_of(
            "llama.cpp",
            "The folder that holds llama-server. The program starts that file for every answer.",
            parts::field(&form.llama_dir, "the folder of llama.cpp", Msg::LlamaDir, pal),
            pal
        ),
        line_of(
            "Models",
            "The folder the model files are read from.",
            parts::field(&form.models_dir, "the folder of the models", Msg::ModelsDir, pal),
            pal
        ),
        line_of(
            "Repositories",
            "The folder the places of the chats are kept in. Empty means the usual place.",
            parts::field(&form.repositories_dir, "the usual place", Msg::ReposDir, pal),
            pal
        ),
        parts::section("Network", pal),
        line_of(
            "Port",
            "The number the page of the browser listens on. Empty lets the machine pick one.",
            parts::field(&form.port, "no port", Msg::Port, pal),
            pal
        ),
        line_of(
            "Offer on the network",
            "The page can then be opened from other machines of the same network.",
            parts::switch(form.upnp, true, Some(Msg::Upnp(!form.upnp)), pal),
            pal
        ),
        parts::section("Places of the chats", pal),
        line_of(
            "New repository",
            "Make a folder for one project. A chat inside a repository runs its terminal commands in that folder.",
            row![
                container(parts::field(
                    &form.new_repo,
                    "name of the repository",
                    Msg::NewRepoText,
                    pal
                ))
                .width(Length::Fill),
                parts::word_button(
                    "Create",
                    true,
                    Msg::NewRepository(form.new_repo.clone()),
                    pal
                ),
            ]
            .spacing(8)
            .align_y(alignment::Vertical::Center),
            pal
        ),
        line_of(
            "Repositories",
            "Pick the repository the chats of this window belong to. The folder on disk is named name_id.",
            list,
            pal
        ),
        parts::word(app.snap.sandbox_note.clone(), pal.text_dim, 12.0),
        parts::section("Look", pal),
        line_of(
            "Dark colours",
            "The warm dark skin, instead of the pale one. It changes at once.",
            parts::switch(form.dark, true, Some(Msg::Dark(!form.dark)), pal),
            pal
        ),
    ]
    .spacing(10)
    .max_width(680)
    .padding(Padding { top: 0.0, right: 22.0, bottom: 10.0, left: 22.0 });

    shell(app, which, body.into(), head("Settings", pal), pal)
}

// The window with the settings of one model.
pub fn model<'a>(
    app: &App,
    which: window::Id,
    form: &'a ModelForm,
    pal: Pal,
) -> Element<'a, Msg> {
    let held = &form.held;
    let mut body = column![
        parts::section("Model", pal),
        parts::strong(form.name.clone(), pal.text, 15.0),
        parts::section("Sizing", pal),
        line_of(
            "Memory of the talk",
            "How much of the talk the model keeps in mind at once. More costs more memory.",
            typed(held.ctx_size as i64, 256, 2097152, Msg::Ctx, "tokens", pal),
            pal
        ),
        line_of(
            "Numbers kept for the talk",
            "Fewer numbers make the memory of the talk smaller, but the answers a little rougher.",
            parts::chooser(&form.kinds, held.kv_index, Msg::KvKind, pal),
            pal
        ),
        line_of(
            "How the file is read",
            "Keeps the model in the memory of the machine, on the disk, or in both.",
            parts::chooser(&form.loads, held.load_index, Msg::LoadKind, pal),
            pal
        ),
        parts::section("Guessing", pal),
        line_of(
            "Guessing",
            "The model may guess a few words ahead and then check them. It is quicker with a small draft model.",
            parts::chooser(&form.specs, held.spec_index, Msg::SpecKind, pal),
            pal
        ),
        line_of(
            "Words guessed at once",
            "How many words the model guesses before it checks them.",
            typed(held.draft_tokens as i64, 1, 32, Msg::DraftTokens, "words", pal),
            pal
        ),
        parts::section("Working", pal),
        line_of(
            "Chats at once",
            "How many chats the model serves at the same time. One is right for this program.",
            typed(held.parallel as i64, 1, 32, Msg::Parallel, "chats", pal),
            pal
        ),
        line_of(
            "Words at a time",
            "How many words are put through the model in one step.",
            typed(held.batch_size as i64, 16, 65536, Msg::Batch, "words", pal),
            pal
        ),
        line_of(
            "Words in a piece",
            "How many of those words the card takes in one piece.",
            typed(held.ubatch_size as i64, 8, 65536, Msg::Ubatch, "words", pal),
            pal
        ),
        line_of(
            "Pictures",
            "Read the pictures of a chat, when the model can look at them.",
            parts::switch(held.vision, true, Some(Msg::Vision(!held.vision)), pal),
            pal
        ),
        parts::section("Thoughts", pal),
        line_of(
            "Room for the thoughts",
            "How many words a model may spend on thinking before it answers. -1 leaves it to the model.",
            typed(held.reasoning_budget, -1, 2097152, Msg::Budget, "words", pal),
            pal
        ),
        parts::section("Cards", pal),
    ]
    .spacing(10)
    .max_width(680)
    .padding(Padding { top: 0.0, right: 22.0, bottom: 10.0, left: 22.0 });

    // One line for every card the machine has, with the memory of that card
    // beside its name, and a switch for each.
    for (place, card) in form.gpus.iter().enumerate() {
        let on = held.gpu_enabled.get(place).copied().unwrap_or(false);
        let named = format!("{}, {}", card.name, card.memory_text());
        body = body.push(line_of(
            &named,
            "Let this card draw part of the model.",
            parts::switch(on, true, Some(Msg::CardOn(place)), pal),
            pal,
        ));
    }
    if form.gpus.is_empty() {
        body = body.push(parts::word("No card was found.", pal.text_dim, 12.0));
    }

    // The whole of the cards is the top of the line, so the reader cannot ask
    // for more memory than the machine has.
    let total: i64 = form
        .gpus
        .iter()
        .filter(|one| one.is_offload)
        .map(|one| one.memory_mb as i64)
        .sum();
    body = body.push(parts::section("Memory", pal));
    body = body.push(line_of(
        "Memory of the cards",
        "How much of the cards is spent on the model. Leave the whole of it to use every card fully.",
        row![
            container(parts::line(
                held.vram_budget as i64,
                0,
                total.max(1),
                128,
                Msg::Memory,
                pal,
            ))
            .width(Length::Fill),
            container(parts::word(
                format!("{} MiB", held.vram_budget as i64),
                pal.text_dim,
                12.0,
            ))
            .width(Length::Fixed(110.0)),
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill),
        pal,
    ));
    body = body.push(parts::section("Words of the talk", pal));
    body = body.push(parts::area(
        &form.template,
        "The template of the talk. Leave it empty to use the one inside the model file.",
        120.0,
        Msg::Template,
        true,
        pal,
    ));
    body = body.push(parts::area(
        &form.extra,
        "Arguments given to llama-server as they stand.",
        70.0,
        Msg::Extra,
        true,
        pal,
    ));

    // The command line the next answer will use. It cannot be written in, and
    // it is read again whenever a field of this window moves.
    body = body.push(parts::section("The command line", pal));
    body = body.push(
        container(
            container(parts::mono(form.preview.clone(), pal.text_dim, 12.0))
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .padding([9.0, 11.0])
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(pal.code_bg)),
            border: border::rounded(8.0).color(pal.line).width(1.0),
            ..container::Style::default()
        }),
    );

    shell(app, which, body.into(), head(&form.name, pal), pal)
}

// A number the reader writes down, with the word for what is counted beside it.
// The number may not leave the two ends it is given, which are the ends the
// server takes.
fn typed(
    value: i64,
    low: i64,
    top: i64,
    on: fn(i64) -> Msg,
    after: &str,
    pal: Pal,
) -> parts::Elem {
    row![
        container(parts::number_field(value, low, top, on, pal)).width(Length::Fill),
        container(parts::word(String::from(after), pal.text_dim, 12.0))
            .width(Length::Fixed(70.0)),
    ]
    .spacing(10)
    .align_y(alignment::Vertical::Center)
    .width(Length::Fill)
    .into()
}

// The name at the top of a settings window.
fn head(name: &str, pal: Pal) -> parts::Elem {
    container(parts::strong(name.to_string(), pal.text, 17.0))
        .width(Length::Fill)
        .padding(Padding { top: 0.0, right: 22.0, bottom: 0.0, left: 22.0 })
        .into()
}
