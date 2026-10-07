//! Harness installs on this machine, through each owner's own installer.
//! Every harness ships one for macOS and Linux (`curl … | bash`) and
//! Windows (`irm … | iex`); the command shown in the console is the
//! command that runs. Two gaps are filled with the owners' own artifacts,
//! and the console says so:
//!
//! - OpenCode publishes no Windows script, so its official release zip is
//!   unpacked into the same `~\.opencode\bin` root its Unix script uses.
//! - Pi runs on Node.js 22.19+, and its installer won't fetch Node without
//!   a terminal to ask in. With the human's go-ahead (`with_runtime`), the
//!   official Node.js 22 build is set up first - checksum-verified, in the
//!   `pi-node` folder pi's own installer and launcher use.
//!
//! Installs run detached from any terminal with stdin closed, so an
//! installer that would prompt takes its non-interactive default instead
//! of hanging.

use std::path::PathBuf;
use std::time::Duration;

use openremote_core::InstallSpec;
use openremote_harness::paths::{home_dir, local_app_data, under, which};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Macos,
    Linux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X64,
    Arm64,
}

/// The machine the daemon runs on. Other Unixes ride the Linux recipes -
/// the owners' scripts detect their own platform and refuse honestly.
pub fn this_os() -> Os {
    if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Macos
    } else {
        Os::Linux
    }
}

/// x64 or arm64 - every installer's supported pair. Anything else gets no
/// install rows rather than a command that can only fail.
pub fn this_arch() -> Option<Arch> {
    match std::env::consts::ARCH {
        "x86_64" => Some(Arch::X64),
        "aarch64" => Some(Arch::Arm64),
        _ => None,
    }
}

/// Where a recipe comes from: the owner's own install script, or the
/// owner's release artifact unpacked by OpenRemote.
const OFFICIAL: &str = "official";
const RELEASE: &str = "release";

/// One harness's install on one platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    /// Shown verbatim in the console.
    pub command: String,
    pub source: &'static str,
    /// A line of plain text beside the command, when the recipe does
    /// something the command alone doesn't say.
    pub note: Option<&'static str>,
    /// What the shell runs: the command itself for an official installer.
    pub script: String,
}

/// The display order and each harness's own name.
const HARNESSES: &[(&str, &str)] = &[
    ("claude", "Claude Code"),
    ("codex", "Codex"),
    ("grok", "Grok Build"),
    ("pi", "Pi Agent"),
    ("opencode", "OpenCode"),
    ("agy", "Antigravity"),
];

/// Each owner's documented one-liners: (id, macOS/Linux, Windows).
const OFFICIAL_COMMANDS: &[(&str, &str, &str)] = &[
    (
        "claude",
        "curl -fsSL https://claude.ai/install.sh | bash",
        "irm https://claude.ai/install.ps1 | iex",
    ),
    (
        "codex",
        "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
        "irm https://chatgpt.com/codex/install.ps1 | iex",
    ),
    (
        "grok",
        "curl -fsSL https://x.ai/cli/install.sh | bash",
        "irm https://x.ai/cli/install.ps1 | iex",
    ),
    (
        "pi",
        "curl -fsSL https://pi.dev/install.sh | sh",
        "irm https://pi.dev/install.ps1 | iex",
    ),
    (
        "opencode",
        "curl -fsSL https://opencode.ai/install | bash",
        "",
    ),
    (
        "agy",
        "curl -fsSL https://antigravity.google/cli/install.sh | bash",
        "irm https://antigravity.google/cli/install.ps1 | iex",
    ),
];

