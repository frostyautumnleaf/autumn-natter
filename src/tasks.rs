// The tasks of a working directory: a list of items to be done that the model writes itself, works
// through, and checks before it ends an answer.
//
// The eight tools are behind one line of the badge of the tools, "Tasks suite", which is off
// until the user turns it on. They are: `create_task`, `view_tasks`, `view_task`, `edit_task`,
// `append_task`, `complete_task`, `cancel_task` and `delete_task`. `edit_task` changes the name,
// the summary or the content of a task by matching the text to change, as the tool that changes a
// file does, and `append_task` adds to the content of a task without writing all of it again. The
// last two change what the user requested, so their descriptions say that they are only for when
// the user requested it, and the prompt of the suite reports it again.
//
// A task belongs to the working directory the chat works in - the directory of the repository, or
// `work/<chat id>/` for a loose chat - and not to the chat. Two chats of one repository
// therefore see the same tasks, and a task outlives the chat that made it. The list is keyed by
// the path of that directory.
//
// The rows are in `tasks.db`, under the data folder of the program. Nothing is written into the
// working directory itself: a model works there, and a file of the program's own lying in a
// directory a command runs in would be read, changed and committed like any other file. See
// `src/db.rs`.
//
// The index of a task is a UUID, given out when the task is created and never changed. The rows
// are kept in the order they were created in, which is the order `view_tasks` writes them out
// in. A call may name a task by the whole index or by the start of it, as long as the start
// names one task and not two.

use rusqlite::params;
use serde_json::{json, Value};

use crate::config;
use crate::db;
use crate::proglog;
use crate::tools;

// The eight names the model calls the suite by.
pub const CREATE_NAME: &str = "create_task";
pub const VIEW_ALL_NAME: &str = "view_tasks";
pub const VIEW_ONE_NAME: &str = "view_task";
pub const EDIT_NAME: &str = "edit_task";
pub const APPEND_NAME: &str = "append_task";
pub const COMPLETE_NAME: &str = "complete_task";
pub const CANCEL_NAME: &str = "cancel_task";
pub const DELETE_NAME: &str = "delete_task";

// The three states a task may be in. A task is created open, and the model ends it by
// completing it. Only the user may request a task to be cancelled or taken out of the list.
pub const STATUS_OPEN: &str = "open";
pub const STATUS_DONE: &str = "completed";
pub const STATUS_CANCELLED: &str = "cancelled";

// True for a name of one of the eight tools of the suite.
pub fn is_tool(name: &str) -> bool {
    matches!(
        name,
        CREATE_NAME
            | VIEW_ALL_NAME
            | VIEW_ONE_NAME
            | EDIT_NAME
            | APPEND_NAME
            | COMPLETE_NAME
            | CANCEL_NAME
            | DELETE_NAME
    )
}

// The three parts of a task a call may be pointed at, and the column each is kept in. A name or a
// summary is never left without a word in it, for a task is not made without them.
pub const FIELDS: [&str; 3] = ["name", "summary", "content"];

// The most changes of a task one call of `edit_task` may carry.
const MOST_EDITS: usize = 20;

// One row of the table of the tasks.
#[derive(Clone, Debug, Default)]
pub struct Task {
    // The index of the task: a UUID, given out when it was created.
    pub id: String,
    // The name the user of the list reads first.
    pub name: String,
    // The short text about the task, which is what a list of tasks shows.
    pub summary: String,
    // All of what is to be done. This is not shown by `view_tasks`, and is what `view_task` is
    // for.
    pub content: String,
    pub status: String,
    pub created: i64,
    pub updated: i64,
}

// The key the tasks of one working directory are under in `tasks.db`: the path of the directory,
// made absolute and with any trailing separator taken off, so the same directory is always
// named the same way. Nothing of the chat is in the key: two chats of one repository ask the
// same key and so read one list.
pub fn key_of(workdir: &std::path::Path) -> String {
    let made = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf());
    let words = made.display().to_string();
    let both = words.trim_end_matches(std::path::MAIN_SEPARATOR);
    if both.is_empty() {
        words
    } else {
        both.to_string()
    }
}

// The database the tasks of every working directory are in.
fn database() -> Result<std::sync::Arc<db::Db>, String> {
    db::open(&config::paths().data.join(db::TASKS_DB), db::Kind::Tasks)
}

// A UUID for one new task: sixteen markers of the system random source, with the version and
// the variant was as a UUID has them, and written out in the usual five groups. A computer with
// no random source is answered with a marker of the clock, which is unique enough for a list of
// tasks of one run.
pub fn new_index() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        let clock = crate::store::now_secs() as u32;
        for (at, byte) in bytes.iter_mut().enumerate() {
            *byte = ((clock >> ((at % 4) * 8)) ^ (at as u32 * 101)) as u8;
        }
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let marks: Vec<String> = bytes.iter().map(|one| format!("{:02x}", one)).collect();
    format!(
        "{}-{}-{}-{}-{}",
        marks[0..4].concat(),
        marks[4..6].concat(),
        marks[6..8].concat(),
        marks[8..10].concat(),
        marks[10..16].concat()
    )
}

// Read the rows of one working directory, in the order the tasks were created in.
fn rows(work: &str) -> Vec<Task> {
    let Ok(kept) = database() else {
        return Vec::new();
    };
    kept.read(|conn| {
        let mut said = conn
            .prepare(
                "SELECT id, name, summary, content, status, created, updated FROM tasks
                 WHERE work = ?1 ORDER BY created ASC, id ASC",
            )
            .map_err(db::fault)?;
        let made = said
            .query_map(params![work], read_row)
            .map_err(db::fault)?;
        Ok(made.filter_map(|one| one.ok()).collect::<Vec<Task>>())
    })
    .unwrap_or_default()
}

