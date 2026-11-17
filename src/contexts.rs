// The talks the model worked its way through, kept in a database.
//
// The model understands a talk in one slot of the running server and nowhere
// else. llama.cpp can copy that state out into a file and lay it back into a
// slot again, and the engine asks for both: when a chat gives its slot to
// another chat, when an answer is stopped by the user, and when the server is
// stopped.
//
// The server reads and writes a talk by name in a folder of its own, so the
// folder of talks stands between this program and the server. It is a place of
// passage and not a place of keeping: a talk is laid out of the database into it
// just before the server reads it, and taken away again just after. What the
// program keeps stands in `contexts.db`, in the same folder, in pieces.
//
// Such a talk is as big as the room the model set aside for the talk. A chat on
// a long context costs several gigabytes, and on one machine the folder of them
// had grown to 58 Gbyte of the disk. The bytes are the 16-bit numbers of the
// attention cache, which stand near to random, so no scheme of shortening does
// much with them: the most any general scheme reached, measured on the talks of
// that machine, was a fifth. Two things therefore rule this module: it must not
// cost the user a wait, and it must not change one byte of a talk.
//
//   * The bytes of a block are laid out in two planes first -- the bytes at the
//     even places, then the bytes at the odd ones. The cache is made of pairs of
//     bytes, and the two halves of a pair differ widely in how much they give
//     away, so splitting them lets the coder work on each of them alone. On the
//     talk of a real machine this took the shortening from 1.153 to 1.215.
//   * Each block of four megabytes then goes through deflate at level 1: 88
//     Mbyte a second out, 174 Mbyte a second back, measured on the same machine.
//     Level 1 is the whole of the speed that is to be had there; level 3 costs
//     five times the time for a shorter file nobody asked for.
//   * Everything is streamed, block by block, from the file to the database and
//     back, so a talk of ten gigabytes never stands in memory and the work costs
//     the room of two blocks. Nothing is asked of the database that a plain file
//     does not do better: the pieces are put down one after another and read back
//     the same way.
//   * The row that says a talk stands in the database is put down only when the
//     last piece of it stands. A shortening that cannot be finished therefore
//     leaves the talk exactly as it was, and no half talk is read back.
//   * A talk is only shortened when the short shape is really smaller than the
//     talk. Where nothing is to be had the raw file is kept, for two shapes of
//     a talk nobody can shorten are worse than one.
//
// The name the server reads is the raw name, so the raw file is laid out again
// for the moment of the read and taken away directly after it. A folder of talks
// that were never shortened -- one left by an older run of the program -- is
// therefore shortened by itself the first time that talk is read.
//
// Before the database the short shape stood beside the raw file, under the same
// name with `.packed` at the end of it. That shape is still read, so a talk kept
// by an older run is laid back out and comes into the database with it.
//
// One cost follows from the server's liking for a file: while a talk is being
// written, or read, both shapes of it stand on the disk -- the talk in the
// database and the raw file laid out of it -- so the room the folder needs at
// its worst is near to twice the size of the biggest talk, though it settles
// back after. On a machine with the disk near full the shortening is then simply
// refused, the raw file is kept, and the log of the program says so.

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

// The mark at the end of the name of a shortened talk. It stands after the whole
// name of the talk, so the raw name the server asks for is never touched.
pub const SHORT: &str = ".packed";

// The words at the head of a shortened talk. The last two of them stand for the
// shape of what follows, and a file that does not begin with them is not one.
const HEAD: &[u8; 8] = b"ANTXPK\x01\x00";

// The bytes of one talk worked at a time: the room of the copy, and the size
// every block of the file is cut to.
const BLOCK: usize = 4 * 1024 * 1024;

// The level the talks are shortened at. It is written down so that a change of
// it is a choice, and an accident.
const LEVEL: u8 = 1;

// The name of the short shape of one talk. It is the name a talk was kept under
// before the database, and a file of that shape is still read.
pub fn short_of(name: &str) -> String {
    format!("{name}{SHORT}")
}

// The two places one talk stands: the name the server reads, and the short shape
// the program keeps.
fn raw_of(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}

fn short_of_place(dir: &Path, name: &str) -> PathBuf {
    dir.join(short_of(name))
}

// What `lay_out` found in the folder of talks. It says what to do once the
// server has read the talk: a raw file this program laid out is thrown away,
// while a raw file that stood there on its own is shortened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Laid {
    // The short shape was read out into a raw file, which is the talk as it
    // stands now. Nothing of it is new.
    FromShort,
    // A raw file stood there already: from an older run of the program, or from
    // a shortening that could not be finished. It is newer than any short shape.
    RawStood,
    // Neither shape is there, so there is no talk to read.
    Nothing,
}

