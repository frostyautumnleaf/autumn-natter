// The contexts the model has processed, kept in a database.
//
// The model only holds a conversation while it is loaded in one slot of the running server.
// llama.cpp can copy that state out into a file and lay it back into a slot again, and the
// engine requests both: when a chat gives its slot to another chat, when an answer is stopped
// by the user, and when the server is stopped.
//
// The server reads and writes a context by name in a directory of its own, so the directory of
// contexts is between this program and the server. It is a place of passage and not a place of
// keeping: a context is decompressed of the database into it just before the server reads it,
// and removed again just after. What the program keeps is in `contexts.db`, in the same folder,
// in parts.
//
// Such a context is as big as the space the model set aside for the context. A chat on a
// long context takes several gigabytes, and on one computer the directory of them had grown to
// 58 Gbyte of the disk. The bytes are the 16-bit numbers of the attention cache, which remain
// near to random, so no scheme of shortening does much with them: the most any general scheme
// reached, measured on the contexts of that computer, was a fifth. Two items therefore rule
// this module: it must not cost the user a wait, and it must not change one byte of a context.
//
// * The bytes of a block are de-interleaved into two byte planes first -- the bytes at the even
//    places, then the bytes at the odd ones. The cache is made of pairs of bytes, and the two
//    halves of a pair differ widely in how much they give away, so splitting them lets the
//    encoder work on each of them alone. On the context of a real computer this took the
//    shortening from 1.153 to 1.215.
// * Each block of four megabytes then goes through deflate at level 1: 88 Mbyte a second out,
//    174 Mbyte a second back, measured on the same computer. Level 1 is all of the speed that
//    is to be had there; level 3 costs five times the time for a shorter file nobody requested.
// * Everything is streamed, block by block, from the file to the database and back, so a
//    context of ten gigabytes never is in memory and the work costs the space of two
//    blocks. Nothing is asked of the database that a plain file does not do better: the parts
//    are put down one after another and read back the same way.
// * The row that shows a context is in the database is put down only when the last piece of it
//    remains. A shortening that cannot be finished therefore leaves the context exactly as it
//    was, and no half context is read back.
// * A context is only compressed when the compressed form is really smaller than the context.
//    Where nothing is to be had the raw file is kept, for two forms of a context nobody can
//    compress are worse than one.
//
// The name the server reads is the raw name, so the raw file is decompressed again for the
// moment of the read and removed directly after it. A directory of contexts that were never
// compressed -- one left by an older run of the program -- is therefore compressed by itself
// the first time that context is read.
//
// Before the database the compressed form was beside the raw file, under the same name with
// `.packed` at the end of it. That form is still read, so a context kept by an older run is
// laid back out and comes into the database with it.
//
// One cost follows from the server's liking for a file: while a context is being written, or
// read, both forms of it are on the disk -- the context in the database and the raw file
// decompressed from it -- so the space the folder needs at its worst is near to twice the
// size of the biggest context, though it settles back after. On a computer with the disk near
// full the shortening is then simply refused, the raw file is kept, and the log of the program
// records it.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;

use rusqlite::{params, OptionalExtension};

use crate::db;
use crate::proglog;
use crate::store;

// The marker at the end of the name of a compressed context. It is after the whole name of the
// context, so the raw name the server requests is never touched.
pub const SHORT: &str = ".packed";

// The text at the header of a compressed context. The last two of them mean the form of what
// follows, and a file that does not begin with them is not one.
const HEAD: &[u8; 8] = b"ANTXPK\x01\x00";

// The bytes of one context worked at a time: the space of the copy, and the size every
// block of the file is cut to.
const BLOCK: usize = 4 * 1024 * 1024;

// The level the contexts are compressed at. It is written down so that a change of it is a
// choice, and an accident.
const LEVEL: u8 = 1;

// The name of the summary form of one context. It is the name a context was kept under before
// the database, and a file of that form is still read.
pub fn short_of(name: &str) -> String {
    format!("{name}{SHORT}")
}

// The two places one context remains: the name the server reads, and the compressed form the
// program keeps.
fn raw_of(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}

fn short_of_place(dir: &Path, name: &str) -> PathBuf {
    dir.join(short_of(name))
}

// What `lay_out` found in the directory of contexts. It reports what to do once the server has
// read the context: a raw file this program decompressed is deleted, while a raw file that was
// there on its own is compressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Laid {
    // The compressed form was extracted into a raw file, which is the context as it is now.
    // Nothing of it is new.
    FromShort,
    // A raw file was there already: from an older run of the program, or from a shortening that
    // could not be finished. It is newer than any compressed form.
    RawStood,
    // Neither form is there, so there is no sends requests to read.
    Nothing,
}

