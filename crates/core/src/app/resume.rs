//! The restore of the last team (M3, decision 10 of the milestone, 2026-11-21).
//!
//! The app stops all sessions when it closes. Before it does, `shutdown` stores the slugs of the
//! employees that had a live session (`Database::replace_resume_marks`). The operator stops a
//! session by hand with `session_stop`: that clears the mark of that employee. `FloorView`
//! lists the marked employees that have no live session now and that can resume
//! (`resumable_slugs`). The command `team_resume` resumes them one by one in a background
//! thread, with a pause between two starts. A failure of one start does not stop the others: it
//! goes to the operator as a notice (`resume_failed`). Nothing resumes by itself at the start of
//! the app, because each start costs tokens.

use super::{
    FloorHandle, Live, Result, SETUP_SLUG, SessionView, TowerCore, TowerError, locked, now_ms,
    other,
};
use crate::model::Employee;
use crate::status::Status;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

/// The pause between two starts of the resume.
pub const RESUME_PAUSE: Duration = Duration::from_secs(2);

/// The terminal size of a session that the resume starts. The UI resizes it when it attaches.
const RESUME_ROWS: u16 = 30;
const RESUME_COLS: u16 = 120;

/// How far the resume of the last team is. `done` counts the starts that were tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeProgress {
    pub done: usize,
    pub total: usize,
}

/// The depth of an employee in the `manager` chain. A loop in the chain stops the count.
fn depth(employee: &Employee, employees: &[Employee]) -> usize {
    let mut depth = 0;
    let mut current = employee;
    while let Some(manager) = current
        .manager
        .as_deref()
        .and_then(|slug| employees.iter().find(|e| e.slug == slug))
    {
        depth += 1;
        if depth > employees.len() {
            break;
        }
        current = manager;
    }
    depth
}

/// The marked employees that can be resumed now, managers first (the order of the resume). An
/// employee is in the list when it is in the team, it has no live session, and its last session
/// can resume (`SessionView::can_resume`).
pub(super) fn resumable_slugs(
    marks: &[String],
    employees: &[Employee],
    sessions: &[SessionView],
) -> Vec<String> {
    let mut chosen: Vec<&Employee> = employees
        .iter()
        .filter(|e| e.slug != SETUP_SLUG && marks.contains(&e.slug))
        .filter(|e| {
            sessions
                .iter()
                .any(|s| s.slug == e.slug && s.status == Status::Stopped && s.can_resume)
        })
        .collect();
    chosen.sort_by_key(|e| depth(e, employees));
    chosen.iter().map(|e| e.slug.clone()).collect()
}

/// Clears the progress of the resume when the thread ends, also after a panic.
struct ProgressGuard {
    core: Arc<TowerCore>,
    floor: Arc<FloorHandle>,
}

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        *locked(&self.floor.resume) = None;
        self.core.notifier.floor_changed(&self.floor.path);
    }
}

impl TowerCore {
    /// The floor handle if the floor is open. It never opens a floor.
    fn open_floor(&self, path: &Path) -> Option<Arc<FloorHandle>> {
        locked(&self.floors).get(path).cloned()
    }

    /// Store the employees that are live now, for each floor. It runs at the start of `shutdown`,
    /// before the sessions are killed. A floor with no live session keeps its old list: the
    /// operator did not choose to drop it.
    pub(super) fn mark_live_for_resume(&self, lives: &[Arc<Live>]) {
        let by_hand = locked(&self.registry).manual_stops.clone();
        let mut by_floor: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
        for live in lives {
            if !live.exited() && live.slug != SETUP_SLUG && !by_hand.contains(&live.id) {
                by_floor
                    .entry(live.path.clone())
                    .or_default()
                    .push(live.slug.clone());
            }
        }
        for (path, mut slugs) in by_floor {
            slugs.sort();
            let Some(floor) = self.open_floor(&path) else {
                continue;
            };
            let stored = locked(&floor.store)
                .db()
                .replace_resume_marks(&slugs, now_ms());
            if let Err(error) = stored {
                super::live::warn(&format!("cannot store the team to resume: {error}"));
            }
        }
    }

    /// The operator stopped the session of `slug` by hand.
    pub(super) fn clear_resume_mark(&self, path: &Path, slug: &str) {
        let Some(floor) = self.open_floor(path) else {
            return;
        };
        let cleared = locked(&floor.store).db().clear_resume_mark(slug);
        if let Err(error) = cleared {
            super::live::warn(&format!("cannot clear the resume mark of {slug}: {error}"));
        }
    }

    /// Resume the employees of `FloorView::resumable`, one at a time, in a background thread. It
    /// returns at once with the progress at the start. The observer gets `floor_changed` after
    /// each start, and `notice` with the kind `resume_failed` for a start that failed.
    pub fn team_resume(&self, path: &Path) -> Result<ResumeProgress> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TowerError::Closing);
        }
        let floor = self.floor_handle(path)?;
        let core = self
            .me
            .upgrade()
            .ok_or_else(|| other("the app is closing"))?;
        if locked(&floor.resume).is_some() {
            return Err(other("the last team is resuming now"));
        }
        // `floor_view` reads the progress too, so no lock is held here.
        let list = self.floor_view(&floor)?.resumable;
        if list.is_empty() {
            return Err(other("there is no session of the last team to resume"));
        }
        {
            let mut running = locked(&floor.resume);
            if running.is_some() {
                return Err(other("the last team is resuming now"));
            }
            *running = Some(ResumeProgress {
                done: 0,
                total: list.len(),
            });
        }
        let total = list.len();
        self.notifier.floor_changed(&floor.path);
        let guard = ProgressGuard {
            core: Arc::clone(&core),
            floor: Arc::clone(&floor),
        };
        let pause = self.resume_pause;
        let spawned = thread::Builder::new()
            .name("app-team-resume".into())
            .spawn(move || {
                let guard = guard;
                for (index, slug) in list.iter().enumerate() {
                    if index > 0 && !sleep_unless_closed(&guard.core, pause) {
                        break;
                    }
                    if guard.core.closed.load(Ordering::SeqCst) {
                        break;
                    }
                    let started = guard.core.session_resume(
                        &guard.floor.path,
                        slug,
                        None,
                        RESUME_ROWS,
                        RESUME_COLS,
                    );
                    if let Err(error) = started {
                        guard.core.notifier.notice(
                            &guard.floor.path,
                            "resume_failed",
                            &format!("Could not resume {slug}: {error}"),
                        );
                    }
                    *locked(&guard.floor.resume) = Some(ResumeProgress {
                        done: index + 1,
                        total,
                    });
                    guard.core.notifier.floor_changed(&guard.floor.path);
                }
            });
        if let Err(error) = spawned {
            // The closure was dropped, and so was its guard: the progress is cleared.
            return Err(other(format!("cannot start the resume thread: {error}")));
        }
        Ok(ResumeProgress { done: 0, total })
    }
}

/// Sleep for `pause`, in short steps. Returns `false` when the app closed in the meantime.
fn sleep_unless_closed(core: &TowerCore, pause: Duration) -> bool {
    let deadline = Instant::now() + pause;
    while Instant::now() < deadline {
        if core.closed.load(Ordering::SeqCst) {
            return false;
        }
        thread::sleep(Duration::from_millis(20).min(pause));
    }
    !core.closed.load(Ordering::SeqCst)
}
