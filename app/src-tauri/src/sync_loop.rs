// Claude — Date 11/03/2026
// When the app syncs. One task makes every sync, so they never overlap (M2's
// sync assumes one at a time per Db). It wakes up:
// - at launch, and when the user asks (`Kick::Now`);
// - when the window comes to the front, unless it synced in the last 10 s;
// - half a second after a change made here, so a burst of changes goes in one
//   push;
// - every 60 s, but only while the window is on screen. Hidden, the app makes
//   no requests at all.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;

use crate::state::App;

const EVERY: Duration = Duration::from_secs(60);
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

pub fn spawn(app: Arc<App>, mut kicks: mpsc::UnboundedReceiver<Kick>) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(EVERY);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.tick().await; // the first tick is immediate; launch sends its own kick
        let mut last: Option<Instant> = None;
        loop {
            tokio::select! {
                kick = kicks.recv() => match kick {
                    None => return,
                    Some(Kick::Soon) => tokio::time::sleep(SETTLE).await,
                    Some(Kick::Focus) if last.is_some_and(|t| t.elapsed() < FOCUS_GAP) => continue,
                    Some(_) => {}
                },
                _ = tick.tick() => if !app.visible() { continue },
            }
            // Whatever else asked meanwhile is covered by this sync.
            while kicks.try_recv().is_ok() {}
            app.run_sync().await;
            last = Some(Instant::now());
            tick.reset();
        }
    });
}