// ------------------------------------------------------------------ database

// The database the contexts of one folder are in. It is in the contexts directory, because that
// folder may be pointed at a bigger disk than the one the program data is on, and a context is
// the bulk of what a chat costs.
pub fn database(dir: &Path) -> Result<Arc<db::Db>, String> {
    db::open(&dir.join(db::CONTEXTS_DB), db::Kind::Contexts)
}

// The size of one piece of a context in the database. A piece is small enough that a context of
// ten gigabytes is ten thousand rows rather than one row the database cannot hold, and big
// enough that the work is a run of large writes and not a run of calls.
const PIECE: usize = 1024 * 1024;

// True when one context is in the database, whole. A context whose parts finished half way
// through has no row of its own, so it is not was.
fn holds(database: &db::Db, name: &str) -> bool {
    database
        .read(|conn| {
            Ok(conn
                .prepare("SELECT 1 FROM contexts WHERE name = ?1 AND done = 1")
                .map_err(db::fault)?
                .exists(params![name])
                .map_err(db::fault)?)
        })
        .unwrap_or(false)
}

// The contexts the folder holds, with the bytes that was and the bytes they came to. The Logs
// tab has no page of its own for the contexts, so this is what a check of the folder is read
// against.
pub fn held_list(dir: &Path) -> Vec<(String, u64, u64)> {
    let database = match database(dir) {
        Ok(kept) => kept,
        Err(_) => return Vec::new(),
    };
    database
        .read(|conn| {
            let mut said = conn
                .prepare("SELECT name, bytes, held FROM contexts WHERE done = 1 ORDER BY name")
                .map_err(db::fault)?;
            let rows = said
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)? as u64,
                        row.get::<_, i64>(2)? as u64,
                    ))
                })
                .map_err(db::fault)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(db::fault)?);
            }
            Ok(out)
        })
        .unwrap_or_default()
}

// Put the compressed form that is at `from` into the database under `name`, and say how many
// parts it came to. Every piece is put down before the row that shows the context is there, so
// a write that cannot be finished leaves no half context behind: the parts of the context that
// was before is laid aside in the same turn as the new ones arrive.
pub fn keep(
    database: &db::Db,
    name: &str,
    from: &Path,
    stood: u64,
) -> Result<i64, String> {
    let now = store::now_secs();
    database.write(|conn| {
        conn.execute("DELETE FROM context_parts WHERE name = ?1", params![name])
            .map_err(db::fault)?;
        let mut from = BufReader::with_capacity(BLOCK, File::open(from).map_err(reason)?);
        let mut index: i64 = 0;
        let mut buffer = vec![0u8; PIECE];
        loop {
            let got = read_some(&mut from, &mut buffer).map_err(reason)?;
            if got == 0 {
                break;
            }
            put(conn, name, index, &buffer[..got])?;
            index += 1;
        }
        conn.execute(
            "INSERT OR REPLACE INTO contexts (name, bytes, held, packed_at, parts, done) VALUES (?1, ?2, (SELECT COALESCE(SUM(LENGTH(part)), 0) FROM context_parts WHERE name = ?1), ?3, ?4, 1)",
            params![name, stood as i64, now, index],
        )
        .map_err(db::fault)?;
        Ok(index)
    })
}

// One piece of one context.
fn put(conn: &rusqlite::Connection, name: &str, index: i64, piece: &[u8]) -> Result<(), String> {
    conn.execute(
        "INSERT INTO context_parts (name, idx, part) VALUES (?1, ?2, ?3)",
        params![name, index, piece],
    )
    .map_err(db::fault)?;
    Ok(())
}

// A context that is with parts and no row of its own came from a write that could not be
// finished. Its parts are removed, so the space they cost is not in the database for
// nothing.
fn drop_half(database: &db::Db, name: &str) {
    let gone = database.write(|conn| {
        conn.execute(
            "DELETE FROM context_parts WHERE name = ?1 AND NOT (EXISTS (SELECT 1 FROM contexts WHERE name = ?1 AND done = 1))",
            params![name],
        )
        .map_err(db::fault)?;
        Ok(())
    });
    if let Err(problem) = gone {
        proglog::note(
            proglog::STORE,
            &format!("the parts of an unfinished context are not deleted: {}", problem),
        );
    }
}

