// The database files the words of the program stand in.
//
// The words of the chats, the records of the repositories, the talks the model
// worked through, and the files of the servers of the program itself used to
// stand in a folder of plain files, one file for one thing. That shape cost
// three things. A list of the chats was a walk over a folder and a read of
// every file in it. A write was a rename that could not be made to cover two
// things at once, so a chat and the mark of when a repository was used could
// part company. And a user who copied the folder of the program took a folder
// of two thousand little files, of which one cut short was one chat lost.
//
// The words now stand in three SQLite files inside the folder of the program:
//
//   * `chats.db` - the chats that belong to no repository, the chats of the
//     repositories, and the record of every repository. These three are read
//     together on every change of the side bar, so they are kept in one file
//     and read with one query. The planning words of a chat stand here too.
//   * `contexts.db` - the talks the model worked through, which the program
//     shortens before it keeps them. This stands in the folder of the talks,
//     which the user may point somewhere else, because a talk of one chat can
//     cost several gigabytes and the whole point of that folder is that it may
//     live on a bigger disk.
//   * `mcp.db` - the files of the servers of the program itself.
//
// Three things stay as they were, on purpose:
//
//   * `settings.json` stays a plain file, so a user may look at it and mend it
//     while the program is shut.
//   * The logs stay plain files, because they are written while the program
//     dies and are read by other programs.
//   * The folders a model works in - a repository, and `work/<chat id>/` for a
//     loose chat - hold nothing of the database. The work of a model is the
//     work of a model, and it stands where the model put it.
//
// A SQLite file is opened once for a run and kept open, because opening is the
// dear part and the program is a single process with threads. One connection
// stands behind a lock: every write of this program is small and quick, and a
// lock is cheaper than a pool. A second process that comes at the same file - a
// second window, or a run that never came to an end - is answered by the busy
// timeout, and by the write transaction of SQLite, which is what a rename was
// meant to do and does better.
//
// `migrate` in `src/migrate.rs` moves the words of the old plain files into
// these databases the first time the program starts after this change.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;

use crate::proglog;

// The name of the database of the talks, of the chats, and of the servers. The
// three are told apart by these names, and by nothing else.
pub const CHATS_DB: &str = "chats.db";
pub const CONTEXTS_DB: &str = "contexts.db";
pub const MCP_DB: &str = "mcp.db";

// How long one wait for another writer of the same file lasts, in
// milliseconds. A write of this program takes a few milliseconds, so a wait of
// two seconds is a generous ceiling and still short enough that the user does
// not sit on it.
const BUSY_MS: i32 = 2000;

// What a database file holds. The schema that goes with it is stood when the
// file is opened, and the same file asked for another kind is a fault of the
// caller, not of the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    // The chats, the chats of the repositories, the records of the
    // repositories, and the planning words of a chat.
    Chats,
    // The talks the model worked through, kept in pieces.
    Contexts,
    // The files of the servers of the program itself.
    Mcp,
}

