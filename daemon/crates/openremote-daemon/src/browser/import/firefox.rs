//! Firefox: bookmarks and history both live in the profile's
//! `places.sqlite`, a write-ahead-log database that Firefox keeps open while
//! it runs (its newest changes sit in the `-wal` file beside it, which the
//! copy takes too).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::sqlite::Database;
use super::{
    Ctx, Entry, Failure, MAX_KEY, MAX_TEXT, MAX_TITLE, Newest, Profile, READ_BOOKMARKS, Scratch,
    cut, fits,
};
use crate::browser::store::IMPORT_VISITS;

/// Folders nested deeper than this are not followed.
const MAX_DEPTH: usize = 32;
/// A `profiles.ini` listing more profiles than this is not a person's.
const MAX_PROFILES: usize = 64;

/// The profiles Firefox lists in `profiles.ini` (in its own data folder),
/// the default first: (folder name, shown name, folder).
pub fn profiles(root: &Path) -> Vec<(String, String, PathBuf)> {
    let Ok(bytes) = Profile::new(root).read("profiles.ini", MAX_TEXT) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut found: Vec<(String, String, PathBuf, bool)> = Vec::new();
    let mut installs: Vec<String> = Vec::new();
    let mut section = String::new();
    let (mut name, mut path, mut relative, mut default) =
        (String::new(), String::new(), true, false);
    let mut flush = |section: &str, name: &str, path: &str, relative: bool, default: bool| {
        if section.starts_with("Profile") && !path.is_empty() && found.len() < MAX_PROFILES {
            let dir = if relative {
                root.join(path)
            } else {
                PathBuf::from(path)
            };
            // A profile reached through a link, or a relative path that
            // climbs out of Firefox's folder, is not read.
            if !super::plain_folder(&dir, relative.then_some(root)) {
                return;
            }
            let id = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let shown = if name.is_empty() {
                id.clone()
            } else {
                name.to_string()
            };
            found.push((id, shown, dir, default));
        }
    };
    for line in text.lines().map(str::trim) {
        if let Some(head) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            flush(&section, &name, &path, relative, default);
            section = head.to_string();
            (name, path, relative, default) = (String::new(), String::new(), true, false);
        } else if let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            match key.trim() {
                "Name" => name = value.to_string(),
                "Default" if section.starts_with("Install") => installs.push(value.to_string()),
                "Path" => path = value.to_string(),
                "IsRelative" => relative = value != "0",
                "Default" => default = value == "1",
                _ => {}
            }
        }
    }
    flush(&section, &name, &path, relative, default);
    // The profile an install runs by default comes first, then the one
    // `profiles.ini` marks, then the rest.
    found.sort_by_key(|(_, _, dir, marked)| {
        let by_install = installs.iter().any(|p| dir.ends_with(p));
        (!by_install, !marked)
    });
    found
        .into_iter()
        .map(|(id, name, dir, _)| (id, name, dir))
        .collect()
}

struct Node {
    id: i64,
    kind: i64,
    place: Option<i64>,
    parent: i64,
    position: i64,
    title: String,
    added: i64,
    guid: String,
}