// A user over the parts of one context, in the order they remain. The compressed form is read
// back exactly as it was written, block after block, so the user of the form needs no word of
// the database under it.
struct Pieces<'a> {
    conn: &'a rusqlite::Connection,
    name: String,
    index: i64,
    stood: Vec<u8>,
    at: usize,
}

impl<'a> Pieces<'a> {
    fn new(conn: &'a rusqlite::Connection, name: &str) -> Pieces<'a> {
        Pieces {
            conn,
            name: name.to_string(),
            index: 0,
            stood: Vec::new(),
            at: 0,
        }
    }
}

impl Read for Pieces<'_> {
    fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
        if self.at >= self.stood.len() {
            let next = self
                .conn
                .query_row(
                    "SELECT part FROM context_parts WHERE name = ?1 AND idx = ?2",
                    params![self.name, self.index],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
                .map_err(carried)?;
            let Some(next) = next else {
                return Ok(0);
            };
            self.index += 1;
            self.stood = next;
            self.at = 0;
        }
        if into.is_empty() {
            return Ok(0);
        }
        let left = &self.stood[self.at..];
        let got = left.len().min(into.len());
        into[..got].copy_from_slice(&left[..got]);
        self.at += got;
        Ok(got)
    }
}

// A reason of this module for an error in the database, so that the readers of the summary form
// keep one type of reason and one way of answering it.
fn carried(problem: rusqlite::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, format!("{}", problem))
}

// The reason a step failed, in text. Every caller of this module answers with a line for the
// log of the program, so a reason is a string here.
fn reason<T: std::fmt::Display>(problem: T) -> String {
    format!("{}", problem)
}

// ------------------------------------------------------------------ compress

// Compress the raw file of one context and put the compressed form into the database. The
// answer is the bytes that was and the bytes it came to; nothing returns when there was nothing
// to compress, when the compressed form would not be smaller, or when the work could not be
// finished -- and in that last case the raw file is left where it was, which is the whole point
// of writing beside it first. The compressed form is written into a second file and carried
// into the database from there, so a context is never half in the database and half on the
// disk.
pub fn pack(dir: &Path, name: &str) -> Option<(u64, u64)> {
    let raw = raw_of(dir, name);
    let stood = fs::metadata(&raw).ok()?.len();
    if stood == 0 {
        return None;
    }
    let held = store::held_path(&raw);
    let made = match write_short(&raw, &held) {
        Ok(made) => made,
        Err(problem) => {
            let _ = fs::remove_file(&held);
            proglog::note(
                proglog::STORE,
                &format!("a context was not compressed and remains as it was: {}", problem),
            );
            return None;
        }
    };
    if made >= stood {
        // Nothing was to be had out of this context.
        let _ = fs::remove_file(&held);
        return None;
    }
    let kept = database(dir).map_err(|problem| {
        proglog::note(
            proglog::STORE,
            &format!("a context was not saved and remains as it was: {}", problem),
        );
    });
    match kept {
        Ok(database) => match keep(&database, name, &held, stood) {
            Ok(_pieces) => {
                // The context remains whole in the database now, so the raw file and the second
                // file it came through may go.
                let _ = fs::remove_file(&held);
                let _ = fs::remove_file(&raw);
                proglog::note(
                    proglog::STORE,
                    &format!(
                        "the context of a chat was compressed from {} bytes to {} in the database",
                        stood, made
                    ),
                );
                Some((stood, made))
            }
            Err(problem) => {
                let _ = fs::remove_file(&held);
                proglog::note(
                    proglog::STORE,
                    &format!("a context was not saved and remains as it was: {}", problem),
                );
                None
            }
        },
        Err(()) => None,
    }
}

