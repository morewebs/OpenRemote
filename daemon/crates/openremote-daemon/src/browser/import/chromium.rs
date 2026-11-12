//! Chrome, Edge, Brave, Vivaldi, Opera and Chromium: bookmarks from the
//! profile's `Bookmarks` file (JSON), history from its `History` database.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::sqlite::Database;
use super::{
    Ctx, Entry, Failure, MAX_TEXT, MAX_TITLE, Newest, Profile, READ_BOOKMARKS, Scratch, cut, fits,
};
use crate::browser::store::IMPORT_VISITS;

/// Chromium counts microseconds from 1601; the unix epoch is this many
/// milliseconds later.
const EPOCH_MS: i64 = 11_644_473_600_000;
/// Folders nested deeper than this are not followed.
const MAX_DEPTH: usize = 32;

fn from_chromium(micros: i64) -> i64 {
    micros / 1000 - EPOCH_MS
}

/// Every bookmark in the bar, Other bookmarks and the phone's folder, in the
/// order the browser shows them.
pub fn bookmarks(profile: &Profile, ctx: &Ctx) -> Result<Vec<Entry>, Failure> {
    let bytes = profile.read("Bookmarks", MAX_TEXT)?;
    let json: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Failure::Damaged("the bookmarks file is damaged".into()))?;
    let roots = json
        .get("roots")
        .ok_or_else(|| Failure::Damaged("the bookmarks file has no bookmarks in it".into()))?;
    let mut out = Vec::new();
    for key in ["bookmark_bar", "other", "synced"] {
        if let Some(root) = roots.get(key) {
            walk(root, 0, ctx, &mut out);
        }
    }
    Ok(out)
}

fn walk(node: &Value, depth: usize, ctx: &Ctx, out: &mut Vec<Entry>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        if out.len() >= READ_BOOKMARKS {
            return;
        }
        match child.get("type").and_then(Value::as_str) {
            Some("folder") => walk(child, depth + 1, ctx, out),
            Some("url") => {
                let Some(url) = child.get("url").and_then(Value::as_str) else {
                    continue;
                };
                let key = child
                    .get("guid")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_else(|| format!("{url}\n{}", out.len()));
                out.push(Entry {
                    key,
                    url: url.to_string(),
                    title: child
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    at: child
                        .get("date_added")
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse::<i64>().ok())
                        .filter(|&t| t > 0)
                        .map_or(ctx.now, from_chromium),
                });
            }
            _ => {}
        }
    }
}

