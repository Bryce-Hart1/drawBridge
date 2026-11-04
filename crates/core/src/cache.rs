// Claude — Date 11/04/2026
// The chunk cache: whole chunks of blobs kept on local disk, so replaying a
// song or seeking back in a video doesn't fetch the same bytes from the store
// again. One file per chunk at `<dir>/<blob>/<i>`.
//
// Only chunks that have passed their manifest sha256 (`blob::chunk`) are
// written, and blobs never change, so a cached chunk is good until it is
// evicted. The cache is capped in bytes; past the cap, the chunks used longest
// ago are deleted until it's back under 90% of it. When a chunk was last used
// lives in memory and in the file's mtime, which is how the order survives a
// restart.
//
// Callers that want the same chunk at the same time share one fetch: the
// first one fetches, the rest wait for it and then read the file.

use std::collections::HashMap;
use std::io::{self, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::Semaphore;

use crate::blob::{self, Manifest};
use crate::error::Error;
use crate::keys;
use crate::remote::Remote;

/// Background read-ahead fetches in flight at once, across every blob.
const PREFETCH: usize = 2;

type Slot = (String, u32);

pub struct Cache {
    dir: PathBuf,
    state: Mutex<State>,
    /// One lock per chunk being fetched; a second caller waits on it.
    fetching: Mutex<HashMap<Slot, Arc<tokio::sync::Mutex<()>>>>,
    prefetch: Arc<Semaphore>,
}

struct State {
    cap: u64,
    total: u64,
    chunks: HashMap<Slot, Chunk>,
}

#[derive(Clone, Copy)]
struct Chunk {
    len: u64,
    /// Milliseconds since the epoch.
    used: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Usage {
    pub bytes: u64,
    pub chunks: usize,
}

impl Cache {
    /// The cache in `dir`, made if missing. What's already there is counted
    /// (and trimmed to `cap`); half-written files from a crash are removed.
    pub fn open(dir: &Path, cap: u64) -> Result<Arc<Cache>, Error> {
        std::fs::create_dir_all(dir)?;
        let mut chunks = HashMap::new();
        let mut total = 0;
        for blob_dir in std::fs::read_dir(dir)? {
            let blob_dir = blob_dir?;
            let blob = blob_dir.file_name().to_string_lossy().into_owned();
            if !blob_dir.file_type()?.is_dir() || keys::manifest(&blob).is_err() {
                continue;
            }
            for file in std::fs::read_dir(blob_dir.path())? {
                let file = file?;
                let name = file.file_name().to_string_lossy().into_owned();
                let Ok(index) = name.parse::<u32>() else {
                    // A temp file left by a crash mid-write.
                    let _ = std::fs::remove_file(file.path());
                    continue;
                };
                let meta = file.metadata()?;
                let used = meta.modified().map(millis).unwrap_or(0);
                chunks.insert((blob.clone(), index), Chunk { len: meta.len(), used });
                total += meta.len();
            }
        }
        let cache = Cache {
            dir: dir.to_owned(),
            state: Mutex::new(State { cap, total, chunks }),
            fetching: Mutex::default(),
            prefetch: Arc::new(Semaphore::new(PREFETCH)),
        };
        cache.evict(&mut cache.state());
        Ok(Arc::new(cache))
    }

    pub fn usage(&self) -> Usage {
        let s = self.state();
        Usage { bytes: s.total, chunks: s.chunks.len() }
    }

    pub fn cap(&self) -> u64 {
        self.state().cap
    }

    pub fn set_cap(&self, cap: u64) {
        let mut s = self.state();
        s.cap = cap;
        self.evict(&mut s);
    }

    /// Delete everything cached. Fetches in flight still finish and are kept.
    pub fn clear(&self) -> Result<(), Error> {
        let mut s = self.state();
        for (blob, index) in s.chunks.keys() {
            let _ = std::fs::remove_file(self.path(blob, *index));
        }
        s.chunks.clear();
        s.total = 0;
        drop(s);
        for entry in std::fs::read_dir(&self.dir)? {
            let _ = std::fs::remove_dir_all(entry?.path());
        }
        Ok(())
    }

    pub fn has(&self, blob: &str, index: u32) -> bool {
        self.state().chunks.contains_key(&(blob.to_owned(), index))
    }

    /// Bytes `start..=end` of chunk `index` of `m`, from the cache, or `None`
    /// if that chunk isn't cached.
    pub async fn read(&self, m: &Manifest, index: u32, start: u64, end: u64) -> Option<Bytes> {
        let len = self.touch(&m.blob, index)?;
        if end >= len || start > end {
            return None;
        }
        match read_at(&self.path(&m.blob, index), start, end - start + 1).await {
            Ok(bytes) => Some(bytes),
            Err(_) => {
                // Deleted or cut short behind our back: forget it.
                self.forget(&m.blob, index);
                None
            }
        }
    }

    /// Chunk `index` of `m`, whole: from the cache, or fetched, checked
    /// against its sha256 and cached.
    pub async fn get(&self, remote: &Remote, m: &Manifest, index: u32) -> Result<Bytes, Error> {
        let len = m.chunk_len(index);
        if let Some(bytes) = self.read(m, index, 0, len - 1).await {
            return Ok(bytes);
        }
        let slot = (m.blob.clone(), index);
        let lock = self.fetching().entry(slot.clone()).or_default().clone();
        let result = async {
            let _turn = lock.lock().await;
            // Someone else may have fetched it while this waited.
            if let Some(bytes) = self.read(m, index, 0, len - 1).await {
                return Ok(bytes);
            }
            let bytes = blob::chunk(remote, m, index).await?;
            self.store(&m.blob, index, &bytes).await?;
            Ok(bytes)
        }
        .await;
        let mut fetching = self.fetching();
        // The map's copy and this one: nobody else is waiting.
        if Arc::strong_count(&lock) <= 2 {
            fetching.remove(&slot);
        }
        result
    }

    /// Fetch chunk `index` of `m` in the background, unless it's cached, past
    /// the end, or the read-ahead fetches are all busy. Errors are dropped:
    /// whoever reads the chunk later fetches it again and sees them.
    pub fn prefetch(self: &Arc<Self>, remote: Arc<Remote>, m: Arc<Manifest>, index: u32) {
        if index as usize >= m.chunks.len() || self.has(&m.blob, index) {
            return;
        }
        let Ok(permit) = self.prefetch.clone().try_acquire_owned() else {
            return;
        };
        let cache = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = cache.get(&remote, &m, index).await;
        });
    }

    async fn store(&self, blob: &str, index: u32, bytes: &[u8]) -> Result<(), Error> {
        // Also what keeps `blob` safe as a folder name: one key segment.
        keys::chunk(blob, index)?;
        let path = self.path(blob, index);
        let dir = path.parent().expect("a chunk path has a folder");
        tokio::fs::create_dir_all(dir).await?;
        let tmp = dir.join(format!(".{index}.{}", uuid::Uuid::now_v7()));
        tokio::fs::write(&tmp, bytes).await?;
        if let Err(e) = tokio::fs::rename(&tmp, &path).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(e.into());
        }
        let mut s = self.state();
        let len = bytes.len() as u64;
        let old = s.chunks.insert((blob.to_owned(), index), Chunk { len, used: now() });
        s.total = s.total - old.map_or(0, |c| c.len) + len;
        self.evict(&mut s);
        Ok(())
    }

    /// Mark a chunk used now. Its length, if it's cached.
    fn touch(&self, blob: &str, index: u32) -> Option<u64> {
        let len = {
            let mut s = self.state();
            let chunk = s.chunks.get_mut(&(blob.to_owned(), index))?;
            chunk.used = now();
            chunk.len
        };
        // Best effort: it only matters for the order after a restart.
        if let Ok(file) = std::fs::File::options().write(true).open(self.path(blob, index)) {
            let _ = file.set_modified(SystemTime::now());
        }
        Some(len)
    }

    fn forget(&self, blob: &str, index: u32) {
        let mut s = self.state();
        if let Some(c) = s.chunks.remove(&(blob.to_owned(), index)) {
            s.total -= c.len;
        }
    }

    /// Down to 90% of the cap, oldest first, once over it.
    fn evict(&self, s: &mut State) {
        if s.total <= s.cap {
            return;
        }
        let goal = s.cap / 10 * 9;
        let mut by_age: Vec<(Slot, Chunk)> = s.chunks.iter().map(|(k, c)| (k.clone(), *c)).collect();
        by_age.sort_by_key(|(_, c)| c.used);
        for ((blob, index), c) in by_age {
            if s.total <= goal {
                break;
            }
            let path = self.path(&blob, index);
            let _ = std::fs::remove_file(&path);
            // Gone only once its last chunk is.
            let _ = std::fs::remove_dir(path.parent().expect("a chunk path has a folder"));
            s.chunks.remove(&(blob, index));
            s.total -= c.len;
        }
    }

    fn path(&self, blob: &str, index: u32) -> PathBuf {
        self.dir.join(blob).join(index.to_string())
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fetching(&self) -> MutexGuard<'_, HashMap<Slot, Arc<tokio::sync::Mutex<()>>>> {
        self.fetching.lock().unwrap_or_else(|e| e.into_inner())
    }
}

