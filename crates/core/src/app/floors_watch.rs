//! The watcher of the floor folders. A floor can be deleted or moved while the app runs. The
//! watcher checks each open floor every few seconds (`TowerConfig::floor_watch`), and tells the
//! observer one time when a floor becomes gone, and one time when it is back.
//!
//! The rule for "gone" is `store::floor_is_gone`. Live sessions of a gone floor keep running:
//! the app does not kill their processes here.

use super::live::locked;
use super::{FloorHandle, TowerCore};
use crate::store::floor_is_gone;
use std::io;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};

/// The watcher sleeps in steps of this size, so that `shutdown` does not wait for a whole pause.
const SLICE: Duration = Duration::from_millis(20);

/// Start the thread. It holds only a `Weak` handle, and it ends when the core is dropped or
/// closed.
pub(super) fn spawn(core: Weak<TowerCore>, pause: Duration) -> io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("app-floor-watch".into())
        .spawn(move || {
            loop {
                let deadline = Instant::now() + pause;
                while Instant::now() < deadline {
                    thread::sleep(SLICE.min(pause));
                    match core.upgrade() {
                        Some(core) if !core.closed.load(Ordering::SeqCst) => {}
                        _ => return,
                    }
                }
                match core.upgrade() {
                    Some(core) if !core.closed.load(Ordering::SeqCst) => core.check_floors(),
                    _ => return,
                }
            }
        })
}

impl TowerCore {
    /// Check each open floor one time. A floor that became gone: the observer gets `floor_changed`
    /// and the notice `floor_gone`. A floor that is back: the app opens its database again from
    /// the path, and the observer gets `floor_changed` and the notice `floor_back`. A floor that
    /// stays in its state gives no event, so a pause of some seconds does not repeat anything.
    ///
    /// The watcher thread calls this. A test that turned the thread off can call it by hand.
    pub fn check_floors(&self) {
        let floors: Vec<Arc<FloorHandle>> = locked(&self.floors).values().cloned().collect();
        for floor in floors {
            if self.closed.load(Ordering::SeqCst) {
                return;
            }
            let announced = floor.announced_gone.load(Ordering::SeqCst);
            if floor_is_gone(&floor.path) {
                // The database handle is not safe to use until it is opened again.
                floor.stale.store(true, Ordering::SeqCst);
                if !announced {
                    floor.announced_gone.store(true, Ordering::SeqCst);
                    self.notifier.floor_changed(&floor.path);
                    let text = self.gone_text(&floor);
                    self.notifier.notice(&floor.path, "floor_gone", &text);
                }
            } else if announced && self.check_alive(&floor).is_ok() {
                floor.announced_gone.store(false, Ordering::SeqCst);
                self.notifier.floor_changed(&floor.path);
                let text = format!(
                    "The folder of the floor {} is back: {}.",
                    floor_name(&floor),
                    floor.path.display()
                );
                self.notifier.notice(&floor.path, "floor_back", &text);
            }
            // A folder that is back but not readable yet (`check_alive` failed) stays announced
            // as gone. The next check tries again, and sends nothing until it works.
        }
    }

    fn gone_text(&self, floor: &FloorHandle) -> String {
        let live = locked(&self.registry)
            .live
            .values()
            .filter(|l| l.path == floor.path && !l.exited())
            .count();
        let mut text = format!(
            "The folder of the floor {} is gone: {}.",
            floor_name(floor),
            floor.path.display()
        );
        match live {
            0 => {}
            1 => text.push_str(" 1 session of it still runs."),
            n => text.push_str(&format!(" {n} sessions of it still run.")),
        }
        text
    }
}

/// The name of the floor from its last known view, or the name of its folder.
fn floor_name(floor: &FloorHandle) -> String {
    if let Some(view) = locked(&floor.last_view).as_ref() {
        return view.name.clone();
    }
    floor.path.file_name().map_or_else(
        || floor.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}