/// The newest visits of the history window, from a copy of `History`.
pub fn history(profile: &Profile, scratch: &Scratch, ctx: &Ctx) -> Result<Vec<Entry>, Failure> {
    let copy = profile.copy("History", scratch)?;
    let mut db = Database::open(&copy)?;
    let visits = db.table("visits")?;
    let mut newest = Newest::new(IMPORT_VISITS);
    db.scan(
        &visits,
        &["id", "url", "visit_time", "transition"],
        |_, row| {
            let (Some(id), Some(url), Some(at)) = (row[0].int(), row[1].int(), row[2].int()) else {
                return true;
            };
            let at = from_chromium(at);
            // Pages inside pages (frames) are not visits.
            let core = row[3].int().unwrap_or(0) & 0xff;
            if ctx.keeps(at) && core != 3 && core != 4 {
                newest.push(id, url, at);
            }
            true
        },
    )?;
    let recent = newest.done();
    let wanted: HashSet<i64> = recent.iter().map(|r| r.1).collect();
    let urls = db.table("urls")?;
    let mut pages: HashMap<i64, (String, String)> = HashMap::new();
    db.scan(&urls, &["id", "url", "title", "hidden"], |_, row| {
        let Some(id) = row[0].int().filter(|id| wanted.contains(id)) else {
            return true;
        };
        if let (Some(url), false) = (row[1].text().filter(|u| fits(u)), row[3].int() == Some(1)) {
            pages.insert(
                id,
                (url.to_string(), cut(row[2].text().unwrap_or(""), MAX_TITLE)),
            );
        }
        true
    })?;
    Ok(recent
        .into_iter()
        .filter_map(|(id, page, at)| {
            let (url, title) = pages.get(&page)?.clone();
            Some(Entry {
                key: id.to_string(),
                url,
                title,
                at,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{NOW, fixtures};
    use super::super::{Kind, Source, Want, read};

    fn chrome() -> Source {
        Source {
            id: "chrome-default".into(),
            browser: "Chrome".into(),
            profile: "Default".into(),
            kind: Kind::Chromium,
            dir: fixtures().join("chromium/Default"),
        }
    }

    fn import() -> super::super::Found {
        let data = tempfile::tempdir().unwrap();
        read(
            &chrome(),
            Want {
                bookmarks: true,
                history: true,
            },
            NOW,
            data.path(),
        )
        .unwrap()
    }

    #[test]
    fn bookmarks_come_from_the_bar_other_and_the_phone() {
        let found = import();
        let urls: Vec<&str> = found.bookmarks.iter().map(|b| b.url.as_str()).collect();
        // The bar's folder, the bar, Other, the phone's; the duplicate is the
        // store's to drop.
        assert_eq!(
            urls,
            [
                "https://work.example.com/",
                "https://example.org/",
                "https://example.net/a",
                "https://example.org/",
                "https://m.example.com/",
            ]
        );
        let first = &found.bookmarks[0];
        assert_eq!(
            first.id,
            "imp-chrome-default-0b5c1c0e-0000-4000-8000-000000000001"
        );
        assert_eq!(first.title, "Work page");
        assert_eq!(first.added, NOW - 10 * 86_400_000);
        assert_eq!(first.folder, "other");
        assert_eq!(found.bookmarks[2].title, "Tabs and newlines");
        // A guid that is not plain is replaced by a digest.
        assert!(!found.bookmarks[4].id.contains('/'));
    }

    #[test]
    fn a_bookmarklet_a_local_file_and_inline_data_are_skipped() {
        let found = import();
        for mark in &found.bookmarks {
            assert!(mark.url.starts_with("https://"), "{}", mark.url);
        }
        // javascript:, file: and data: bookmarks, and the javascript: page
        // in history (its visits).
        assert!(found.refused >= 3);
    }

    #[test]
    fn history_is_the_window_newest_first_without_frames_or_hidden_pages() {
        let found = import();
        // 240 visits to five pages, of which the hidden one (48) and the
        // javascript: one (48) are left out; the old visit and the two in
        // frames are outside the window or not visits.
        assert_eq!(found.visits.len(), 240 - 48 - 48);
        assert!(found.visits.windows(2).all(|w| w[0].at >= w[1].at));
        assert!(found.visits.iter().all(|v| !v.url.contains("hidden")));
        assert!(found.visits.iter().all(|v| !v.url.contains("frame")));
        assert!(found.visits.iter().all(|v| !v.url.contains("old.")));
        assert!(
            found
                .visits
                .iter()
                .any(|v| v.url.starts_with("https://long.example.com/"))
        );
        assert!(
            found
                .visits
                .iter()
                .all(|v| v.id.starts_with("imp-chrome-default-v"))
        );
        assert_eq!(found.visits[0].at, NOW - 3 * 3_600_000);
    }

    /// Rows far larger than a browser writes are cut while they are read,
    /// so many copies of one large row cannot fill memory before the
    /// import's own cleaning sees them.
    #[test]
    fn oversized_rows_are_cut_while_they_are_read() {
        use super::super::{Ctx, MAX_TITLE, MAX_URL, Profile, Scratch};
        let parent = tempfile::tempdir().unwrap();
        let scratch = Scratch::new(parent.path()).unwrap();
        let profile = Profile::new(&fixtures().join("hostile/chromium"));
        let ctx = Ctx::at(NOW);
        let mut entries = Vec::new();
        entries.extend(super::history(&profile, &scratch, &ctx).unwrap());
        assert!(!entries.is_empty());
        for entry in &entries {
            assert!(
                entry.title.chars().count() <= MAX_TITLE,
                "{}",
                entry.title.len()
            );
            assert!(entry.url.len() <= MAX_URL, "{}", entry.url.len());
            assert!(entry.key.len() <= 128, "{}", entry.key.len());
        }
        // The page with the oversized address is not read at all.
        assert!(
            entries.iter().all(|e| e.url == "https://ok.example/"),
            "{entries:?}"
        );
    }
}