async fn read_at(path: &Path, start: u64, len: u64) -> io::Result<Bytes> {
    let mut file = tokio::fs::File::open(path).await?;
    file.seek(SeekFrom::Start(start)).await?;
    let mut buf = vec![0; len as usize];
    file.read_exact(&mut buf).await?;
    Ok(buf.into())
}

fn millis(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn now() -> u64 {
    millis(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::Kind;

    const A: &str = "0192f0a4-7c1e-7b3a-9f00-5d2c8e1a4b6f";
    const B: &str = "0192f0a4-7c1e-7b3a-9f00-5d2c8e1a4b70";

    fn manifest(blob: &str, size: u64, chunk_size: u32) -> Manifest {
        Manifest {
            v: 1,
            blob: blob.into(),
            size,
            chunk_size,
            chunks: vec!["0".repeat(64); size.div_ceil(chunk_size as u64) as usize],
            sha256: "0".repeat(64),
            mime: "video/mp4".into(),
            kind: Kind::Video,
            enc: "none".into(),
        }
    }

    async fn put(c: &Cache, blob: &str, index: u32, len: usize) {
        c.store(blob, index, &vec![index as u8; len]).await.unwrap();
        // Distinct `used` times, so the eviction order is the order stored.
        std::thread::sleep(std::time::Duration::from_millis(3));
    }

    #[tokio::test]
    async fn reads_back_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::open(dir.path(), 1 << 20).unwrap();
        let m = manifest(A, 250, 100);
        c.store(A, 1, &(0..100).collect::<Vec<u8>>()).await.unwrap();
        assert_eq!(c.read(&m, 1, 10, 19).await.unwrap(), Bytes::from((10..20).collect::<Vec<u8>>()));
        assert_eq!(c.read(&m, 1, 0, 99).await.unwrap().len(), 100);
        assert!(c.read(&m, 1, 0, 100).await.is_none(), "past the chunk");
        assert!(c.read(&m, 0, 0, 9).await.is_none(), "not cached");
        assert_eq!(c.usage(), Usage { bytes: 100, chunks: 1 });
    }

    #[tokio::test]
    async fn evicts_least_recently_used() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::open(dir.path(), 1000).unwrap();
        let m = manifest(A, 10_000, 300);
        put(&c, A, 0, 300).await;
        put(&c, A, 1, 300).await;
        put(&c, A, 2, 300).await;
        // Chunk 0 is used again, so 1 is now the oldest.
        assert!(c.read(&m, 0, 0, 0).await.is_some());
        std::thread::sleep(std::time::Duration::from_millis(3));
        // 1200 > 1000: down to 900, which drops chunk 1 only.
        put(&c, A, 3, 300).await;
        assert_eq!(c.usage(), Usage { bytes: 900, chunks: 3 });
        assert!(c.has(A, 0) && !c.has(A, 1) && c.has(A, 2) && c.has(A, 3));
        assert!(!dir.path().join(A).join("1").exists());
    }

    #[tokio::test]
    async fn lowering_the_cap_trims_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::open(dir.path(), 10_000).unwrap();
        for i in 0..5 {
            put(&c, A, i, 100).await;
        }
        put(&c, B, 0, 100).await;
        c.set_cap(300);
        // Down to 270: the two newest stay.
        assert_eq!(c.usage(), Usage { bytes: 200, chunks: 2 });
        assert!(c.has(A, 4) && c.has(B, 0));
        assert!(!dir.path().join(A).join("0").exists());
        assert_eq!(c.cap(), 300);
    }

    #[tokio::test]
    async fn reopening_counts_what_is_there() {
        let dir = tempfile::tempdir().unwrap();
        {
            let c = Cache::open(dir.path(), 10_000).unwrap();
            put(&c, A, 0, 100).await;
            put(&c, A, 1, 50).await;
            put(&c, B, 0, 100).await;
        }
        std::fs::write(dir.path().join(A).join(".2.tmp"), b"half").unwrap();
        std::fs::create_dir(dir.path().join("not a blob")).unwrap();
        let c = Cache::open(dir.path(), 10_000).unwrap();
        assert_eq!(c.usage(), Usage { bytes: 250, chunks: 3 });
        assert!(!dir.path().join(A).join(".2.tmp").exists());
        // Reopening under a smaller cap keeps the newest.
        drop(c);
        let c = Cache::open(dir.path(), 120).unwrap();
        assert!(c.has(B, 0) && !c.has(A, 0) && !c.has(A, 1));
    }

    #[tokio::test]
    async fn clear_empties_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::open(dir.path(), 10_000).unwrap();
        put(&c, A, 0, 100).await;
        put(&c, B, 3, 100).await;
        c.clear().unwrap();
        assert_eq!(c.usage(), Usage { bytes: 0, chunks: 0 });
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn a_file_removed_behind_its_back_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::open(dir.path(), 10_000).unwrap();
        let m = manifest(A, 100, 100);
        put(&c, A, 0, 100).await;
        std::fs::remove_file(dir.path().join(A).join("0")).unwrap();
        assert!(c.read(&m, 0, 0, 9).await.is_none());
        assert_eq!(c.usage(), Usage { bytes: 0, chunks: 0 });
    }

    #[tokio::test]
    async fn refuses_a_blob_id_that_is_not_one_segment() {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::open(dir.path(), 10_000).unwrap();
        assert!(c.store("../escape", 0, b"x").await.is_err());
        assert!(!dir.path().parent().unwrap().join("escape").exists());
    }
}