// ------------------------------------------------------------------ database

// The database the talks of one folder stand in. It stands in the folder of the
// talks, because that folder may be pointed at a bigger disk than the one the
// program data stands on, and a talk is the bulk of what a chat costs.
pub fn database(dir: &Path) -> Result<Arc<db::Db>, String> {
    db::open(&dir.join(db::CONTEXTS_DB), db::Kind::Contexts)
}

// The size of one piece of a talk in the database. A piece is small enough that
// a talk of ten gigabytes is ten thousand rows rather than one row the database
// cannot hold, and big enough that the work is a run of large writes and not a
// run of calls.
const PIECE: usize = 1024 * 1024;

// True when one talk stands in the database, whole. A talk whose pieces came to
// an end half way through has no row of its own, so it is not stood.
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

// The talks the folder holds, with the bytes that stood and the bytes they came
// to. The Logs tab has no page of its own for the talks, so this is what a check
// of the folder is read against.
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

// Put the short shape that stands at `from` into the database under `name`, and
// say how many pieces it came to. Every piece is put down before the row that
// says the talk is there, so a write that cannot be finished leaves no half talk
// behind: the pieces of the talk that stood before are laid aside in the same
// turn as the new ones come in.
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

// One piece of one talk.
fn put(conn: &rusqlite::Connection, name: &str, index: i64, piece: &[u8]) -> Result<(), String> {
    conn.execute(
        "INSERT INTO context_parts (name, idx, part) VALUES (?1, ?2, ?3)",
        params![name, index, piece],
    )
    .map_err(db::fault)?;
    Ok(())
}

// A talk that stands with pieces and no row of its own came from a write that
// could not be finished. Its pieces are taken away, so the room they cost does
// not stand in the database for nothing.
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
            &format!("the pieces of a half talk are not taken away: {}", problem),
        );
    }
}

// A reader over the pieces of one talk, in the order they stand. The short shape
// is read back exactly as it was written, block after block, so the reader of
// the shape needs no word of the database under it.
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

// A reason of this module for a fault of the database, so that the readers of
// the short shape keep one kind of reason and one way of answering it.
fn carried(problem: rusqlite::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, format!("{}", problem))
}

// The reason a step failed, in words. Every caller of this module answers with a
// line for the log of the program, so a reason is a string here.
fn reason<T: std::fmt::Display>(problem: T) -> String {
    format!("{}", problem)
}

// ------------------------------------------------------------------ shorten

// Shorten the raw file of one talk and put the short shape into the database.
// The answer is the bytes that stood and the bytes it came to; nothing comes
// back when there was nothing to shorten, when the short shape would not be
// smaller, or when the work could not be finished -- and in that last case the
// raw file is left where it stood, which is the whole point of writing beside it
// first. The short shape is written into a second file and carried into the
// database from there, so a talk is never half in the database and half on the
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
                &format!("a talk was not shortened, and stands as it was: {}", problem),
            );
            return None;
        }
    };
    if made >= stood {
        // Nothing was to be had out of this talk.
        let _ = fs::remove_file(&held);
        return None;
    }
    let kept = database(dir).map_err(|problem| {
        proglog::note(
            proglog::STORE,
            &format!("a talk was not kept, and stands as it was: {}", problem),
        );
    });
    match kept {
        Ok(database) => match keep(&database, name, &held, stood) {
            Ok(_pieces) => {
                // The talk stands whole in the database now, so the raw file and
                // the second file it came through may go.
                let _ = fs::remove_file(&held);
                let _ = fs::remove_file(&raw);
                proglog::note(
                    proglog::STORE,
                    &format!(
                        "the talk of a chat was shortened from {} bytes to {} in the database",
                        stood, made
                    ),
                );
                Some((stood, made))
            }
            Err(problem) => {
                let _ = fs::remove_file(&held);
                proglog::note(
                    proglog::STORE,
                    &format!("a talk was not kept, and stands as it was: {}", problem),
                );
                None
            }
        },
        Err(()) => None,
    }
}

