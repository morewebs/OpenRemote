//! What ad and tracker blocking decides, with no WebView2 in it: the
//! engine built from the user's picked lists, which requests it may filter
//! at all, and the hide rules a page gets. Kept apart from the hooks so all
//! of it is tested without a window.
//!
//! A list is someone else's text. It reaches here already checked by the
//! daemon, and is checked again as it is read: a size cap, the format line,
//! overlong or control-character lines dropped, a cap on rules, and the
//! engine built inside `catch_unwind`. The engine's regex rules compile
//! with the `regex` crate, which matches in linear time, so no rule can
//! make a request take long to check.

use std::collections::HashSet;
use std::net::IpAddr;
use std::panic::{catch_unwind, AssertUnwindSafe};

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use tauri::Url;

/// More than any list on offer weighs; a bigger file is not read.
pub const MAX_LIST_BYTES: u64 = 16 << 20;
/// A rule longer than this is dropped.
pub const MAX_LINE: usize = 4096;
/// Rules read from one list.
pub const MAX_RULES: usize = 300_000;
/// Classes and ids a page may hand over for the class and id rules.
pub const MAX_NAMES: usize = 5_000;

/// A list's text as the engine reads it: `None` when it is not a filter
/// list. Bounded whatever the file holds.
pub fn clean(text: &str) -> Option<String> {
    let text = text.trim_start_matches('\u{feff}');
    if !text.trim_start().starts_with("[Adblock") {
        return None;
    }
    let mut out = String::with_capacity(text.len().min(MAX_LIST_BYTES as usize));
    for line in text
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| l.len() <= MAX_LINE && !l.chars().any(|c| c.is_control() && c != '\t'))
        .take(MAX_RULES)
    {
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

/// The engine for these lists' texts: `None` when none of them is a list
/// (nothing to block with), or when building it failed.
pub fn engine_from(texts: impl IntoIterator<Item = String>) -> Option<Engine> {
    let mut set = FilterSet::new(false);
    let mut any = false;
    for text in texts {
        if let Some(text) = clean(&text) {
            set.add_filter_list(text, ParseOptions::default());
            any = true;
        }
    }
    if !any {
        return None;
    }
    catch_unwind(AssertUnwindSafe(|| Engine::new_with_filter_set(set))).ok()
}

/// The engine for the lists the daemon kept, read from these files.
pub fn build(files: &[String]) -> Option<Engine> {
    engine_from(files.iter().filter_map(|path| {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > MAX_LIST_BYTES {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }))
}

/// Whether the lists block this request (never panicking: a request the
/// engine trips on goes through).
pub fn blocks(engine: &Engine, url: &str, source: &str, kind: &str, method: &str) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        Request::new(url, source, kind, method)
            .map(|request| engine.check_network_request(&request).should_block())
            .unwrap_or(false)
    }))
    .unwrap_or(false)
}

/// The user's allowed sites as the page matching wants them.
pub fn sites(raw: Vec<String>) -> Vec<String> {
    raw.into_iter()
        .map(|s| s.trim().trim_end_matches('.').to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// An address's host, and whether it names this computer or its network:
/// the user's own dev servers, devices and this app's daemon (127.0.0.1)
/// are never filtered.
pub fn host_of(address: &str) -> Option<(String, bool)> {
    let url = Url::parse(address).ok()?;
    let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let local = match bare.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        Ok(IpAddr::V6(ip)) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|v4| v4.is_loopback() || v4.is_private() || v4.is_link_local())
        }
        Err(_) => host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local"),
    };
    Some((host, local))
}

/// Whether two addresses are one document: the same but for a fragment
/// and for how a bare host is written.
pub fn same_document(a: &str, b: &str) -> bool {
    match (Url::parse(a), Url::parse(b)) {
        (Ok(mut a), Ok(mut b)) => {
            a.set_fragment(None);
            b.set_fragment(None);
            a == b
        }
        _ => a == b,
    }
}

