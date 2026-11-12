"""Makes the browser-import fixtures: one small, made-up profile per browser.

Run from this folder with `python make.py`. Every address and title here is
invented (example.* hosts); nothing comes from a real profile. The files are
written with small pages (1 KiB) so a few hundred rows already need interior
b-tree pages and overflow pages, which is what the reader has to walk.

The tests read these files as they are committed; rerunning this script makes
new copies with the same content (SQLite may lay the bytes out differently).

NOW is the moment the tests import at: 2026-10-01T00:00:00Z.
"""

import json
import os
import plistlib
import shutil
import sqlite3

NOW_MS = 1_790_812_800_000
DAY_MS = 86_400_000
HERE = os.path.dirname(os.path.abspath(__file__))

# Chromium counts microseconds from 1601-01-01.
CHROMIUM_EPOCH_MS = 11_644_473_600_000
# Safari counts seconds from 2001-01-01.
SAFARI_EPOCH_S = 978_307_200


def chromium_time(ms):
    return (ms + CHROMIUM_EPOCH_MS) * 1000


def fresh(path):
    for suffix in ("", "-wal", "-shm", "-journal"):
        if os.path.exists(path + suffix):
            os.remove(path + suffix)


def small(db):
    db.execute("PRAGMA page_size=1024")


def chromium():
    profile = os.path.join(HERE, "chromium", "Default")
    os.makedirs(profile, exist_ok=True)

    def url(guid, name, address, added_days_ago=10):
        return {
            "type": "url",
            "guid": guid,
            "name": name,
            "url": address,
            "date_added": str(chromium_time(NOW_MS - added_days_ago * DAY_MS)),
        }

    bookmarks = {
        "checksum": "0",
        "version": 1,
        "roots": {
            "bookmark_bar": {
                "type": "folder",
                "name": "Bookmarks bar",
                "children": [
                    {
                        "type": "folder",
                        "name": "Work",
                        "children": [
                            url("0b5c1c0e-0000-4000-8000-000000000001", "Work page", "https://work.example.com/"),
                        ],
                    },
                    url("0b5c1c0e-0000-4000-8000-000000000002", "Example org", "https://example.org/"),
                    url("0b5c1c0e-0000-4000-8000-000000000003", "A bookmarklet", "javascript:alert(1)"),
                ],
            },
            "other": {
                "type": "folder",
                "name": "Other bookmarks",
                "children": [
                    url("0b5c1c0e-0000-4000-8000-000000000004", "A local file", "file:///C:/notes.txt"),
                    url("0b5c1c0e-0000-4000-8000-000000000005", "Inline page", "data:text/html,<b>hi</b>"),
                    url("0b5c1c0e-0000-4000-8000-000000000006", "Tabs\tand\nnewlines", "https://example.net/a"),
                    url("0b5c1c0e-0000-4000-8000-000000000007", "Example org again", "https://example.org/"),
                ],
            },
            "synced": {
                "type": "folder",
                "name": "Mobile bookmarks",
                "children": [
                    url("../../not-a-guid", "Phone page", "https://m.example.com/"),
                ],
            },
        },
    }
    with open(os.path.join(profile, "Bookmarks"), "w", encoding="utf-8") as f:
        json.dump(bookmarks, f, indent=2)

    path = os.path.join(profile, "History")
    fresh(path)
    db = sqlite3.connect(path)
    small(db)
    db.executescript(
        """
        CREATE TABLE meta(key LONGVARCHAR NOT NULL UNIQUE PRIMARY KEY, value LONGVARCHAR);
        CREATE TABLE urls(id INTEGER PRIMARY KEY AUTOINCREMENT,url LONGVARCHAR,title LONGVARCHAR,visit_count INTEGER DEFAULT 0 NOT NULL,typed_count INTEGER DEFAULT 0 NOT NULL,last_visit_time INTEGER NOT NULL,hidden INTEGER DEFAULT 0 NOT NULL);
        CREATE TABLE visits(id INTEGER PRIMARY KEY AUTOINCREMENT,url INTEGER NOT NULL,visit_time INTEGER NOT NULL,from_visit INTEGER,transition INTEGER DEFAULT 0 NOT NULL,segment_id INTEGER,visit_duration INTEGER DEFAULT 0 NOT NULL);
        """
    )
    long_url = "https://long.example.com/" + "a" * 1500
    pages = [
        (1, "https://news.example.com/", "News", 0),
        (2, "https://docs.example.com/guide", "Guide", 0),
        (3, long_url, "A long address", 0),
        (4, "javascript:void(0)", "Script", 0),
        (5, "https://hidden.example.com/", "Hidden", 1),
        (6, "https://old.example.com/", "Old", 0),
        (7, "https://frame.example.com/", "Frame", 0),
    ]
    for pid, address, title, hidden in pages:
        db.execute(
            "INSERT INTO urls(id,url,title,visit_count,last_visit_time,hidden) VALUES (?,?,?,?,?,?)",
            (pid, address, title, 1, chromium_time(NOW_MS), hidden),
        )
    vid = 0
    # 240 ordinary visits over the last 30 days, spread over pages 1-5: enough
    # rows for interior pages at 1 KiB a page.
    for n in range(240):
        vid += 1
        page = 1 + n % 5
        db.execute(
            "INSERT INTO visits(id,url,visit_time,transition) VALUES (?,?,?,?)",
            (vid, page, chromium_time(NOW_MS - (n + 1) * 3_600_000 * 3), 0x30000000 | 1),
        )
    # Older than the 90 days history is kept for.
    vid += 1
    db.execute(
        "INSERT INTO visits(id,url,visit_time,transition) VALUES (?,?,?,?)",
        (vid, 6, chromium_time(NOW_MS - 120 * DAY_MS), 1),
    )
    # A page loaded in a frame (AUTO_SUBFRAME = 3, MANUAL_SUBFRAME = 4).
    for core in (3, 4):
        vid += 1
        db.execute(
            "INSERT INTO visits(id,url,visit_time,transition) VALUES (?,?,?,?)",
            (vid, 7, chromium_time(NOW_MS - DAY_MS), core),
        )
    db.commit()
    db.close()


