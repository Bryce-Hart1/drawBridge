// Claude — Date 11/03/2026
// When the app syncs. One task makes every sync, so they never overlap (M2's
// sync assumes one at a time per Db). It wakes up:
// - at launch, and when the user asks (`Kick::Now`);
// - when the window comes to the front, unless it synced in the last 10 s;
// - half a second after a change made here, so a burst of changes goes in one
//   push;
// - every 60 s, but only while the window is on screen. Hidden, the app makes
//   no requests at all.
// - sooner while the store can't be reached and the window is on screen: after
//   5 s, 10 s, 20 s and 40 s, then back to every 60 s. The first sync that gets
//   through starts the waiting uploads again (`App::run_sync`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::state::App;

const EVERY: Duration = Duration::from_secs(60);
/// How long to wait before each retry while offline, then `EVERY` after these.
const OFFLINE: [Duration; 4] =
    [Duration::from_secs(5), Duration::from_secs(10), Duration::from_secs(20), Duration::from_secs(40)];
const SETTLE: Duration = Duration::from_millis(500);
const FOCUS_GAP: Duration = Duration::from_secs(10);

pub enum Kick {
    Now,
    Soon,
    Focus,
}

#[derive(Clone)]
pub struct Syncer {
    tx: mpsc::UnboundedSender<Kick>,
}

impl Syncer {
    pub fn new() -> (Syncer, mpsc::UnboundedReceiver<Kick>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Syncer { tx }, rx)
    }

    pub fn now(&self) {
        let _ = self.tx.send(Kick::Now);
    }

    pub fn soon(&self) {
        let _ = self.tx.send(Kick::Soon);
    }

    pub fn focus(&self) {
        let _ = self.tx.send(Kick::Focus);
    }
}

/// How long until the next timed sync, given whether the last one found the
/// store unreachable and how many in a row have.
fn next_wait(offline: bool, misses: usize) -> (Duration, usize) {
    if !offline {
        return (EVERY, 0);
    }
    (OFFLINE.get(misses).copied().unwrap_or(EVERY), misses + 1)
}

pub fn spawn(app: Arc<App>, mut kicks: mpsc::UnboundedReceiver<Kick>) {
    tauri::async_runtime::spawn(async move {
        let mut last: Option<Instant> = None;
        let mut wait = EVERY;
        let mut misses = 0;
        loop {
            // Launch sends its own kick, so nothing waits on an immediate timer.
            let timer = tokio::time::sleep(wait);
            tokio::select! {
                kick = kicks.recv() => match kick {
                    None => return,
                    Some(Kick::Soon) => tokio::time::sleep(SETTLE).await,
                    Some(Kick::Focus) if last.is_some_and(|t| t.elapsed() < FOCUS_GAP) => continue,
                    Some(_) => {}
                },
                () = timer => if !app.visible() { continue },
            }
            // Whatever else asked meanwhile is covered by this sync.
            while kicks.try_recv().is_ok() {}
            app.run_sync().await;
            last = Some(Instant::now());
            (wait, misses) = next_wait(app.offline(), misses);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backs_off_while_offline() {
        let mut misses = 0;
        let mut waits = Vec::new();
        for _ in 0..6 {
            let (wait, m) = next_wait(true, misses);
            waits.push(wait.as_secs());
            misses = m;
        }
        assert_eq!(waits, [5, 10, 20, 40, 60, 60]);
        assert_eq!(next_wait(false, misses), (EVERY, 0), "back online: the usual minute");
    }
}
