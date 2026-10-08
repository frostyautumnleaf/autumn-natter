// Read strings from the header of a GGUF model file. The file is opened
// read-only, and the header is walked until the wanted key is found. The
// tensor data is never touched, so a big model costs a few reads only.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

// The magic of a GGUF file, read as a little-endian number.
const MAGIC: u32 = 0x46_55_47_47;
// A key length above this means a corrupt file, not a key.
const MAX_KEY: u64 = 1 << 20;
// A string length above this means a corrupt file, not a string.
const MAX_STRING: u64 = 1 << 30;
// An array count above this means a corrupt file, not a count.
const MAX_ARRAY: u64 = 1 << 30;

// The size of a fixed-size value, in bytes. None means the type is not fixed
// size, so the value cannot be skipped.
fn fixed_size(vtype: u32) -> Option<u64> {
    match vtype {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

fn read_u64(file: &mut File) -> Option<u64> {
    let mut raw = [0u8; 8];
    file.read_exact(&mut raw).ok()?;
    Some(u64::from_le_bytes(raw))
}

fn read_u32(file: &mut File) -> Option<u32> {
    let mut raw = [0u8; 4];
    file.read_exact(&mut raw).ok()?;
    Some(u32::from_le_bytes(raw))
}

// Read a string value: a length and the bytes.
fn read_string_value(file: &mut File) -> Option<String> {
    let len = read_u64(file)?;
    if len > MAX_STRING {
        return None;
    }
    let mut raw = vec![0u8; len as usize];
    file.read_exact(&mut raw).ok()?;
    Some(String::from_utf8_lossy(&raw).into_owned())
}

// Move the file past one value of the given type.
fn skip_value(file: &mut File, vtype: u32) -> bool {
    match vtype {
        8 => {
            let len = match read_u64(file) {
                Some(len) => len,
                None => return false,
            };
            if len > MAX_STRING {
                return false;
            }
            file.seek(SeekFrom::Current(len as i64)).is_ok()
        }
        9 => {
            let atype = match read_u32(file) {
                Some(atype) => atype,
                None => return false,
            };
            let count = match read_u64(file) {
                Some(count) => count,
                None => return false,
            };
            if count > MAX_ARRAY {
                return false;
            }
            if atype == 8 {
                for _ in 0..count {
                    let len = match read_u64(file) {
                        Some(len) => len,
                        None => return false,
                    };
                    if len > MAX_STRING {
                        return false;
                    }
                    if file.seek(SeekFrom::Current(len as i64)).is_err() {
                        return false;
                    }
                }
                true
            } else {
                let size = match fixed_size(atype) {
                    Some(size) => size,
                    None => return false,
                };
                file.seek(SeekFrom::Current((size * count) as i64)).is_ok()
            }
        }
        other => {
            let size = match fixed_size(other) {
                Some(size) => size,
                None => return false,
            };
            file.seek(SeekFrom::Current(size as i64)).is_ok()
        }
    }
}

// Read one string value from the header of a GGUF file.
//
// The header holds the magic, the version, the tensor count, the count of key
// value pairs, and the pairs. A pair is a key string, a value type, and the
// value. Strings carry a length in front. Arrays carry the type of the
// element and the count.
pub fn read_string(path: &Path, key: &str) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 24];
    file.read_exact(&mut head).ok()?;
    let magic = u32::from_le_bytes(head[0..4].try_into().ok()?);
    if magic != MAGIC {
        return None;
    }
    // Bytes 4 to 8 hold the version, and the walk does not need it.
    let kv_count = u64::from_le_bytes(head[16..24].try_into().ok()?);
    for _ in 0..kv_count {
        let klen = read_u64(&mut file)?;
        if klen > MAX_KEY {
            return None;
        }
        let mut raw = vec![0u8; klen as usize];
        file.read_exact(&mut raw).ok()?;
        let name = String::from_utf8_lossy(&raw).into_owned();
        let vtype = read_u32(&mut file)?;
        if name == key {
            if vtype == 8 {
                return read_string_value(&mut file);
            }
            return None;
        }
        if !skip_value(&mut file, vtype) {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn push_key(file: &mut Vec<u8>, key: &str) {
        file.extend_from_slice(&(key.len() as u64).to_le_bytes());
        file.extend_from_slice(key.as_bytes());
    }

    fn push_string(file: &mut Vec<u8>, text: &str) {
        file.extend_from_slice(&(text.len() as u64).to_le_bytes());
        file.extend_from_slice(text.as_bytes());
    }

    // A file with a number, the wanted string, and a string array after it.
    fn write_gguf(dir: &Path, name: &str) -> PathBuf {
        let mut file = Vec::new();
        file.extend_from_slice(b"GGUF");
        file.extend_from_slice(&3u32.to_le_bytes());
        file.extend_from_slice(&0u64.to_le_bytes());
        file.extend_from_slice(&3u64.to_le_bytes());
        push_key(&mut file, "general.architecture");
        file.extend_from_slice(&4u32.to_le_bytes());
        file.extend_from_slice(&7u32.to_le_bytes());
        push_key(&mut file, "tokenizer.chat_template");
        file.extend_from_slice(&8u32.to_le_bytes());
        push_string(&mut file, "{%- if enable_thinking %}X{% endif %}");
        push_key(&mut file, "tokenizer.ggml.tokens");
        file.extend_from_slice(&9u32.to_le_bytes());
        file.extend_from_slice(&8u32.to_le_bytes());
        file.extend_from_slice(&2u64.to_le_bytes());
        push_string(&mut file, "a");
        push_string(&mut file, "bb");
        let path = dir.join(name);
        std::fs::write(&path, &file).unwrap();
        path
    }

    #[test]
    fn a_string_key_is_read_from_a_header() {
        let dir = Path::new("uidata/gguf-tests");
        let _ = std::fs::create_dir_all(dir);
        let path = write_gguf(dir, "test.gguf");
        let text = read_string(&path, "tokenizer.chat_template").expect("the key is in the file");
        assert_eq!(text, "{%- if enable_thinking %}X{% endif %}");
    }

    #[test]
    fn a_missing_key_gives_none() {
        let dir = Path::new("uidata/gguf-tests");
        let _ = std::fs::create_dir_all(dir);
        let path = write_gguf(dir, "test.gguf");
        assert!(read_string(&path, "general.name").is_none());
    }

    #[test]
    fn a_file_without_the_magic_gives_none() {
        let dir = Path::new("uidata/gguf-tests");
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join("not.gguf");
        std::fs::write(&path, b"not a model file at all").unwrap();
        assert!(read_string(&path, "tokenizer.chat_template").is_none());
    }

    // The bundled test model carries the stock Qwen template. It has the
    // thinking switch, and it has no effort levels.
    #[test]
    fn the_test_model_carries_the_switch() {
        let path = Path::new("../testing_model/Qwen3.5-0.8B-UD-Q6_K_XL.gguf");
        if !path.exists() {
            return;
        }
        let template =
            read_string(path, "tokenizer.chat_template").expect("the template is in the file");
        let options = crate::markdown::analyze_template(&template);
        assert!(options.has_thinking);
        assert!(!options.has_effort);
    }
}
