#!/usr/bin/env bash
# Check the development DMG and the release guard without using signing credentials.
set -euo pipefail
[[ $# -eq 1 && -f "$1" && "$1" == *-development.dmg ]] || { printf '%s\n' 'Pass one development DMG.' >&2; exit 2; }
project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
image_dir="$(cd "$(dirname "$1")" && pwd)"
name="$(basename "$1")"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/framecraft-package-test.XXXXXX")"
mountpoint="$fixture/mount"
mkdir "$mountpoint"
mounted=false
cleanup() {
  if [[ "$mounted" == true ]]; then /usr/bin/hdiutil detach -quiet "$mountpoint" || return; fi
  rm -rf "$fixture"
}
trap cleanup EXIT

for identity in '' '-' 'Developer ID Application: Framecraft Missing Identity (0000000000)'; do
  if FRAMECRAFT_SIGN_IDENTITY="$identity" FRAMECRAFT_NOTARY_PROFILE='' \
    bash "$project_dir/scripts/package-macos.sh" --developer-id > "$fixture/signing.log" 2>&1; then
    printf '%s\n' 'An unavailable distribution identity was accepted.' >&2; exit 1
  fi
  /usr/bin/grep -Eq 'Set FRAMECRAFT_SIGN_IDENTITY|No matching valid Developer ID Application identity' "$fixture/signing.log"
done
(cd "$image_dir" && /usr/bin/shasum -a 256 -c "$name.sha256")
/usr/bin/hdiutil attach -quiet -readonly -nobrowse -mountpoint "$mountpoint" "$image_dir/$name"
mounted=true
bundle="$mountpoint/Framecraft.app"
/usr/bin/codesign --verify --deep --strict "$bundle"
[[ "$(/usr/bin/plutil -extract CFBundleIdentifier raw -o - "$bundle/Contents/Info.plist")" == 'dev.framecraft.app' ]]
[[ "$(readlink "$mountpoint/Applications")" == /Applications ]]
[[ -s "$bundle/Contents/Resources/USER-GUIDE.md" ]]
[[ "$name" == "Framecraft-$(cat "$bundle/Contents/Resources/VERSION")-"* ]]
/usr/bin/grep -q 'Development test build' "$mountpoint/Install Framecraft.txt"
cmp "$project_dir/dist/Framecraft.app/Contents/MacOS/framecraft" "$bundle/Contents/MacOS/framecraft"
printf '%s\n' 'Development DMG checksum, embedded app, installation layout and signing guards passed.'
