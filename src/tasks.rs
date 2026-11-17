// The tasks of a work directory: a list of things to be done that the model
// writes itself, works through, and checks before it ends an answer.
//
// The six tools stand behind one line of the badge of the tools, "Tasks suite",
// which is off until the user turns it on. They are: `create_task`,
// `view_tasks`, `view_task`, `complete_task`, `cancel_task` and `delete_task`.
// The last two change what the user asked for, so their descriptions say that
// they are only for when the user asked for it, and the prompt of the suite says
// it again.
//
// A task belongs to the work directory the chat works in - the directory of the
// repository, or `work/<chat id>/` for a loose chat - and not to the chat. Two
// chats of one repository therefore see the same tasks, and a task outlives the
// chat that made it. The list is keyed by the path of that directory.
//
// The rows stand in `tasks.db`, under the data folder of the program. Nothing is
// written into the work directory itself: a model works there, and a file of the
// program's own lying in a directory a command runs in would be read, changed and
// committed like any other file. See `src/db.rs`.
//
// The index of a task is a UUID, given out when the task is made and never
// changed. The rows are kept in the order they were made in, which is the order
// `view_tasks` writes them out in. A call may name a task by the whole index or
// by the start of it, as long as the start names one task and not two.

use rusqlite::params;
use serde_json::{json, Value};

use crate::config;
use crate::db;
use crate::tools;

// The six names the model calls the suite by.
pub const CREATE_NAME: &str = "create_task";
pub const VIEW_ALL_NAME: &str = "view_tasks";
pub const VIEW_ONE_NAME: &str = "view_task";
pub const COMPLETE_NAME: &str = "complete_task";
pub const CANCEL_NAME: &str = "cancel_task";
pub const DELETE_NAME: &str = "delete_task";

// The three states a task may stand in. A task is made open, and the model ends
// it by completing it. Only the user may ask for a task to be cancelled or
// taken out of the list.
pub const STATUS_OPEN: &str = "open";
pub const STATUS_DONE: &str = "completed";
pub const STATUS_CANCELLED: &str = "cancelled";

// True for a name of one of the six tools of the suite.
pub fn is_tool(name: &str) -> bool {
    matches!(
        name,
        CREATE_NAME | VIEW_ALL_NAME | VIEW_ONE_NAME | COMPLETE_NAME | CANCEL_NAME | DELETE_NAME
    )
}

// One row of the table of the tasks.
#[derive(Clone, Debug, Default)]
pub struct Task {
    // The index of the task: a UUID, given out when it was made.
    pub id: String,
    // The name the user of the list reads first.
    pub name: String,
    // The short words about the task, which are what a list of them shows.
    pub summary: String,
    // The whole of what is to be done. This is not shown by `view_tasks`, and is
    // what `view_task` is for.
    pub content: String,
    pub status: String,
    pub created: i64,
    pub updated: i64,
}

// The key the tasks of one work directory stand under in `tasks.db`: the path of
// the directory, made absolute and with any trailing separator taken off, so the
// same directory is always named the same way. Nothing of the chat is in the
// key: two chats of one repository ask the same key and so read one list.
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

// The database the tasks of every work directory stand in.
fn database() -> Result<std::sync::Arc<db::Db>, String> {
    db::open(&config::paths().data.join(db::TASKS_DB), db::Kind::Tasks)
}

// A UUID for one new task: sixteen marks of the system random source, with the
// version and the variant stood as a UUID has them, and written out in the
// usual five groups. A machine with no random source is answered with a mark of
// the clock, which is unique enough for a list of tasks of one run.
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

// Read the rows of one work directory, in the order the tasks were made in.
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

// The task one call is pointed at, out of the rows of the work directory. The
// whole index is looked for first, and then the start of it: a model that
// writes the first groups of a UUID is answered when only one task can answer
// to them, and refused when two of them could.
fn lookup(work: &str, index: &str) -> Result<Task, String> {
    let wanted = index.trim().to_lowercase();
    if wanted.is_empty() {
        return Err(String::from(
            "No index was named. Use view_tasks to see the index of every task.",
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
                    "{} tasks begin with {}, so the index is not enough. Name the whole of it.",
                    many, wanted
                ));
            }
        }
    }
    Err(format!(
        "No task stands with the index {}. Use view_tasks to see the indexes.",
        wanted
    ))
}