/// The install for one harness on one platform. `avx2` picks OpenCode's
/// baseline Windows build on x64 CPUs without it, the same choice its
/// Unix script makes.
pub fn recipe_for(harness_id: &str, os: Os, arch: Arch, avx2: bool) -> Option<Recipe> {
    let (_, unix, windows) = OFFICIAL_COMMANDS
        .iter()
        .find(|(id, _, _)| *id == harness_id)?;
    if os == Os::Windows && harness_id == "opencode" {
        let asset = match (arch, avx2) {
            (Arch::Arm64, _) => "opencode-windows-arm64.zip",
            (Arch::X64, true) => "opencode-windows-x64.zip",
            (Arch::X64, false) => "opencode-windows-x64-baseline.zip",
        };
        let url = format!("https://github.com/anomalyco/opencode/releases/latest/download/{asset}");
        return Some(Recipe {
            command: format!("{url} → ~\\.opencode\\bin"),
            source: RELEASE,
            note: Some(
                "OpenCode has no Windows install script, so this unpacks its official release.",
            ),
            script: opencode_windows_script(&url),
        });
    }
    let command = if os == Os::Windows { windows } else { unix };
    Some(Recipe {
        command: command.to_string(),
        source: OFFICIAL,
        note: None,
        script: command.to_string(),
    })
}

/// The command shown for a harness on this machine.
pub fn command_for(harness_id: &str) -> Option<String> {
    recipe_for(harness_id, this_os(), this_arch()?, avx2()).map(|r| r.command)
}

/// The install rows for this machine: every harness that isn't installed
/// and has a recipe here. `node_ready` is whether a Node.js new enough for
/// pi is already on the machine.
pub fn installable(installed: &[String], node_ready: bool) -> Vec<InstallSpec> {
    let Some(arch) = this_arch() else {
        return Vec::new();
    };
    installable_for(installed, this_os(), arch, avx2(), node_ready)
}

fn installable_for(
    installed: &[String],
    os: Os,
    arch: Arch,
    avx2: bool,
    node_ready: bool,
) -> Vec<InstallSpec> {
    HARNESSES
        .iter()
        .filter(|(id, _)| !installed.iter().any(|i| i == id))
        .filter_map(|(id, name)| {
            let recipe = recipe_for(id, os, arch, avx2)?;
            Some(InstallSpec {
                harness_id: id.to_string(),
                name: name.to_string(),
                command: recipe.command,
                source: recipe.source.to_string(),
                note: recipe.note.map(String::from),
                needs_runtime: *id == "pi" && !node_ready,
            })
        })
        .collect()
}

fn avx2() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

// ---- Node.js for pi ----

/// Pi's minimum Node.js, from its installer's own preflight.
pub const PI_NODE_MIN: (u64, u64, u64) = (22, 19, 0);

/// The directory pi's installer keeps its standalone Node in, and the
/// one its launcher puts first on PATH: `pi-node\current` on Windows,
/// `${XDG_DATA_HOME:-~/.local/share}/pi-node/current/bin` elsewhere.
pub fn pi_node_bin() -> Option<PathBuf> {
    if cfg!(windows) {
        return local_app_data().map(|l| under(&l, &["pi-node", "current"]));
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home_dir().map(|h| under(&h, &[".local", "share"])))?;
    Some(under(&data, &["pi-node", "current", "bin"]))
}

/// The node pi would run on: its own standalone Node first (what its
/// launcher prefers), then PATH.
pub fn node_for_pi() -> Option<PathBuf> {
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    pi_node_bin()
        .map(|dir| dir.join(name))
        .filter(|p| p.is_file())
        .or_else(|| which(name))
}

/// The nodejs.org build for this platform, as it appears in
/// `SHASUMS256.txt`: a `.tar.gz` on Unix (no xz needed), a `.zip` on
/// Windows.
pub fn node_asset_suffix(os: Os, arch: Arch) -> &'static str {
    match (os, arch) {
        (Os::Windows, Arch::X64) => "-win-x64.zip",
        (Os::Windows, Arch::Arm64) => "-win-arm64.zip",
        (Os::Macos, Arch::X64) => "-darwin-x64.tar.gz",
        (Os::Macos, Arch::Arm64) => "-darwin-arm64.tar.gz",
        (Os::Linux, Arch::X64) => "-linux-x64.tar.gz",
        (Os::Linux, Arch::Arm64) => "-linux-arm64.tar.gz",
    }
}