// Turn one row of the table into a task.
fn read_row(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        name: row.get(1)?,
        summary: row.get(2)?,
        content: row.get(3)?,
        status: row.get(4)?,
        created: row.get(5)?,
        updated: row.get(6)?,
    })
}

// The task one call is pointed at, out of the rows of the working directory. The whole index is
// searched for first, and then the start of it: a model that writes the first groups of a UUID
// is answered when only one task can answer to them, and refused when two of them could.
fn lookup(work: &str, index: &str) -> Result<Task, String> {
    let wanted = index.trim().to_lowercase();
    if wanted.is_empty() {
        return Err(String::from(
            "No index was given. Use view_tasks to see the index of every task.",
        ));
    }
    let held = rows(work);
    if let Some(one) = held.iter().find(|one| one.id == wanted) {
        return Ok(one.clone());
    }
    if wanted.len() >= 4 {
        let named: Vec<&Task> = held
            .iter()
            .filter(|one| one.id.starts_with(&wanted))
            .collect();
        match named.len() {
            1 => return Ok(named[0].clone()),
            0 => {}
            many => {
                return Err(format!(
                    "{} tasks begin with {}, so the index is not enough. Give the whole index.",
                    many, wanted
                ));
            }
        }
    }
    Err(format!(
        "No task has the index {}. Use view_tasks to see the indexes.",
        wanted
    ))
}

// Make a task. The name, the summary and the contents are all wanted, for a task the model
// cannot summarise is a task it has not thought about.
pub fn create(work: &str, name: &str, summary: &str, content: &str) -> Result<Task, String> {
    let name = name.trim();
    let summary = summary.trim();
    let content = content.trim();
    if name.is_empty() || summary.is_empty() || content.is_empty() {
        return Err(String::from(
            "A task needs a name, a summary and content. Write all three.",
        ));
    }
    if work.trim().is_empty() {
        return Err(String::from(
            "This chat has no working directory to hold tasks, so nothing was created.",
        ));
    }
    let made = Task {
        id: new_index(),
        name: name.to_string(),
        summary: summary.to_string(),
        content: content.to_string(),
        status: STATUS_OPEN.to_string(),
        created: crate::store::now_secs(),
        updated: crate::store::now_secs(),
    };
    let kept = database()?;
    kept.write(|conn| {
        conn.execute(
            "INSERT INTO tasks (id, work, name, summary, content, status, created, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                made.id,
                work,
                made.name,
                made.summary,
                made.content,
                made.status,
                made.created,
                made.updated
            ],
        )
        .map_err(db::fault)?;
        Ok(())
    })?;
    Ok(made)
}

// Move one task to another state. The row is written whole, so a state is never changed beside
// text of an older reading of it.
fn set_status(work: &str, id: &str, status: &str) -> Result<Task, String> {
    let found = lookup(work, id)?;
    let kept = database()?;
    let when = crate::store::now_secs();
    kept.write(|conn| {
        conn.execute(
            "UPDATE tasks SET status = ?1, updated = ?2 WHERE id = ?3",
            params![status, when, found.id],
        )
        .map_err(db::fault)?;
        Ok(())
    })?;
    Ok(Task {
        status: status.to_string(),
        updated: when,
        ..found
    })
}

// One change of one part of a task: the text to find, the text to put in its place, and whether it
// is to be replaced wherever it stands.
#[derive(Debug)]
struct Change {
    before: String,
    after: String,
    everywhere: bool,
}

// The words of the part of a task a call points at, and the column that part is kept in. Both
// answers come from the same three names, so a part that has no column cannot be reached.
fn part_of<'a>(one: &'a Task, field: &str) -> Result<&'a str, String> {
    match field {
        "name" => Ok(&one.name),
        "summary" => Ok(&one.summary),
        "content" => Ok(&one.content),
        other => Err(format!(
            "\"{other}\" is not a part of a task. The parts are {}.",
            FIELDS.join(", ")
        )),
    }
}

fn column_of(field: &str) -> Option<&'static str> {
    match field {
        "name" => Some("name"),
        "summary" => Some("summary"),
        "content" => Some("content"),
        _ => None,
    }
}

// How many places one run of text is in another.
fn count_of(text: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut places = 0usize;
    let mut from = 0usize;
    while let Some(found) = text[from..].find(needle) {
        places += 1;
        from += found + needle.len();
    }
    places
}