/// Whether `host` is one of the allowed sites or below one.
pub fn site_allowed(allow: &[String], host: &str) -> bool {
    allow.iter().any(|site| {
        host == site
            || host
                .strip_suffix(site.as_str())
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

/// Whether blocking may filter a request at all. Never filtered: what isn't web
/// traffic, anything on a page the user allows ads on or on their own
/// machine or network, anything going to their own machine or network
/// (the daemon included), and a tab's own page - a document request that
/// is the address the tab shows or is going to (`going`). Only what a
/// page asks for is ever filtered.
pub fn may_filter(
    page: &str,
    url: &str,
    is_document: bool,
    going: Option<&str>,
    allow: &[String],
) -> bool {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return false;
    }
    let (page_host, page_local) = host_of(page).unwrap_or_default();
    if page_local || site_allowed(allow, &page_host) {
        return false;
    }
    let Some((_, false)) = host_of(url) else {
        return false;
    };
    if is_document
        && (same_document(url, page) || going.is_none_or(|going| same_document(url, going)))
    {
        return false;
    }
    true
}

/// Whether a page gets the lists' hide rules: web pages only, and not on
/// the user's own network or a site they allow ads on.
pub fn may_hide(page: &str, allow: &[String]) -> bool {
    page.starts_with("http")
        && host_of(page).is_some_and(|(host, local)| !local && !site_allowed(allow, &host))
}

/// A script that hides these selectors in the page: one rule each, so a
/// selector the page can't parse costs only itself. The selectors travel
/// as a JSON string, so a list cannot write script; one with a brace in it
/// could close its own rule and is dropped.
pub fn hide_script(selectors: &[String]) -> Option<String> {
    let usable: Vec<&String> = selectors
        .iter()
        .filter(|s| !s.is_empty() && s.len() < 1000 && !s.contains(['{', '}']))
        .take(MAX_NAMES)
        .collect();
    if usable.is_empty() {
        return None;
    }
    let list = serde_json::to_string(&usable).ok()?;
    Some(format!(
        "(()=>{{try{{const s=new CSSStyleSheet();for(const r of {list}){{try{{s.insertRule(r+'{{display:none!important}}')}}catch(e){{}}}}document.adoptedStyleSheets=[...document.adoptedStyleSheets,s]}}catch(e){{}}}})()"
    ))
}

/// The classes and ids a page uses, for the lists' class and id rules (a
/// bounded number of each).
pub const COLLECT: &str = "(()=>{const c=new Set(),i=new Set();for(const e of document.querySelectorAll('[class],[id]')){for(const k of e.classList)if(c.size<5000)c.add(k);if(e.id&&i.size<5000)i.add(e.id)}return[[...c],[...i]]})()";

/// The class and id rules for what a page uses, from [`COLLECT`]'s answer.
pub fn class_id_selectors(
    engine: &Engine,
    json: &str,
    exceptions: &HashSet<String>,
) -> Vec<String> {
    let Ok((classes, ids)) = serde_json::from_str::<(Vec<String>, Vec<String>)>(json) else {
        return Vec::new();
    };
    let classes = classes.into_iter().take(MAX_NAMES);
    let ids = ids.into_iter().take(MAX_NAMES);
    catch_unwind(AssertUnwindSafe(|| {
        engine.hidden_class_id_selectors(classes, ids, exceptions)
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const LIST: &str = "[Adblock Plus 2.0]\n! Title: test\n||ads.example^\n||track.example^$third-party\n##.ad-banner\nnews.test##.sponsor\n";

    #[test]
    fn a_site_allows_its_own_pages_and_the_ones_below_it() {
        let allow = sites(vec!["Example.com".into(), "shop.test.".into()]);
        assert!(site_allowed(&allow, "example.com"));
        assert!(site_allowed(&allow, "news.example.com"));
        assert!(site_allowed(&allow, "a.b.shop.test"));
        assert!(!site_allowed(&allow, "notexample.com"));
        assert!(!site_allowed(&allow, "example.com.evil.test"));
        assert!(!site_allowed(&[], "example.com"));
    }

    #[test]
    fn the_users_own_machine_and_network_are_never_filtered() {
        for local in [
            "http://localhost:3000/",
            "http://app.localhost/",
            "http://127.0.0.1:5175/",
            "http://127.0.0.1:8730/sessions",
            "http://192.168.1.20/",
            "http://10.0.0.5/",
            "http://[::1]:8080/",
            "http://[::ffff:127.0.0.1]/",
            "http://[fe80::1]/",
            "http://0.0.0.0/",
            "http://printer.local/",
        ] {
            assert!(host_of(local).unwrap().1, "{local} is local");
        }
        for web in ["https://example.com/", "http://93.184.216.34/"] {
            assert!(!host_of(web).unwrap().1, "{web} is the web");
        }
    }

    #[test]
    fn the_console_and_the_daemon_are_never_filtered() {
        let allow = Vec::new();
        // The daemon, asked from a web page or from anywhere else.
        assert!(!may_filter(
            "https://news.test/",
            "http://127.0.0.1:8730/sessions",
            false,
            None,
            &allow
        ));
        assert!(!may_filter(
            "",
            "http://localhost:5175/src/main.jsx",
            false,
            None,
            &allow
        ));
        // The console's own addresses are not web traffic.
        for own in [
            "tauri://localhost/",
            "http://tauri.localhost/",
            "ipc://localhost/x",
            "asset://x",
        ] {
            assert!(
                !may_filter("https://news.test/", own, false, None, &allow),
                "{own}"
            );
        }
        // Anything a page on the user's own machine asks for.
        assert!(!may_filter(
            "http://localhost:3000/",
            "https://ads.example/a.js",
            false,
            None,
            &allow
        ));
    }

    #[test]
    fn a_tabs_own_page_is_never_filtered_only_what_it_asks_for() {
        let allow = Vec::new();
        let page = "https://news.test/story";
        assert!(!may_filter(
            page,
            "https://news.test/story#top",
            true,
            None,
            &allow
        ));
        // Going somewhere new: that document is the page too.
        assert!(!may_filter(
            page,
            "https://ads.example/",
            true,
            Some("https://ads.example/"),
            &allow
        ));
        // A frame inside the page is filtered.
        assert!(may_filter(
            page,
            "https://ads.example/frame",
            true,
            Some(page),
            &allow
        ));
        assert!(may_filter(
            page,
            "https://ads.example/a.js",
            false,
            Some(page),
            &allow
        ));
        // An allowed site is left alone.
        assert!(!may_filter(
            page,
            "https://ads.example/a.js",
            false,
            None,
            &sites(vec!["news.test".into()])
        ));
    }

    #[test]
    fn hide_rules_go_in_one_at_a_time_and_cannot_close_their_own_rule() {
        let script = hide_script(&[
            ".ad-banner".into(),
            "a{color:red}".into(),
            "".into(),
            "#sponsor".into(),
            "\"]);alert(1);//".into(),
        ])
        .unwrap();
        assert!(script.contains(r##"".ad-banner","#sponsor""##));
        assert!(!script.contains("color:red"));
        // A quote in a selector stays inside its JSON string.
        assert!(script.contains(r#""\"]);alert(1);//""#));
        assert!(hide_script(&[]).is_none());
    }

    #[test]
    fn a_page_is_one_document_whatever_follows_its_hash() {
        assert!(same_document("https://a.test", "https://a.test/#top"));
        assert!(same_document(
            "https://a.test/x?y=1#a",
            "https://a.test/x?y=1"
        ));
        assert!(!same_document("https://a.test/x", "https://a.test/y"));
    }

    #[test]
    fn the_lists_block_an_advert_and_leave_a_page_alone() {
        let engine = engine_from([LIST.to_string()]).unwrap();
        assert!(blocks(
            &engine,
            "https://ads.example/a.js",
            "https://news.test/",
            "script",
            "GET"
        ));
        assert!(blocks(
            &engine,
            "https://track.example/p.gif",
            "https://news.test/",
            "image",
            "GET"
        ));
        assert!(!blocks(
            &engine,
            "https://news.test/app.js",
            "https://news.test/",
            "script",
            "GET"
        ));
        let found = engine.url_cosmetic_resources("https://news.test/");
        assert!(found.hide_selectors.contains(".sponsor"));
        let generic = class_id_selectors(
            &engine,
            r#"[["ad-banner","plain"],["x"]]"#,
            &found.exceptions,
        );
        assert_eq!(generic, vec![".ad-banner".to_string()]);
    }

    #[test]
    fn no_list_means_no_engine() {
        assert!(engine_from(Vec::<String>::new()).is_none());
        assert!(engine_from(["<html>sign in</html>".to_string()]).is_none());
        assert!(build(&["C:/no/such/list.txt".into()]).is_none());
    }

    #[test]
    fn a_file_over_the_cap_is_not_read() {
        let dir = std::env::temp_dir().join(format!("or-adblock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.txt");
        let mut text = String::from("[Adblock Plus 2.0]\n||ads.example^\n");
        text.push_str(&"!".repeat(MAX_LIST_BYTES as usize));
        std::fs::write(&big, &text).unwrap();
        assert!(build(&[big.to_string_lossy().into_owned()]).is_none());
        let small = dir.join("small.txt");
        std::fs::write(&small, LIST).unwrap();
        assert!(build(&[small.to_string_lossy().into_owned()]).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Lists built to hurt: catastrophic-looking regexes, overlong and
    /// broken rules, control bytes, odd options. Building and matching
    /// must neither panic nor take long.
    #[test]
    fn a_hostile_list_neither_panics_nor_stalls() {
        let mut nasty = String::from("[Adblock Plus 2.0]\n");
        for rule in [
            "/(a+)+$/",
            "/(a|aa)+$/",
            "/((((((((((a*)*)*)*)*)*)*)*)*)*)*b/",
            "/^(\\w+\\s?)*$/$script",
            "/[/",
            "/(/",
            "/\\/",
            "/(?<name>a)\\k<name>/",
            "||*$*",
            "||$",
            "$$$$$$$$",
            "@@",
            "@@||",
            "|",
            "||",
            "^",
            "*",
            "##",
            "#@#",
            "#?#:has(:has(:has(:has(a))))",
            "##+js(nosuch)",
            "example.com##+js()",
            "$domain=",
            "||a.test^$domain=|||~|",
            "||a.test^$redirect=",
            "||a.test^$csp=script-src 'none'",
            "||a.test^$removeparam=/(a+)+/",
            "||a.test^$1p,3p,~1p,~3p,important,badfilter",
            "||\u{1F600}.test^",
            "||xn--.test^",
            "##div:style(display: block !important)",
            "##a[href^=\"",
            "##:not(",
        ] {
            nasty.push_str(rule);
            nasty.push('\n');
        }
        nasty.push_str(&"a".repeat(MAX_LINE + 10));
        nasty.push('\n');
        nasty.push_str("||nul\0.test^\n");
        for i in 0..20_000 {
            nasty.push_str(&format!("/x{i}(a+)+y/\n"));
        }
        let started = Instant::now();
        let engine =
            engine_from([nasty.clone(), "\u{feff}\u{feff}[Adblock".into()]).expect("an engine");
        let long = format!("https://ads.test/{}!", "a".repeat(50_000));
        for (url, kind) in [
            (long.as_str(), "script"),
            (
                "https://a.test/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!",
                "xhr",
            ),
            ("https://\u{1F600}.test/", "image"),
            ("https://[::1]/", "other"),
            (
                "https://a.test/?a=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "sub_frame",
            ),
        ] {
            let _ = blocks(&engine, url, "https://news.test/", kind, "GET");
            let _ = blocks(&engine, url, "not a url", kind, "\0");
        }
        let found = engine.url_cosmetic_resources("https://example.com/");
        let _ = class_id_selectors(&engine, "[[\"a\"],[\"b\"]]", &found.exceptions);
        let _ = class_id_selectors(&engine, "not json", &found.exceptions);
        let _ = class_id_selectors(&engine, "[[1,2],[null]]", &found.exceptions);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
        // And the cleaning itself bounds what the engine is handed.
        let cleaned = clean(&nasty).unwrap();
        assert!(cleaned.lines().all(|l| l.len() <= MAX_LINE));
        assert!(!cleaned.contains('\0'));
    }

    #[test]
    fn a_list_is_cut_at_the_rule_cap() {
        let mut many = String::from("[Adblock Plus 2.0]\n");
        for i in 0..(MAX_RULES + 5) {
            many.push_str(&format!("||{i}.test^\n"));
        }
        assert_eq!(clean(&many).unwrap().lines().count(), MAX_RULES);
    }
}
