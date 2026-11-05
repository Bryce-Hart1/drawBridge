// Claude — Date 11/03/2026
// Integration harness: a real swiftSearch in a throwaway store, with one write
// key and one read key, killed when the test ends.
//
// The binary comes from SWIFTSEARCH_BIN, or the build folder of the checkout
// in ~/Developer/swiftSearch (or the older sibling checkout). Build it first
// (`cmake -B build && cmake --build build -j` there).
//
// The port is picked here rather than with `--port 0`: swiftSearch prints the
// port it chose in its banner, but stdout is block-buffered when it isn't a
// terminal, so the banner only appears once the process exits.

#![allow(dead_code)]

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use drawbridge_core::Remote;
use tempfile::TempDir;

pub struct Server {
    child: Child,
    pub base: String,
    pub write_key: String,
    pub read_key: String,
    _dir: TempDir,
}

impl Server {
    pub fn start() -> Server {
        let bin = binary();
        let dir = tempfile::tempdir().expect("temp dir");
        let store = dir.path().join("store");
        let write_key = key_add(&bin, dir.path(), &store, "write");
        let read_key = key_add(&bin, dir.path(), &store, "read");

        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("free port")
            .port();
        let child = Command::new(&bin)
            .arg("serve")
            .arg("--store")
            .arg(&store)
            .args(["--port", &port.to_string(), "-j", "2"])
            .current_dir(dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start swiftSearch");

        let deadline = Instant::now() + Duration::from_secs(10);
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "swiftSearch did not start listening on {port}");
            std::thread::sleep(Duration::from_millis(20));
        }
        Server { child, base: format!("http://127.0.0.1:{port}"), write_key, read_key, _dir: dir }
    }

    pub fn writer(&self) -> Remote {
        Remote::new(&self.base, &self.write_key).expect("write client")
    }

    pub fn reader(&self) -> Remote {
        Remote::new(&self.base, &self.read_key).expect("read client")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn binary() -> PathBuf {
    // SWIFTSEARCH_BIN, else the first build found: ~/Developer/swiftSearch,
    // where the checkout lives now, then the old sibling checkout.
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let candidates = [
        home.join("Developer/swiftSearch/build/bin/swiftSearch"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../multithread_Search/swiftSearch/build/bin/swiftSearch"),
    ];
    let bin = std::env::var_os("SWIFTSEARCH_BIN")
        .map(PathBuf::from)
        .or_else(|| candidates.iter().find(|p| p.is_file()).cloned())
        .unwrap_or_else(|| candidates[0].clone());
    assert!(
        bin.is_file(),
        "no swiftSearch binary at {}: build it, or set SWIFTSEARCH_BIN",
        bin.display()
    );
    bin
}

// Every swiftSearch command, `key` included, writes a swiftSearchLogs file into
// its working directory, so each one runs in the temp dir.
fn key_add(bin: &Path, workdir: &Path, store: &Path, access: &str) -> String {
    let out = Command::new(bin)
        .current_dir(workdir)
        .args(["key", "add", "--store"])
        .arg(store)
        .args(["--access", access, "--name", access])
        .output()
        .expect("swiftSearch key add");
    assert!(out.status.success(), "key add failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).expect("utf-8 key").trim().to_owned()
}

/// Deterministic, incompressible-looking bytes (xorshift), so a round trip that
/// drops or reorders anything can't compare equal by accident.
pub fn bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}
