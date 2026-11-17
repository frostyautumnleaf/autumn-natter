// The move from the plain files of the folder of the program into the databases.
//
// Before the databases the words of the program stood in a folder of plain
// files: `chats/<id>.json`, `repo_chats/<repository id>/<id>.json`,
// `repo_records/<repository id>.json`, `planning/<chat id>.md`,
// `mcp/<server>/<file>`, and the talks of the model with `.packed` at the end of
// their names. A user who starts this build on a machine that already holds
// those files must find their chats, their repositories, their plans and their
// talks where they left them: nothing of the old folders is thrown away, and
// nothing of a chat is written twice.
//
// This module is that hunt. It runs once, at the start of the program, and again
// whenever the folder of the program is pointed at another place. The mark of it
// is `databases.txt` inside the folder of the program, which says the shape of
// what the program reads now; a folder whose mark stands is not walked again.
//
// Every file that is read into a database is then moved into
// `pre-database/` inside the folder of the program, under the same name. The
// program never reads that folder: it is what the words looked like before, kept
// for the day a person needs to look. A file that could not be read is moved
// there too, and the log of the program says so, so a fault is a line to read
// and not a chat that went missing.
//
// The work directories of the chats - a repository, and `work/<chat id>/` for a
// loose chat - are not touched. What a model wrote there stands there, and no
// database holds it.

use std::fs;
use std::path::Path;

use rusqlite::params;

use crate::config::{self, Config};
use crate::contexts;
use crate::db;
use crate::proglog;
use crate::planning;
use crate::store::{self, Brought};

// The mark that says the words of this folder have come into the databases. It
// is written last, so a run that came to an end in the middle of the hunt starts
// the hunt again rather than say it was done.
pub const MARKER: &str = "databases.txt";

// The shape the files of the program are in now. A later change of the shape
// raises this word, and the hunt runs again over whatever the older shape left.
// The shape the files of the program are in now. A later change of the shape
// raises this word, and the hunt runs again over whatever the older shape left.
// `databases-v2` brought the planning words of the chats into `chats.db`, so a
// machine whose hunt was made under `databases-v1` walks the planning folder once
// more and nothing else, because every other step finds its folder gone.
const VERSION: &str = "databases-v2\n";

// The folder the old files are laid in when they have been read. The program
// never reads it.
pub const BEFORE: &str = "pre-database";

// What the hunt brought across. It is said in the log of the program, so the
// first start after the change has an account of itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    // The talks, of the loose chats and of the repositories together.
    pub chats: usize,
    // The records of the repositories.
    pub repos: usize,
    // The planning words of the chats.
    pub plans: usize,
    // The files of the servers of the program itself.
    pub mcp_files: usize,
    // The talks the model worked its way through.
    pub contexts: usize,
    // The files that did not read, and were laid in the folder of the old files
    // without coming into a database.
    pub unreadable: usize,
}

impl Report {
    // True when nothing at all came across: the machine was fresh, or the hunt
    // has been made before.
    pub fn is_empty(&self) -> bool {
        self.chats == 0
            && self.repos == 0
            && self.plans == 0
            && self.mcp_files == 0
            && self.contexts == 0
            && self.unreadable == 0
    }
}

// True when the mark of the hunt stands in this folder of the program.
pub fn already_ran(data: &Path) -> bool {
    fs::read_to_string(data.join(MARKER))
        .map(|said| said.trim() == VERSION.trim())
        .unwrap_or(false)
}

// Stand the mark of the hunt. A run that means to leave a folder of the program
// alone from here stands this mark itself: the tests of the engine do it, so no
// check reads the folders of the older shape of the machine it works on.
pub fn mark_ran(data: &Path) {
    let _ = fs::create_dir_all(data);
    let _ = fs::write(&data.join(MARKER), VERSION);
}

