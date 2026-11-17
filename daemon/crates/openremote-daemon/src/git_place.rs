//! Where a chat's folder stands in git: its branch, and whether it is a
//! linked worktree (one `git worktree add` or Worktrunk made) of another
//! folder's repository. The console shows it beside each chat, so chats
//! working in parallel worktrees of one project read apart at a glance.
//!
//! Read from git's own files (`.git`, `HEAD`), never by starting git: a
//! sidebar full of chats asks for many folders at once. This device's
//! console only (not in the API a peer reaches): a chat on another device
//! has its folder there.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::app::App;

/// Folders asked about in one request, at most.
pub const MAX_PATHS: usize = 200;
/// A `.git` file or `HEAD` larger than this is not git's.
const MAX_READ: u64 = 4096;
/// Folders climbed looking for `.git`.
const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Place {
    /// The checked-out branch, or None on a detached HEAD.
    pub branch: Option<String>,
    /// The short commit a detached HEAD is at.
    pub detached: Option<String>,
    /// The folder is a linked worktree, not the repository's main one.
    pub worktree: bool,
    /// The worktree's top folder.
    pub root: PathBuf,
    /// The repository's main worktree (the same as `root` for it).
    pub main: PathBuf,
}

fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_READ {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The git place of `path`, or None outside any repository.
pub fn place(path: &Path) -> Option<Place> {
    let mut dir = Some(path);
    for _ in 0..MAX_DEPTH {
        let d = dir?;
        let dotgit = d.join(".git");
        if dotgit.is_dir() {
            return head(&dotgit, d, d.to_path_buf(), false);
        }
        if dotgit.is_file() {
            let text = read_small(&dotgit)?;
            let raw = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
            let gitdir = if Path::new(raw).is_absolute() {
                PathBuf::from(raw)
            } else {
                d.join(raw)
            };
            // A linked worktree's gitdir is <main>/.git/worktrees/<name>; a
            // submodule's lies under .git/modules and is a main of its own.
            let parent = gitdir.parent()?;
            let linked = parent.file_name().is_some_and(|n| n == "worktrees");
            let main = if linked {
                let common = parent.parent()?;
                if common.file_name().is_some_and(|n| n == ".git") {
                    common.parent()?.to_path_buf()
                } else {
                    common.to_path_buf()
                }
            } else {
                d.to_path_buf()
            };
            return head(&gitdir, d, main, linked);
        }
        dir = d.parent();
    }
    None
}

fn head(gitdir: &Path, root: &Path, main: PathBuf, worktree: bool) -> Option<Place> {
    let text = read_small(&gitdir.join("HEAD"))?;
    let text = text.trim();
    let (branch, detached) = match text.strip_prefix("ref:") {
        Some(r) => (
            Some(
                r.trim()
                    .strip_prefix("refs/heads/")
                    .unwrap_or(r.trim())
                    .to_string(),
            ),
            None,
        ),
        None if text.len() >= 7 && text.bytes().all(|b| b.is_ascii_hexdigit()) => {
            (None, Some(text[..7].to_string()))
        }
        None => return None,
    };
    Some(Place {
        branch,
        detached,
        worktree,
        root: root.to_path_buf(),
        main,
    })
}

pub fn routes() -> Router<Arc<App>> {
    Router::new().route("/git/places", post(places))
}

#[derive(Deserialize)]
struct PlacesBody {
    paths: Vec<String>,
}

/// `{"paths": [...]}` → `{"places": {"<path>": place or null}}`, each
/// absolute folder on this computer read where it stands.
async fn places(State(_app): State<Arc<App>>, Json(body): Json<PlacesBody>) -> Json<Value> {
    let paths: Vec<String> = body.paths.into_iter().take(MAX_PATHS).collect();
    let found = tokio::task::spawn_blocking(move || {
        let mut out = Map::new();
        for raw in paths {
            let path = Path::new(&raw);
            let answer = if path.is_absolute() {
                place(path)
                    .and_then(|p| serde_json::to_value(p).ok())
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            };
            out.insert(raw, answer);
        }
        out
    })
    .await
    .unwrap_or_default();
    Json(json!({"places": found}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_main_checkout_and_its_linked_worktree_read_apart() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("repo");
        let linked = tmp.path().join("repo.feature");
        let wt_git = main.join(".git").join("worktrees").join("repo.feature");
        std::fs::create_dir_all(&wt_git).unwrap();
        std::fs::create_dir_all(main.join("src")).unwrap();
        std::fs::create_dir_all(linked.join("deep").join("er")).unwrap();
        std::fs::write(main.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(wt_git.join("HEAD"), "ref: refs/heads/feature/login\n").unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", wt_git.display()),
        )
        .unwrap();

        let m = place(&main.join("src")).unwrap();
        assert_eq!(m.branch.as_deref(), Some("main"));
        assert!(!m.worktree);
        assert_eq!(m.root, main);
        assert_eq!(m.main, main);

        let w = place(&linked.join("deep").join("er")).unwrap();
        assert_eq!(w.branch.as_deref(), Some("feature/login"));
        assert!(w.worktree);
        assert_eq!(w.root, linked);
        assert_eq!(w.main, main);

        std::fs::write(
            wt_git.join("HEAD"),
            "0352a86a7eceee865ddb8ea5c5abefc8e66d4ab7\n",
        )
        .unwrap();
        let d = place(&linked).unwrap();
        assert_eq!(d.branch, None);
        assert_eq!(d.detached.as_deref(), Some("0352a86"));
    }

    #[test]
    fn outside_a_repository_or_with_a_stranger_head_there_is_no_place() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(place(tmp.path()), None);
        let repo = tmp.path().join("r");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git").join("HEAD"), "not git\n").unwrap();
        assert_eq!(place(&repo), None);
        std::fs::write(repo.join(".git").join("HEAD"), "x".repeat(10_000)).unwrap();
        assert_eq!(place(&repo), None);
    }

    #[test]
    fn a_submodule_is_a_main_checkout_of_its_own() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("super").join("lib");
        let gitdir = tmp
            .path()
            .join("super")
            .join(".git")
            .join("modules")
            .join("lib");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(sub.join(".git"), "gitdir: ../.git/modules/lib\n").unwrap();
        let p = place(&sub).unwrap();
        assert!(!p.worktree);
        assert_eq!(p.main, sub);
    }
}