def firefox():
    root = os.path.join(HERE, "firefox")
    profile = os.path.join(root, "Profiles", "abcd1234.default-release")
    os.makedirs(profile, exist_ok=True)
    with open(os.path.join(root, "profiles.ini"), "w", encoding="utf-8", newline="\n") as f:
        f.write(
            "[Install4F96D1932A9F858E]\nDefault=Profiles/abcd1234.default-release\nLocked=1\n\n"
            "[Profile1]\nName=spare\nIsRelative=1\nPath=Profiles/zzzz.spare\n\n"
            "[Profile0]\nName=default-release\nIsRelative=1\nPath=Profiles/abcd1234.default-release\nDefault=1\n\n"
            "[General]\nStartWithLastProfile=1\nVersion=2\n"
        )
    path = os.path.join(profile, "places.sqlite")
    fresh(path)
    db = sqlite3.connect(path, isolation_level=None)
    small(db)
    db.execute("PRAGMA journal_mode=WAL")
    db.executescript(
        """
        CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR, rev_host LONGVARCHAR, visit_count INTEGER DEFAULT 0, hidden INTEGER DEFAULT 0 NOT NULL, typed INTEGER DEFAULT 0 NOT NULL, frecency INTEGER DEFAULT -1 NOT NULL, last_visit_date INTEGER , guid TEXT, foreign_count INTEGER DEFAULT 0 NOT NULL, url_hash INTEGER DEFAULT 0 NOT NULL);
        CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, from_visit INTEGER, place_id INTEGER, visit_date INTEGER, visit_type INTEGER, session INTEGER, source INTEGER DEFAULT 0 NOT NULL, triggeringPlaceId INTEGER);
        CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER DEFAULT NULL, parent INTEGER, position INTEGER, title LONGVARCHAR, keyword_id INTEGER, folder_type TEXT, dateAdded INTEGER, lastModified INTEGER, guid TEXT, syncStatus INTEGER NOT NULL DEFAULT 0, syncChangeCounter INTEGER NOT NULL DEFAULT 1);
        """
    )
    us = lambda ms: ms * 1000
    places = [
        (1, "https://mozilla.example/", "Mozilla example"),
        (2, "https://read.example/", "Reading"),
        (3, "place:sort=8&maxResults=10", "Recent"),
        (4, "https://tagged.example/", "Tagged"),
        (5, "https://phone.example/", "Phone"),
        (6, "https://visited.example/", "Visited"),
        (7, "https://embed.example/", "Embedded"),
        (8, "data:text/plain,hello", "Data"),
    ]
    for pid, address, title in places:
        db.execute("INSERT INTO moz_places(id,url,title) VALUES (?,?,?)", (pid, address, title))
    added = us(NOW_MS - 5 * DAY_MS)
    rows = [
        # id, type, fk, parent, position, title, guid
        (1, 2, None, 0, 0, "", "root________"),
        (2, 2, None, 1, 0, "menu", "menu________"),
        (3, 2, None, 1, 1, "toolbar", "toolbar_____"),
        (4, 2, None, 1, 2, "tags", "tags________"),
        (5, 2, None, 1, 3, "unfiled", "unfiled_____"),
        (6, 2, None, 1, 4, "mobile", "mobile______"),
        (10, 1, 1, 3, 0, "Mozilla example", "bmtoolbar001"),
        (11, 2, None, 2, 0, "Reading", "fldreading01"),
        (12, 1, 2, 11, 0, "Read later", "bmreading001"),
        (13, 1, 3, 11, 1, "Recent", "bmquery00001"),
        (14, 2, None, 4, 0, "a-tag", "fldtag000001"),
        (15, 1, 4, 14, 0, "", "bmtagged0001"),
        (16, 1, 8, 5, 0, "Data", "bmdata000001"),
        # A folder that is its own parent: a damaged file must not loop.
        (20, 2, None, 20, 0, "Loop", "fldloop00001"),
        (21, 1, 1, 20, 0, "In the loop", "bmloop000001"),
    ]
    for rid, kind, fk, parent, position, title, guid in rows:
        db.execute(
            "INSERT INTO moz_bookmarks(id,type,fk,parent,position,title,dateAdded,lastModified,guid) VALUES (?,?,?,?,?,?,?,?,?)",
            (rid, kind, fk, parent, position, title, added, added, guid),
        )
    vid = 0
    for n in range(40):
        vid += 1
        db.execute(
            "INSERT INTO moz_historyvisits(id,place_id,visit_date,visit_type) VALUES (?,?,?,?)",
            (vid, 6, us(NOW_MS - (n + 1) * 3_600_000), 1),
        )
    for kind in (4, 8):  # EMBED, FRAMED_LINK
        vid += 1
        db.execute(
            "INSERT INTO moz_historyvisits(id,place_id,visit_date,visit_type) VALUES (?,?,?,?)",
            (vid, 7, us(NOW_MS - DAY_MS), kind),
        )
    # Everything so far goes into the main file.
    db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    # From here on writes stay in the write-ahead log, as they do while
    # Firefox is running: the reader must apply it to see them.
    db.execute("PRAGMA wal_autocheckpoint=0")
    db.execute(
        "INSERT INTO moz_bookmarks(id,type,fk,parent,position,title,dateAdded,lastModified,guid) VALUES (?,?,?,?,?,?,?,?,?)",
        (30, 1, 5, 6, 0, "Phone", added, added, "bmmobile0001"),
    )
    db.execute("UPDATE moz_places SET title='Visited (renamed)' WHERE id=6")
    # Copy while the connection is open, so the log is not folded back in.
    snapshot = os.path.join(HERE, "firefox-snapshot")
    os.makedirs(snapshot, exist_ok=True)
    shutil.copyfile(path, os.path.join(snapshot, "places.sqlite"))
    shutil.copyfile(path + "-wal", os.path.join(snapshot, "places.sqlite-wal"))
    db.close()
    fresh(path)
    shutil.move(os.path.join(snapshot, "places.sqlite"), path)
    shutil.move(os.path.join(snapshot, "places.sqlite-wal"), path + "-wal")
    os.rmdir(snapshot)


