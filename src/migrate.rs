// The move from the plain files of the program directory into the databases.
//
// Before the databases the text of the program was in a directory of plain files:
// `chats/<id>.json`, `repo_chats/<repository id>/<id>.json`, `repo_records/<repository
// id>.json`, `planning/<chat id>.md`, `mcp/<server>/<file>`, and the conversations of the model
// with `.packed` at the end of their names. A user who starts this build on a computer that
// already holds those files must find their chats, their repositories, their plans and their
// conversations where they left them: nothing of the old folders is deleted, and nothing of a
// chat is written twice.
//
// This module is that scan. It runs once, at the start of the program, and again whenever the
// program directory is pointed at another place. The marker of it is `databases.txt` inside the
// program directory, which shows the form of what the program reads now; a folder whose marker
// remains is not walked again.
//
// Every file that is read into a database is then moved into `pre-database/` inside the program
// directory, under the same name. The program never reads that folder: it is what the text
// looked like before, kept for the day a person needs to look. A file that could not be read is
// moved there too, and the log of the program records it, so an error is a line to read and not
// a chat that went missing.
//
// The work directories of the chats - a repository, and `work/<chat id>/` for a loose chat -
// are not touched. What a model wrote there is there, and no database holds it.

use std::fs;
use std::path::Path;

use rusqlite::params;

use crate::config::{self, Config};
use crate::contexts;
use crate::db;
use crate::proglog;
use crate::planning;
use crate::store::{self, Brought};

// The marker that shows the text of this folder has come into the databases. It is written
// last, so a run that finished in the middle of the scan starts the scan again rather than say
// it was done.
pub const MARKER: &str = "databases.txt";

// The form the files of the program are in now. A later change of the format raises this word,
// and the scan runs again over whatever the older format left. `databases-v2` brought the
// planning text of the chats into `chats.db`, so a computer whose scan ran under `databases-v1`
// walks the planning folder once more and nothing else, because every other step finds its
// folder removed.
const VERSION: &str = "databases-v2\n";

// The folder the old files are written as when they have been read. The program never reads it.
pub const BEFORE: &str = "pre-database";

// What the scan brought across. It is reported in the log of the program, so the first start
// after the change has an account of itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    // The conversations, of the loose chats and of the repositories together.
    pub chats: usize,
    // The records of the repositories.
    pub repos: usize,
    // The planning text of the chats.
    pub plans: usize,
    // The files of the servers of the program itself.
    pub mcp_files: usize,
    // The contexts the model has processed.
    pub contexts: usize,
    // The files that did not read, and were written as the directory of the old files without
    // coming into a database.
    pub unreadable: usize,
}

impl Report {
    // True when nothing at all came across: the computer was fresh, or the scan has been made
    // before.
    pub fn is_empty(&self) -> bool {
        self.chats == 0
            && self.repos == 0
            && self.plans == 0
            && self.mcp_files == 0
            && self.contexts == 0
            && self.unreadable == 0
    }
}

// True when the search marker is in this folder of the program.
pub fn already_ran(data: &Path) -> bool {
    fs::read_to_string(data.join(MARKER))
        .map(|said| said.trim() == VERSION.trim())
        .unwrap_or(false)
}

// Remain the search marker. A run that means to leave a directory of the program alone from
// here remains this marker itself: the tests of the engine do it, so no check reads the
// directories of the older format of the computer it works on.
pub fn mark_ran(data: &Path) {
    let _ = fs::create_dir_all(data);
    let _ = fs::write(&data.join(MARKER), VERSION);
}

