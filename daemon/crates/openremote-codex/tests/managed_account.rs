use std::ffi::OsStr;
use std::path::Path;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use openremote_codex::{configure_managed_account, managed_identity_from_auth};
use serde_json::{Value, json};
use tokio::process::Command;

fn token(auth_claims: Value) -> String {
    let claims = json!({ "https://api.openai.com/auth": auth_claims });
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
    format!("eyJhbGciOiJSUzI1NiJ9.{payload}.c2ln")
}

fn account_token(user: &str, workspace: &str) -> String {
    token(json!({ "chatgpt_user_id": user, "chatgpt_account_id": workspace }))
}

fn auth(token: &str, workspace: &str) -> Value {
    json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": token,
            "access_token": "fixture-access",
            "refresh_token": "fixture-refresh",
            "account_id": workspace
        }
    })
}

fn identity(auth: &Value) -> Option<String> {
    managed_identity_from_auth(&serde_json::to_vec(auth).unwrap())
}

#[test]
fn reconnect_requires_the_same_user_and_workspace_across_refreshes() {
    let personal_token = account_token("user-1", "workspace-1");
    let mut personal = auth(&personal_token, "workspace-1");
    let before = identity(&personal).unwrap();
    personal["tokens"]["access_token"] = json!("new-fixture-access");
    assert_eq!(identity(&personal).as_deref(), Some(before.as_str()));
    assert_eq!(
        identity(&auth(&token(json!({"user_id": "user-1"})), "workspace-1")),
        Some(before.clone())
    );
    assert_ne!(
        identity(&auth(
            &account_token("user-1", "workspace-2"),
            "workspace-2"
        )),
        Some(before.clone())
    );
    assert_ne!(
        identity(&auth(
            &account_token("user-2", "workspace-1"),
            "workspace-1"
        )),
        Some(before)
    );
    assert_eq!(identity(&auth(&personal_token, "workspace-2")), None);
}

#[test]
fn api_keys_missing_refresh_material_and_malformed_tokens_do_not_identify_an_account() {
    let personal_token = account_token("user-1", "workspace-1");
    let mut api_key = auth(&personal_token, "workspace-1");
    api_key["auth_mode"] = json!("apikey");
    assert_eq!(identity(&api_key), None);
    let mut incomplete = auth(&personal_token, "workspace-1");
    incomplete["tokens"]["refresh_token"] = json!("");
    assert_eq!(identity(&incomplete), None);
    assert_eq!(
        identity(&auth("one.invalid.signature", "workspace-1")),
        None
    );
    assert_eq!(
        managed_identity_from_auth(&vec![b' '; 128 * 1024 + 1]),
        None
    );
}

#[test]
fn each_managed_process_uses_its_own_home_and_ignores_inherited_credentials() {
    for profile in ["personal", "work"] {
        let mut command = Command::new("codex");
        command.args(["app-server", "--listen", "stdio://"]);
        command.env("OPENAI_API_KEY", "fixture-global-key");
        command.env("CODEX_ACCESS_TOKEN", "fixture-global-token");
        let home = Path::new("managed").join(profile);
        configure_managed_account(&mut command, &home);

        let command = command.as_std();
        let environments: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(
            environments[OsStr::new("CODEX_HOME")],
            Some(home.as_os_str())
        );
        assert_eq!(
            environments[OsStr::new("CODEX_SQLITE_HOME")],
            Some(home.as_os_str())
        );
        for name in [
            "OPENAI_API_KEY",
            "CODEX_API_KEY",
            "CODEX_ACCESS_TOKEN",
            "CODEX_EXEC_SERVER_URL",
            "OPENAI_IDENTITY_TOKEN_FILE",
            "OPENAI_WORKLOAD_IDENTITY_CONTEXT",
        ] {
            assert_eq!(environments[OsStr::new(name)], None);
        }
        let args = command.get_args().collect::<Vec<_>>();
        assert!(args.contains(&OsStr::new("cli_auth_credentials_store=\"file\"")));
        assert!(args.contains(&OsStr::new("forced_login_method=\"chatgpt\"")));
        assert!(args.contains(&OsStr::new(
            "chatgpt_base_url=\"https://chatgpt.com/backend-api/\""
        )));
        assert!(args.contains(&OsStr::new(
            "openai_base_url=\"https://chatgpt.com/backend-api/codex\""
        )));
    }
}