// Make a task. The name, the summary and the contents are all wanted, for a
// task the model cannot summarise is a task it has not thought about.
pub fn create(work: &str, name: &str, summary: &str, content: &str) -> Result<Task, String> {
    let name = name.trim();
    let summary = summary.trim();
    let content = content.trim();
    if name.is_empty() || summary.is_empty() || content.is_empty() {
        return Err(String::from(
            "A task needs a name, a summary and contents. Write all three.",
        ));
    }
    if work.trim().is_empty() {
        return Err(String::from(
            "This chat has no work directory to hold tasks, so nothing was made.",
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

// Move one task to another state. The row is written whole, so a state is never
// changed beside words of an older reading of it.
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

// The words of one task in a list of them: the index, the state, the name, and
// under them the summary. The contents of a task are never written into a list,
// for a list of the contents of twelve tasks is a wall of words the model has
// to read through to find the one it is looking for.
fn line_of(one: &Task) -> String {
    format!(
        "{}  [{}] {}\n    {}",
        one.id,
        one.status,
        one.name,
        one.summary.replace('\n', " ")
    )
}

// The count of the tasks in each state, for the head of a list.
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

// The whole of the list, in the order the tasks were made in.
fn list_words(work: &str) -> tools::Report {
    let held = rows(work);
    if held.is_empty() {
        return report(
            true,
            String::from(
                "No task stands for this work directory. Make one with create_task when the work has parts that have to be gone back to.",
            ),
        );
    }
    let mut out = format!(
        "{} tasks stand for this work directory: {}.\n\n",
        held.len(),
        tally(&held)
    );
    for one in held.iter() {
        out.push_str(&line_of(one));
        out.push('\n');
    }
    out.push_str(
        "\nEach index above is the number of the task. The contents of a task are not listed here: call view_task with its index for those.",
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

// A small thing the tool answers with.
fn report(ok: bool, output: String) -> tools::Report {
    tools::Report { ok, output }
}

// The report of one of the six calls. `work` is the directory the tasks of this
// chat belong to.
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
                            "Task made.\nindex: {}\nstatus: {}\nname: {}\nsummary: {}\n\n{} task or tasks of this work directory stand open.",
                            one.id, one.status, one.name, one.summary, left
                        ),
                    )
                }
            }
        }
        VIEW_ALL_NAME => list_words(work),
        VIEW_ONE_NAME => one_words(work, word_of(&value, "index").as_str()),
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
                        "Task {} is completed: {}.\n\n{} task or tasks of this work directory stand open.",
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
                format!("Task {} was taken out of the list: {}.", one.id, one.name),
            ),
        },
        other => report(
            false,
            format!("{} is no tool of the tasks suite", other),
        ),
    };
    made
}

// The one word of a call that names a task, or the one word that names it. A
// part that is not there is read as nothing written, which the caller answers
// about.
fn word_of(value: &Value, part: &str) -> String {
    match value.get(part) {
        Some(Value::String(one)) => one.clone(),
        Some(Value::Number(one)) => one.to_string(),
        _ => String::new(),
    }
}

// The first words of a call, for the head of its block: the name of the task
// being made, or the index of the task being pointed at.
pub fn partial_words(name: &str, raw: &str) -> String {
    let value = serde_json::from_str::<Value>(raw).unwrap_or_else(|_| json!({}));
    let part = if name == CREATE_NAME { "name" } else { "index" };
    word_of(&value, part)
}

