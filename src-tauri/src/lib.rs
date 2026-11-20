//! The Tauri shell of Agent Tower. It holds no logic: `tower_core::app::Tower` does the work.
//! This crate turns Tauri commands into Tower calls (`commands`), Tower changes into Tauri
//! events (`TauriObserver`), and the types into the JSON of `docs/ipc-m1.md` (`views`).

mod commands;
mod signals;
mod views;

use commands::{Shared, WriteQueues};
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime};
use tower_core::app::{Observer, SessionView, Tower, TowerConfig};

/// Sends the changes of the Tower to all windows as Tauri events.
struct TauriObserver<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> Observer for TauriObserver<R> {
    fn session_changed(&self, path: &Path, session: &SessionView) {
        let payload = views::SessionChanged {
            path: path.to_string_lossy().into_owned(),
            session: session.into(),
        };
        let _ = self.app.emit("session-changed", payload);
    }

    fn floor_changed(&self, path: &Path) {
        let payload = views::FloorChanged {
            path: path.to_string_lossy().into_owned(),
        };
        let _ = self.app.emit("floor-changed", payload);
    }
}

/// Register the commands of `docs/ipc-m1.md`, `docs/ipc-visuals-init.md` and `docs/ipc-settings.md` on a builder. Public for the tests of the shell.
#[doc(hidden)]
pub fn register_commands<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        commands::floor_list,
        commands::floor_open,
        commands::floor_create,
        commands::floor_forget,
        commands::employee_save,
        commands::employee_remove,
        commands::session_start,
        commands::session_resume,
        commands::session_stop,
        commands::session_attach,
        commands::pty_write,
        commands::pty_resize,
        commands::clear_draft,
        commands::debug_paste,
        commands::programs_get,
        commands::programs_set,
        commands::floor_logo,
        commands::logo_generate,
        commands::setup_start,
        commands::settings_get,
        commands::settings_set,
        commands::models_get,
        commands::models_set_custom,
        commands::floor_defaults_set,
    ])
}

/// Put the Tower in the managed state, and send its changes as events.
#[doc(hidden)]
pub fn install_tower<R: Runtime>(app: &impl Manager<R>, tower: Arc<Tower>) {
    tower.set_observer(Arc::new(TauriObserver {
        app: app.app_handle().clone(),
    }));
    app.manage(tower);
    app.manage(WriteQueues::default());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = register_commands(tauri::Builder::default()).setup(|app| {
        let config_dir = app.path().app_config_dir()?;
        install_tower(app, Arc::new(Tower::new(TowerConfig::new(config_dir))?));
        signals::install(app.handle().clone())?;
        Ok(())
    });
    // A failure to build the app (no display, a bad config) is a failure of the process.
    let app = match builder.build(tauri::generate_context!()) {
        Ok(app) => app,
        Err(error) => {
            eprintln!("agent-tower: cannot start: {error}");
            std::process::exit(1);
        }
    };
    app.run(|handle, event| {
        // The last window closed, `AppHandle::exit`, or a signal: stop the sessions and remove
        // the socket before the process ends.
        if let RunEvent::Exit = event
            && let Some(tower) = handle.try_state::<Shared>()
        {
            tower.shutdown();
        }
    });
}