// Change the name, the summary or the content of one task by matching the text that is to be
// changed. The changes are made one after the other, each against the part as the change before it
// left it, and nothing at all is written when one of them matches no place or more than one - the
// way the tool that changes a file behaves, so a task is never left holding half of a change. An
// empty text to find adds the words at the end of the part, which is how the content of a task is
// written out in pieces. A name or a summary is never left without a word in it, for a task is not
// made without them.
pub fn edit(work: &str, index: &str, field: &str, edits: &[Change]) -> Result<Task, String> {
    let found = lookup(work, index)?;
    let held = part_of(&found, field)?;
    let said = column_of(field).ok_or_else(|| format!("\"{field}\" has no place in the list"))?;
    if edits.is_empty() {
        return Err(String::from(
            "The call named no changes. Give the changes of the task in `edits`.",
        ));
    }
    if edits.len() > MOST_EDITS {
        return Err(format!(
            "The call asks for {} changes, and one call takes at most {}."
            ,
            edits.len(),
            MOST_EDITS
        ));
    }
    let mut made = held.to_string();
    for (place, one) in edits.iter().enumerate() {
        let number = place + 1;
        if one.before.is_empty() {
            made.push_str(&one.after);
            continue;
        }
        let hits = count_of(&made, &one.before);
        if hits == 0 {
            return Err(format!(
                "Change {number} matched nothing in the {field} of task {}, so nothing was \
                 written. Copy the text to change out of view_task, spacing included.",
                found.id
            ));
        }
        if hits > 1 && !one.everywhere {
            return Err(format!(
                "Change {number} is in {hits} places of the {field} of task {}, so it is not \
                 enough. Give more of the text, or set replace_all.",
                found.id
            ));
        }
        made = if one.everywhere {
            made.replace(&one.before, &one.after)
        } else {
            let at = made.find(&one.before).unwrap_or(0);
            format!(
                "{}{}{}",
                &made[..at],
                one.after,
                &made[at + one.before.len()..]
            )
        };
    }
    let made = made.trim().to_string();
    if made.is_empty() && field != "content" {
        return Err(format!(
            "The changes would leave the {field} of task {} with nothing in it. A task needs a {}.",
            found.id, field
        ));
    }
    let when = crate::store::now_secs();
    let kept = database()?;
    let text = made.clone();
    kept.write(|conn| {
        conn.execute(
            &format!(
                "UPDATE tasks SET {said} = ?1, updated = ?2 WHERE id = ?3",
                said = said
            ),
            params![text, when, found.id],
        )
        .map_err(db::fault)?;
        Ok(())
    })?;
    let mut updated = found;
    match said {
        "name" => updated.name = made,
        "summary" => updated.summary = made,
        _ => updated.content = made,
    }
    updated.updated = when;
    Ok(updated)
}

// Add to the content of one task, without writing the whole of it again. The words go under the
// content that stands, with an empty line between the two, which is how the content of a task reads
// when it was written in one call.
pub fn append(work: &str, index: &str, more: &str) -> Result<Task, String> {
    let found = lookup(work, index)?;
    let more = more.trim();
    if more.is_empty() {
        return Err(String::from(
            "Nothing was given to add. Give the words to add to the content of the task.",
        ));
    }
    let made = if found.content.trim().is_empty() {
        more.to_string()
    } else {
        format!("{}\n\n{}", found.content.trim_end(), more)
    };
    let when = crate::store::now_secs();
    let kept = database()?;
    let text = made.clone();
    kept.write(|conn| {
        conn.execute(
            "UPDATE tasks SET content = ?1, updated = ?2 WHERE id = ?3",
            params![text, when, found.id],
        )
        .map_err(db::fault)?;
        Ok(())
    })?;
    Ok(Task {
        content: made,
        updated: when,
        ..found
    })
}

// The changes of one call: the text to find, the text to put in its place, and whether it stands
// everywhere it is. A call that named no change is refused, for a change that does nothing is read
// as the task being looked at.
fn changes_of(value: &Value) -> Result<Vec<Change>, String> {
    let Some(items) = value.get("edits").and_then(Value::as_array) else {
        return Err(String::from(
            "The call named no changes. Give the changes of the task in `edits`.",
        ));
    };
    if items.is_empty() {
        return Err(String::from("The call named no changes."));
    }
    let mut out: Vec<Change> = Vec::with_capacity(items.len());
    for item in items {
        let before = item
            .get("old_string")
            .and_then(Value::as_str)
            .ok_or_else(|| String::from("A change of the call has no old_string."))?
            .to_string();
        let after = item
            .get("new_string")
            .and_then(Value::as_str)
            .ok_or_else(|| String::from("A change of the call has no new_string."))?
            .to_string();
        let everywhere = item
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        out.push(Change {
            before,
            after,
            everywhere,
        });
    }
    Ok(out)
}

// Take one task out of the list for good.
fn remove(work: &str, id: &str) -> Result<Task, String> {
    let found = lookup(work, id)?;
    let kept = database()?;
    kept.write(|conn| {
        conn.execute("DELETE FROM tasks WHERE id = ?1", params![found.id])
            .map_err(db::fault)?;
        Ok(())
    })?;
    Ok(found)
}

// Take the whole task list of one working directory away, and say how many tasks went. This is for
// when the working directory of a repository, or of a loose chat, is removed: the tasks it held
// have nothing left to be done in, and a list keyed by a path that no longer exists would be
// text kept for nothing. It is not a call the model has: the suite answers to one task at a
// time.
pub fn drop_work(work: &str) -> usize {
    if work.is_empty() {
        return 0;
    }
    let Ok(kept) = database() else {
        return 0;
    };
    match kept.write(|conn| {
        conn.execute("DELETE FROM tasks WHERE work = ?1", params![work])
            .map_err(db::fault)
    }) {
        Ok(gone) => gone,
        Err(problem) => {
            proglog::note(
                proglog::STORE,
                &format!("the tasks of {} are not removed: {}", work, problem),
            );
            0
        }
    }
}

// The text of one task in a list of them: the index, the state, the name, and under them the
// summary. The contents of a task are never written into a list, for a list of the contents of
// twelve tasks is a wall of text the model has to read through to find the one it is looking
// for.
fn line_of(one: &Task) -> String {
    format!(
        "{}  [{}] {}\n    {}",
        one.id,
        one.status,
        one.name,
        one.summary.replace('\n', " ")
    )
}