// Lay the raw file of one context out again, so that the server can read it. The answer reports
// what was found, so that the caller knows whether the raw file it leaves behind is one this
// program made.
pub fn lay_out(dir: &Path, name: &str) -> Laid {
    let raw = raw_of(dir, name);
    if fs::metadata(&raw).map(|one| one.len()).unwrap_or(0) > 0 {
        return Laid::RawStood;
    }
    // A context kept by an older run is beside the raw file, under the name of the context with
    // `.packed` at the end. It is extracted from that form, and put into the database while it
    // is, so the older format is not needed again.
    let short = short_of_place(dir, name);
    if fs::metadata(&short).map(|one| one.len()).unwrap_or(0) > 0 {
        let held = store::held_path(&raw);
        let came = File::open(&short)
            .map_err(|problem| {
                std::io::Error::new(problem.kind(), "a compressed context could not be read")
            })
            .and_then(|file| {
                let mut from = BufReader::with_capacity(BLOCK, file);
                write_raw_from(&mut from, &held)
            });
        match came {
            Ok(()) if fs::rename(&held, &raw).is_ok() => {
                keep_the_older_shape(dir, name, &short);
                return Laid::FromShort;
            }
            _ => {
                let _ = fs::remove_file(&held);
                let _ = fs::remove_file(&raw);
            }
        }
    }
    let database = match database(dir) {
        Ok(kept) => kept,
        Err(problem) => {
            proglog::note(
                proglog::STORE,
                &format!("the contexts database could not be opened: {}", problem),
            );
            return Laid::Nothing;
        }
    };
    if !holds(&database, name) {
        drop_half(&database, name);
        return Laid::Nothing;
    }
    let held = store::held_path(&raw);
    let came = database.read(|conn| {
        let mut from = Pieces::new(conn, name);
        write_raw_from(&mut from, &held).map_err(reason)
    });
    match came {
        Ok(()) if fs::rename(&held, &raw).is_ok() => Laid::FromShort,
        Ok(_) => {
            let _ = fs::remove_file(&held);
            let _ = fs::remove_file(&raw);
            Laid::Nothing
        }
        Err(problem) => {
            // A context that cannot be extracted from the database is a context that was not
            // kept: the server is asked without it, and the context is read through. What is in
            // the database is left for the appearance of a person, because a context that will
            // not extracted may still be mended.
            let _ = fs::remove_file(&held);
            let _ = fs::remove_file(&raw);
            proglog::note(
                proglog::STORE,
                &format!("a context could not be read from the database: {}", problem),
            );
            Laid::Nothing
        }
    }
}

// Put a context that is in the older format into the database, and take the file away. A
// context that will not go in is left in the file it is in: it was extracted from there a
// moment ago, so nothing is lost by leaving it.
fn keep_the_older_shape(dir: &Path, name: &str, short: &Path) {
    let stood = fs::metadata(short).map(|one| one.len()).unwrap_or(0);
    let database = match database(dir) {
        Ok(kept) => kept,
        Err(_) => return,
    };
    match keep(&database, name, short, stood) {
        Ok(_) => {
            let _ = fs::remove_file(short);
            proglog::note(
                proglog::STORE,
                &format!(
                    "a context kept in the older format was migrated into the database: {}",
                    name
                ),
            );
        }
        Err(problem) => {
            proglog::note(
                proglog::STORE,
                &format!(
                    "a context kept in the older format was not migrated into the database: {}",
                    problem
                ),
            );
        }
    }
}

// Remove the raw file that `lay_out` wrote, once the server has read it.
pub fn take_away(dir: &Path, name: &str) {
    let _ = fs::remove_file(raw_of(dir, name));
}

// Take the contexts of one chat out of the database, and say how many went. The contexts of a
// chat are under names that begin with the chat's own marker and are distinguished by the model
// that read them through, so all of a chat's contexts is the names that begin with that marker
// and carry the marker between after it. The names are read and matched here rather than asked
// of the database with a LIKE, for the marker of a chat may carry an under score, which a LIKE
// takes for one of its own and would answer with other chats' contexts besides.
pub fn drop_of_chat(dir: &Path, mark: &str) -> usize {
    if mark.is_empty() {
        return 0;
    }
    let Ok(kept) = database(dir) else {
        return 0;
    };
    let held: Vec<String> = kept
        .read(|conn| {
            let mut said = conn
                .prepare("SELECT name FROM contexts")
                .map_err(db::fault)?;
            let rows = said
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(db::fault)?;
            let mut out = Vec::new();
            for row in rows.flatten() {
                out.push(row);
            }
            Ok(out)
        })
        .unwrap_or_default();
    let mine: Vec<String> = held
        .into_iter()
        .filter(|name| talk_of_mark(name, mark))
        .collect();
    if mine.is_empty() {
        return 0;
    }
    let went = mine.len();
    if let Err(problem) = kept.write(|conn| {
        for name in &mine {
            conn.execute("DELETE FROM contexts WHERE name = ?1", params![name])
                .map_err(db::fault)?;
            conn.execute("DELETE FROM context_parts WHERE name = ?1", params![name])
                .map_err(db::fault)?;
        }
        Ok(())
    }) {
        proglog::note(
            proglog::STORE,
            &format!("the contexts of {} are not removed: {}", mark, problem),
        );
        return 0;
    }
    went
}