// The scan. Nothing is done when the marker remains, so a start of the program after the first
// costs one read of one small file.
pub fn run(cfg: &Config, store: &store::Store) -> Report {
    let places = config::paths();
    if already_ran(&places.data) {
        return Report::default();
    }
    let told = hunt(cfg, store);
    // The marker remains last. A run that finished before this line starts the scan again, and
    // every step of the scan leaves what it read in the directory of the old files, so nothing
    // is brought twice.
    mark_ran(&places.data);
    if !told.is_empty() {
        proglog::note(
            proglog::STORE,
            &format!(
                "the text of the older format came into the databases: {} chats, {} repositories, {} plans, {} files of the servers, {} conversations of the model, {} files that did not read",
                told.chats,
                told.repos,
                told.plans,
                told.mcp_files,
                told.contexts,
                told.unreadable
            ),
        );
    }
    told
}

// The scan, whether the marker remains or no. The engine calls this when the user points the
// program at another folder of repositories: that folder may hold conversations and records of
// the older format inside the directories of its repositories, and the program icon directory
// reports nothing about it. The steps of the scan each do nothing when the folder they read is
// not there.
pub fn hunt(cfg: &Config, store: &store::Store) -> Report {
    let places = config::paths();
    let mut told = Report::default();
    let before = places.data.join(BEFORE);
    // The conversations of the repositories and the records of them must first come out of the
    // directories of the repositories, where a command of the terminal tool could reach them.
    // The engine makes this itself; it is created here as well, so the scan never reads a
    // directory of the old form that still is inside a repository.
    store.migrate_chats_out();
    store.migrate_records_out();
    let chats = match store.database() {
        Ok(kept) => kept,
        Err(problem) => {
            proglog::note(
                proglog::STORE,
                &format!("the database of the chats did not open: {}", problem),
            );
            return told;
        }
    };
    // The conversations, the conversations of the repositories and the records of them are the
    // business of the store, which owns the database they come into.
    let came: Brought = store.bring_older_shape(&before);
    told.chats = came.chats;
    told.repos = came.repos;
    told.unreadable = came.unreadable;
    bring_plans(&mut told, &chats, &planning::folder(), &before);
    bring_mcp(&mut told, &places.data, &before);
    bring_contexts(&mut told, &config::contexts_dir(cfg), &before);
    told
}

// The planning text of the chats. They were one file to a chat, in the directory of the plans,
// under the name `<chat id>.md`; they now are in the database of the chats, beside the
// conversation they were written for. The name kept with a chat is still the name of the older
// keeping, because it is the key the row is read by, so a chat that planned something before
// the scan reads its plan as it did.
fn bring_plans(told: &mut Report, database: &db::Db, dir: &Path, before: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let root = Path::new("planning");
    let now = store::now_secs();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().map(|one| one.to_string_lossy().to_string()) else {
            continue;
        };
        // The name of the file of a plan is the id of the chat, with the ending of a plan file
        // on it. A name of another form is not one of ours and is left where it is.
        let Some(chat) = planning::chat_of(&name) else {
            continue;
        };
        let Ok(words) = fs::read_to_string(&path) else {
            told.unreadable += 1;
            continue;
        };
        // A plan that is in the table is the newer of the two, and is kept.
        let made = database
            .write(|conn| {
                let changed = conn
                    .execute(
                        "INSERT INTO plans (chat_id, text, updated) VALUES (?1, ?2, ?3)
                         ON CONFLICT(chat_id) DO UPDATE SET text = excluded.text, updated = excluded.updated
                         WHERE excluded.updated >= plans.updated",
                        params![chat, words, now],
                    )
                    .map_err(db::fault)?;
                Ok(changed > 0)
            })
            .unwrap_or(false);
        if retire(&path, before, &root.join(&name)).is_ok() && made {
            told.plans += 1;
        }
    }
    let _ = fs::remove_dir(dir);
}