// The count of the tasks in each state, for the header of a list.
fn tally(held: &[Task]) -> String {
    let open = held
        .iter()
        .filter(|one| one.status == STATUS_OPEN)
        .count();
    let done = held
        .iter()
        .filter(|one| one.status == STATUS_DONE)
        .count();
    let off = held
        .iter()
        .filter(|one| one.status == STATUS_CANCELLED)
        .count();
    format!(
        "{} open, {} completed, {} cancelled",
        open,
        done,
        off
    )
}

// All of the list, in the order the tasks were created in.
fn list_words(work: &str) -> tools::Report {
    let held = rows(work);
    if held.is_empty() {
        return report(
            true,
            String::from(
                "No tasks for this working directory. Create one with create_task when the work has parts that have to be returned to later.",
            ),
        );
    }
    let mut out = format!(
        "{} tasks for this working directory: {}\n\n",
        held.len(),
        tally(&held)
    );
    for one in held.iter() {
        out.push_str(&line_of(one));
        out.push('\n');
    }
    out.push_str(
        "\nEach index above is the number of the task. The content of a task is not listed here: call view_task with its index to get it.",
    );
    report(true, out)
}

// One task, with its contents.
fn one_words(work: &str, index: &str) -> tools::Report {
    match lookup(work, index) {
        Err(why) => report(false, why),
        Ok(one) => report(
            true,
            format!(
                "index: {}\nstatus: {}\nname: {}\nsummary: {}\ncontent:\n{}",
                one.id, one.status, one.name, one.summary, one.content
            ),
        ),
    }
}

// What the answer of `edit_task` reports: the task as it now stands, and the part that was changed
// in full when it is short enough to be repeated. A content longer than that is not written into
// the answer and not cut up in it either - `view_task` is the tool for reading a content, and half
// of a content in an answer is the kind of text a model mistakes for the whole of it.
fn changed_words(value: &Value, one: &Task) -> String {
    let field = word_of(value, "field");
    let held = part_of(one, field.as_str()).unwrap_or("");
    let mut out = format!(
        "Task {} is changed.\nstatus: {}\n{}: {}\n",
        one.id,
        one.status,
        field,
        held
    );
    if held.chars().count() > 80 || held.contains('\n') {
        out = format!(
            "Task {} is changed.\nstatus: {}\n{}\nThe {} now holds {} characters and {} lines. Call view_task with the index to read all of it.",
            one.id,
            one.status,
            line_of(one),
            field,
            held.chars().count(),
            held.lines().count()
        );
    }
    out.push_str("\n\nNothing else of the task is changed.");
    out
}

// A small item the tool answers with.
fn report(ok: bool, output: String) -> tools::Report {
    tools::Report { ok, output }
}

// The report of one of the six calls. `work` is the directory the tasks of this chat belong to.
pub fn run(name: &str, work: &str, raw: &str) -> tools::Report {
    let value = serde_json::from_str::<Value>(raw).unwrap_or_else(|_| json!({}));
    let made = match name {
        CREATE_NAME => {
            let held = create(
                work,
                word_of(&value, "name").as_str(),
                word_of(&value, "summary").as_str(),
                word_of(&value, "content").as_str(),
            );
            match held {
                Err(why) => report(false, why),
                Ok(one) => {
                    let left = rows(work)
                        .iter()
                        .filter(|task| task.status == STATUS_OPEN)
                        .count();
                    report(
                        true,
                        format!(
                            "Task created.\nindex: {}\nstatus: {}\nname: {}\nsummary: {}\n\n{} open task(s) for this working directory.",
                            one.id, one.status, one.name, one.summary, left
                        ),
                    )
                }
            }
        }
        VIEW_ALL_NAME => list_words(work),
        VIEW_ONE_NAME => one_words(work, word_of(&value, "index").as_str()),
        EDIT_NAME => {
            let index = word_of(&value, "index");
            let field = word_of(&value, "field");
            match changes_of(&value).and_then(|edits| edit(work, index.as_str(), field.as_str(), &edits)) {
                Err(why) => report(false, why),
                Ok(one) => report(true, changed_words(&value, &one)),
            }
        }
        APPEND_NAME => match append(
            work,
            word_of(&value, "index").as_str(),
            word_of(&value, "content").as_str(),
        ) {
            Err(why) => report(false, why),
            Ok(one) => report(
                true,
                format!(
                    "The content of task {} is added to.\nname: {}\n{} characters and {} lines of content now.\n\nCall view_task with the index when you need all of the content.",
                    one.id,
                    one.name,
                    one.content.chars().count(),
                    one.content.lines().count()
                ),
            ),
        },
        COMPLETE_NAME => match set_status(work, word_of(&value, "index").as_str(), STATUS_DONE) {
            Err(why) => report(false, why),
            Ok(one) => {
                let left = rows(work)
                    .iter()
                    .filter(|task| task.status == STATUS_OPEN)
                    .count();
                report(
                    true,
                    format!(
                        "Task {} completed: {}.\n\n{} open task(s) for this working directory.",
                        one.id,
                        one.name,
                        left
                    ),
                )
            }
        },
        CANCEL_NAME => match set_status(work, word_of(&value, "index").as_str(), STATUS_CANCELLED) {
            Err(why) => report(false, why),
            Ok(one) => report(
                true,
                format!("Task {} is cancelled: {}.", one.id, one.name),
            ),
        },
        DELETE_NAME => match remove(work, word_of(&value, "index").as_str()) {
            Err(why) => report(false, why),
            Ok(one) => report(
                true,
                format!("Task {} deleted: {}.", one.id, one.name),
            ),
        },
        other => report(
            false,
            format!("{} is no tool of the tasks suite", other),
        ),
    };
    made
}