const NODE_DIST: &str = "https://nodejs.org/dist/latest-v22.x";

/// Fetch the latest Node.js 22, verify it against the release's own
/// SHASUMS256.txt, and make it pi-node's `current` - the layout pi's
/// installer itself writes.
fn node_prelude_unix(suffix: &str) -> String {
    format!(
        r#"set -eu
base="{NODE_DIST}"
dest="${{XDG_DATA_HOME:-$HOME/.local/share}}/pi-node"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$base/SHASUMS256.txt" -o "$tmp/SHASUMS256.txt"
file="$(awk '$2 ~ /^node-v[0-9.]+/ && substr($2, length($2) - length("{suffix}") + 1) == "{suffix}" {{ print $2; exit }}' "$tmp/SHASUMS256.txt")"
[ -n "$file" ] || {{ echo "No Node.js 22 build for this platform" >&2; exit 1; }}
curl -fsSL "$base/$file" -o "$tmp/$file"
cd "$tmp"
grep "  $file\$" SHASUMS256.txt > want.txt
if command -v sha256sum >/dev/null 2>&1; then sha256sum -c want.txt; else shasum -a 256 -c want.txt; fi
tar -xzf "$file"
dir="${{file%.tar.gz}}"
mkdir -p "$dest"
rm -rf "$dest/$dir"
mv "$dir" "$dest/$dir"
ln -sfn "$dest/$dir" "$dest/current"
cd /
"#
    )
}

fn node_prelude_windows(suffix: &str) -> String {
    format!(
        r#"$ErrorActionPreference = 'Stop'
$base = '{NODE_DIST}'
$tmp = Join-Path ([IO.Path]::GetTempPath()) "openremote-pi-node-$PID"
New-Item -ItemType Directory -Force $tmp | Out-Null
try {{
  $sums = (Invoke-WebRequest -UseBasicParsing "$base/SHASUMS256.txt").Content
  $line = ($sums -split "`n") | Where-Object {{ $_.Trim().EndsWith('{suffix}') -and $_ -match 'node-v[0-9.]+' }} | Select-Object -First 1
  if (-not $line) {{ throw 'No Node.js 22 build for this platform' }}
  $hash, $file = $line.Trim() -split '\s+'
  $zip = Join-Path $tmp $file
  Invoke-WebRequest -UseBasicParsing "$base/$file" -OutFile $zip
  if ((Get-FileHash -Algorithm SHA256 $zip).Hash -ne $hash) {{ throw "Node.js download did not match its published checksum" }}
  Expand-Archive $zip $tmp -Force
  $dest = Join-Path $env:LOCALAPPDATA 'pi-node'
  New-Item -ItemType Directory -Force $dest | Out-Null
  $current = Join-Path $dest 'current'
  if (Test-Path $current) {{ Remove-Item $current -Recurse -Force }}
  Move-Item (Join-Path $tmp ($file -replace '\.zip$', '')) $current
  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (-not (($userPath -split ';') -contains $current)) {{
    [Environment]::SetEnvironmentVariable('Path', "$current;$userPath", 'User')
  }}
  $env:Path = "$current;$env:Path"
}} finally {{
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}}
$ErrorActionPreference = 'Continue'
"#
    )
}

/// OpenCode's release zip, unpacked where its Unix script installs, with
/// that directory put on the user PATH the way the Unix script does.
fn opencode_windows_script(url: &str) -> String {
    format!(
        r#"$ErrorActionPreference = 'Stop'
$tmp = Join-Path ([IO.Path]::GetTempPath()) "openremote-opencode-$PID"
New-Item -ItemType Directory -Force $tmp | Out-Null
try {{
  $zip = Join-Path $tmp 'opencode.zip'
  Invoke-WebRequest -UseBasicParsing '{url}' -OutFile $zip
  Expand-Archive $zip $tmp -Force
  $exe = Get-ChildItem $tmp -Recurse -Filter 'opencode.exe' | Select-Object -First 1
  if (-not $exe) {{ throw 'The OpenCode release did not contain opencode.exe' }}
  $bin = Join-Path $HOME '.opencode\bin'
  New-Item -ItemType Directory -Force $bin | Out-Null
  Copy-Item $exe.FullName (Join-Path $bin 'opencode.exe') -Force
  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (-not (($userPath -split ';') -contains $bin)) {{
    [Environment]::SetEnvironmentVariable('Path', "$bin;$userPath", 'User')
  }}
}} finally {{
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}}
"#
    )
}

