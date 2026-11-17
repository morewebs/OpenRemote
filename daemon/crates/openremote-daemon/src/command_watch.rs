//! Hot reload for the / menu: the folders an App reads its custom commands,
//! skills, prompts and MCP settings from are watched, so a file added,
//! edited or removed there reaches the menu without restarting a chat or
//! the daemon.
//!
//! A folder that does not exist yet is watched through its nearest parent
//! that does (without recursing), so the first file in a new
//! `.claude/commands` counts too. Changes are gathered until things have
//! been quiet for a moment, then handed on once, with every path touched.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

/// How long the folders must be quiet before a burst of changes counts.
pub const QUIET: Duration = Duration::from_millis(300);

/// A workspace nobody has asked about for this long stops being watched.
pub const FORGET_AFTER: Duration = Duration::from_secs(30 * 60);

/// What a burst of changes is handed to: the paths it touched.
pub type OnChange = Box<dyn Fn(Vec<PathBuf>) + Send + Sync>;

/// The places under a workspace an App reads commands from.
pub fn workspace_interests(workspace: &Path) -> Vec<PathBuf> {
    [
        ".claude/commands",
        ".claude/skills",
        ".claude/agents",
        ".mcp.json",
        ".codex/skills",
        ".codex/prompts",
        ".agents/skills",
        ".opencode",
        "opencode.json",
        "opencode.jsonc",
        ".pi/prompts",
        ".pi/skills",
        ".pi/extensions",
        ".grok/commands",
        ".grok/skills",
        ".agents/workflows",
        ".agent/workflows",
        ".agent/skills",
    ]
    .iter()
    .map(|rel| workspace.join(rel))
    .collect()
}

/// The places under the user's home an App reads commands from.
pub fn home_interests(home: &Path) -> Vec<PathBuf> {
    [
        ".claude/commands",
        ".claude/skills",
        ".claude/agents",
        ".claude/plugins/installed_plugins.json",
        ".codex/prompts",
        ".codex/skills",
        ".codex/config.toml",
        ".agents/skills",
        ".config/opencode/command",
        ".config/opencode/commands",
        ".config/opencode/opencode.json",
        ".config/opencode/opencode.jsonc",
        ".pi/agent/prompts",
        ".pi/agent/skills",
        ".pi/agent/extensions",
        ".grok/commands",
        ".grok/skills",
        ".gemini/antigravity/global_workflows",
        ".gemini/antigravity/skills",
    ]
    .iter()
    .map(|rel| home.join(rel))
    .collect()
}

/// Where to watch so `interest` is seen, and whether recursively: a folder
/// that exists is watched with everything in it; a file, or anything not
/// there yet, through its nearest existing parent, alone. `None` when not
/// even `base` (the workspace or home) exists.
pub fn watch_point(interest: &Path, base: &Path) -> Option<(PathBuf, RecursiveMode)> {
    if interest.is_dir() {
        return Some((interest.to_path_buf(), RecursiveMode::Recursive));
    }
    let mut at = interest.parent()?;
    loop {
        if at.is_dir() {
            return Some((at.to_path_buf(), RecursiveMode::NonRecursive));
        }
        if at == base || !at.starts_with(base) {
            return None;
        }
        at = at.parent()?;
    }
}

/// Whether a change at `changed` can matter to `interest`: inside it, or on
/// the way to it (its folder being created).
pub fn relevant(changed: &Path, interest: &Path) -> bool {
    changed.starts_with(interest) || interest.starts_with(changed)
}

/// Gather a burst: after the first change, keep taking changes until none
/// came for `quiet`. Returns every path seen, or `None` once the sender is
/// gone.
pub fn gather(rx: &mpsc::Receiver<Vec<PathBuf>>, quiet: Duration) -> Option<Vec<PathBuf>> {
    let mut seen: BTreeSet<PathBuf> = rx.recv().ok()?.into_iter().collect();
    loop {
        match rx.recv_timeout(quiet) {
            Ok(more) => seen.extend(more),
            Err(mpsc::RecvTimeoutError::Timeout) => return Some(seen.into_iter().collect()),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Some(seen.into_iter().collect()),
        }
    }
}

