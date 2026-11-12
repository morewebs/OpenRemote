#!/usr/bin/env sh
# Build the Worktrunk plugin's program and put it where openremote.json
# names it: bin/worktrunk-plugin, or bin/worktrunk-plugin.exe on Windows.
# OpenRemote runs only programs inside the plugin's own folder, so a
# process plugin ships what it runs. Then install this folder from Plugins
# > Install from a folder, or from OpenRemote's catalog.
# Needs Rust (https://rustup.rs). On Windows, run it from Git Bash, or run
# the two steps by hand: cargo build --release in program/, then copy
# program/target/release/worktrunk-plugin.exe to bin/worktrunk-plugin.exe.
# Worktrunk itself is not needed to build: the plugin finds it at run time,
# or installs it from the Worktrunk view.
set -e
cd "$(dirname "$0")"
cargo build --release --locked --manifest-path program/Cargo.toml
target="${CARGO_TARGET_DIR:-program/target}"
mkdir -p bin
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) cp "$target/release/worktrunk-plugin.exe" bin/worktrunk-plugin.exe ;;
  *) cp "$target/release/worktrunk-plugin" bin/worktrunk-plugin ;;
esac
echo "built bin/worktrunk-plugin"