// ---- running ----

/// What the console asked for beyond the harness itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct InstallOptions {
    /// The human agreed to set up Node.js for pi.
    pub with_runtime: bool,
    /// A Node.js new enough for pi is already here.
    pub node_ready: bool,
}

/// Whether an install may go ahead: pi without a usable Node waits for
/// the human's go-ahead rather than fetching a runtime unasked.
pub fn check(harness_id: &str, options: InstallOptions) -> Result<(), String> {
    if harness_id == "pi" && !options.node_ready && !options.with_runtime {
        return Err(
            "Pi runs on Node.js 22.19 or newer, which isn't on this machine yet".to_string(),
        );
    }
    Ok(())
}

/// Run the install for one harness. `OPENREMOTE_INSTALL_CMD` replaces the
/// whole command (e2e points it at a script that materializes the harness
/// binary); production runs the owner's installer. Ten minutes, then it's
/// failed - an installer hangs, not us.
pub async fn install(harness_id: &str, options: InstallOptions) -> Result<String, String> {
    let custom = std::env::var("OPENREMOTE_INSTALL_CMD").ok();
    install_with(harness_id, options, custom.as_deref()).await
}

/// The install runner itself - the command override rides in as a
/// parameter so tests never mutate process env.
pub async fn install_with(
    harness_id: &str,
    options: InstallOptions,
    command: Option<&str>,
) -> Result<String, String> {
    let arch = this_arch().ok_or("no installer for this processor")?;
    let os = this_os();
    let Some(recipe) = recipe_for(harness_id, os, arch, avx2()) else {
        return Err(format!("'{harness_id}' has no installer"));
    };
    check(harness_id, options)?;

    let mut cmd = if let Some(command) = command {
        let mut parts = command.split_whitespace();
        let program = parts.next().ok_or("empty install command")?;
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(parts);
        cmd
    } else {
        let needs_node = harness_id == "pi" && !options.node_ready;
        let script = match (os, needs_node) {
            (Os::Windows, true) => {
                node_prelude_windows(node_asset_suffix(os, arch)) + &recipe.script
            }
            (Os::Windows, false) => recipe.script,
            (_, true) => node_prelude_unix(node_asset_suffix(os, arch)) + &recipe.script,
            (_, false) => recipe.script,
        };
        shell(os, &script)
    };
    // Pi's installer looks for node on PATH; its standalone Node rides
    // first so a fresh (or earlier) pi-node is the one it finds.
    if harness_id == "pi" {
        if let Some(bin) = pi_node_bin() {
            let path = std::env::var_os("PATH").unwrap_or_default();
            let mut dirs = vec![bin];
            dirs.extend(std::env::split_paths(&path));
            if let Ok(joined) = std::env::join_paths(dirs) {
                cmd.env("PATH", joined);
            }
        }
    }
    detach(&mut cmd);
    let run = cmd
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = match tokio::time::timeout(Duration::from_secs(600), run).await {
        Ok(output) => output,
        Err(_) => return Err("the installer timed out after ten minutes".to_string()),
    };
    finish(output)
}

/// The shell an installer runs under: bash with pipefail where it exists
/// (a failed download must not pipe nothing into `bash` and "succeed"),
/// PowerShell on Windows with the script passed encoded so no quoting
/// layer can mangle it.
fn shell(os: Os, script: &str) -> tokio::process::Command {
    if os == Os::Windows {
        let script = format!("$ProgressPreference = 'SilentlyContinue'\n{script}");
        let mut cmd = tokio::process::Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encode_powershell(&script),
        ]);
        return cmd;
    }
    let mut cmd;
    if which("bash").is_some() {
        cmd = tokio::process::Command::new("bash");
        cmd.args(["-c", &format!("set -o pipefail\n{script}")]);
    } else {
        cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", script]);
    }
    cmd
}