/// Every bookmark in the menu, the toolbar, Other and the phone's folder,
/// in the order Firefox shows them. Tags and queries are not bookmarks.
pub fn bookmarks(profile: &Profile, scratch: &Scratch, ctx: &Ctx) -> Result<Vec<Entry>, Failure> {
    let copy = profile.copy("places.sqlite", scratch)?;
    let mut db = Database::open(&copy)?;
    let table = db.table("moz_bookmarks")?;
    let mut nodes: Vec<Node> = Vec::new();
    db.scan(
        &table,
        &[
            "id",
            "type",
            "fk",
            "parent",
            "position",
            "title",
            "dateAdded",
            "guid",
        ],
        |_, row| {
            if let (Some(id), Some(kind), Some(parent)) = (row[0].int(), row[1].int(), row[3].int())
            {
                nodes.push(Node {
                    id,
                    kind,
                    place: row[2].int(),
                    parent,
                    position: row[4].int().unwrap_or(0),
                    title: cut(row[5].text().unwrap_or(""), MAX_TITLE),
                    added: row[6].int().unwrap_or(0),
                    guid: cut(row[7].text().unwrap_or(""), MAX_KEY),
                });
            }
            // A tree this large is not a person's; what was read is kept.
            nodes.len() < READ_BOOKMARKS * 4
        },
    )?;
    let wanted: HashSet<i64> = nodes
        .iter()
        .filter(|n| n.kind == 1)
        .filter_map(|n| n.place)
        .collect();
    let places = db.table("moz_places")?;
    let mut urls: HashMap<i64, String> = HashMap::new();
    db.scan(&places, &["id", "url"], |_, row| {
        if let (Some(id), Some(url)) = (
            row[0].int().filter(|id| wanted.contains(id)),
            row[1].text().filter(|u| fits(u)),
        ) {
            urls.insert(id, url.to_string());
        }
        true
    })?;
    let mut children: HashMap<i64, Vec<&Node>> = HashMap::new();
    for node in &nodes {
        // A node that is its own parent belongs nowhere.
        if node.parent != node.id {
            children.entry(node.parent).or_default().push(node);
        }
    }
    for list in children.values_mut() {
        list.sort_by_key(|n| (n.position, n.id));
    }
    let root = nodes
        .iter()
        .find(|n| n.guid == "root________")
        .map_or(1, |n| n.id);
    let mut out = Vec::new();
    let mut visited = HashSet::from([root]);
    // The root's children are the menu, toolbar, Other and the phone's
    // folders, and the tag folder, whose entries are tags, not bookmarks.
    for top in children.get(&root).into_iter().flatten() {
        if top.guid == "tags________" {
            continue;
        }
        let walk = Walk {
            children: &children,
            urls: &urls,
            ctx,
        };
        walk.descend(top.id, 0, &mut visited, &mut out);
    }
    Ok(out)
}

struct Walk<'a> {
    children: &'a HashMap<i64, Vec<&'a Node>>,
    urls: &'a HashMap<i64, String>,
    ctx: &'a Ctx,
}

impl Walk<'_> {
    fn descend(&self, parent: i64, depth: usize, visited: &mut HashSet<i64>, out: &mut Vec<Entry>) {
        // A folder is walked once, however often the file names it.
        if depth > MAX_DEPTH || !visited.insert(parent) {
            return;
        }
        for node in self.children.get(&parent).into_iter().flatten() {
            if out.len() >= READ_BOOKMARKS {
                return;
            }
            match node.kind {
                2 => self.descend(node.id, depth + 1, visited, out),
                1 => {
                    let Some(url) = node.place.and_then(|id| self.urls.get(&id)) else {
                        continue;
                    };
                    out.push(Entry {
                        key: if node.guid.is_empty() {
                            node.id.to_string()
                        } else {
                            node.guid.clone()
                        },
                        url: url.clone(),
                        title: node.title.clone(),
                        at: if node.added > 0 {
                            node.added / 1000
                        } else {
                            self.ctx.now
                        },
                    });
                }
                _ => {}
            }
        }
    }
}