// Lay the raw file of one talk out again, so that the server can read it. The
// answer says what was found, so that the caller knows whether the raw file it
// leaves behind is one this program made.
pub fn lay_out(dir: &Path, name: &str) -> Laid {
    let raw = raw_of(dir, name);
    if fs::metadata(&raw).map(|one| one.len()).unwrap_or(0) > 0 {
        return Laid::RawStood;
    }
    // A talk kept by an older run stands beside the raw file, under the name of
    // the talk with `.packed` at the end. It is read out of that shape, and put
    // into the database while it is, so the older shape is not needed again.
    let short = short_of_place(dir, name);
    if fs::metadata(&short).map(|one| one.len()).unwrap_or(0) > 0 {
        let held = store::held_path(&raw);
        let came = File::open(&short)
            .map_err(|problem| {
                std::io::Error::new(problem.kind(), "the short shape of a talk did not open")
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
                &format!("the database of the talks did not open: {}", problem),
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
            // A talk that cannot be read out of the database is a talk that was
            // not kept: the server is asked without it, and the talk is read
            // through. What stands in the database is left for the look of a
            // person, because a talk that will not read out may still be mended.
            let _ = fs::remove_file(&held);
            let _ = fs::remove_file(&raw);
            proglog::note(
                proglog::STORE,
                &format!("a talk did not read out of the database: {}", problem),
            );
            Laid::Nothing
        }
    }
}

// Put a talk that stands in the older shape into the database, and take the file
// away. A talk that will not go in is left in the file it stands in: it was read
// out of there a moment ago, so nothing is lost by leaving it.
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
                    "a talk kept in the older shape came into the database: {}",
                    name
                ),
            );
        }
        Err(problem) => {
            proglog::note(
                proglog::STORE,
                &format!(
                    "a talk kept in the older shape did not come into the database: {}",
                    problem
                ),
            );
        }
    }
}

// Take away the raw file that `lay_out` wrote, once the server has read it.
pub fn take_away(dir: &Path, name: &str) {
    let _ = fs::remove_file(raw_of(dir, name));
}

// Cut the bytes of a block into two planes: the bytes at the even places, then
// the bytes at the odd ones. The answer is the whole of the block again, in a
// different order, so the length is the length of what went in.
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

// Lay the two planes of a block back over one another. `whole` is how many bytes
// the block held, which says how many of each plane there were.
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

// Read a block of a file into `block`, and give back how many bytes came. The
// read is asked again when it comes back short, which it does on its own.
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

// Write the short shape of a talk into `held`, and give back the bytes it came
// to. The head says the whole length of the raw file, so a short shape that was
// cut short is found out when it is read back.
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

// Read the short shape of a talk out of `from` into `held`, which is the raw
// file. The shape may come from a file of the older keeping or from the pieces
// in the database, which is why the reader of it is asked for and not the name.
// Every block is looked at against the length its head says, so a shape that was
// cut short, or that was written by something else, is said against rather than
// laid out as a talk.
fn write_raw_from<R: Read>(mut from: R, held: &Path) -> std::io::Result<()> {
    let mut head = [0u8; 8];
    from.read_exact(&mut head)?;
    if &head != HEAD {
        return Err(fault("the words at the head are not those of a shortened talk"));
    }
    let mut whole = [0u8; 8];
    from.read_exact(&mut whole)?;
    let whole = u64::from_le_bytes(whole);
    let mut room = [0u8; 4];
    from.read_exact(&mut room)?;
    let room = u32::from_le_bytes(room) as usize;
    if room == 0 || room > BLOCK {
        return Err(fault("the block of a shortened talk is bigger than the room of the copy"));
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
            return Err(fault("a shortened talk came to an end in the middle of a mark"));
        }
        let got = u32::from_le_bytes(mark) as usize;
        let mut size = [0u8; 4];
        from.read_exact(&mut size)?;
        let size = u32::from_le_bytes(size) as usize;
        if got == 0 || got > room {
            return Err(fault("a block of a shortened talk is bigger than the room of the copy"));
        }
        let mut coded = vec![0u8; size];
        from.read_exact(&mut coded)?;
        planes.clear();
        let mut dec = DeflateDecoder::new(&coded[..]);
        // A block cannot hold more than the length its head gives, so the read
        // back is stopped there: words that expand past it are words that were
        // never meant as a talk.
        dec.by_ref()
            .take((got + 1) as u64)
            .read_to_end(&mut planes)?;
        if planes.len() != got {
            return Err(fault("a block of a shortened talk did not read to its length"));
        }
        merge(&planes, got, &mut block);
        to.write_all(&block)?;
        stood += got as u64;
    }
    to.flush()?;
    if stood != whole {
        return Err(fault(
            "a shortened talk came to an end before the length its head said",
        ));
    }
    Ok(())
}

