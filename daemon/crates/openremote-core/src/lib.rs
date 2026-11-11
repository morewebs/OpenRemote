//! OpenRemote core domain: sessions, turns, events, decisions, receipts.
//!
//! The daemon-neutral vocabulary (the harness payload rides inside verbatim;
//! parity is about the options the user sees, not our data model):
//!
//! - **Harness** - an installed agent CLI the daemon can drive.
//! - **Session** - one conversation with one harness in one workspace.
//!   Daemon-generated UUID identity; the harness's own id is *data*
//!   (`harness_session_ref`), never identity. The console's UI word is *chat*.
//! - **Turn** - one prompt→result cycle; per-session monotonic.
//! - **Event** - append-only, per-session, monotonic `seq`; the console renders
//!   only from events.
//! - **Decision** - one concept, two kinds (approval | question). The daemon
//!   never invents resolution events: retiring emits nothing; late answers
//!   get an explicit error.
//! - **Receipt** - request dedup for every mutating call. Crash between
//!   `accepted` and a terminal state is surfaced as `unknown`; uncertain work
//!   is never resent.

pub mod blobs;
pub mod events;
pub mod fsx;
pub mod ids;
pub mod model;
// The store is the desktop daemon's on-disk memory: paths, files and random
// ids. A browser peer only converts ids it already has, so this module is not
// part of the WebAssembly build.
#[cfg(not(target_arch = "wasm32"))]
pub mod store;

pub use events::{Event, EventPayload};
pub use ids::{DecisionId, DeviceId, MachineId, ProjectId, RuleId, SessionId};
pub use model::{
    ApprovedCommand, AutomationRule, ChatMessage, Decision, DecisionKind, DecisionOption,
    DecisionState, EndpointAuthKind, EndpointModel, EndpointSecret, Harness, HarnessHonors,
    ImageRef, InstallSpec, InstalledPlugin, LOCAL_MARKETPLACE, Machine, MachineStatus, MachineView,
    MarketplaceRecord, MarketplaceSource, McpServerEntry, MessageRole, ModelEndpoint,
    ModelEndpointView, Origin, ParentLink, PluginHarnessRef, PluginPower, PluginRuntime,
    PluginScope, PluginSource, Project, QueuedMessage, Receipt, ReceiptStatus, Session,
    SessionStatus, Trigger, TriggerKind, TurnOutcome, endpoint_models_url,
    normalize_endpoint_base_url, normalize_endpoint_models_path, validate_endpoint_harness,
    validate_endpoint_name, validate_model_id,
};
// Same reason as mod store: the desktop's store type is not in a browser build.
#[cfg(not(target_arch = "wasm32"))]
pub use store::Store;

/// This computer's name: COMPUTERNAME on Windows, HOSTNAME when it is
/// exported, else the kernel's own name. On Linux HOSTNAME is usually a shell
/// variable that never reaches child processes, so the environment alone
/// named every Linux computer "this-computer".
pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| {
            // A phone has no hostname command, and spawning one inside an
            // app process can hang the caller: the forked child that
            // never execs holds the vforked thread that asked. The phone
            // passes its own name (Build.MODEL) instead.
            #[cfg(any(target_os = "android", target_os = "ios"))]
            {
                None
            }
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                std::process::Command::new("hostname")
                    .output()
                    .ok()
                    .and_then(|out| String::from_utf8(out.stdout).ok())
            }
        })
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "this-computer".to_string())
}

/// A chat's name: the first line of its first prompt, at most 44 characters
/// (the console's own rule).
pub fn title_from(text: &str) -> String {
    let first = text.trim().lines().next().unwrap_or("");
    if first.chars().count() > 44 {
        let cut: String = first.chars().take(44).collect();
        format!("{}…", cut.trim_end())
    } else {
        first.to_string()
    }
}

/// Compares two secrets in time that doesn't depend on where they differ.
pub fn same_secret(presented: &str, expected: &str) -> bool {
    let (a, b) = (presented.as_bytes(), expected.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A word that is about to become one argv element as a flag's value.
///
/// Two shapes are never a legitimate value. An empty word makes the harness
/// read the flag as value-less and take the following word instead, so the
/// caller's next argument becomes the value. A dash-leading word hands its
/// meaning to the harness's parser, and what such a parser picks up is another
/// flag. Either way a user's chosen word becomes part of the command line's
/// grammar instead of staying data. What the word may say is the harness's own
/// business (its model names, its effort tiers, its approval words); this
/// refuses only the shapes that can change what the command means.
pub fn validate_argv_word(word: &str) -> Result<(), String> {
    if word.trim().is_empty() {
        return Err("it is empty".to_string());
    }
    if word.starts_with('-') {
        return Err("it starts with a dash".to_string());
    }
    if word.contains('\0') {
        return Err("it contains a NUL".to_string());
    }
    Ok(())
}

/// Millis since the Unix epoch - the crate's clock surface, so tests can pin time.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_argv_word_may_not_be_a_flag_or_empty() {
        // Real words from every harness's own vocabulary stay welcome.
        assert!(validate_argv_word("sonnet").is_ok());
        assert!(validate_argv_word("claude-opus-4-5-20260301").is_ok());
        assert!(validate_argv_word("bypassPermissions").is_ok());
        assert!(validate_argv_word("xhigh").is_ok());
        // Shapes that make a parser read the word as the command's grammar.
        assert!(validate_argv_word("--dangerously-skip-permissions").is_err());
        assert!(validate_argv_word("-x").is_err());
        assert!(validate_argv_word("").is_err());
        assert!(validate_argv_word("   ").is_err());
        assert!(validate_argv_word("a\0b").is_err());
    }

    #[test]
    fn hostname_answers_quickly_and_never_empty() {
        // The Android hang: a `hostname` spawn that never execs held the
        // vforked caller forever (the daemon never came up). Every path,
        // mobile included, must answer fast with something. No length
        // cap here: a macOS runner's hostname is longer than 64 chars,
        // and the registry applies its own limit at registration.
        let started = std::time::Instant::now();
        let name = hostname();
        assert!(!name.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "hostname took {:?}",
            started.elapsed()
        );
    }
}