// The hunt. Nothing is done when the mark stands, so a start of the program
// after the first costs one read of one small file.
pub fn run(cfg: &Config, store: &store::Store) -> Report {
    let places = config::paths();
    if already_ran(&places.data) {
        return Report::default();
    }
    let told = hunt(cfg, store);
    // The mark stands last. A run that came to an end before this line starts
    // the hunt again, and every step of the hunt leaves what it read in the
    // folder of the old files, so nothing is brought twice.
    mark_ran(&places.data);
    if !told.is_empty() {
        proglog::note(
            proglog::STORE,
            &format!(
                "the words of the older shape came into the databases: {} chats, {} repositories, {} plans, {} files of the servers, {} talks of the model, {} files that did not read",
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

// The hunt, whether the mark stands or no. The engine calls this when the user
// points the program at another folder of repositories: that folder may hold
// talks and records of the older shape inside the directories of its
// repositories, and the mark of the folder of the program says nothing about it.
// The steps of the hunt each do nothing when the folder they read is not there.
pub fn hunt(cfg: &Config, store: &store::Store) -> Report {
    let places = config::paths();
    let mut told = Report::default();
    let before = places.data.join(BEFORE);
    // The talks of the repositories and the records of them must first come out
    // of the directories of the repositories, where a command of the terminal
    // tool could come at them. The engine makes this itself; it is made here as
    // well, so the hunt never reads a folder of the old shape that still stands
    // inside a repository.
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
    // The talks, the talks of the repositories and the records of them are the
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

// The planning words of the chats. They stood one file to a chat, in the folder
// of the plans, under the name `<chat id>.md`; they now stand in the database of
// the chats, beside the talk they were written for. The name kept with a chat is
// still the name of the older keeping, because it is the key the row is read by,
// so a chat that planned something before the hunt reads its plan as it did.
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
        // The name of the file of a plan is the id of the chat, with the ending
        // of a plan file on it. A name of another shape is not one of ours and is
        // left where it stands.
        let Some(chat) = planning::chat_of(&name) else {
            continue;
        };
        let Ok(words) = fs::read_to_string(&path) else {
            told.unreadable += 1;
            continue;
        };
        // A plan that stands in the table is the newer of the two, and is kept.
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

// The files of the servers of the program itself. Two files stand in the folder
// of one server: the words of the user, and the runner of this program. They are
// written out again on every start of the server, so the database holds what the
// sheet holds and the folder is a place of passage.
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

// The talks the model worked its way through. Only the short shape is brought
// across: a raw talk stands where the server left it, and is shortened into the
// database the first time it is read again, as it always was.
fn bring_contexts(told: &mut Report, dir: &Path, before: &Path) {
    let database = match contexts::database(dir) {
        Ok(kept) => kept,
        Err(problem) => {
            proglog::note(
                proglog::STORE,
                &format!("the database of the talks did not open: {}", problem),
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

// Put one file out of the shape of the program and into the folder of the files
// that were. The road under that folder is the road the file stood in, so a
// person who looks finds the shape of the folder of the program as it was. The
// store retires the words of the chats with it, and this module retires the
// words of the servers and of the talks.
pub(crate) fn retire(from: &Path, before: &Path, rel: &Path) -> std::io::Result<()> {
    let into = before.join(rel);
    if let Some(parent) = into.parent() {
        fs::create_dir_all(parent)?;
    }
    // A second file of the same name can stand when the hunt is made twice, once
    // for a place and once for another. The newer keeps the name and the older
    // takes a mark of the number of this process, so nothing is thrown away.
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

// Move a file, or copy it and take the first away when the two places are not on
// one disk.
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
        fs::create_dir_all(&place).expect("the room of the check is made");
        place
    }

    // The mark of the hunt is what says the folder of the program has been walked
    // already. A folder of the program with no mark is walked, and the mark of it
    // stands only when the walk came to an end.
    #[test]
    fn the_mark_of_the_hunt_stands_when_the_hunt_came_to_its_end() {
        let data = room("mark");
        assert!(!already_ran(&data), "a fresh place has no mark");
        mark_ran(&data);
        assert!(already_ran(&data), "the mark is read back");
        // Words of another shape are no mark: the hunt is made again over them.
        fs::write(data.join(MARKER), "databases-v0\n").expect("other words stand");
        assert!(!already_ran(&data), "the word of the shape is read as well");
        let _ = fs::remove_dir_all(&data);
    }

    // The files of the servers of the program itself came out of a folder of the
    // program and into `mcp.db`, and the folder they stood in is gone.
    #[test]
    fn the_files_of_the_servers_come_into_their_database() {
        let data = room("mcp");
        let server = data.join("mcp").join("sv01");
        fs::create_dir_all(&server).expect("the folder of the server is made");
        fs::write(server.join("natter_code.py"), "print(1)").expect("the words of the user stand");
        fs::write(server.join("natter_runner.py"), "# runner").expect("the runner stands");
        let before = data.join(BEFORE);
        let mut told = Report::default();
        bring_mcp(&mut told, &data, &before);
        assert_eq!(told.mcp_files, 2, "both files came across");
        assert!(!server.exists(), "the folder of the server is gone");
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

    // The talks the model worked its way through came into `contexts.db` out of
    // the files with `.packed` at the end of their names. The raw talk is left to
    // the server, which reads and writes it by name, and is shortened into the
    // database the first time it is read again.
    #[test]
    fn the_talks_come_into_their_database() {
        let dir = room("contexts");
        let talk = "ab12cd34ef-0123456789abcdef.ctx";
        // The short shape of a talk, of the shape an older run left: the program
        // puts the bytes of it into the database without reading them.
        fs::write(dir.join(contexts::short_of(talk)), vec![7u8; 4096])
            .expect("the short shape of the older keeping stands");
        // A raw talk of an interrupted run is left where the server left it.
        fs::write(dir.join(talk), vec![9u8; 2048]).expect("a raw talk stands");
        let before = dir.join(BEFORE);
        let mut told = Report::default();
        bring_contexts(&mut told, &dir, &before);
        assert_eq!(told.contexts, 1, "the short shape came across");
        let kept = contexts::held_list(&dir);
        assert_eq!(kept.len(), 1, "the talk stands in the database");
        assert_eq!(kept[0].0, talk, "the talk stands under the name the server reads");
        assert!(
            !dir.join(contexts::short_of(talk)).exists(),
            "the file of the older shape is gone"
        );
        assert!(dir.join(talk).is_file(), "the raw talk is left to the server");
        let _ = fs::remove_dir_all(&dir);
    }
}