/// No terminal for the installer: its own session on Unix (an installer
/// that opens /dev/tty to ask finds none and takes its default), no
/// console window on Windows. The one unsafe block in the daemon: a
/// `pre_exec` hook is the only way to call setsid between fork and exec,
/// and a process group alone would stop on SIGTTIN instead of failing to
/// open the terminal.
#[allow(unsafe_code)]
fn detach(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    unsafe {
        // SAFETY: setsid is async-signal-safe and touches no Rust state,
        // which is all a pre_exec hook may do in the forked child.
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// `-EncodedCommand` takes base64 of the UTF-16LE script.
fn encode_powershell(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// How much installer output a failure carries: the end is where an
/// installer says what went wrong.
const TAIL_LINES: usize = 40;

fn finish(output: std::io::Result<std::process::Output>) -> Result<String, String> {
    let output = output.map_err(|e| e.to_string())?;
    let text = tail(&format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ));
    if output.status.success() {
        Ok(if text.is_empty() {
            "done".to_string()
        } else {
            text
        })
    } else {
        Err(if text.is_empty() {
            format!("the installer exited with {}", output.status)
        } else {
            text
        })
    }
}

/// The last lines of installer output, with terminal color codes and
/// carriage-return progress redraws dropped.
pub fn tail(text: &str) -> String {
    let clean = strip_ansi(text);
    let lines: Vec<&str> = clean
        .lines()
        .map(|line| line.rsplit('\r').next().unwrap_or(line).trim_end())
        .filter(|line| !line.is_empty())
        .collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    lines[start..].join("\n")
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // CSI: parameters, then one final byte in @..~
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            } else {
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [&str; 6] = ["claude", "codex", "grok", "pi", "opencode", "agy"];

    #[test]
    fn every_harness_installs_on_every_desktop_platform() {
        for os in [Os::Windows, Os::Macos, Os::Linux] {
            for arch in [Arch::X64, Arch::Arm64] {
                for id in ALL {
                    let recipe = recipe_for(id, os, arch, true)
                        .unwrap_or_else(|| panic!("{id} on {os:?}/{arch:?}"));
                    assert!(!recipe.command.is_empty());
                    assert!(!recipe.script.is_empty());
                }
            }
        }
    }

    #[test]
    fn the_commands_are_the_owners_own() {
        let unix = |id| recipe_for(id, Os::Linux, Arch::X64, true).unwrap();
        let win = |id| recipe_for(id, Os::Windows, Arch::X64, true).unwrap();
        assert_eq!(
            unix("claude").command,
            "curl -fsSL https://claude.ai/install.sh | bash"
        );
        assert_eq!(
            win("claude").command,
            "irm https://claude.ai/install.ps1 | iex"
        );
        assert_eq!(
            win("agy").command,
            "irm https://antigravity.google/cli/install.ps1 | iex"
        );
        // An official installer runs exactly what's shown.
        for id in ALL {
            let r = unix(id);
            assert_eq!(r.source, OFFICIAL);
            assert_eq!(r.script, r.command);
            assert!(r.note.is_none());
        }
        // Grok and Antigravity are no longer manual.
        assert_eq!(
            macos_cmd("grok"),
            "curl -fsSL https://x.ai/cli/install.sh | bash"
        );
    }

    fn macos_cmd(id: &str) -> String {
        recipe_for(id, Os::Macos, Arch::Arm64, false)
            .unwrap()
            .command
    }

    #[test]
    fn opencode_on_windows_unpacks_its_release_and_says_so() {
        let r = recipe_for("opencode", Os::Windows, Arch::X64, true).unwrap();
        assert_eq!(r.source, RELEASE);
        assert!(r.note.is_some());
        assert!(r.script.contains("opencode-windows-x64.zip"));
        let baseline = recipe_for("opencode", Os::Windows, Arch::X64, false).unwrap();
        assert!(
            baseline
                .script
                .contains("opencode-windows-x64-baseline.zip")
        );
        let arm = recipe_for("opencode", Os::Windows, Arch::Arm64, true).unwrap();
        assert!(arm.script.contains("opencode-windows-arm64.zip"));
    }

    #[test]
    fn install_rows_skip_what_is_installed_and_flag_pis_runtime() {
        let rows = installable_for(
            &["claude".to_string(), "grok".to_string()],
            Os::Linux,
            Arch::X64,
            true,
            false,
        );
        let ids: Vec<&str> = rows.iter().map(|r| r.harness_id.as_str()).collect();
        assert_eq!(ids, ["codex", "pi", "opencode", "agy"]);
        for row in &rows {
            assert_eq!(row.needs_runtime, row.harness_id == "pi");
        }
        let ready = installable_for(&[], Os::Linux, Arch::X64, true, true);
        assert_eq!(ready.len(), 6);
        assert!(ready.iter().all(|r| !r.needs_runtime));
    }

    #[test]
    fn pi_without_node_waits_for_the_go_ahead() {
        let none = InstallOptions::default();
        assert!(check("pi", none).is_err());
        assert!(
            check(
                "pi",
                InstallOptions {
                    with_runtime: true,
                    node_ready: false
                }
            )
            .is_ok()
        );
        assert!(
            check(
                "pi",
                InstallOptions {
                    with_runtime: false,
                    node_ready: true
                }
            )
            .is_ok()
        );
        assert!(check("codex", none).is_ok());
    }

    #[test]
    fn the_node_build_matches_the_platform() {
        assert_eq!(node_asset_suffix(Os::Linux, Arch::X64), "-linux-x64.tar.gz");
        assert_eq!(
            node_asset_suffix(Os::Macos, Arch::Arm64),
            "-darwin-arm64.tar.gz"
        );
        assert_eq!(
            node_asset_suffix(Os::Windows, Arch::Arm64),
            "-win-arm64.zip"
        );
        assert!(node_prelude_unix("-linux-x64.tar.gz").contains("sha256sum -c"));
        assert!(node_prelude_windows("-win-x64.zip").contains("Get-FileHash"));
    }

    #[test]
    fn powershell_gets_utf16_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        // `-EncodedCommand` for "dir": d\0i\0r\0
        assert_eq!(encode_powershell("dir"), "ZABpAHIA");
    }

    #[test]
    fn a_failure_carries_the_tail_without_terminal_noise() {
        let long: String = (0..100).map(|i| format!("line {i}\n")).collect();
        let t = tail(&long);
        assert_eq!(t.lines().count(), TAIL_LINES);
        assert!(t.ends_with("line 99"));
        assert_eq!(tail("\u{1b}[31merror\u{1b}[0m: no\n"), "error: no");
        assert_eq!(tail("10%\r50%\r100%\n"), "100%");
    }

    #[tokio::test]
    async fn the_runner_runs_a_command_override_and_reports_failure() {
        // The honest custom command: echo is a cmd builtin on Windows.
        let echo = if cfg!(windows) {
            "cmd /c echo install"
        } else {
            "echo install"
        };
        let ok = InstallOptions::default();
        let out = install_with("codex", ok, Some(echo)).await.unwrap();
        assert!(out.contains("install"), "got: {out}");

        // A failing command surfaces its output, not a generic error.
        let fail = if cfg!(windows) {
            "cmd /c exit 3"
        } else {
            "false"
        };
        let err = install_with("codex", ok, Some(fail)).await.unwrap_err();
        assert!(err.contains("exited"), "got: {err}");

        // Not a harness: an explicit refusal, not a run.
        assert!(install_with("nope", ok, Some(echo)).await.is_err());
        // Pi without Node or a go-ahead: refused before anything runs.
        assert!(install_with("pi", ok, Some(echo)).await.is_err());
    }
}