def safari():
    root = os.path.join(HERE, "safari")
    os.makedirs(root, exist_ok=True)

    def leaf(uuid, title, address):
        return {
            "WebBookmarkType": "WebBookmarkTypeLeaf",
            "WebBookmarkUUID": uuid,
            "URLString": address,
            "URIDictionary": {"title": title},
        }

    def folder(title, children):
        return {"WebBookmarkType": "WebBookmarkTypeList", "Title": title, "Children": children}

    tree = {
        "WebBookmarkType": "WebBookmarkTypeList",
        "Title": "",
        "WebBookmarkFileVersion": 1,
        "Children": [
            {"WebBookmarkType": "WebBookmarkTypeProxy", "Title": "History"},
            folder(
                "BookmarksBar",
                [
                    leaf("5A1D0000-0000-4000-8000-000000000001", "Apple example", "https://apple.example/"),
                    leaf("5A1D0000-0000-4000-8000-000000000002", "Bookmarklet", "javascript:void(0)"),
                ],
            ),
            folder(
                "BookmarksMenu",
                [folder("Dev", [leaf("5A1D0000-0000-4000-8000-000000000003", "Dev docs", "https://dev.example/")])],
            ),
            folder(
                "com.apple.ReadingList",
                [leaf("5A1D0000-0000-4000-8000-000000000004", "Later", "https://later.example/")],
            ),
        ],
    }
    with open(os.path.join(root, "Bookmarks.plist"), "wb") as f:
        plistlib.dump(tree, f, fmt=plistlib.FMT_BINARY)

    path = os.path.join(root, "History.db")
    fresh(path)
    db = sqlite3.connect(path)
    small(db)
    db.executescript(
        """
        CREATE TABLE history_items (id INTEGER PRIMARY KEY AUTOINCREMENT,url TEXT NOT NULL UNIQUE,domain_expansion TEXT NULL,visit_count INTEGER NOT NULL,daily_visit_counts BLOB NOT NULL,weekly_visit_counts BLOB NULL,autocomplete_triggers BLOB NULL,should_recompute_derived_visit_counts INTEGER NOT NULL,visit_count_score INTEGER NOT NULL);
        CREATE TABLE history_visits (id INTEGER PRIMARY KEY AUTOINCREMENT,history_item INTEGER NOT NULL REFERENCES history_items(id) ON DELETE CASCADE,visit_time REAL NOT NULL,title TEXT NULL,load_successful BOOLEAN NOT NULL DEFAULT 1,http_non_get BOOLEAN NOT NULL DEFAULT 0,synthesized BOOLEAN NOT NULL DEFAULT 0,redirect_source INTEGER NULL UNIQUE,redirect_destination INTEGER NULL UNIQUE,origin INTEGER NOT NULL DEFAULT 0,generation INTEGER NOT NULL DEFAULT 0,attributes INTEGER NOT NULL DEFAULT 0,score INTEGER NOT NULL DEFAULT 0);
        """
    )
    items = [
        (1, "https://apple.example/"),
        (2, "https://swift.example/book"),
        (3, "file:///Users/someone/notes.txt"),
        (4, "https://failed.example/"),
    ]
    for iid, address in items:
        db.execute(
            "INSERT INTO history_items(id,url,visit_count,daily_visit_counts,should_recompute_derived_visit_counts,visit_count_score) VALUES (?,?,?,?,?,?)",
            (iid, address, 1, b"", 0, 0),
        )
    secs = lambda ms: ms / 1000 - SAFARI_EPOCH_S
    visits = [
        (1, 1, secs(NOW_MS - 2 * 3_600_000), "Apple example", 1),
        (2, 2, secs(NOW_MS - 3 * 3_600_000), "The Swift book", 1),
        (3, 2, secs(NOW_MS - 200 * DAY_MS), "Too old", 1),
        (4, 3, secs(NOW_MS - 4 * 3_600_000), "Notes", 1),
        (5, 4, secs(NOW_MS - 5 * 3_600_000), "Did not load", 0),
    ]
    for vid, item, at, title, ok in visits:
        db.execute(
            "INSERT INTO history_visits(id,history_item,visit_time,title,load_successful) VALUES (?,?,?,?,?)",
            (vid, item, at, title, ok),
        )
    db.commit()
    db.close()