// The tables of one kind. `IF NOT EXISTS` throughout, so opening a file of the
// kind again costs nothing and an older file of the kind gains what it lacks.
fn schema(kind: Kind) -> &'static str {
    match kind {
        Kind::Chats => {
            "
CREATE TABLE IF NOT EXISTS repos (
    id        TEXT PRIMARY KEY,
    name      TEXT NOT NULL DEFAULT '',
    created   INTEGER NOT NULL DEFAULT 0,
    last_used INTEGER NOT NULL DEFAULT 0,
    pinned    INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS chats (
    id        TEXT PRIMARY KEY,
    repo_id   TEXT NOT NULL DEFAULT '',
    title     TEXT NOT NULL DEFAULT '',
    created   INTEGER NOT NULL DEFAULT 0,
    updated   INTEGER NOT NULL DEFAULT 0,
    messages  INTEGER NOT NULL DEFAULT 0,
    data      TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS chats_of_place ON chats (repo_id, updated DESC);
CREATE INDEX IF NOT EXISTS repos_of_use ON repos (last_used DESC);
CREATE TABLE IF NOT EXISTS plans (
    chat_id  TEXT PRIMARY KEY,
    text     TEXT NOT NULL DEFAULT '',
    updated  INTEGER NOT NULL DEFAULT 0
);
"
        }
        Kind::Contexts => {
            "
CREATE TABLE IF NOT EXISTS contexts (
    name      TEXT PRIMARY KEY,
    bytes     INTEGER NOT NULL DEFAULT 0,
    held      INTEGER NOT NULL DEFAULT 0,
    packed_at INTEGER NOT NULL DEFAULT 0,
    parts     INTEGER NOT NULL DEFAULT 0,
    done      INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS context_parts (
    name TEXT NOT NULL,
    idx  INTEGER NOT NULL,
    part BLOB NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS context_part_place ON context_parts (name, idx);
"
        }
        Kind::Mcp => {
            "
CREATE TABLE IF NOT EXISTS mcp_files (
    server_id TEXT NOT NULL,
    name      TEXT NOT NULL,
    updated   INTEGER NOT NULL DEFAULT 0,
    data      BLOB NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS mcp_file_place ON mcp_files (server_id, name);
"
        }
    }
}

// One database file, held open for the run.
pub struct Db {
    path: PathBuf,
    kind: Kind,
    conn: Mutex<Connection>,
}

impl Db {
    // Where the file stands.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    // Read from the file. The step is given a connection to read with.
    pub fn read<T, F: FnOnce(&Connection) -> Result<T, String>>(&self, f: F) -> Result<T, String> {
        let guard = self.lock();
        f(&guard)
    }

    // Write to the file inside one transaction, so a step either stands whole
    // or does not stand at all. The step is given a connection to write with;
    // it returns the value the caller wants, or the reason it could not be had.
    pub fn write<T, F: FnOnce(&Connection) -> Result<T, String>>(
        &self,
        f: F,
    ) -> Result<T, String> {
        let mut guard = self.lock();
        // SQLite takes the outermost `BEGIN` and no other, so a step that runs
        // inside another step is not asked to stand twice.
        let held = guard
            .transaction()
            .map_err(|e| format!("the database could not be written: {}", e))?;
        let made = f(&held)?;
        held.commit()
            .map_err(|e| format!("the database could not be written: {}", e))?;
        Ok(made)
    }

    // Write without a transaction of its own. A step that lays a big thing down
    // piece by piece uses this, so one piece stands at a time and a stop in the
    // middle costs the pieces that came after it and nothing before them.
    pub fn write_each<T, F: FnOnce(&Connection) -> Result<T, String>>(
        &self,
        f: F,
    ) -> Result<T, String> {
        let guard = self.lock();
        f(&guard)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|gone| gone.into_inner())
    }
}

// The files this run has open, by the place they stand in. Two calls that ask
// for the same file get the same handle, so one file never stands open twice
// in one process.
fn pool() -> &'static Mutex<HashMap<PathBuf, Arc<Db>>> {
    static POOL: OnceLock<Mutex<HashMap<PathBuf, Arc<Db>>>> = OnceLock::new();
    POOL.get_or_init(Default::default)
}

// Open a database file, and make it when it is not there. The parent folder is
// made when needed, because a first run has no folders at all.
pub fn open(path: &Path, kind: Kind) -> Result<Arc<Db>, String> {
    let key = key_of(path);
    let held = pool().lock().unwrap_or_else(|gone| gone.into_inner());
    if let Some(db) = held.get(&key) {
        if db.kind == kind {
            return Ok(db.clone());
        }
        return Err(format!(
            "{} is a database of another kind",
            path.display()
        ));
    }
    drop(held);

    let made = open_and_prepare(path, kind)?;
    let mut held = pool().lock().unwrap_or_else(|gone| gone.into_inner());
    // Another thread can get here between the read of the pool and the open of
    // the file. Whichever handle is in the pool when the lock is taken again is
    // the handle both callers get, and the file is opened once.
    if let Some(db) = held.get(&key) {
        return Ok(db.clone());
    }
    held.insert(key, made.clone());
    Ok(made)
}

// The key a handle is kept under: the place the file really stands, when it
// stands, so the same file named two ways is one handle.
fn key_of(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn open_and_prepare(path: &Path, kind: Kind) -> Result<Arc<Db>, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|why| {
                format!("cannot make {}: {}", parent.display(), why)
            })?;
        }
    }
    let fresh = !path.is_file();
    let conn = Connection::open(path)
        .map_err(|why| format!("cannot open {}: {}", path.display(), why))?;
    prepare(&conn, kind, path)?;
    if fresh {
        proglog::note(
            proglog::STORE,
            &format!("the database of the program is made: {}", path.display()),
        );
    }
    Ok(Arc::new(Db {
        path: path.to_path_buf(),
        kind,
        conn: Mutex::new(conn),
    }))
}