// The files of the servers of the program itself. Two files are in the directory of one server:
// the text of the user, and the runner of this program. They are decompressed on every start of
// the server, so the database holds what the settings panel holds and the folder is a place of
// passage.
fn bring_mcp(told: &mut Report, data: &Path, before: &Path) {
    let dir = data.join("mcp");
    let database = match db::open(&data.join(db::MCP_DB), db::Kind::Mcp) {
        Ok(kept) => kept,
        Err(problem) => {
            proglog::note(
                proglog::STORE,
                &format!("the database of the servers did not open: {}", problem),
            );
            return;
        }
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let now = store::now_secs();
    for entry in entries.flatten() {
        let server = entry.path();
        if !server.is_dir() {
            continue;
        }
        let Some(id) = entry
            .file_name()
            .to_str()
            .map(|one| one.to_string())
        else {
            continue;
        };
        let Ok(files) = fs::read_dir(&server) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().map(|one| one.to_string_lossy().to_string()) else {
                continue;
            };
            let Ok(words) = fs::read(&path) else {
                told.unreadable += 1;
                continue;
            };
            let placed = database
                .write(|conn| {
                    conn.execute(
                        "INSERT INTO mcp_files (server_id, name, updated, data) VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(server_id, name) DO UPDATE SET updated = excluded.updated, data = excluded.data",
                        params![id, name, now, words],
                    )
                    .map_err(db::fault)?;
                    Ok(())
                })
                .is_ok();
            let here = Path::new("mcp").join(&id).join(&name);
            if retire(&path, before, &here).is_ok() && placed {
                told.mcp_files += 1;
            }
        }
        let _ = fs::remove_dir(&server);
    }
    let _ = fs::remove_dir(&dir);
}

// The contexts the model has processed. Only the compressed form is brought across: a raw
// conversation is where the server left it, and is compressed into the database the first time
// it is read again, as it always was.
fn bring_contexts(told: &mut Report, dir: &Path, before: &Path) {
    let database = match contexts::database(dir) {
        Ok(kept) => kept,
        Err(problem) => {
            proglog::note(
                proglog::STORE,
                &format!("the database of the conversations did not open: {}", problem),
            );
            return;
        }
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let ending = contexts::SHORT;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().map(|one| one.to_string_lossy().to_string()) else {
            continue;
        };
        let Some(talk) = name.strip_suffix(ending) else {
            continue;
        };
        if !path.is_file() {
            continue;
        }
        let stood = fs::metadata(&path).map(|one| one.len()).unwrap_or(0);
        if stood == 0 {
            let _ = fs::remove_file(&path);
            continue;
        }
        if contexts::keep(&database, talk, &path, stood).is_err() {
            told.unreadable += 1;
            continue;
        }
        let here = Path::new("contexts").join(&name);
        if retire(&path, before, &here).is_ok() {
            told.contexts += 1;
        }
    }
}

// Put one file out of the format of the program and into the directory of the files that were.
// The path under that folder is the path the file was in, so a person who looks finds the form
// of the program directory as it was. The store retires the text of the chats with it, and this
// module retires the text of the servers and of the conversations.
pub(crate) fn retire(from: &Path, before: &Path, rel: &Path) -> std::io::Result<()> {
    let into = before.join(rel);
    if let Some(parent) = into.parent() {
        fs::create_dir_all(parent)?;
    }
    // A second file of the same name can remain when the scan is created twice, once for a
    // place and once for another. The newer keeps the name and the older takes a marker of the
    // number of this process, so nothing is deleted.
    if into.exists() {
        let said = format!(
            "{}.{}.kept",
            rel.file_name()
                .map(|one| one.to_string_lossy().to_string())
                .unwrap_or_else(|| String::from("file")),
            std::process::id()
        );
        let into = before.join(rel).with_file_name(said);
        return carry(from, &into);
    }
    carry(from, &into)
}

// Move a file, or copy it and take the first away when the two places are not on one disk.
fn carry(from: &Path, into: &Path) -> std::io::Result<()> {
    if fs::rename(from, into).is_ok() {
        return Ok(());
    }
    fs::copy(from, into)?;
    fs::remove_file(from)
}

#[cfg(test)]
mod checks {
    use super::*;
    use std::path::PathBuf;