// The six definitions of the suite, in the order the model is offered them in.
pub fn definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": CREATE_NAME,
                "description": "Make a task in the list of tasks of this work directory, and remember its index. Use it when the work has parts that cannot be done at once, or that have to be gone back to later. All three parts are needed: the name is the short words the list shows, the summary is one sentence about what the task is for, and the content is the whole of what is to be done, in enough detail that you could do it from the content alone and from nothing else you have in mind. The answer carries the index of the task: it is a UUID, and every other tool of this suite is pointed at the task by it.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "The name of the task, in a few words." },
                        "summary": { "type": "string", "description": "One sentence: what the task is for." },
                        "content": { "type": "string", "description": "The whole of what is to be done, in detail." }
                    },
                    "required": ["name", "summary", "content"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": VIEW_ALL_NAME,
                "description": "List every task of this work directory, with the index, the state, the name and the summary of each. Nothing is asked for. The contents of the tasks are not returned: call view_task with the index of a task for its contents. Call this before you write your final message, to check that every task that stands open is dealt with.",
                "parameters": { "type": "object", "properties": {} }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": VIEW_ONE_NAME,
                "description": "Look at one task: its name, its summary and its content. The whole index, or the start of it while it names one task and no more.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "index": { "type": "string", "description": "The index of the task, as create_task gave it out." }
                    },
                    "required": ["index"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": COMPLETE_NAME,
                "description": "Set the state of one task to completed. Call it when the work of the task is really done, and not because the answer is coming to an end.",
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
                "description": "Set the state of one task to cancelled, leaving it in the list. Only ever use this when the user has asked for the task to be cancelled. Never cancel a task of your own accord, and never to clear a list that has grown long.",
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
                "description": "Take one task out of the list for good. Only ever use this when the user has asked for the task to be deleted. A task that is no longer wanted is cancelled and stays in the list to be read; deleting is the user's call and not yours.",
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

// The words added to the system prompt while the suite is on. The rule the suite
// exists to keep is the last one: a model that ends its turn with tasks still
// open leaves the work in the middle, and the user has to ask what became of it.
pub fn system_prompt() -> String {
    String::from(
        "TASKS SUITE\n\n\
You have a list of tasks for the work directory of this chat, and six tools that work on it: create_task, view_tasks, view_task, complete_task, cancel_task and delete_task. The tasks belong to the work directory, not to this chat, and they stand there after this chat ends. Every task has an index - a UUID, which create_task gives out - and the other five tools are pointed at a task by that index.\n\n\
Make a task when the work has parts that cannot be done at once, or that have to be gone back to later. Write the content of a task so that it can be done from the content alone. Set a task to completed with complete_task when its work is really done.\n\n\
cancel_task and delete_task are for the user alone. Use either of them only when the user has asked for that task to be cancelled or deleted. A task you no longer want is left in the list, or cancelled only if the user asked.\n\n\
Before you write your final message to the user, and so before you end this turn, you must call view_tasks and read the list. Every task that stands open has to be dealt with: finish it, or say in your message why it stands open and what you need from the user to finish it. Never end a turn with a task left open without a word about it.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // A directory of the machine for one check.
    fn place(what: &str) -> String {
        let dir = std::env::temp_dir()
            .join("an-tasks")
            .join(what);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the place of the check is made");
        dir.display().to_string()
    }

    // An index is a UUID: thirty-six marks, in five groups, with the version of
    // it stood, and no two of them the same.
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

    // A task is made with all three of its parts, and refused without them. The
    // rows of one work directory are kept apart from the rows of another, for a
    // task belongs to the directory it was made in.
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
            "It stands elsewhere.",
            "Nothing to do with the first.",
        )
        .expect("the second task is made");
        let held = rows(&first);
        assert_eq!(held.len(), 1, "only the task of this directory stands");
        assert_eq!(held[0].name, "Lay out the parser");
        assert_eq!(rows(&other).len(), 1);
    }

    // The six calls of the suite, read the same list over: the list without the
    // contents of the tasks, one task with them, the three ways a task is ended,
    // and a task that is not there.
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
            "an index that is not there is said"
        );
        // Completed, and the list says so.
        let done = run(
            COMPLETE_NAME,
            &work,
            &format!("{{\"index\":\"{}\"}}", index),
        );
        assert!(done.ok, "{}", done.output);
        assert!(done.output.contains("is completed"));
        let list = run(VIEW_ALL_NAME, &work, "{}");
        assert!(list.output.contains("[completed] Write the reader"), "{}", list.output);
        // Cancelled, then taken out of the list: after it, the list is empty.
        let off = run(
            CANCEL_NAME,
            &work,
            &format!("{{\"index\":\"{}\"}}", index),
        );
        assert!(off.ok, "{}", off.output);
        assert!(off.output.contains("is cancelled"));
        let gone = run(
            DELETE_NAME,
            &work,
            &format!("{{\"index\":\"{}\"}}", index),
        );
        assert!(gone.ok, "{}", gone.output);
        assert!(gone.output.contains("was taken out of the list"));
        let empty = run(VIEW_ALL_NAME, &work, "{}");
        assert!(empty.output.contains("No task stands"), "{}", empty.output);
        let _ = std::fs::remove_dir_all(&work);
    }

    // The definitions of the suite: six of them, all parts wanted of a task, and
    // the two tools of the user's own saying so in their words.
    #[test]
    fn the_six_definitions_are_offered() {
        let list = definitions();
        assert_eq!(list.len(), 6);
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
        let cancel = list[4]["function"]["description"]
            .as_str()
            .expect("words")
            .to_lowercase();
        assert!(cancel.contains("only ever use this when the user has asked"), "{cancel}");
        let remove = list[5]["function"]["description"]
            .as_str()
            .expect("words")
            .to_lowercase();
        assert!(remove.contains("only ever use this when the user has asked"), "{remove}");
    }

    // The prompt of the suite names all six tools and the rule the suite exists
    // for: the list is read before an answer ends.
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
            assert!(said.contains(name), "the prompt says nothing of {}", name);
        }
        assert!(said.contains("you must call view_tasks"), "{said}");
        assert!(said.contains("Before you write your final message"), "{said}");
        assert!(said.contains("belong to the work directory"), "{said}");
        assert!(said.contains("user alone"), "{said}");
    }

    // The checks work in a folder of their own, so a machine of the older shape
    // is never read into them. The list is emptied first: a check reads only the
    // tasks it made itself, and not the rows an earlier run of the checks left
    // standing under the same work directory.
    fn setup() {
        config::set_data_dir(std::path::PathBuf::from("uidata/tasks-tests"));
        let _ = config::ensure_dirs();
        crate::migrate::mark_ran(&config::paths().data);
        let kept = database().expect("the database of the checks stands open");
        kept.write(|conn| {
            conn.execute("DELETE FROM tasks", params![])
                .map_err(db::fault)?;
            Ok(())
        })
        .expect("the list is emptied for the checks");
    }
}
