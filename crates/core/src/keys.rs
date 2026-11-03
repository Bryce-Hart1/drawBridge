// Claude — Date 11/03/2026
// Store keys, and the layout drawBridge keeps under its root in swiftSearch.
//
// File names never go into a key. They live in the op log; keys are built only
// from fixed words, ids and integers. That keeps every key inside a character
// set narrower than swiftSearch's rules (USE.md, "Keys"), so a key never needs
// percent-encoding and can never be refused for a `..` segment, a `%2F` or a
// control byte, whatever the user names their files.
//
// Layout (see docs/FORMAT.md once M2 lands):
//
//   dbx/v1/devices.json               every device, read-merge-write
//   dbx/v1/dev/<device>/head.json     that device's latest seq, one writer
//   dbx/v1/dev/<device>/log/<seq>.json  an immutable batch of ops
//   dbx/v1/dev/<device>/ckpt/<n>.json   a snapshot of the whole index
//   dbx/v1/obj/<blob>/<i>             content chunk i, immutable
//   dbx/v1/obj/<blob>/manifest.json   size, chunk hashes, type; written last
//   dbx/v1/obj/<blob>/thumb           thumbnail or video poster

use std::fmt;

/// Everything drawBridge writes lives under this prefix, so the store can be
/// shared with other clients and the format can move to `v2` side by side.
pub const ROOT: &str = "dbx/v1";

/// Our own cap, far below swiftSearch's 64 KiB key limit and its 8 KiB request
/// head. The longest key the layout makes is well under 200 bytes.
const MAX_LEN: usize = 1024;

/// A key that is valid for swiftSearch as it stands, with no encoding.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Key(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyError(String);

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bad store key: {}", self.0)
    }
}

impl std::error::Error for KeyError {}

impl Key {
    /// Segments separated by `/`, each made of ASCII letters, digits, `-`, `_`
    /// and `.`, and none of them empty, `.` or `..`.
    pub fn new(s: impl Into<String>) -> Result<Key, KeyError> {
        let s = s.into();
        if s.is_empty() {
            return Err(KeyError("empty".into()));
        }
        if s.len() > MAX_LEN {
            return Err(KeyError(format!("{} bytes, over {MAX_LEN}", s.len())));
        }
        for seg in s.split('/') {
            if seg.is_empty() {
                return Err(KeyError(format!("empty segment in {s:?}")));
            }
            if seg == "." || seg == ".." {
                return Err(KeyError(format!("`{seg}` segment in {s:?}")));
            }
            if let Some(c) = seg
                .chars()
                .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
            {
                return Err(KeyError(format!("character {c:?} in {s:?}")));
            }
        }
        Ok(Key(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key({:?})", self.0)
    }
}

// Device and blob ids are UUIDs today, but they arrive here as plain strings
// (read back from the store, typed into the CLI), so each one is checked as a
// single segment before it is spliced into a path.
fn id<'a>(kind: &str, id: &'a str) -> Result<&'a str, KeyError> {
    if id.contains('/') {
        return Err(KeyError(format!("{kind} id {id:?} contains `/`")));
    }
    Key::new(id)?;
    Ok(id)
}

pub fn devices() -> Key {
    Key(format!("{ROOT}/devices.json"))
}

pub fn head(device: &str) -> Result<Key, KeyError> {
    Ok(Key(format!("{ROOT}/dev/{}/head.json", id("device", device)?)))
}

pub fn log(device: &str, seq: u64) -> Result<Key, KeyError> {
    Ok(Key(format!("{ROOT}/dev/{}/log/{seq}.json", id("device", device)?)))
}

pub fn checkpoint(device: &str, n: u64) -> Result<Key, KeyError> {
    Ok(Key(format!("{ROOT}/dev/{}/ckpt/{n}.json", id("device", device)?)))
}

pub fn chunk(blob: &str, index: u32) -> Result<Key, KeyError> {
    Ok(Key(format!("{ROOT}/obj/{}/{index}", id("blob", blob)?)))
}

pub fn manifest(blob: &str) -> Result<Key, KeyError> {
    Ok(Key(format!("{ROOT}/obj/{}/manifest.json", id("blob", blob)?)))
}

pub fn thumb(blob: &str) -> Result<Key, KeyError> {
    Ok(Key(format!("{ROOT}/obj/{}/thumb", id("blob", blob)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "0192f0a4-7c1e-7b3a-9f00-5d2c8e1a4b6f";

    #[test]
    fn accepts_plain_paths() {
        for ok in ["a", "a/b", "dbx/v1/obj/x/0", "notes.v2/hello_world-1.json", "a/.hidden"] {
            assert!(Key::new(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn refuses_what_swiftsearch_would() {
        for bad in [
            "", "/a", "a/", "a//b", ".", "..", "a/./b", "a/../b", "a b", "a%2Fb", "a\u{7f}",
            "a\nb", "é", "a?b", "a#b", "a+b",
        ] {
            assert!(Key::new(bad).is_err(), "{bad:?}");
        }
        assert!(Key::new("a".repeat(MAX_LEN + 1)).is_err());
        assert!(Key::new("a".repeat(MAX_LEN)).is_ok());
    }

    #[test]
    fn layout() {
        assert_eq!(devices().as_str(), "dbx/v1/devices.json");
        assert_eq!(head(UUID).unwrap().as_str(), format!("dbx/v1/dev/{UUID}/head.json"));
        assert_eq!(log(UUID, 42).unwrap().as_str(), format!("dbx/v1/dev/{UUID}/log/42.json"));
        assert_eq!(checkpoint(UUID, 3).unwrap().as_str(), format!("dbx/v1/dev/{UUID}/ckpt/3.json"));
        assert_eq!(chunk(UUID, 0).unwrap().as_str(), format!("dbx/v1/obj/{UUID}/0"));
        assert_eq!(manifest(UUID).unwrap().as_str(), format!("dbx/v1/obj/{UUID}/manifest.json"));
        assert_eq!(thumb(UUID).unwrap().as_str(), format!("dbx/v1/obj/{UUID}/thumb"));
    }

    #[test]
    fn ids_are_one_segment() {
        assert!(chunk("a/b", 0).is_err());
        assert!(chunk("..", 0).is_err());
        assert!(head("").is_err());
        assert!(log("x y", 1).is_err());
    }
}