// Whether `name` is one of the contexts of the chat whose marker is `mark`.
fn talk_of_mark(name: &str, mark: &str) -> bool {
    name.strip_prefix(mark)
        .is_some_and(|rest| rest.starts_with('-'))
}

// Cut the bytes of a block into two byte planes: the bytes at the even offsets, then the bytes
// at the odd offsets. The answer is all of the block again, in a different order, so the length
// is the length of what went in.
fn split(data: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.reserve(data.len());
    let mut at = 0;
    while at < data.len() {
        out.push(data[at]);
        at += 2;
    }
    let mut at = 1;
    while at < data.len() {
        out.push(data[at]);
        at += 2;
    }
}

// Lay the two byte planes of a block back over one another. `whole` is how many bytes the block
// held, which shows how many of each plane there were.
fn merge(planes: &[u8], whole: usize, out: &mut Vec<u8>) {
    out.clear();
    out.reserve(whole);
    let (even, odd) = planes.split_at(whole.div_ceil(2));
    for step in 0..odd.len() {
        out.push(even[step]);
        out.push(odd[step]);
    }
    if even.len() > odd.len() {
        out.push(even[even.len() - 1]);
    }
}

// Read a block of a file into `block`, and give back how many bytes came. The read is asked
// again when it returns short, which it does on its own.
fn read_block<R: Read>(from: &mut R, block: &mut [u8]) -> std::io::Result<usize> {
    let mut got = 0;
    while got < block.len() {
        match from.read(&mut block[got..]) {
            Ok(0) => break,
            Ok(more) => got += more,
            Err(problem) => return Err(problem),
        }
    }
    Ok(got)
}

// Write the compressed form of a context into `held`, and give back the bytes it came to. The
// header reports the whole length of the raw file, so a compressed form that was truncated is
// found out when it is read back.
fn write_short(raw: &Path, held: &Path) -> std::io::Result<u64> {
    let whole = fs::metadata(raw)?.len();
    let mut from = BufReader::with_capacity(BLOCK, File::open(raw)?);
    let mut to = BufWriter::with_capacity(BLOCK, File::create(held)?);
    let mut block = vec![0u8; BLOCK];
    let mut planes: Vec<u8> = Vec::with_capacity(BLOCK);
    let mut coded: Vec<u8> = Vec::with_capacity(BLOCK / 2);
    to.write_all(HEAD)?;
    to.write_all(&whole.to_le_bytes())?;
    to.write_all(&(BLOCK as u32).to_le_bytes())?;
    loop {
        let got = read_block(&mut from, &mut block)?;
        if got == 0 {
            break;
        }
        split(&block[..got], &mut planes);
        coded.clear();
        {
            let mut enc = DeflateEncoder::new(&mut coded, Compression::new(LEVEL as u32));
            enc.write_all(&planes)?;
            enc.finish()?;
        }
        to.write_all(&(got as u32).to_le_bytes())?;
        to.write_all(&(coded.len() as u32).to_le_bytes())?;
        to.write_all(&coded)?;
        if got < BLOCK {
            break;
        }
    }
    to.flush()?;
    let made = to.into_inner()?.stream_position()?;
    Ok(made)
}

// Read the compressed form of a context out of `from` into `held`, which is the raw file. The
// form may come from a file of the older keeping or from the parts in the database, which is
// why the user of it is requested and not the name. Every block is checked against the length
// its header reports, so a form that was truncated, or that was written by something else, is
// reported against rather than decompressed as a context.
fn write_raw_from<R: Read>(mut from: R, held: &Path) -> std::io::Result<()> {
    let mut head = [0u8; 8];
    from.read_exact(&mut head)?;
    if &head != HEAD {
        return Err(fault("the header is not that of a compressed context"));
    }
    let mut whole = [0u8; 8];
    from.read_exact(&mut whole)?;
    let whole = u64::from_le_bytes(whole);
    let mut room = [0u8; 4];
    from.read_exact(&mut room)?;
    let room = u32::from_le_bytes(room) as usize;
    if room == 0 || room > BLOCK {
        return Err(fault("a block of a compressed context is larger than the block size"));
    }
    let mut to = BufWriter::with_capacity(BLOCK, File::create(held)?);
    let mut planes: Vec<u8> = Vec::with_capacity(room);
    let mut block: Vec<u8> = Vec::with_capacity(room);
    let mut stood: u64 = 0;
    loop {
        let mut mark = [0u8; 4];
        let came = read_some(&mut from, &mut mark)?;
        if came == 0 {
            break;
        }
        if came != mark.len() {
            return Err(fault("a compressed context ended in the middle of a block marker"));
        }
        let got = u32::from_le_bytes(mark) as usize;
        let mut size = [0u8; 4];
        from.read_exact(&mut size)?;
        let size = u32::from_le_bytes(size) as usize;
        if got == 0 || got > room {
            return Err(fault("a block of a compressed context is larger than the block size"));
        }
        let mut coded = vec![0u8; size];
        from.read_exact(&mut coded)?;
        planes.clear();
        let mut dec = DeflateDecoder::new(&coded[..]);
        // A block cannot hold more than the length its header gives, so the read back is
        // stopped there: text that expand past it is text that was never meant as a context.
        dec.by_ref()
            .take((got + 1) as u64)
            .read_to_end(&mut planes)?;
        if planes.len() != got {
            return Err(fault("a block of a compressed context did not reach the length its header gave"));
        }
        merge(&planes, got, &mut block);
        to.write_all(&block)?;
        stood += got as u64;
    }
    to.flush()?;
    if stood != whole {
        return Err(fault(
            "a compressed context ended before the length its header gave",
        ));
    }
    Ok(())
}

