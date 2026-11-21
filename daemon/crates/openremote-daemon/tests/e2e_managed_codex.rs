//! Managed Codex logins and chat starts use the same machine-local account.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use common::{TestDaemon, call, fixture_agent, spawn_daemon_process, until_status};
use serde_json::{Value, json};

const ACCOUNTS: &str = "/harnesses/codex/accounts";

async fn request(daemon: &TestDaemon, method: &str, path: &str, body: Option<Value>) -> Value {
    let reply = call(daemon, method, path, body).await;
    assert!(
        (200..300).contains(&reply.status),
        "{method} {path}: {}",
        reply.raw
    );
    reply.body
}

async fn add_account(daemon: &TestDaemon, root: &Path, label: &str) -> String {
    let created = request(
        daemon,
        "POST",
        ACCOUNTS,
        Some(json!({ "request_id": format!("create-{label}"), "label": label })),
    )
    .await;
    let id = created["result"]["id"].as_str().unwrap().to_owned();
    let sign_in = format!("{ACCOUNTS}/{id}/signin");
    let started = request(
        daemon,
        "POST",
        &sign_in,
        Some(json!({ "request_id": format!("login-{label}") })),
    )
    .await;
    let run = started["run_id"].as_str().unwrap();
    let poll = format!("{sign_in}/{run}");
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let view = request(daemon, "GET", &poll, None).await;
            if view["lines"].as_array().unwrap().len() == 2 {
                assert_eq!(
                    view["lines"],
                    json!(["https://auth.openai.com/codex/device", "FIXT-URE8"])
                );
                break;
            }
            assert!(view["done"].is_null(), "{view}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        // The fixture's login exits once its provider wrote private credentials.
        let claims = json!({
            "https://api.openai.com/auth": {
                "chatgpt_user_id": format!("fixture-{label}"),
                "chatgpt_account_id": "workspace-1"
            }
        });
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let auth = json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": format!("eyJhbGciOiJSUzI1NiJ9.{payload}.c2ln"),
                "access_token": "fixture-access",
                "refresh_token": "fixture-refresh"
            }
        });
        let stage = root.join("sign-ins").join(&id).join(run);
        openremote_core::fsx::write_private(
            &stage.join("auth.json"),
            &serde_json::to_vec(&auth).unwrap(),
        )
        .unwrap();
        loop {
            let view = request(daemon, "GET", &poll, None).await;
            if !view["done"].is_null() {
                assert_eq!(view["done"]["ok"], true, "{view}");
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("fixture sign-in settles");
    id
}

#[tokio::test]
async fn two_codex_accounts_keep_distinct_homes_on_start_and_resume() {
    let workspace = tempfile::tempdir().unwrap();
    let bin = workspace.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let codex = bin.join(if cfg!(windows) { "codex.exe" } else { "codex" });
    std::fs::copy(fixture_agent(), &codex).unwrap();
    let path = std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let default_home = workspace.path().join("default");
    std::fs::create_dir(&default_home).unwrap();
    let default_env = default_home.to_str().unwrap().to_string();
    let daemon = spawn_daemon_process(&[
        ("PATH", path.into_string().unwrap()),
        ("FIXTURE_CODEX_DEVICE_AUTH", "1".into()),
        ("CODEX_HOME", default_env.clone()),
        ("CODEX_SQLITE_HOME", default_env),
        ("OPENAI_API_KEY", "fixture-default-key".into()),
    ])
    .await;
    let root = daemon.data_dir().join("app-accounts").join("codex");
    let personal = add_account(&daemon, &root, "Personal").await;
    let work = add_account(&daemon, &root, "Work").await;

    let accounts = request(&daemon, "GET", ACCOUNTS, None).await;
    assert_eq!(
        accounts,
        json!({ "accounts": [
            {"id": personal, "label": "Personal", "status": "ready"},
            {"id": work, "label": "Work", "status": "ready"}
        ] })
    );

    let mut sessions = Vec::new();
    for account in [&personal, &work] {
        let reply = request(
            &daemon,
            "POST",
            "/sessions/with-app-account",
            Some(json!({
                "request_id": format!("start-{account}"),
                "harness": "codex",
                "workspace": workspace.path(),
                "app_account_id": account,
            })),
        )
        .await;
        let id = reply["id"].as_str().unwrap().to_owned();
        let row = until_status(&daemon, &id, &["idle"]).await;
        assert_eq!(row["app_account_id"].as_str(), Some(account.as_str()));
        sessions.push((id, account));
    }
    for (id, account) in sessions.into_iter().rev() {
        for action in ["stop", "resume"] {
            let path = format!("/sessions/{id}/{action}");
            request(
                &daemon,
                "POST",
                &path,
                Some(json!({ "request_id": format!("{action}-{id}") })),
            )
            .await;
        }
        let home = root.join("homes").join(account);
        assert_eq!(
            profile_calls(home),
            ["thread/start", "thread/resume"],
            "the same account owns both process launches"
        );
    }
    assert!(!default_home.join("fixture-managed-calls").exists());
    assert!(!default_home.join("auth.json").exists());
}

fn profile_calls(home: PathBuf) -> Vec<String> {
    std::fs::read_to_string(home.join("fixture-managed-calls"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}
