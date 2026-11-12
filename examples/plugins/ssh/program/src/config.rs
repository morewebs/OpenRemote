//! The plugin's settings, checked. Everything here reaches ssh's command
//! line or its environment, so each value is held to a shape that cannot
//! say anything else: a host is a name, a user, an address and a port, each
//! its own argument; a key is a file's path, never the key; ssh is a full
//! path, never a name looked up on PATH.
//!
//! None of the settings is a secret, and none may hold one. A private key
//! or a password typed into one is refused and never used, and the refusal
//! does not repeat what was typed.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub const MAX_HOSTS: usize = 100;
const MAX_NAME: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    /// What the agent and the user call it.
    pub name: String,
    pub user: Option<String>,
    /// A host name, an IPv4 address, or an IPv6 address without brackets.
    pub address: String,
    pub port: u16,
}

impl Host {
    /// How ssh files the host's key in known_hosts.
    pub fn known_hosts_field(&self) -> String {
        if self.port == 22 && !self.address.contains(':') {
            self.address.clone()
        } else {
            format!("[{}]:{}", self.address, self.port)
        }
    }

    /// user@address:port, as people read it.
    pub fn shown(&self) -> String {
        let user = self
            .user
            .as_ref()
            .map(|u| format!("{u}@"))
            .unwrap_or_default();
        let address = if self.address.contains(':') {
            format!("[{}]", self.address)
        } else {
            self.address.clone()
        };
        if self.port == 22 {
            format!("{user}{address}")
        } else {
            format!("{user}{address}:{}", self.port)
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub hosts: Vec<Host>,
    /// Entries of the hosts setting that were left out, each with why.
    /// They name the entry, never repeat it: it may hold a password.
    pub left_out: Vec<String>,
    pub key_file: Option<Result<PathBuf, String>>,
    pub agent_socket: Option<Result<String, String>>,
    pub ssh: Result<PathBuf, String>,
}

fn setting<'a>(settings: &'a Value, key: &str) -> &'a str {
    settings
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
}

fn has_control(s: &str) -> bool {
    s.chars().any(|c| c.is_control())
}

/// Whether a value looks like key material rather than a path to it.
pub fn looks_like_a_key(value: &str) -> bool {
    let v = value.trim();
    v.contains("-----BEGIN")
        || v.contains("PRIVATE KEY")
        || v.starts_with("ssh-")
        || v.starts_with("ecdsa-sha2-")
        || v.starts_with("sk-ssh-")
        || v.starts_with("sk-ecdsa-")
        || v.starts_with("PuTTY-User-Key-File")
}

/// The user's home folder, from the variables the daemon passes.
pub fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .or_else(|| std::env::var_os("HOME"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        if let Some(h) = home() {
            return h;
        }
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        if let Some(h) = home() {
            return h.join(rest);
        }
    }
    PathBuf::from(value)
}

fn key_file(value: &str) -> Result<PathBuf, String> {
    if looks_like_a_key(value) {
        return Err(
            "The Key file setting holds what looks like a key itself. Clear it, and give the path of the key file instead (like ~/.ssh/id_ed25519): the key is never typed or kept in OpenRemote."
                .into(),
        );
    }
    if has_control(value) || value.len() > 1024 {
        return Err("The Key file setting must be one path, like ~/.ssh/id_ed25519.".into());
    }
    let path = expand_home(value);
    if !path.is_absolute() {
        return Err("The Key file setting must be a full path, or start with ~/.".into());
    }
    if !path.is_file() {
        return Err(format!(
            "The key file {} was not found on this computer. Check its path in Settings > Plugins > SSH.",
            path.display()
        ));
    }
    Ok(path)
}

fn agent_socket(value: &str) -> Result<String, String> {
    if has_control(value) || value.len() > 1024 || looks_like_a_key(value) {
        return Err("The ssh-agent setting must be the path of the agent's socket.".into());
    }
    let path = expand_home(value);
    if !path.is_absolute() {
        return Err(
            "The ssh-agent setting must be a full path, like /run/user/1000/ssh-agent.socket (the value of SSH_AUTH_SOCK in your terminal).".into(),
        );
    }
    Ok(path.to_string_lossy().into_owned())
}