    fn room(what: &str) -> PathBuf {
        static TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let place = std::env::temp_dir().join(format!(
            "an-migrate-{}-{}-{}",
            what,
            std::process::id(),
            TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        fs::create_dir_all(&place).expect("the directory of the test is created");
        place
    }

    // The search marker is what reports the program directory has been scanned already. A
    // directory of the program with no marker is scanned, and the marker of it remains only
    // when the walk finished.
    #[test]
    fn the_mark_of_the_hunt_stands_when_the_hunt_came_to_its_end() {
        let data = room("mark");
        assert!(!already_ran(&data), "a fresh place has no marker");
        mark_ran(&data);
        assert!(already_ran(&data), "the marker is read back");
        // Text in another form is not the marker: the file is read again.
        fs::write(data.join(MARKER), "databases-v0\n").expect("other text is written");
        assert!(!already_ran(&data), "the text of the marker is read as well");
        let _ = fs::remove_dir_all(&data);
    }

    // The files of the servers of the program itself came out of a directory of the program and
    // into `mcp.db`, and the folder they were in is removed.
    #[test]
    fn the_files_of_the_servers_come_into_their_database() {
        let data = room("mcp");
        let server = data.join("mcp").join("sv01");
        fs::create_dir_all(&server).expect("the directory of the server is created");
        fs::write(server.join("natter_code.py"), "print(1)").expect("the text of the user remains");
        fs::write(server.join("natter_runner.py"), "# runner").expect("the runner stands");
        let before = data.join(BEFORE);
        let mut told = Report::default();
        bring_mcp(&mut told, &data, &before);
        assert_eq!(told.mcp_files, 2, "both files came across");
        assert!(!server.exists(), "the directory of the server is removed");
        let database = db::open(&data.join(db::MCP_DB), db::Kind::Mcp).expect("the database opened");
        let names: Vec<String> = database
            .read(|conn| {
                let mut said = conn
                    .prepare("SELECT name FROM mcp_files WHERE server_id = ?1 ORDER BY name")
                    .map_err(db::fault)?;
                let rows = said
                    .query_map(rusqlite::params!["sv01"], |row| row.get::<_, String>(0))
                    .map_err(db::fault)?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(db::fault)?);
                }
                Ok(out)
            })
            .expect("the files are read back");
        assert_eq!(
            names,
            vec![String::from("natter_code.py"), String::from("natter_runner.py")]
        );
        assert!(before.join("mcp").join("sv01").join("natter_code.py").is_file());
        let _ = fs::remove_dir_all(&data);
    }

    // The contexts the model has processed came into `contexts.db` out of the files with
    // `.packed` at the end of their names. The raw conversation is left to the server, which
    // reads and writes it by name, and is compressed into the database the first time it is
    // read again.
    #[test]
    fn the_talks_come_into_their_database() {
        let dir = room("contexts");
        let talk = "ab12cd34ef-0123456789abcdef.ctx";
        // The compressed form of a conversation, of the form an older run left: the program
        // puts the bytes of it into the database without reading them.
        fs::write(dir.join(contexts::short_of(talk)), vec![7u8; 4096])
            .expect("the compressed form of the older keeping remains");
        // A raw conversation of an interrupted run is left where the server left it.
        fs::write(dir.join(talk), vec![9u8; 2048]).expect("a raw conversation remains");
        let before = dir.join(BEFORE);
        let mut told = Report::default();
        bring_contexts(&mut told, &dir, &before);
        assert_eq!(told.contexts, 1, "the compressed context was migrated");
        let kept = contexts::held_list(&dir);
        assert_eq!(kept.len(), 1, "the conversation is in the database");
        assert_eq!(kept[0].0, talk, "the conversation is under the name the server reads");
        assert!(
            !dir.join(contexts::short_of(talk)).exists(),
            "the file of the older format is removed"
        );
        assert!(dir.join(talk).is_file(), "the raw conversation is left to the server");
        let _ = fs::remove_dir_all(&dir);
    }
}