struct State {
    watcher: Option<notify::RecommendedWatcher>,
    /// What is watched now, so a resync only adds and drops the difference.
    points: HashMap<PathBuf, RecursiveMode>,
    /// Workspaces the composer asked about, and when last.
    workspaces: HashMap<PathBuf, Instant>,
    interests: Vec<PathBuf>,
}

/// The watcher. Cheap to share; does nothing until `start`.
pub struct CommandWatch {
    home: PathBuf,
    state: Mutex<State>,
}

impl CommandWatch {
    pub fn new(home: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            home,
            state: Mutex::new(State {
                watcher: None,
                points: HashMap::new(),
                workspaces: HashMap::new(),
                interests: Vec::new(),
            }),
        })
    }

    /// Start watching; each quiet burst of relevant changes calls
    /// `on_change` on a thread of its own. A platform with no watcher
    /// leaves the menu to the App's own reports and the plugin changes.
    pub fn start(self: &Arc<Self>, on_change: OnChange) {
        let (raw_tx, raw_rx) = mpsc::channel::<Vec<PathBuf>>();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                if !matches!(event.kind, notify::EventKind::Access(_)) {
                    let _ = raw_tx.send(event.paths);
                }
            }
        });
        let Ok(watcher) = watcher else {
            return;
        };
        self.lock().watcher = Some(watcher);
        self.resync();
        let me = Arc::downgrade(self);
        std::thread::Builder::new()
            .name("command-watch".into())
            .spawn(move || {
                while let Some(paths) = gather(&raw_rx, QUIET) {
                    let Some(me) = me.upgrade() else { return };
                    let interests = me.lock().interests.clone();
                    let touched: Vec<PathBuf> = paths
                        .into_iter()
                        .filter(|p| interests.iter().any(|i| relevant(p, i)))
                        .collect();
                    if touched.is_empty() {
                        continue;
                    }
                    // A folder that appeared is watched with what is in it
                    // from now on.
                    me.resync();
                    on_change(touched);
                }
            })
            .ok();
    }

    /// The composer asked about `workspace`: watch its folders too.
    pub fn note_workspace(&self, workspace: &Path) {
        let added = self
            .lock()
            .workspaces
            .insert(workspace.to_path_buf(), Instant::now())
            .is_none();
        if added {
            self.resync();
        }
    }

    /// The workspaces being watched (for tests and the debug view).
    pub fn workspaces(&self) -> Vec<PathBuf> {
        self.lock().workspaces.keys().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bring the watched points in line with what exists now.
    pub fn resync(&self) {
        let mut state = self.lock();
        let now = Instant::now();
        state
            .workspaces
            .retain(|_, asked| now.duration_since(*asked) < FORGET_AFTER);
        let mut interests = home_interests(&self.home);
        let mut wanted: HashMap<PathBuf, RecursiveMode> = HashMap::new();
        for interest in &interests {
            if let Some((point, mode)) = watch_point(interest, &self.home) {
                merge(&mut wanted, point, mode);
            }
        }
        let workspaces: Vec<PathBuf> = state.workspaces.keys().cloned().collect();
        for workspace in workspaces {
            for interest in workspace_interests(&workspace) {
                if let Some((point, mode)) = watch_point(&interest, &workspace) {
                    merge(&mut wanted, point, mode);
                }
                interests.push(interest);
            }
        }
        state.interests = interests;
        let State {
            watcher, points, ..
        } = &mut *state;
        let Some(watcher) = watcher.as_mut() else {
            return;
        };
        points.retain(|point, mode| {
            if wanted.get(point) == Some(mode) {
                true
            } else {
                let _ = watcher.unwatch(point);
                false
            }
        });
        for (point, mode) in wanted {
            if !points.contains_key(&point) && watcher.watch(&point, mode).is_ok() {
                points.insert(point, mode);
            }
        }
    }
}