// The one part of a call that names a task. A part that is not there is read as nothing
// written, and the caller reports that.
fn word_of(value: &Value, part: &str) -> String {
    match value.get(part) {
        Some(Value::String(one)) => one.clone(),
        Some(Value::Number(one)) => one.to_string(),
        _ => String::new(),
    }
}

// The first text of a call, for the header of its block: the name of the task being made, or
// the index of the task being pointed at.
pub fn partial_words(name: &str, raw: &str) -> String {
    let value = serde_json::from_str::<Value>(raw).unwrap_or_else(|_| json!({}));
    let part = if name == CREATE_NAME { "name" } else { "index" };
    word_of(&value, part)
}

// The eight definitions of the suite, in the order the model is proposed them in.
pub fn definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": CREATE_NAME,
                "description": "Create a task in the task list of this working directory, and remember its index. Use it when the work has parts that cannot be done at once, or that have to be returned to later. All three parts are required: the name is the short text the list shows, the summary is one sentence about what the task is for, and the content is everything that has to be done, in enough detail that you could do it from the content alone. The answer reports the index of the task: it is a UUID, and every other tool of this suite takes it.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "The name of the task, in a few words." },
                        "summary": { "type": "string", "description": "One sentence: what the task is for." },
                        "content": { "type": "string", "description": "All of what is to be done, in detail." }
                    },
                    "required": ["name", "summary", "content"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": VIEW_ALL_NAME,
                "description": "List every task of this working directory, with the index, the state, the name and the summary of each. Nothing is requested. The contents of the tasks are not returned: call view_task with the index of a task for its contents. Call this before you write your final message, to check that every task that is left open is dealt with.",
                "parameters": { "type": "object", "properties": {} }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": VIEW_ONE_NAME,
                "description": "View one task: its name, its summary and its content. Give the whole index, or the start of it when that identifies one task and no other.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "index": { "type": "string", "description": "The index of the task, as create_task returned it." }
                    },
                    "required": ["index"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": EDIT_NAME,
                "description": "Change the name, the summary or the content of one task by matching the text that is to be changed. Say which part of the task is changed under `field`, and give the changes under `edits`. Copy each old_string exactly from the task as view_task returned it, spacing included. The changes are made one after another, each against the part as the change before it left it. Each old_string must match one place, unless replace_all is set for it; if one matches no place, or more than one, nothing at all is written. An old_string that is empty adds the words of new_string at the end of the part. A name or a summary is never left with nothing in it. To add to the content without writing it again, use append_task.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "index": { "type": "string", "description": "The index of the task, or the start of it when that identifies one task and no other." },
                        "field": { "type": "string", "enum": ["name", "summary", "content"], "description": "Which part of the task the changes are made in." },
                        "edits": {
                            "type": "array",
                            "description": "The changes to make in one call, each giving the text to replace and the text to put in its place.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "old_string": { "type": "string", "description": "The text to replace, copied from the task. Empty means: add the words of new_string at the end of the part." },
                                    "new_string": { "type": "string", "description": "The text to put in its place." },
                                    "replace_all": { "type": "boolean", "description": "Replace every place this text appears, instead of requiring that it appears once. Off when it is not given." }
                                },
                                "required": ["old_string", "new_string"]
                            }
                        }
                    },
                    "required": ["index", "field", "edits"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": APPEND_NAME,
                "description": "Add to the content of one task, without writing the whole of it again. The words you give go under the content that stands, with an empty line between the two. Use it when a task gains a further requirement, or when the content is written out in pieces. Only the content is changed: the name, the summary and the status stay as they were, and view_task returns the content as it now is.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "index": { "type": "string", "description": "The index of the task, or the start of it when that identifies one task and no other." },
                        "content": { "type": "string", "description": "The words to add to the content of the task." }
                    },
                    "required": ["index", "content"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": COMPLETE_NAME,
                "description": "Set the status of one task to completed. Call it when the work of the task is really done, and not because the answer is coming to an end.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "index": { "type": "string", "description": "The index of the task." }
                    },
                    "required": ["index"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": CANCEL_NAME,
                "description": "Set the status of one task to cancelled, leaving it in the list. Only use this when the user has requested the task to be cancelled. Never cancel a task on your own initiative, and never to clear a list that has grown long.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "index": { "type": "string", "description": "The index of the task." }
                    },
                    "required": ["index"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": DELETE_NAME,
                "description": "Delete one task from the list. Only use this when the user has requested the task to be deleted. A task that is no longer wanted is cancelled and stays in the list to be read; deleting is the user's decision, not yours.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "index": { "type": "string", "description": "The index of the task." }
                    },
                    "required": ["index"]
                }
            }
        }),
    ]
}

