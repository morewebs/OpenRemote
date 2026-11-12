#!/usr/bin/env sh
# Build the plugin's program and put it where openremote.json names it:
# bin/openremote-ssh, or bin/openremote-ssh.exe on Windows. OpenRemote runs
# only programs inside the plugin's own folder (never node or python from
# PATH), so the plugin ships what it runs. Then install this folder from
# Plugins > Install from a folder.
# Needs Rust (https://rustup.rs). On Windows, run it from Git Bash, or run
# the two steps by hand: cargo build --release in program/, then copy
# program/target/release/openremote-ssh.exe to bin/openremote-ssh.exe.
# The program's tests: cargo test in program/.
set -e
cd "$(dirname "$0")"
cargo build --release --locked --manifest-path program/Cargo.toml
mkdir -p bin
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) cp program/target/release/openremote-ssh.exe bin/openremote-ssh.exe ;;
  *) cp program/target/release/openremote-ssh bin/openremote-ssh ;;
esac
echo "built bin/openremote-ssh"