/// The newest visits of the history window.
pub fn history(profile: &Profile, scratch: &Scratch, ctx: &Ctx) -> Result<Vec<Entry>, Failure> {
    let copy = profile.copy("places.sqlite", scratch)?;
    let mut db = Database::open(&copy)?;
    let visits = db.table("moz_historyvisits")?;
    let mut newest = Newest::new(IMPORT_VISITS);
    db.scan(
        &visits,
        &["id", "place_id", "visit_date", "visit_type"],
        |_, row| {
            let (Some(id), Some(place), Some(at)) = (row[0].int(), row[1].int(), row[2].int())
            else {
                return true;
            };
            let at = at / 1000;
            // Embedded and framed loads are not visits.
            let kind = row[3].int().unwrap_or(1);
            if ctx.keeps(at) && kind != 4 && kind != 8 {
                newest.push(id, place, at);
            }
            true
        },
    )?;
    let recent = newest.done();
    let wanted: HashSet<i64> = recent.iter().map(|r| r.1).collect();
    let places = db.table("moz_places")?;
    let mut pages: HashMap<i64, (String, String)> = HashMap::new();
    db.scan(&places, &["id", "url", "title", "hidden"], |_, row| {
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
        .filter_map(|(id, place, at)| {
            let (url, title) = pages.get(&place)?.clone();
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
    use super::profiles;

    fn import() -> super::super::Found {
        let data = tempfile::tempdir().unwrap();
        let source = Source {
            id: "firefox-abcd".into(),
            browser: "Firefox".into(),
            profile: "default-release".into(),
            kind: Kind::Firefox,
            dir: fixtures().join("firefox/Profiles/abcd1234.default-release"),
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
    fn the_install_default_profile_comes_first() {
        let listed = profiles(&fixtures().join("firefox"));
        let names: Vec<&str> = listed.iter().map(|p| p.1.as_str()).collect();
        // `spare` names a folder that is not there.
        assert_eq!(names, ["default-release"]);
        assert_eq!(listed[0].0, "abcd1234.default-release");
        assert!(profiles(&fixtures().join("nowhere")).is_empty());
    }

    /// A profile `profiles.ini` names through a link (a junction on
    /// Windows), or by a relative path that climbs out of Firefox's folder,
    /// is not read: it could be any folder on the computer.
    #[test]
    fn a_profile_reached_through_a_link_or_out_of_the_folder_is_not_listed() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("Firefox");
        let real = home.path().join("elsewhere");
        std::fs::create_dir_all(root.join("Profiles")).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        std::fs::copy(
            fixtures().join("firefox/Profiles/abcd1234.default-release/places.sqlite"),
            real.join("places.sqlite"),
        )
        .unwrap();
        assert!(super::super::tests::folder_link(
            &real,
            &root.join("Profiles/linked")
        ));
        std::fs::write(
            root.join("profiles.ini"),
            "[Profile0]
Name=linked
IsRelative=1
Path=Profiles/linked

[Profile1]
Name=climbs
IsRelative=1
Path=../elsewhere
",
        )
        .unwrap();
        assert!(profiles(&root).is_empty());
    }

    #[test]
    fn bookmarks_skip_tags_queries_and_loops_and_read_the_log() {
        let found = import();
        let urls: Vec<&str> = found.bookmarks.iter().map(|b| b.url.as_str()).collect();
        // Menu (its Reading folder), toolbar, Other, then the phone's
        // bookmark, which only the write-ahead log holds. The tagged page,
        // the place: query, the data: page and the folder that is its own
        // parent are not.
        assert_eq!(
            urls,
            [
                "https://read.example/",
                "https://mozilla.example/",
                "https://phone.example/",
            ]
        );
        assert_eq!(found.bookmarks[0].id, "imp-firefox-abcd-bmreading001");
        assert_eq!(found.bookmarks[0].added, NOW - 5 * 86_400_000);
    }

    #[test]
    fn history_leaves_out_embedded_loads_and_reads_the_log() {
        let found = import();
        assert_eq!(found.visits.len(), 40);
        assert!(
            found
                .visits
                .iter()
                .all(|v| v.url == "https://visited.example/")
        );
        // Renamed after the last checkpoint.
        assert_eq!(found.visits[0].title, "Visited (renamed)");
        assert_eq!(found.visits[0].at, NOW - 3_600_000);
    }

    /// Rows far larger than a browser writes are cut while they are read,
    /// so many copies of one large row cannot fill memory before the
    /// import's own cleaning sees them.
    #[test]
    fn oversized_rows_are_cut_while_they_are_read() {
        use super::super::{Ctx, MAX_TITLE, MAX_URL, Profile, Scratch};
        let parent = tempfile::tempdir().unwrap();
        let scratch = Scratch::new(parent.path()).unwrap();
        let profile = Profile::new(&fixtures().join("hostile/firefox"));
        let ctx = Ctx::at(NOW);
        let mut entries = Vec::new();
        entries.extend(super::bookmarks(&profile, &scratch, &ctx).unwrap());
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