// The text added to the system prompt while the suite is on. The rule the suite exists to keep
// is the last one: a model that ends its turn with tasks still open leaves the work in the
// middle, and the user has to ask what became of it.
pub fn system_prompt() -> String {
    String::from(
        "TASKS SUITE\n\n\
You have a list of tasks for the working directory of this chat, and eight tools that work on it: create_task, view_tasks, view_task, edit_task, append_task, complete_task, cancel_task and delete_task. The tasks belong to the working directory, not to this chat, and they stand there after this chat ends. Every task has an index - a UUID, which create_task gives out - and the other seven tools are pointed at a task by that index.\n\n\
Make a task when the work has parts that cannot be done at once, or that have to be gone back to later. Write the content of a task so that it can be done from the content alone. Set a task to completed with complete_task when its work is really done.\n\n\
When a task turns out to ask for something different, or for more than it did, change it rather than make a second task about the same work: edit_task changes its name, its summary or its content by matching the text to change, and append_task adds to the content without writing the whole of it again. Read the task with view_task before you change it, and copy the text you are matching from what it returned.\n\n\
cancel_task and delete_task are for the user alone. Use either of them only when the user has asked for that task to be cancelled or deleted. A task you no longer want is left in the list, or cancelled only if the user asked.\n\n\
Before you write your final message to the user, and so before you end this turn, you must call view_tasks and read the list. Every task that stands open has to be dealt with: finish it, or say in your message why it stands open and what you need from the user to finish it. Never end a turn with a task left open without a word about it.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // A directory of the computer for one check.
    fn place(what: &str) -> String {
        let dir = std::env::temp_dir()
            .join("an-tasks")
            .join(what);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the place of the check is created");
        dir.display().to_string()
    }

    // An index is a UUID: thirty-six markers, in five groups, with the version of it was, and
    // no two of them the same.
    #[test]
    fn an_index_is_a_uuid() {
        let one = new_index();
        assert_eq!(one.len(), 36, "{}", one);
        let groups: Vec<&str> = one.split('-').collect();
        assert_eq!(
            groups.iter().map(|one| one.len()).collect::<Vec<usize>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(one.chars().all(|one| one.is_ascii_hexdigit() || one == '-'));
        assert_eq!(&one[14..15], "4", "the version of the UUID");
        assert!(
            matches!(&one[19..20], "8" | "9" | "a" | "b"),
            "the variant of the UUID"
        );
        assert_ne!(one, new_index());
    }

    // A task is created with all three of its parts, and refused without them. The rows of one
    // working directory are kept apart from the rows of another, for a task belongs to the
    // directory it was created in.
    #[test]
    fn a_task_is_made_with_its_three_parts_and_kept_apart_by_work_directory() {
        let _guard = config::test_turn();
        setup();
        let first = place("one");
        let other = place("two");
        assert!(create(&first, "  ", "summary", "content").is_err());
        assert!(create(&first, "name", " ", "content").is_err());
        assert!(create(&first, "name", "summary", "   ").is_err());
        let made = create(
            &first,
            " Lay out the parser ",
            "Read the grammar and write the table.",
            "Take the grammar, then write the table of rules.",
        )
        .expect("the task is made");
        assert_eq!(made.name, "Lay out the parser", "the words are trimmed");
        assert_eq!(made.status, STATUS_OPEN);
        assert_eq!(made.id.len(), 36);
        create(
            &other,
            "A task of another directory",
            "It remains elsewhere.",
            "Nothing to do with the first.",
        )
        .expect("the second task is created");
        let held = rows(&first);
        assert_eq!(held.len(), 1, "only the task of this directory remains");
        assert_eq!(held[0].name, "Lay out the parser");
        assert_eq!(rows(&other).len(), 1);
    }

    // The eight calls of the suite, read the same list over: the list without the contents of the
    // tasks, one task with them, the three ways a task is ended, and a task that is not there.
    #[test]
    fn the_six_calls_work_on_the_same_list() {
        let _guard = config::test_turn();
        setup();
        let work = place("calls");
        let made = create(&work, "Write the reader", "Read the file.", "Read the file whole.").expect("a task");
        let index = made.id.clone();
        let short = index[..8].to_string();
        let said = run(VIEW_ALL_NAME, &work, "{}");
        assert!(said.ok, "{}", said.output);
        assert!(said.output.contains(&index), "{}", said.output);
        assert!(said.output.contains("[open] Write the reader"), "{}", said.output);
        assert!(
            !said.output.contains("Read the file whole"),
            "the list does not carry the contents of a task"
        );
        // One task, contents and all, by the whole index and by the start of it.
        let one = run(VIEW_ONE_NAME, &work, &format!("{{\"index\":\"{}\"}}", short));
        assert!(one.ok, "{}", one.output);
        assert!(one.output.contains("content:\nRead the file whole."), "{}", one.output);
        assert_eq!(
            run(VIEW_ONE_NAME, &work, r#"{"index":"ffffffff"}"#).ok,
            false,
            "an index that is not there is reported"
        );
        // Completed, and the list records it.
        let done = run(
            COMPLETE_NAME,
            &work,
            &format!("{{\"index\":\"{}\"}}", index),
        );
        assert!(done.ok, "{}", done.output);
        assert!(done.output.contains("completed"));
        let list = run(VIEW_ALL_NAME, &work, "{}");
        assert!(list.output.contains("[completed] Write the reader"), "{}", list.output);
        // Cancelled, then taken out of the list: after it, the list is empty.
        let off = run(
            CANCEL_NAME,
            &work,
            &format!("{{\"index\":\"{}\"}}", index),
        );
        assert!(off.ok, "{}", off.output);
        assert!(off.output.contains("cancelled"));
        let gone = run(
            DELETE_NAME,
            &work,
            &format!("{{\"index\":\"{}\"}}", index),
        );
        assert!(gone.ok, "{}", gone.output);
        assert!(gone.output.contains("deleted"));
        let empty = run(VIEW_ALL_NAME, &work, "{}");
        assert!(empty.output.contains("No tasks for this working directory"), "{}", empty.output);
        let _ = std::fs::remove_dir_all(&work);
    }

    // A task is changed part by part, and added to, without the whole of it being written again.
    // A change that matches nothing, or matches in two places, leaves the task exactly as it was.
    #[test]
    fn a_task_is_changed_and_added_to() {
        let _guard = config::test_turn();
        setup();
        let work = place("changed");
        let made = create(
            &work,
            "Read the reader",
            "Read the file.",
            "Read the file whole.\nThen write the table.",
        )
        .expect("a task");
        let index = made.id.clone();
        // One line of the content, matched by its exact words.
        let said = run(
            EDIT_NAME,
            &work,
            &format!(
                "{{\"index\":\"{index}\",\"field\":\"content\",\"edits\":[{{\"old_string\":\"Then write the table.\",\"new_string\":\"Then write the table of rules.\"}}]}}"
            ),
        );
        assert!(said.ok, "{}", said.output);
        let one = run(VIEW_ONE_NAME, &work, &format!("{{\"index\":\"{index}\"}}"));
        assert!(
            one.output.contains("Then write the table of rules."),
            "{}",
            one.output
        );
        assert!(
            one.output.contains("Read the file whole."),
            "the words that were not named stay: {}",
            one.output
        );
        assert!(
            one.output.contains("Read the reader"),
            "the name of the task is not moved: {}",
            one.output
        );
        // The name and the summary are parts of a task as the content is.
        let named = run(
            EDIT_NAME,
            &work,
            &format!(
                "{{\"index\":\"{index}\",\"field\":\"name\",\"edits\":[{{\"old_string\":\"reader\",\"new_string\":\"reader of the crate\",\"replace_all\":true}}]}}"
            ),
        );
        assert!(named.ok, "{}", named.output);
        let held = rows(&work);
        assert_eq!(held[0].name, "Read the reader of the crate", "{}", held[0].name);
        // A change that matches nothing says so, and writes nothing.
        let missed = run(
            EDIT_NAME,
            &work,
            &format!(
                "{{\"index\":\"{index}\",\"field\":\"content\",\"edits\":[{{\"old_string\":\"nowhere at all\",\"new_string\":\"x\"}}]}}"
            ),
        );
        assert!(!missed.ok, "a change that matches nothing is refused");
        assert!(missed.output.contains("matched nothing"), "{}", missed.output);
        assert_eq!(
            rows(&work)[0].content,
            held[0].content,
            "nothing was written"
        );
        // A change in two places is refused until replace_all is set for it.
        let twice = run(
            EDIT_NAME,
            &work,
            &format!(
                "{{\"index\":\"{index}\",\"field\":\"content\",\"edits\":[{{\"old_string\":\"the\",\"new_string\":\"one\"}}]}}"
            ),
        );
        assert!(!twice.ok, "a change in two places is not enough");
        assert!(twice.output.contains("2 places"), "{}", twice.output);
        let whole = run(
            EDIT_NAME,
            &work,
            &format!(
                "{{\"index\":\"{index}\",\"field\":\"content\",\"edits\":[{{\"old_string\":\"the\",\"new_string\":\"one\",\"replace_all\":true}}]}}"
            ),
        );
        assert!(whole.ok, "{}", whole.output);
        assert!(!rows(&work)[0].content.contains(" the "), "every place was changed");
        // A name is never left with nothing in it.
        let empty = run(
            EDIT_NAME,
            &work,
            &format!(
                "{{\"index\":\"{index}\",\"field\":\"name\",\"edits\":[{{\"old_string\":\"Read the reader of the crate\",\"new_string\":\"  \"}}]}}"
            ),
        );
        assert!(!empty.ok, "a name is never left empty");
        assert!(empty.output.contains("needs a name"), "{}", empty.output);
        // A part of a task that is not one of the three is refused by name.
        let wrong = run(
            EDIT_NAME,
            &work,
            &format!(
                "{{\"index\":\"{index}\",\"field\":\"status\",\"edits\":[{{\"old_string\":\"open\",\"new_string\":\"done\"}}]}}"
            ),
        );
        assert!(!wrong.ok, "a part that is not one of the three is refused: {}", wrong.output);
        assert!(wrong.output.contains("not a part of a task"), "{}", wrong.output);
        // The words of a task are added to, under the content that stands, and nothing else of
        // the task moves.
        let before = rows(&work)[0].clone();
        let more = run(
            APPEND_NAME,
            &work,
            &format!(
                "{{\"index\":\"{index}\",\"content\":\"Also: keep the words of each fault.\"}}"
            ),
        );
        assert!(more.ok, "{}", more.output);
        let after = rows(&work)[0].clone();
        assert!(
            after.content.starts_with(before.content.trim_end()),
            "the content that stood is kept: {:?}",
            after.content
        );
        assert!(
            after.content.ends_with("Also: keep the words of each fault."),
            "the words were added under it: {:?}",
            after.content
        );
        assert!(after.content.contains("\n\nAlso:"), "with an empty line between");
        assert_eq!(after.name, before.name, "the name is not moved");
        assert_eq!(after.summary, before.summary, "the summary is not moved");
        assert_eq!(after.status, before.status, "the status is not moved");
        assert!(after.updated >= before.updated, "the row records the change");
        // Nothing given to add is a call that would empty nothing and change nothing.
        let nothing = run(
            APPEND_NAME,
            &work,
            &format!("{{\"index\":\"{index}\",\"content\":\"   \"}}"),
        );
        assert!(!nothing.ok, "nothing given to add is refused: {}", nothing.output);
        assert_eq!(rows(&work)[0].content, after.content, "nothing was added");
        let _ = std::fs::remove_dir_all(&work);
    }

    // The definitions of the suite: eight of them, all parts wanted of a task, and the two tools
    // of the user's own saying so in their text.
    #[test]
    fn the_eight_definitions_are_offered() {
        let list = definitions();
        assert_eq!(list.len(), 8);
        let names: Vec<&str> = list
            .iter()
            .map(|one| one["function"]["name"].as_str().expect("a name"))
            .collect();
        assert_eq!(
            names,
            vec![
                CREATE_NAME,
                VIEW_ALL_NAME,
                VIEW_ONE_NAME,
                EDIT_NAME,
                APPEND_NAME,
                COMPLETE_NAME,
                CANCEL_NAME,
                DELETE_NAME
            ]
        );
        for one in list.iter() {
            let name = one["function"]["name"].as_str().unwrap_or("");
            assert!(is_tool(name), "{} is named as a tool", name);
        }
        let wanted = list[0]["function"]["parameters"]["required"]
            .as_array()
            .expect("the parts wanted")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<&str>>();
        assert_eq!(wanted, vec!["name", "summary", "content"]);
        // The two tools that change what a task says name the part they change, and the words to
        // change it by.
        let changed = &list[3]["function"];
        assert_eq!(changed["name"], EDIT_NAME);
        assert_eq!(
            changed["parameters"]["required"]
                .as_array()
                .expect("the parts wanted")
                .iter()
                .map(|one| one.as_str().unwrap_or(""))
                .collect::<Vec<&str>>(),
            vec!["index", "field", "edits"]
        );
        assert_eq!(
            changed["parameters"]["properties"]["field"]["enum"]
                .as_array()
                .expect("the parts of a task")
                .iter()
                .map(|one| one.as_str().unwrap_or(""))
                .collect::<Vec<&str>>(),
            FIELDS
        );
        let added = &list[4]["function"];
        assert_eq!(added["name"], APPEND_NAME);
        assert!(
            added["description"]
                .as_str()
                .unwrap_or("")
                .contains("without writing the whole of it again"),
            "{}",
            added["description"]
        );
        let cancel = list[6]["function"]["description"]
            .as_str()
            .expect("words")
            .to_lowercase();
        assert!(cancel.contains("only use this when the user has requested"), "{cancel}");
        let remove = list[7]["function"]["description"]
            .as_str()
            .expect("words")
            .to_lowercase();
        assert!(remove.contains("only use this when the user has requested"), "{remove}");
    }

    // The prompt of the suite names all eight tools and the rule the suite exists for: the list
    // is read before an answer ends.
    #[test]
    fn the_prompt_names_the_suite_and_the_rule_at_the_end_of_a_turn() {
        let said = system_prompt();
        for name in [
            CREATE_NAME,
            VIEW_ALL_NAME,
            VIEW_ONE_NAME,
            COMPLETE_NAME,
            CANCEL_NAME,
            DELETE_NAME,
        ] {
            assert!(said.contains(name), "the prompt reports nothing of {}", name);
        }
        assert!(said.contains("call view_tasks and read the list"), "{said}");
        assert!(said.contains("Before you write your final message"), "{said}");
        assert!(said.contains("belong to the working directory"), "{said}");
        assert!(said.contains("user alone"), "{said}");
        assert!(said.contains(EDIT_NAME), "the prompt names {EDIT_NAME}");
        assert!(said.contains(APPEND_NAME), "the prompt names {APPEND_NAME}");
    }

    // The checks work in a directory of their own, so a computer of the older format is never
    // read into them. The list is emptied first: a check reads only the tasks it made itself,
    // and not the rows an earlier run of the checks left remaining under the same work
    // directory.
    fn setup() {
        config::set_data_dir(std::path::PathBuf::from("uidata/tasks-tests"));
        let _ = config::ensure_dirs();
        crate::migrate::mark_ran(&config::paths().data);
        let kept = database().expect("the database of the checks is left open");
        kept.write(|conn| {
            conn.execute("DELETE FROM tasks", params![])
                .map_err(db::fault)?;
            Ok(())
        })
        .expect("the list is emptied for the checks");
    }

    // The whole list of a working directory goes when the directory goes. A list keyed by a path
    // that no longer exists is nothing to work from, and the next repository found at that path
    // must not inherit the tasks of the one that went. The list of another directory remains
    // untouched.
    #[test]
    fn the_list_of_a_work_directory_goes_with_it() {
        let _guard = config::test_turn();
        setup();
        let going = place("going");
        let staying = place("staying");
        create(&going, "One", "Its summary.", "Its content.").expect("a task is made");
        create(&going, "Two", "Its summary.", "Its content.").expect("a second task");
        create(&staying, "Three", "Its summary.", "Its content.").expect("a task elsewhere");
        assert_eq!(drop_work(""), 0, "no directory has no list to give");
        assert_eq!(drop_work(&going), 2, "both tasks of the directory went");
        assert!(rows(&going).is_empty(), "nothing of that list is left");
        assert_eq!(rows(&staying).len(), 1, "the list of another directory remains");
        assert_eq!(drop_work(&going), 0, "a list that is removed gives nothing");
        let _ = std::fs::remove_dir_all(&going);
        let _ = std::fs::remove_dir_all(&staying);
    }
}