def hostile():
    """One profile per browser whose rows are far larger than a browser
    writes: titles of 50,000 characters, a 1,000-character guid and an
    address of 5,000. A reader must cut them while it reads, not after."""
    root = os.path.join(HERE, "hostile")
    big_title = "x" * 50_000
    big_url = "https://big.example/" + "u" * 5_000

    profile = os.path.join(root, "chromium")
    os.makedirs(profile, exist_ok=True)
    path = os.path.join(profile, "History")
    fresh(path)
    db = sqlite3.connect(path)
    small(db)
    db.executescript(
        """
        CREATE TABLE urls(id INTEGER PRIMARY KEY AUTOINCREMENT,url LONGVARCHAR,title LONGVARCHAR,visit_count INTEGER DEFAULT 0 NOT NULL,typed_count INTEGER DEFAULT 0 NOT NULL,last_visit_time INTEGER NOT NULL,hidden INTEGER DEFAULT 0 NOT NULL);
        CREATE TABLE visits(id INTEGER PRIMARY KEY AUTOINCREMENT,url INTEGER NOT NULL,visit_time INTEGER NOT NULL,from_visit INTEGER,transition INTEGER DEFAULT 0 NOT NULL,segment_id INTEGER,visit_duration INTEGER DEFAULT 0 NOT NULL);
        """
    )
    db.execute("INSERT INTO urls(id,url,title,last_visit_time) VALUES (1,?,?,0)", ("https://ok.example/", big_title))
    db.execute("INSERT INTO urls(id,url,title,last_visit_time) VALUES (2,?,?,0)", (big_url, "Big"))
    for vid, page in ((1, 1), (2, 2)):
        db.execute(
            "INSERT INTO visits(id,url,visit_time,transition) VALUES (?,?,?,1)",
            (vid, page, chromium_time(NOW_MS - vid * 3_600_000)),
        )
    db.commit()
    db.close()

    profile = os.path.join(root, "firefox")
    os.makedirs(profile, exist_ok=True)
    path = os.path.join(profile, "places.sqlite")
    fresh(path)
    db = sqlite3.connect(path)
    small(db)
    db.executescript(
        """
        CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR, hidden INTEGER DEFAULT 0 NOT NULL);
        CREATE TABLE moz_historyvisits (id INTEGER PRIMARY KEY, from_visit INTEGER, place_id INTEGER, visit_date INTEGER, visit_type INTEGER);
        CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER DEFAULT NULL, parent INTEGER, position INTEGER, title LONGVARCHAR, dateAdded INTEGER, guid TEXT);
        """
    )
    db.execute("INSERT INTO moz_places(id,url,title) VALUES (1,'https://ok.example/',?)", (big_title,))
    db.execute("INSERT INTO moz_places(id,url,title) VALUES (2,?,'Big')", (big_url,))
    rows = [
        (1, 2, None, 0, 0, "", "root________"),
        (2, 2, None, 1, 0, "toolbar", "toolbar_____"),
        (3, 1, 1, 2, 0, big_title, "g" * 1_000),
        (4, 1, 2, 2, 1, "Big", "bigurl000001"),
    ]
    for rid, kind, fk, parent, position, title, guid in rows:
        db.execute(
            "INSERT INTO moz_bookmarks(id,type,fk,parent,position,title,dateAdded,guid) VALUES (?,?,?,?,?,?,?,?)",
            (rid, kind, fk, parent, position, title, NOW_MS * 1000, guid),
        )
    for vid, place in ((1, 1), (2, 2)):
        db.execute(
            "INSERT INTO moz_historyvisits(id,place_id,visit_date,visit_type) VALUES (?,?,?,1)",
            (vid, place, (NOW_MS - vid * 3_600_000) * 1000),
        )
    db.commit()
    db.close()

    profile = os.path.join(root, "safari")
    os.makedirs(profile, exist_ok=True)
    path = os.path.join(profile, "History.db")
    fresh(path)
    db = sqlite3.connect(path)
    small(db)
    db.executescript(
        """
        CREATE TABLE history_items (id INTEGER PRIMARY KEY AUTOINCREMENT,url TEXT NOT NULL UNIQUE);
        CREATE TABLE history_visits (id INTEGER PRIMARY KEY AUTOINCREMENT,history_item INTEGER NOT NULL,visit_time REAL NOT NULL,title TEXT NULL,load_successful BOOLEAN NOT NULL DEFAULT 1);
        """
    )
    db.execute("INSERT INTO history_items(id,url) VALUES (1,'https://ok.example/')")
    db.execute("INSERT INTO history_items(id,url) VALUES (2,?)", (big_url,))
    for vid, item in ((1, 1), (2, 2)):
        db.execute(
            "INSERT INTO history_visits(id,history_item,visit_time,title) VALUES (?,?,?,?)",
            (vid, item, (NOW_MS - vid * 3_600_000) / 1000 - SAFARI_EPOCH_S, big_title),
        )
    db.commit()
    db.close()


if __name__ == "__main__":
    chromium()
    firefox()
    safari()
    hostile()