/// Where the system keeps its ssh, looked for in fixed places only.
fn default_ssh_paths() -> Vec<PathBuf> {
    if cfg!(windows) {
        let mut paths = Vec::new();
        let root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        paths.push(root.join(r"System32\OpenSSH\ssh.exe"));
        let programs = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        paths.push(programs.join(r"OpenSSH\ssh.exe"));
        paths.push(programs.join(r"Git\usr\bin\ssh.exe"));
        paths
    } else {
        [
            "/usr/bin/ssh",
            "/usr/local/bin/ssh",
            "/opt/homebrew/bin/ssh",
            "/bin/ssh",
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    }
}

fn ssh_program(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() {
        let candidates = default_ssh_paths();
        return candidates.iter().find(|p| p.is_file()).cloned().ok_or_else(|| {
            format!(
                "ssh was not found at {}. Install the OpenSSH client (on Windows, the \"OpenSSH Client\" optional feature), or set its full path in Settings > Plugins > SSH.",
                candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        });
    }
    if has_control(value) {
        return Err("Path to ssh must be one full path.".into());
    }
    let path = expand_home(value);
    if !path.is_absolute() {
        return Err(
            "Path to ssh must be a full path, like /usr/bin/ssh or C:\\Windows\\System32\\OpenSSH\\ssh.exe: ssh is never looked up on PATH."
                .into(),
        );
    }
    if !path.is_file() {
        return Err(format!("ssh was not found at {}.", path.display()));
    }
    Ok(path)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn valid_user(user: &str) -> bool {
    let bytes = user.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && (bytes[0].is_ascii_alphanumeric() || bytes[0] == b'_')
        && bytes
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn valid_hostname(host: &str) -> bool {
    host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

fn valid_ipv6(host: &str) -> bool {
    host.contains(':') && host.len() <= 45 && host.parse::<std::net::Ipv6Addr>().is_ok()
}

/// One `name=[user@]address[:port]` entry.
fn parse_entry(entry: &str) -> Result<Host, String> {
    let (name, rest) = entry
        .split_once('=')
        .ok_or("is not written as name=user@address")?;
    let name = name.trim();
    if !valid_name(name) {
        return Err(format!(
            "has a name that is not lowercase letters, digits and single dashes (up to {MAX_NAME})"
        ));
    }
    let rest = rest.trim();
    if rest.is_empty() || rest.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("has an address with spaces in it".into());
    }
    let (user, address) = match rest.split_once('@') {
        Some((user, address)) => {
            if !valid_user(user) {
                return Err(
                    "has a user that is not a plain account name (only keys sign in here, so no password is written in it)"
                        .into(),
                );
            }
            (Some(user.to_string()), address)
        }
        None => (None, rest),
    };
    let (host, port) = if let Some(inner) = address.strip_prefix('[') {
        let (host, after) = inner.split_once(']').ok_or("has an unclosed [")?;
        if !valid_ipv6(host) {
            return Err("has a bracketed address that is not IPv6".into());
        }
        let port = match after {
            "" => None,
            p => Some(
                p.strip_prefix(':')
                    .ok_or("has something after its address")?,
            ),
        };
        (host.to_string(), port)
    } else {
        match address.split_once(':') {
            Some((host, port)) => (host.to_string(), Some(port)),
            None => (address.to_string(), None),
        }
    };
    if !host.contains(':') && !valid_hostname(&host) {
        return Err("has an address that is not a host name or IP address".into());
    }
    let port = match port {
        None => 22,
        Some(p) => p
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
            .ok_or("has a port that is not 1 to 65535")?,
    };
    Ok(Host {
        name: name.to_string(),
        user,
        address: host,
        port,
    })
}

/// The hosts setting: entries split by commas, semicolons or new lines.
pub fn parse_hosts(value: &str) -> (Vec<Host>, Vec<String>) {
    let mut hosts: Vec<Host> = Vec::new();
    let mut left_out = Vec::new();
    for (i, entry) in value
        .split([',', ';', '\n', '\r'])
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .enumerate()
    {
        // Name the entry by its name when it has a plain one, else by place.
        let label = entry
            .split_once('=')
            .map(|(n, _)| n.trim())
            .filter(|n| valid_name(n))
            .map(|n| format!("\"{n}\""))
            .unwrap_or_else(|| format!("number {}", i + 1));
        if looks_like_a_key(entry) {
            left_out.push(format!(
                "Entry {label} looks like a key; only addresses go in Hosts."
            ));
            continue;
        }
        match parse_entry(entry) {
            Ok(host) if hosts.iter().any(|h| h.name == host.name) => {
                left_out.push(format!("Entry {label} repeats a name already used."));
            }
            Ok(_) if hosts.len() >= MAX_HOSTS => {
                left_out.push(format!("Entry {label} is past the {MAX_HOSTS}-host limit."));
            }
            Ok(host) => hosts.push(host),
            Err(why) => left_out.push(format!("Entry {label} {why}.")),
        }
    }
    (hosts, left_out)
}

impl Config {
    pub fn from_settings(settings: &Value) -> Config {
        let (hosts, left_out) = parse_hosts(setting(settings, "hosts"));
        let key = setting(settings, "key_file");
        let agent = setting(settings, "agent_socket");
        Config {
            hosts,
            left_out,
            key_file: (!key.is_empty()).then(|| key_file(key)),
            agent_socket: (!agent.is_empty()).then(|| agent_socket(agent)),
            ssh: ssh_program(setting(settings, "ssh_path")),
        }
    }

    pub fn host(&self, name: &str) -> Option<&Host> {
        self.hosts.iter().find(|h| h.name == name)
    }
}

/// A path as ssh reads it in an option's value: forward slashes, `%`
/// doubled (ssh expands `%` tokens there), and quoted when it has spaces.
/// A double quote can't be said at all, so such a path is refused.
pub fn option_path(path: &Path) -> Result<String, String> {
    let value = path.to_string_lossy().replace('\\', "/").replace('%', "%%");
    if value.contains('"') || has_control(&value) {
        return Err(format!(
            "{} can't be handed to ssh: it has a quote or a control character in it.",
            path.display()
        ));
    }
    Ok(if value.contains(char::is_whitespace) {
        format!("\"{value}\"")
    } else {
        value
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_entry_reads_its_name_user_address_and_port() {
        let (hosts, out) = parse_hosts(
            "web-1=deploy@203.0.113.5, db=root@db.example.com:2222;\nv6=me@[2001:db8::1]:2200\nbare=example.org",
        );
        assert!(out.is_empty(), "{out:?}");
        assert_eq!(
            hosts[0],
            Host {
                name: "web-1".into(),
                user: Some("deploy".into()),
                address: "203.0.113.5".into(),
                port: 22
            }
        );
        assert_eq!(hosts[1].port, 2222);
        assert_eq!(hosts[1].known_hosts_field(), "[db.example.com]:2222");
        assert_eq!(hosts[2].address, "2001:db8::1");
        assert_eq!(hosts[2].shown(), "me@[2001:db8::1]:2200");
        assert_eq!(hosts[3].user, None);
        assert_eq!(hosts[3].known_hosts_field(), "example.org");
    }

    #[test]
    fn entries_that_could_reach_ssh_options_are_left_out() {
        for entry in [
            "a=-oProxyCommand=x",
            "a=user@-oProxyCommand=x",
            "a=-l@host",
            "a=user@host -F x",
            "a=user@host:22 -oX=y",
            "a=user@ho\tst",
            "a=user@.host",
            "a=user@host:0",
            "a=user@host:65536",
            "a=user@host:22:33",
            "a=user@[::1",
            "a=user@[not-v6]",
            "a=user:pass@host",
            "a=us@er@host",
            "A=user@host",
            "a b=user@host",
            "a",
        ] {
            let (hosts, out) = parse_hosts(entry);
            assert!(hosts.is_empty(), "{entry} -> {hosts:?}");
            assert_eq!(out.len(), 1, "{entry}");
        }
    }

    #[test]
    fn a_left_out_entry_is_named_never_repeated() {
        let (_, out) = parse_hosts("web=deploy:hunter2@host, x=y@-oProxyCommand=calc");
        let said = out.join(" ");
        assert!(!said.contains("hunter2"), "{said}");
        assert!(!said.contains("calc"), "{said}");
        assert!(said.contains("\"web\""), "{said}");
    }

    #[test]
    fn key_material_is_recognised() {
        assert!(looks_like_a_key("-----BEGIN OPENSSH PRIVATE KEY-----\nabc"));
        assert!(looks_like_a_key("ssh-ed25519 AAAAC3Nza"));
        assert!(looks_like_a_key("PuTTY-User-Key-File-3: ssh-ed25519"));
        assert!(!looks_like_a_key("~/.ssh/id_ed25519"));
        assert!(!looks_like_a_key("/home/me/.ssh/ssh_host_key"));
    }

    #[test]
    fn an_option_path_is_one_value_ssh_reads_as_given() {
        assert_eq!(
            option_path(Path::new("/a b/100%/kh")).unwrap(),
            "\"/a b/100%%/kh\""
        );
        assert_eq!(option_path(Path::new("C:\\x\\kh")).unwrap(), "C:/x/kh");
        assert!(option_path(Path::new("/a\"b")).is_err());
    }

    #[test]
    fn ssh_is_never_a_bare_name() {
        let err = ssh_program("ssh").unwrap_err();
        assert!(err.contains("full path"), "{err}");
        assert!(ssh_program("./ssh").is_err());
    }
}
