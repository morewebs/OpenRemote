#!/usr/bin/env bash
# Publish already-built nightly installers. Run from the repository root.
set -euo pipefail

# GitHub's upload endpoint can return a 502 and leave a starter asset behind.
# Retry individual files with --clobber so a partial upload can be replaced.
retry() {
  local delay
  for delay in 5 15 30; do
    if "$@"; then return 0; fi
    echo "GitHub release operation failed; retrying in ${delay}s" >&2
    sleep "$delay"
  done
  "$@"
}

installers=(
  OpenRemote-Nightly-Setup-x64.exe
  OpenRemote-Nightly-Setup-x64.exe.sig
  OpenRemote-Nightly-x86_64.AppImage
  OpenRemote-Nightly-x86_64.AppImage.sig
  OpenRemote-Nightly-universal.apk
  OpenRemote-Nightly-arm64.apk
  OpenRemote-Nightly-armv7.apk
)
for name in "${installers[@]}"; do
  if [ ! -s "assets/$name" ]; then
    echo "::error::missing nightly asset: $name"
    exit 1
  fi
done
for kind in update patch; do
  name="OpenRemote-Nightly-Setup-x64.${kind}.zst"
  if [ -e "assets/$name" ] || [ -e "assets/$name.sig" ]; then
    test -s "assets/$name" && test -s "assets/$name.sig" || exit 1
    installers+=("$name" "$name.sig")
  fi
done
test -s latest.json

ensure_release() {
  gh release view "$1" --json isDraft --jq .isDraft 2>/dev/null ||
    gh release create "$1" --target "$SHA" --draft --prerelease \
      --title "$title" --notes-file nightly-notes.md >&2
}

upload_installers() {
  local tag=$1 name
  for name in "${installers[@]}"; do
    retry gh release upload "$tag" --clobber "assets/$name"
  done
}

dated="nightly-$STAMP"
title="OpenRemote nightly $VERSION"
# Keep the dated release private until every installer is present. A failed
# publish can resume the same draft, or skip a dated release already published.
dated_draft="$(retry ensure_release "$dated")"
if [[ "$dated_draft" != false ]]; then
  upload_installers "$dated"
  retry gh release edit "$dated" --draft=false
fi

retry ensure_release nightly > /dev/null
rolling_title="$(retry gh release view nightly --json name --jq .name)"
if [[ "$rolling_title" =~ -nightly\.([0-9]{12})$ ]] && [[ "${BASH_REMATCH[1]}" > "$STAMP" ]]; then
  echo "Leaving newer rolling release in place: $rolling_title"
  exit 0
fi
upload_installers nightly
# Publish the feed after the files it names.
retry gh release upload nightly --clobber latest.json
retry gh release edit nightly --title "$title" --notes-file nightly-notes.md --prerelease --draft=false
retry gh api -X PATCH "repos/${GITHUB_REPOSITORY}/git/refs/tags/nightly" \
  -f sha="$SHA" -F force=true > /dev/null

keep=" ${installers[*]} latest.json "
rolling_assets="$(retry gh release view nightly --json assets --jq '.assets[].name')"
while IFS= read -r name; do
  if [ -n "$name" ] && [[ "$keep" != *" $name "* ]]; then
    retry gh release delete-asset nightly "$name" --yes
  fi
done <<< "$rolling_assets"
