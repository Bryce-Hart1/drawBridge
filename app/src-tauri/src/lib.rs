// Claude — Date 11/03/2026
// The drawBridge desktop app: one window on the system webview, and
// drawbridge-core underneath. The UI (app/src) calls the commands in
// commands.rs and listens for four events:
//   index-changed  the tree changed (here, or pulled from another device)
//   sync-status    syncing, synced, offline or failed
//   transfer       an upload or download moved along
//   originals      uploaded originals went to the Trash, or were kept (originals.rs)
// Thumbnails load over the `thumb://` protocol (thumbs.rs). Photos, audio,
// video and PDFs load from core's stream server, on loopback, started when
// something is first opened and stopped when nothing has been for a while.

mod commands;
mod config;
mod originals;
mod state;
mod sync_loop;
mod thumbs;
mod transfers;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};

use crate::state::App;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("thumb", thumbs::protocol)
        .setup(|app| {
            let (state, kicks) = App::open(app.handle().clone())?;
            let state = Arc::new(state);
            sync_loop::spawn(state.clone(), kicks);
            transfers::spawn(state.clone());
            state.syncer.now();
            state.resume_uploads();
            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Focused(true) = event
                && let Some(app) = window.try_state::<Arc<App>>()
            {
                app.syncer.focus();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::connect,
            commands::disconnect,
            commands::list,
            commands::search,
            commands::mkdir,
            commands::rename,
            commands::move_to,
            commands::remove,
            commands::upload,
            commands::download,
            commands::cancel,
            commands::transfers,
            commands::note_read,
            commands::note_save,
            commands::stream_url,
            commands::open_external,
            commands::cache_info,
            commands::cache_set_cap,
            commands::cache_clear,
            commands::thumb_save,
            commands::export_file,
            commands::sync_now,
        ])
        .run(tauri::generate_context!())
        .expect("drawBridge failed to start");
}