// Read as much as is requested, and stop at the end of the file without making an error in it.
fn read_some<R: Read>(from: &mut R, into: &mut [u8]) -> std::io::Result<usize> {
    let mut got = 0;
    while got < into.len() {
        match from.read(&mut into[got..]) {
            Ok(0) => break,
            Ok(more) => got += more,
            Err(problem) => return Err(problem),
        }
    }
    Ok(got)
}

// A reason of this module, for a form that is not a context.
fn fault(words: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, words.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(what: &str) -> PathBuf {
        static TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let place = std::env::temp_dir().join(format!(
            "autumn-natter-{}-{}-{}",
            what,
            std::process::id(),
            TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&place);
        fs::create_dir_all(&place).expect("the directory of the test could not be made");
        place
    }

    // Bytes that remain near to the bytes of a context: pairs of bytes where one of the two
    // gives away a great deal more than the other.
    fn cache_bytes(how_many: usize, seed: &mut u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(how_many);
        for step in 0..how_many / 2 {
            // The high byte of the pair is among a handful of values, the low one runs over all
            // of the byte.
            let high = ((step % 7) as u8) << 4;
            *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let low = (*seed >> 33) as u8;
            out.push(high);
            out.push(low);
        }
        out
    }

    // A context is compressed, the raw file goes, and the compressed form reads back byte for
    // byte as the context that went into it.
    #[test]
    fn a_shortened_talk_reads_back_byte_for_byte() {
        let dir = room("round");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        let words = cache_bytes(300_002, &mut 7);
        fs::write(dir.join(name), &words).expect("the conversation is written");
        let (stood, made) = pack(&dir, name).expect("the conversation is compressed");
        assert_eq!(stood, words.len() as u64);
        assert!(
            made < stood,
            "the compressed form is shorter: {} of {}",
            made,
            stood
        );
        assert!(!dir.join(name).exists(), "the raw file is removed");
        assert!(
            !dir.join(short_of(name)).exists(),
            "no file of the summary form is left beside the folder"
        );
        let kept = held_list(&dir);
        assert_eq!(kept.len(), 1, "the context is in the database");
        assert_eq!(kept[0].0, name, "the context is under its own name");
        assert_eq!(kept[0].1, stood, "the bytes that was are informed");
        assert_eq!(kept[0].2, made, "the compressed bytes are reported");

        assert_eq!(lay_out(&dir, name), Laid::FromShort);
        let back = fs::read(dir.join(name)).expect("the raw file is decompressed again");
        assert_eq!(back, words, "not one byte of the context moved");
        take_away(&dir, name);
        assert!(!dir.join(name).exists(), "the raw file goes after the read");
        let _ = fs::remove_dir_all(&dir);
    }

    // A context kept by an older run is beside the raw file, under the name of the context with
    // `.packed` at the end. It is extracted from that form, decompressed again, and comes into
    // the database with it, so the older format is not needed a second time.
    #[test]
    fn a_talk_of_the_older_shape_is_read_and_comes_into_the_database() {
        let dir = room("older-shape");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        let words = cache_bytes(300_002, &mut 13);
        let raw = dir.join(name);
        fs::write(&raw, &words).expect("the conversation is written");
        // The form of the older keeping: the compressed form beside the raw file, and the raw
        // file removed.
        let made = write_short(&raw, &dir.join(short_of(name))).expect("the compressed form is created");
        fs::remove_file(&raw).expect("the raw file goes");
        assert!(made > 0);
        assert_eq!(lay_out(&dir, name), Laid::FromShort);
        assert_eq!(
            fs::read(&raw).expect("the conversation is decompressed"),
            words,
            "not one byte of the older context moved"
        );
        // The database holds it now, and the file it was in does not.
        assert_eq!(held_list(&dir).len(), 1, "the context came into the database");
        take_away(&dir, name);
        assert!(!dir.join(short_of(name)).exists(), "the older format is removed");
        assert_eq!(lay_out(&dir, name), Laid::FromShort, "and is read from the database");
        let _ = fs::remove_dir_all(&dir);
    }

    // The blocks are checked one at a time, so a context of more than one block returns whole,
    // with a last block of another length than the rest.
    #[test]
    fn a_talk_of_more_than_one_block_comes_back_whole() {
        let dir = room("blocks");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        let mut words = cache_bytes(BLOCK + 3, &mut 11);
        // A whole block, a block of another size, and one of nothing but one byte.
        words.extend_from_slice(&[3u8; BLOCK - 5]);
        words.push(9);
        fs::write(dir.join(name), &words).expect("the conversation is written");
        let (stood, made) = pack(&dir, name).expect("the conversation is compressed");
        assert_eq!(stood, words.len() as u64);
        assert!(made < stood);
        assert_eq!(lay_out(&dir, name), Laid::FromShort);
        assert_eq!(
            fs::read(dir.join(name)).expect("the conversation is decompressed"),
            words,
            "the context of many blocks returned as it went in"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // A context that cannot be compressed keeps its raw form: the short one is deleted rather
    // than put into the database beside it.
    #[test]
    fn a_talk_that_cannot_be_shortened_keeps_its_shape() {
        let dir = room("nothing");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        // Text of nothing at all is compressed to a handful of bytes, so the context that
        // cannot be compressed is text of no rule at all.
        let mut seed = 99u64;
        let mut words = Vec::with_capacity(64 * 1024);
        for _ in 0..words.capacity() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            words.push((seed >> 33) as u8);
        }
        fs::write(dir.join(name), &words).expect("the conversation is written");
        let _ = pack(&dir, name);
        assert!(dir.join(name).is_file(), "the conversation remains as it was");
        assert!(
            held_list(&dir).is_empty(),
            "no compressed form of it is in the database"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // A context that is not there is not compressed, and nothing is reported about it.
    #[test]
    fn a_talk_that_is_not_there_is_not_shortened() {
        let dir = room("absent");
        assert!(pack(&dir, "gone-0123456789abcdef.ctx").is_none());
        assert_eq!(lay_out(&dir, "gone-0123456789abcdef.ctx"), Laid::Nothing);
        let _ = fs::remove_dir_all(&dir);
    }

    // A raw file that is there on its own -- from an older run, or from a shortening that could
    // not be finished -- is informed from one this program decompressed, so that the newer of
    // the two forms is the one kept.
    #[test]
    fn a_raw_file_that_stood_is_told_from_one_laid_out() {
        let dir = room("stood");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        assert_eq!(lay_out(&dir, name), Laid::Nothing);
        fs::write(dir.join(name), cache_bytes(2048, &mut 3)).expect("a raw conversation remains");
        assert_eq!(lay_out(&dir, name), Laid::RawStood);
        // Compressed now, and the next read lays the context out of the summary form.
        assert!(pack(&dir, name).is_some());
        assert_eq!(lay_out(&dir, name), Laid::FromShort);
        let _ = fs::remove_dir_all(&dir);
    }

    // A context that was put into the database half way through is not decompressed as a
    // context: the context of it is read through again, which is what happens with no keeping
    // at all. A half context is what a run that finished in the middle of a shortening leaves
    // behind, and its parts are removed when it is next requested.
    #[test]
    fn a_shortened_talk_cut_short_is_not_laid_out() {
        let dir = room("cut");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        let words = cache_bytes(200_000, &mut 5);
        fs::write(dir.join(name), &words).expect("the conversation is written");
        let kept = database(&dir).expect("the database of the contexts is opened");
        let short = dir.join("the-short-shape");
        let made = write_short(&dir.join(name), &short).expect("the compressed form is created");
        fs::remove_file(dir.join(name)).expect("the raw context goes, as it does when a context is kept");
        // The compressed form is truncated, and what is left of it is put into the database as
        // if it were whole.
        let stood = fs::read(&short).expect("the compressed form is read");
        let cut = &stood[..stood.len() - 4096];
        fs::write(&short, cut).expect("the compressed form is truncated");
        keep(&kept, name, &short, made).expect("the half conversation is put in");
        assert_eq!(lay_out(&dir, name), Laid::Nothing);
        assert!(!dir.join(name).exists(), "no raw file is left standing");
        // Text of another form is stated against too.
        fs::write(&short, "not a conversation at all").expect("other text is in its place");
        keep(&kept, name, &short, 17).expect("the other words are put in");
        assert_eq!(lay_out(&dir, name), Laid::Nothing);
        let _ = fs::remove_dir_all(&dir);
    }

    // A context whose parts finished half way through is with no row of its own, so it is not a
    // context at all; and its parts cost the database nothing after it is next requested.
    #[test]
    fn a_half_talk_is_not_read_and_its_pieces_go() {
        let dir = room("half");
        let kept = database(&dir).expect("the database of the contexts is opened");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        let short = dir.join("the-short-shape");
        fs::write(&short, cache_bytes(4096, &mut 21)).expect("a shape stands");
        kept.write(|conn| {
            conn.execute(
                "INSERT INTO context_parts (name, idx, part) VALUES (?1, 0, ?2)",
                params![name, vec![7u8; 4096]],
            )
            .map_err(db::fault)?;
            Ok(())
        })
        .expect("parts remain with no row of their own");
        assert_eq!(lay_out(&dir, name), Laid::Nothing, "no row, no conversation");
        let left: i64 = kept
            .read(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM context_parts WHERE name = ?1",
                    params![name],
                    |row| row.get(0),
                )
                .map_err(db::fault)
            })
            .expect("the pieces are counted");
        assert_eq!(left, 0, "the parts of a half context is removed");
        let _ = fs::remove_dir_all(&dir);
    }

    // The two cuts of a block are each other's form: what is sent in two byte planes returns in
    // order it went in, at every length.
    #[test]
    fn the_two_planes_of_a_block_lay_over_one_another() {
        for whole in 0..=9usize {
            let data: Vec<u8> = (0..whole).map(|one| (one * 7) as u8).collect();
            let mut planes = Vec::new();
            let mut back = Vec::new();
            split(&data, &mut planes);
            assert_eq!(planes.len(), data.len(), "{} bytes", whole);
            merge(&planes, data.len(), &mut back);
            assert_eq!(back, data, "{} bytes", whole);
        }
    }

    // The contexts of one chat go with it, and the contexts of no other. The contexts of a chat
    // are the names that begin with the marker of the chat and carry a dash after it, so a chat
    // whose id is the beginning of another chat's id keeps every one of its contexts, and the
    // parts of the contexts that went are taken out with the rows that named them.
    #[test]
    fn the_talks_of_one_chat_go_with_it() {
        let dir = room("drop-of-chat");
        let kept = database(&dir).expect("the database of the contexts is created");
        let stood = dir.join("the-talk");
        fs::write(&stood, cache_bytes(4096, &mut 5)).expect("a conversation is written");
        let names = [
            "ab12cd34ef-0000000000000001.ctx",
            "ab12cd34ef-0000000000000002.ctx",
            "ab12cd34-0000000000000001.ctx",
            "ab12_cd34-0000000000000001.ctx",
        ];
        for one in names {
            keep(&kept, one, &stood, 4096).expect("the conversation is kept");
        }
        assert_eq!(held_list(&dir).len(), 4, "four conversations remain");
        assert_eq!(drop_of_chat(&dir, ""), 0, "no chat has no sends requests to give");
        assert_eq!(drop_of_chat(&dir, "ab12cd34"), 1, "its own conversations only");
        assert_eq!(drop_of_chat(&dir, "ab12cd34ef"), 2, "both contexts of that chat");
        let left = held_list(&dir);
        assert_eq!(left.len(), 1, "one conversation is left");
        assert_eq!(left[0].0, "ab12_cd34-0000000000000001.ctx");
        assert_eq!(
            pieces(&kept, "ab12cd34ef-0000000000000001.ctx"),
            0,
            "no piece of a context that went is left behind"
        );
        assert_eq!(drop_of_chat(&dir, "nothing-at-all"), 0, "a chat with no contexts");
        let _ = fs::remove_dir_all(&dir);
    }

    // The parts of a context that is in the database, counted.
    fn pieces(kept: &db::Db, name: &str) -> i64 {
        kept.read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM context_parts WHERE name = ?1",
                params![name],
                |row| row.get::<_, i64>(0),
            )
            .map_err(db::fault)
        })
        .unwrap_or(0)
    }
}