// The pragmas and the tables of one kind. Every connection gets them: the
// journal mode and the busy timeout belong to a connection and not to a file.
fn prepare(conn: &Connection, kind: Kind, path: &Path) -> Result<(), String> {
    for pragma in [
        // A second process that comes at the file waits rather than fails, and
        // the writes of two processes are laid after one another.
        &format!("PRAGMA busy_timeout = {}", BUSY_MS),
        // The journal stands beside the file and the readers are never held up
        // by a writer. This is what lets a window and the page of the remote
        // mode read one file while the engine writes it.
        "PRAGMA journal_mode = WAL",
        // Enough that a write that came to the machine stands, and short
        // enough that a chat is not a wait.
        "PRAGMA synchronous = NORMAL",
    ] {
        conn.execute_batch(pragma)
            .map_err(|why| format!("cannot set up {}: {}", path.display(), why))?;
    }
    conn.execute_batch(schema(kind))
        .map_err(|why| format!("cannot stand the tables of {}: {}", path.display(), why))?;
    Ok(())
}

// The reason a step of SQLite failed, in words. Every caller of this module
// answers a database fault with a line for the user, so a reason is a string
// here and never a type the caller has to know.
pub fn fault(problem: rusqlite::Error) -> String {
    format!("{}", problem)
}

// Run one statement with no value to bind.
pub fn exec(conn: &Connection, sql: &str) -> Result<(), String> {
    conn.execute_batch(sql).map_err(fault)
}

// The true of an integer column that holds a mark.
pub fn mark_of(value: i64) -> bool {
    value != 0
}

// The number that stands for a mark in a column.
pub fn mark_num(value: bool) -> i64 {
    i64::from(value)
}

// Take the handles of the databases that stand under one folder out of the pool
// and close them. Only the code that moves the folder of the program uses it:
// after the move the same names mean other files, and a handle kept open would
// go on writing into the folder that was left.
pub fn close_under(dir: &Path) {
    let mut held = pool().lock().unwrap_or_else(|gone| gone.into_inner());
    let gone: Vec<PathBuf> = held
        .iter()
        .filter(|(key, _)| key.starts_with(dir))
        .map(|(key, _)| key.clone())
        .collect();
    for key in gone {
        if let Some(db) = held.remove(&key) {
            let guard = db.lock();
            let _ = guard.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
        }
    }
}

// A word for the log of the program: how many rows a table holds. Nothing is
// said when the table cannot be read, because the caller has the reason already.
pub fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
}

#[cfg(test)]
mod checks {
    use super::*;

    fn room(name: &str) -> PathBuf {
        let place = std::env::temp_dir().join(format!("an-db-{}", name));
        let _ = fs::remove_dir_all(&place);
        fs::create_dir_all(&place).expect("the room of the check is made");
        place
    }

    // A file that is not there is made, and the tables of its kind stand in it.
    #[test]
    fn a_database_is_made_with_the_tables_of_its_kind() {
        let place = room("made");
        let file = place.join(CHATS_DB);
        assert!(!file.is_file());
        let db = open(&file, Kind::Chats).expect("the database is opened");
        assert!(file.is_file());
        assert_eq!(db.kind(), Kind::Chats);
        let held = db
            .read(|conn| Ok(count(conn, "chats")))
            .expect("the table is read");
        assert_eq!(held, 0);
        close_under(&place);
        let _ = fs::remove_dir_all(&place);
    }

    // The same file asked for as another kind is refused: the schema of one
    // kind is never laid over the words of another.
    #[test]
    fn a_file_of_another_kind_is_refused() {
        let place = room("kind");
        let file = place.join(MCP_DB);
        let db = open(&file, Kind::Mcp).expect("the database is opened");
        let again = open(&file, Kind::Mcp).expect("the same handle comes back");
        assert!(Arc::ptr_eq(&db, &again));
        let other = open(&file, Kind::Chats);
        assert!(other.is_err(), "one file is one kind");
        close_under(&place);
        let _ = fs::remove_dir_all(&place);
    }

    // A write either stands whole or does not stand at all.
    #[test]
    fn a_write_that_fails_leaves_nothing_behind() {
        let place = room("transaction");
        let file = place.join(CHATS_DB);
        let db = open(&file, Kind::Chats).expect("the database is opened");
        let first = db.write(|conn| {
            conn.execute(
                "INSERT INTO chats (id, repo_id, title, updated, data) VALUES ('aaaaaaaa01','','First',1,'{}')",
                [],
            )
            .map_err(fault)?;
            Ok(())
        });
        assert!(first.is_ok());
        let second = db.write(|conn| -> Result<(), String> {
            conn.execute(
                "INSERT INTO chats (id, repo_id, title, updated, data) VALUES ('aaaaaaaa02','','Second',2,'{}')",
                [],
            )
            .map_err(fault)?;
            Err(String::from("the step gave up"))
        });
        assert!(second.is_err(), "the step says so");
        let held = db
            .read(|conn| Ok(count(conn, "chats")))
            .expect("the table is read");
        assert_eq!(held, 1, "only the write that came to the end stands");
        close_under(&place);
        let _ = fs::remove_dir_all(&place);
    }
}
