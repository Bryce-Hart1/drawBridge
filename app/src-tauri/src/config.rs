// Claude — Date 11/03/2026
// What the app remembers between launches besides the index: which store it
// talks to, what this device is called and how big the chunk cache may grow
// (`config.json` in the app's data folder), and the store's access key.
//
// Release builds keep the key in the macOS Keychain. Debug builds keep it in a
// file beside the config (readable by this user only) instead: each rebuild is
// a new unsigned binary, and the Keychain would ask for permission again after
// every one.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// The store, as `Remote::base` normalizes it.
    pub url: String,
    pub device_name: String,
    /// The chunk cache's cap in bytes; the default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_cap: Option<u64>,
}

const FILE: &str = "config.json";

pub fn load(dir: &Path) -> Option<Config> {
    serde_json::from_slice(&fs::read(dir.join(FILE)).ok()?).ok()
}

pub fn save(dir: &Path, config: &Config) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(config).expect("a config always serializes");
    write_atomic(&dir.join(FILE), &json)
}

/// Write to a temporary name, then rename over the old file, so a crash leaves
/// one whole version or the other.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

/// This computer's name, as the setup screen suggests for the device:
/// "Bryce's MacBook Pro" on macOS, the host name elsewhere.
pub fn computer_name() -> String {
    let run = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    };
    #[cfg(target_os = "macos")]
    if let Some(name) = run("scutil", &["--get", "ComputerName"]) {
        return name;
    }
    run("hostname", &[]).map(|h| h.trim_end_matches(".local").to_owned()).unwrap_or_else(|| "this computer".into())
}

pub use key_store::{forget_key, load_key, save_key};

#[cfg(not(debug_assertions))]
mod key_store {
    use std::path::Path;

    use keyring::Entry;

    // One Keychain item per store URL, under this service name.
    const SERVICE: &str = "drawBridge";

    pub fn load_key(_dir: &Path, url: &str) -> Option<String> {
        Entry::new(SERVICE, url).ok()?.get_password().ok()
    }

    pub fn save_key(_dir: &Path, url: &str, key: &str) -> Result<(), String> {
        let entry = Entry::new(SERVICE, url).map_err(|e| e.to_string())?;
        entry.set_password(key).map_err(|e| e.to_string())
    }

    pub fn forget_key(_dir: &Path, url: &str) {
        if let Ok(entry) = Entry::new(SERVICE, url) {
            let _ = entry.delete_credential();
        }
    }
}

#[cfg(debug_assertions)]
mod key_store {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    const FILE: &str = "dev-keys.json";

    fn read(dir: &Path) -> BTreeMap<String, String> {
        fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn write(dir: &Path, keys: &BTreeMap<String, String>) -> Result<(), String> {
        let path = dir.join(FILE);
        let json = serde_json::to_vec(keys).expect("a map of strings serializes");
        super::write_atomic(&path, &json).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn load_key(dir: &Path, url: &str) -> Option<String> {
        read(dir).remove(url)
    }

    pub fn save_key(dir: &Path, url: &str, key: &str) -> Result<(), String> {
        let mut keys = read(dir);
        keys.insert(url.to_owned(), key.to_owned());
        write(dir, &keys)
    }

    pub fn forget_key(dir: &Path, url: &str) {
        let mut keys = read(dir);
        if keys.remove(url).is_some() {
            let _ = write(dir, &keys);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_and_keys_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_none());
        let c = Config { url: "http://127.0.0.1:8080".into(), device_name: "laptop".into(), cache_cap: Some(1 << 30) };
        save(dir.path(), &c).unwrap();
        assert_eq!(load(dir.path()), Some(c.clone()));

        // Debug builds: the file store. (Release builds use the Keychain, which
        // tests leave alone.)
        assert!(load_key(dir.path(), &c.url).is_none());
        save_key(dir.path(), &c.url, "ssk_test").unwrap();
        assert_eq!(load_key(dir.path(), &c.url).as_deref(), Some("ssk_test"));
        assert!(load_key(dir.path(), "https://other.example").is_none());
        forget_key(dir.path(), &c.url);
        assert!(load_key(dir.path(), &c.url).is_none());
    }
}
