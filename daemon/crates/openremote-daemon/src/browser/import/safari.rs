//! Safari, on a Mac: bookmarks from `Bookmarks.plist` (a binary property
//! list), history from `History.db`, both in `~/Library/Safari`. macOS keeps
//! that folder from other apps until the user gives OpenRemote Full Disk
//! Access, which the error then says.

use std::collections::{HashMap, HashSet};

use super::plist::{self, Plist};
use super::sqlite::Database;
use super::{
    Ctx, Entry, Failure, MAX_TEXT, MAX_TITLE, Newest, Profile, READ_BOOKMARKS, Scratch, cut, fits,
};
use crate::browser::store::IMPORT_VISITS;

/// Safari counts seconds from 2001-01-01; the unix epoch is this many
/// seconds earlier.
const EPOCH_S: f64 = 978_307_200.0;
/// Folders nested deeper than this are not followed.
const MAX_DEPTH: usize = 32;
/// The Reading List is a list of pages to read, not bookmarks.
const READING_LIST: &str = "com.apple.ReadingList";

fn from_safari(seconds: f64) -> i64 {
    ((seconds + EPOCH_S) * 1000.0) as i64
}

/// Every bookmark in the Favourites bar and the menu, in Safari's order.
pub fn bookmarks(profile: &Profile, ctx: &Ctx) -> Result<Vec<Entry>, Failure> {
    let bytes = profile.read("Bookmarks.plist", MAX_TEXT)?;
    let top = plist::parse(&bytes)
        .ok_or_else(|| Failure::Damaged("the bookmarks file is damaged".into()))?;
    let mut out = Vec::new();
    walk(&top, 0, ctx, &mut out);
    Ok(out)
}

fn walk(node: &Plist, depth: usize, ctx: &Ctx, out: &mut Vec<Entry>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Some(children) = node.get("Children").and_then(Plist::array) else {
        return;
    };
    for child in children {
        if out.len() >= READ_BOOKMARKS {
            return;
        }
        match child.get("WebBookmarkType").and_then(Plist::text) {
            Some("WebBookmarkTypeList") => {
                if child.get("Title").and_then(Plist::text) != Some(READING_LIST) {
                    walk(child, depth + 1, ctx, out);
                }
            }
            Some("WebBookmarkTypeLeaf") => {
                let Some(url) = child.get("URLString").and_then(Plist::text) else {
                    continue;
                };
                let key = child
                    .get("WebBookmarkUUID")
                    .and_then(Plist::text)
                    .map(String::from)
                    .unwrap_or_else(|| format!("{url}\n{}", out.len()));
                let title = child
                    .get("URIDictionary")
                    .and_then(|d| d.get("title"))
                    .and_then(Plist::text)
                    .unwrap_or("");
                out.push(Entry {
                    key,
                    url: url.to_string(),
                    title: title.to_string(),
                    // Safari's bookmarks file keeps no date for a bookmark.
                    at: ctx.now,
                });
            }
            _ => {}
        }
    }
}

/// The newest visits of the history window that loaded, from a copy of
/// `History.db`.
pub fn history(profile: &Profile, scratch: &Scratch, ctx: &Ctx) -> Result<Vec<Entry>, Failure> {
    let copy = profile.copy("History.db", scratch)?;
    let mut db = Database::open(&copy)?;
    let visits = db.table("history_visits")?;
    let mut newest = Newest::new(IMPORT_VISITS);
    let mut titles: HashMap<i64, String> = HashMap::new();
    db.scan(
        &visits,
        &[
            "id",
            "history_item",
            "visit_time",
            "title",
            "load_successful",
        ],
        |_, row| {
            let (Some(id), Some(item), Some(at)) = (row[0].int(), row[1].int(), row[2].real())
            else {
                return true;
            };
            let at = from_safari(at);
            // A page that failed to load was never read.
            if ctx.keeps(at) && row[4].int() != Some(0) {
                newest.push(id, item, at);
                if let Some(title) = row[3].text() {
                    titles.insert(id, cut(title, MAX_TITLE));
                }
            }
            true
        },
    )?;
    let recent = newest.done();
    let kept: HashSet<i64> = recent.iter().map(|r| r.0).collect();
    titles.retain(|id, _| kept.contains(id));
    let wanted: HashSet<i64> = recent.iter().map(|r| r.1).collect();
    let items = db.table("history_items")?;
    let mut urls: HashMap<i64, String> = HashMap::new();
    db.scan(&items, &["id", "url"], |_, row| {
        if let (Some(id), Some(url)) = (
            row[0].int().filter(|id| wanted.contains(id)),
            row[1].text().filter(|u| fits(u)),
        ) {
            urls.insert(id, url.to_string());
        }
        true
    })?;
    Ok(recent
        .into_iter()
        .filter_map(|(id, item, at)| {
            Some(Entry {
                key: id.to_string(),
                url: urls.get(&item)?.clone(),
                title: titles.remove(&id).unwrap_or_default(),
                at,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{NOW, fixtures};
    use super::super::{Kind, Source, Want, read};

    fn import() -> super::super::Found {
        let data = tempfile::tempdir().unwrap();
        let source = Source {
            id: "safari".into(),
            browser: "Safari".into(),
            profile: "Safari".into(),
            kind: Kind::Safari,
            dir: fixtures().join("safari"),
        };
        read(
            &source,
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
    fn bookmarks_leave_out_the_reading_list_and_bookmarklets() {
        let found = import();
        let urls: Vec<&str> = found.bookmarks.iter().map(|b| b.url.as_str()).collect();
        assert_eq!(urls, ["https://apple.example/", "https://dev.example/"]);
        assert_eq!(found.bookmarks[0].title, "Apple example");
        assert_eq!(
            found.bookmarks[0].id,
            "imp-safari-5A1D0000-0000-4000-8000-000000000001"
        );
        assert_eq!(found.bookmarks[0].added, NOW);
    }

    #[test]
    fn history_is_the_window_that_loaded_with_each_visits_title() {
        let found = import();
        let seen: Vec<(&str, &str)> = found
            .visits
            .iter()
            .map(|v| (v.url.as_str(), v.title.as_str()))
            .collect();
        // The old visit is outside the window, the failed load never read,
        // and the local file is not a web page.
        assert_eq!(
            seen,
            [
                ("https://apple.example/", "Apple example"),
                ("https://swift.example/book", "The Swift book"),
            ]
        );
        assert_eq!(found.visits[0].at, NOW - 2 * 3_600_000);
        // The bookmarklet, and the visit to the local file.
        assert_eq!(found.refused, 2);
    }

    /// Rows far larger than a browser writes are cut while they are read,
    /// so many copies of one large row cannot fill memory before the
    /// import's own cleaning sees them.
    #[test]
    fn oversized_rows_are_cut_while_they_are_read() {
        use super::super::{Ctx, MAX_TITLE, MAX_URL, Profile, Scratch};
        let parent = tempfile::tempdir().unwrap();
        let scratch = Scratch::new(parent.path()).unwrap();
        let profile = Profile::new(&fixtures().join("hostile/safari"));
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