/// One point watched once: recursively when any interest needs it so.
fn merge(wanted: &mut HashMap<PathBuf, RecursiveMode>, point: PathBuf, mode: RecursiveMode) {
    let entry = wanted.entry(point).or_insert(mode);
    if mode == RecursiveMode::Recursive {
        *entry = RecursiveMode::Recursive;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_folder_is_watched_through_its_nearest_parent() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let commands = ws.join(".claude").join("commands");
        assert_eq!(
            watch_point(&commands, ws),
            Some((ws.to_path_buf(), RecursiveMode::NonRecursive))
        );
        std::fs::create_dir_all(&commands).unwrap();
        assert_eq!(
            watch_point(&commands, ws),
            Some((commands.clone(), RecursiveMode::Recursive))
        );
        // A file is watched through its folder.
        let mcp = ws.join(".mcp.json");
        std::fs::write(&mcp, "{}").unwrap();
        assert_eq!(
            watch_point(&mcp, ws),
            Some((ws.to_path_buf(), RecursiveMode::NonRecursive))
        );
        // Nothing to watch when the workspace itself is gone.
        let gone = ws.join("gone");
        assert_eq!(watch_point(&gone.join(".claude/commands"), &gone), None);
    }

    #[test]
    fn only_changes_inside_or_on_the_way_to_a_folder_count() {
        let interest = Path::new("/w/.claude/commands");
        assert!(relevant(Path::new("/w/.claude/commands/x.md"), interest));
        assert!(relevant(Path::new("/w/.claude"), interest));
        assert!(!relevant(Path::new("/w/README.md"), interest));
        assert!(!relevant(Path::new("/w/.claude/settings.json"), interest));
    }

    #[test]
    fn a_burst_is_handed_on_once_after_it_goes_quiet() {
        let (tx, rx) = mpsc::channel();
        let sender = std::thread::spawn(move || {
            for i in 0..5 {
                tx.send(vec![PathBuf::from(format!("/w/{i}"))]).unwrap();
                std::thread::sleep(Duration::from_millis(10));
            }
            tx.send(vec![PathBuf::from("/w/0")]).unwrap();
            // Held open past the quiet window, so the burst ends on quiet.
            std::thread::sleep(Duration::from_millis(400));
        });
        let burst = gather(&rx, Duration::from_millis(100)).unwrap();
        assert_eq!(burst.len(), 5, "{burst:?}");
        sender.join().unwrap();
        assert!(gather(&rx, Duration::from_millis(100)).is_none());
    }

    #[test]
    fn a_new_command_file_is_heard_without_a_restart() {
        let home = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let watch = CommandWatch::new(home.path().to_path_buf());
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        watch.start(Box::new(move |paths| {
            let _ = tx.lock().unwrap().send(paths);
        }));
        watch.note_workspace(ws.path());
        // The folder does not exist yet: its creation and the file in it
        // both count.
        let commands = ws.path().join(".claude").join("commands");
        std::fs::create_dir_all(&commands).unwrap();
        let heard = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("heard the folder");
        assert!(heard.iter().all(|p| p.starts_with(ws.path())), "{heard:?}");
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(commands.join("ship.md"), "---\ndescription: Ship it\n---\n").unwrap();
        let heard = loop {
            let paths = rx
                .recv_timeout(Duration::from_secs(10))
                .expect("heard the file");
            if paths.iter().any(|p| p.ends_with("ship.md")) {
                break paths;
            }
        };
        assert!(!heard.is_empty());
        // Whatever else the platform said about that file settles first.
        while rx.recv_timeout(Duration::from_millis(800)).is_ok() {}
        // A file elsewhere in the workspace is not news.
        std::fs::write(ws.path().join("notes.txt"), "x").unwrap();
        while let Ok(paths) = rx.recv_timeout(Duration::from_millis(800)) {
            assert!(!paths.iter().any(|p| p.ends_with("notes.txt")), "{paths:?}");
        }
    }
}