// Read as much as is asked for, and stop at the end of the file without making a
// fault of it.
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

// A reason of this module, for a shape that is not a talk.
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
        fs::create_dir_all(&place).expect("the room of the test could not be made");
        place
    }

    // Bytes that stand near to the bytes of a talk: pairs of bytes where one of
    // the two gives away a great deal more than the other.
    fn cache_bytes(how_many: usize, seed: &mut u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(how_many);
        for step in 0..how_many / 2 {
            // The high byte of the pair stands among a handful of values, the low
            // one runs over the whole of the byte.
            let high = ((step % 7) as u8) << 4;
            *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let low = (*seed >> 33) as u8;
            out.push(high);
            out.push(low);
        }
        out
    }

    // A talk is shortened, the raw file goes, and the short shape reads back
    // byte for byte as the talk that went into it.
    #[test]
    fn a_shortened_talk_reads_back_byte_for_byte() {
        let dir = room("round");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        let words = cache_bytes(300_002, &mut 7);
        fs::write(dir.join(name), &words).expect("the talk is written");
        let (stood, made) = pack(&dir, name).expect("the talk is shortened");
        assert_eq!(stood, words.len() as u64);
        assert!(
            made < stood,
            "the short shape is shorter: {} of {}",
            made,
            stood
        );
        assert!(!dir.join(name).exists(), "the raw file is taken away");
        assert!(
            !dir.join(short_of(name)).exists(),
            "no file of the short shape is left beside the folder"
        );
        let kept = held_list(&dir);
        assert_eq!(kept.len(), 1, "the talk stands in the database");
        assert_eq!(kept[0].0, name, "the talk stands under its own name");
        assert_eq!(kept[0].1, stood, "the bytes that stood are told");
        assert_eq!(kept[0].2, made, "the bytes it came to are told");

        assert_eq!(lay_out(&dir, name), Laid::FromShort);
        let back = fs::read(dir.join(name)).expect("the raw file is laid out again");
        assert_eq!(back, words, "not one byte of the talk moved");
        take_away(&dir, name);
        assert!(!dir.join(name).exists(), "the raw file goes after the read");
        let _ = fs::remove_dir_all(&dir);
    }

    // A talk kept by an older run stands beside the raw file, under the name of
    // the talk with `.packed` at the end. It is read out of that shape, laid out
    // again, and comes into the database with it, so the older shape is not
    // needed a second time.
    #[test]
    fn a_talk_of_the_older_shape_is_read_and_comes_into_the_database() {
        let dir = room("older-shape");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        let words = cache_bytes(300_002, &mut 13);
        let raw = dir.join(name);
        fs::write(&raw, &words).expect("the talk is written");
        // The shape of the older keeping: the short shape beside the raw file, and
        // the raw file gone.
        let made = write_short(&raw, &dir.join(short_of(name))).expect("the short shape is made");
        fs::remove_file(&raw).expect("the raw file goes");
        assert!(made > 0);
        assert_eq!(lay_out(&dir, name), Laid::FromShort);
        assert_eq!(
            fs::read(&raw).expect("the talk is laid out"),
            words,
            "not one byte of the older talk moved"
        );
        // The database holds it now, and the file it stood in does not.
        assert_eq!(held_list(&dir).len(), 1, "the talk came into the database");
        take_away(&dir, name);
        assert!(!dir.join(short_of(name)).exists(), "the older shape is gone");
        assert_eq!(lay_out(&dir, name), Laid::FromShort, "and is read from the database");
        let _ = fs::remove_dir_all(&dir);
    }

    // The blocks are looked at one at a time, so a talk of more than one block
    // comes back whole, with a last block of another length than the rest.
    #[test]
    fn a_talk_of_more_than_one_block_comes_back_whole() {
        let dir = room("blocks");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        let mut words = cache_bytes(BLOCK + 3, &mut 11);
        // A whole block, a block of another size, and one of nothing but one byte.
        words.extend_from_slice(&[3u8; BLOCK - 5]);
        words.push(9);
        fs::write(dir.join(name), &words).expect("the talk is written");
        let (stood, made) = pack(&dir, name).expect("the talk is shortened");
        assert_eq!(stood, words.len() as u64);
        assert!(made < stood);
        assert_eq!(lay_out(&dir, name), Laid::FromShort);
        assert_eq!(
            fs::read(dir.join(name)).expect("the talk is laid out"),
            words,
            "the talk of many blocks came back as it went in"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // A talk that cannot be shortened keeps its raw shape: the short one is thrown
    // away rather than put into the database beside it.
    #[test]
    fn a_talk_that_cannot_be_shortened_keeps_its_shape() {
        let dir = room("nothing");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        // Words of nothing at all are shortened to a handful of bytes, so the
        // talk that cannot be shortened is words of no rule at all.
        let mut seed = 99u64;
        let mut words = Vec::with_capacity(64 * 1024);
        for _ in 0..words.capacity() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            words.push((seed >> 33) as u8);
        }
        fs::write(dir.join(name), &words).expect("the talk is written");
        let _ = pack(&dir, name);
        assert!(dir.join(name).is_file(), "the talk stands as it was");
        assert!(
            held_list(&dir).is_empty(),
            "no short shape of it stands in the database"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // A talk that is not there is not shortened, and nothing is said about it.
    #[test]
    fn a_talk_that_is_not_there_is_not_shortened() {
        let dir = room("absent");
        assert!(pack(&dir, "gone-0123456789abcdef.ctx").is_none());
        assert_eq!(lay_out(&dir, "gone-0123456789abcdef.ctx"), Laid::Nothing);
        let _ = fs::remove_dir_all(&dir);
    }

    // A raw file that stands there on its own -- from an older run, or from a
    // shortening that could not be finished -- is told from one this program laid
    // out, so that the newer of the two shapes is the one kept.
    #[test]
    fn a_raw_file_that_stood_is_told_from_one_laid_out() {
        let dir = room("stood");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        assert_eq!(lay_out(&dir, name), Laid::Nothing);
        fs::write(dir.join(name), cache_bytes(2048, &mut 3)).expect("a raw talk stands");
        assert_eq!(lay_out(&dir, name), Laid::RawStood);
        // Shortened now, and the next read lays the talk out of the short shape.
        assert!(pack(&dir, name).is_some());
        assert_eq!(lay_out(&dir, name), Laid::FromShort);
        let _ = fs::remove_dir_all(&dir);
    }

    // A talk that was put into the database half way through is not laid out as a
    // talk: the talk of it is read through again, which is what happens with no
    // keeping at all. A half talk is what a run that came to an end in the middle
    // of a shortening leaves behind, and its pieces are taken away when it is next
    // asked for.
    #[test]
    fn a_shortened_talk_cut_short_is_not_laid_out() {
        let dir = room("cut");
        let name = "ab12cd34ef-0123456789abcdef.ctx";
        let words = cache_bytes(200_000, &mut 5);
        fs::write(dir.join(name), &words).expect("the talk is written");
        let kept = database(&dir).expect("the database of the talks is opened");
        let short = dir.join("the-short-shape");
        let made = write_short(&dir.join(name), &short).expect("the short shape is made");
        fs::remove_file(dir.join(name)).expect("the raw talk goes, as it does when a talk is kept");
        // The short shape is cut short, and what is left of it is put into the
        // database as if it were whole.
        let stood = fs::read(&short).expect("the short shape is read");
        let cut = &stood[..stood.len() - 4096];
        fs::write(&short, cut).expect("the short shape is cut short");
        keep(&kept, name, &short, made).expect("the half talk is put in");
        assert_eq!(lay_out(&dir, name), Laid::Nothing);
        assert!(!dir.join(name).exists(), "no raw file is left standing");
        // Words of another shape are said against too.
        fs::write(&short, "not a talk at all").expect("other words stand in its place");
        keep(&kept, name, &short, 17).expect("the other words are put in");
        assert_eq!(lay_out(&dir, name), Laid::Nothing);
        let _ = fs::remove_dir_all(&dir);
    }

    // A talk whose pieces came to an end half way through stands with no row of
    // its own, so it is not a talk at all; and its pieces cost the database
    // nothing after it is next asked for.
    #[test]
    fn a_half_talk_is_not_read_and_its_pieces_go() {
        let dir = room("half");
        let kept = database(&dir).expect("the database of the talks is opened");
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
        .expect("pieces stand with no row of their own");
        assert_eq!(lay_out(&dir, name), Laid::Nothing, "no row, no talk");
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
        assert_eq!(left, 0, "the pieces of a half talk are taken away");
        let _ = fs::remove_dir_all(&dir);
    }

    // The two cuts of a block are each other's shape: what goes out in two planes
    // comes back in the order it went in, at every length.
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
}
