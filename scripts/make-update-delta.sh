#!/usr/bin/env bash
# Makes one channel's in-app update delta, from the build that is already done
# (the release's own installer is staged by the workflow before this runs).
#
#   scripts/make-update-delta.sh <stem> <version> <feed> <raw-config> <stage>
#
#   stem        the release's installer name without .exe, e.g. OpenRemote-Setup-x64
#   version     the version the installer announces, as the feed names it
#   feed        the URL the channel's files are served from: the previous
#               release's latest.json and <stem>.update.zst sit beside it
#   raw-config  a full Tauri config with the NSIS installer uncompressed
#               (scripts/delta.mjs uncompressed writes one)
#   stage       the directory the files travel from to the publish job
#
# It builds the uncompressed installer, makes the patch from the previous
# release's uncompressed installer when the feed has one (unpacked from its
# <stem>.update.zst), packs the new one, signs what it makes, and copies the
# files to stage/. Without a signing key it makes nothing: nothing to sign.
#
# The signing key is read by the Tauri CLI from TAURI_SIGNING_PRIVATE_KEY and
# TAURI_SIGNING_PRIVATE_KEY_PASSWORD. This script never prints either.
set -euo pipefail

if [ "$#" -ne 5 ]; then
  echo "usage: scripts/make-update-delta.sh <stem> <version> <feed> <raw-config> <stage>" >&2
  exit 2
fi
stem="$1"
version="$2"
feed="$3"
raw_config="$4"
stage="$5"

if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  echo "no signing key in this build, so no update delta is made"
  exit 0
fi

root="$(pwd)"
# A fresh folder from mktemp, not RUNNER_TEMP: on a Windows runner that variable
# holds backslashes, which bash does not read as a path.
work="$(mktemp -d)"
mkdir -p "$stage"

# The delta tool is the codec the app itself runs (src-tauri/src/delta_codec.rs),
# built from its own crate. The manifest path is relative, so cargo gets no
# POSIX-style path to translate.
cargo build --release --manifest-path scripts/delta-tool/Cargo.toml
tool="$root/scripts/delta-tool/target/release/delta-tool"
if [ -f "$tool.exe" ]; then
  tool="$tool.exe"
fi

# The previous release's installer, uncompressed, is the base. The feed says
# which version it is; a feed without a stored installer (a release from
# before deltas) gives no base, and the release then ships the full package
# only.
base=()
from=""
if curl -fsSL "$feed/latest.json" -o "$work/previous-latest.json" \
  && curl -fsSL "$feed/$stem.update.zst" -o "$work/previous.update.zst"; then
  # The path goes in as an argument, not inside the script: a Windows path in
  # a quoted require() string would lose its backslashes.
  from="$(node -p "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8')).version" "$work/previous-latest.json")"
  "$tool" unpack "$work/previous.update.zst" "$work/base.exe"
  base=(--base "$work/base.exe" --from "$from")
  echo "the patch is made from ${from}"
else
  echo "no previous update installer in the feed; the full package only"
fi

# The uncompressed build of this same app. Its output has the same name as the
# release's own installer, which the workflow has already staged.
npx tauri build --bundles nsis --config "$raw_config"
shopt -s nullglob
installers=("$root"/src-tauri/target/release/bundle/nsis/*.exe)
if [ "${#installers[@]}" -ne 1 ]; then
  echo "::error::expected one uncompressed installer, found ${#installers[@]}" >&2
  exit 1
fi
cp "${installers[0]}" "$work/installer.exe"

node "$root/scripts/delta.mjs" make --tool "$tool" --stem "$stem" --version "$version" \
  --installer "$work/installer.exe" --out "$work/delta" "${base[@]}"

cp "$work/delta/$stem.update.zst" "$work/delta/$stem.update.zst.sig" "$stage/"
if [ -f "$work/delta/$stem.patch.zst" ]; then
  cp "$work/delta/$stem.patch.zst" "$work/delta/$stem.patch.zst.sig" "$stage/"
fi
cp "$work/delta/delta.json" "$stage/$stem.delta.json"
ls -la "$stage"
