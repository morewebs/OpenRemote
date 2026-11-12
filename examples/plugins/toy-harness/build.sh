#!/usr/bin/env sh
# Build the toy program and put it where openremote.json names it:
# bin/toy, or bin/toy.exe on Windows. OpenRemote runs only programs inside
# the plugin's own folder (never node or python from PATH), so a process
# plugin ships what it runs. Then install this folder from Plugins >
# Install from a folder.
# Needs Rust (https://rustup.rs). On Windows, run it from Git Bash, or run
# the two steps by hand: cargo build --release in program/, then copy
# program/target/release/toy.exe to bin/toy.exe.
set -e
cd "$(dirname "$0")"
cargo build --release --manifest-path program/Cargo.toml
mkdir -p bin
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) cp program/target/release/toy.exe bin/toy.exe ;;
  *) cp program/target/release/toy bin/toy ;;
esac
echo "built bin/toy"
