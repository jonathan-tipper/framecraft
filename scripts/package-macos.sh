#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
if [[ $# -eq 0 ]]; then
  bash scripts/bundle-macos.sh
elif [[ $# -ne 1 || "$1" != --skip-build ]]; then
  printf '%s\n' 'Usage: scripts/package-macos.sh [--skip-build]' >&2
  exit 2
fi
[[ "$(uname -s)" == Darwin ]] || { printf '%s\n' 'DMG packaging requires macOS.' >&2; exit 1; }
bundle="$project_dir/dist/Framecraft.app"
version="$(awk '/^\[package\]/{package=1;next} /^\[/{package=0} package && /^version *=/{gsub(/"/, "", $3); print $3; exit}' Cargo.toml)"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]] || { printf '%s\n' 'Unexpected package version.' >&2; exit 1; }
[[ "$(cat "$bundle/Contents/Resources/VERSION")" == "$version" ]] || { printf '%s\n' 'Bundle version differs from Cargo.toml. Rebuild before packaging.' >&2; exit 1; }
/usr/bin/codesign --verify --deep --strict "$bundle"
architecture="$(/usr/bin/lipo -archs "$bundle/Contents/MacOS/framecraft")"
case "$architecture" in
  arm64|x86_64) ;;
  'x86_64 arm64'|'arm64 x86_64') architecture=universal ;;
  *) printf 'Unsupported bundle architecture: %s\n' "$architecture" >&2; exit 1 ;;
esac
output="$project_dir/dist/Framecraft-$version-macos-$architecture.dmg"
[[ ! -L "$output" && ! -d "$output" ]] || { printf '%s\n' 'Refusing a symbolic-link or directory DMG destination.' >&2; exit 1; }
staging="$(mktemp -d "$project_dir/dist/.framecraft-dmg.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
mkdir "$staging/content"
/usr/bin/ditto "$bundle" "$staging/content/Framecraft.app"
ln -s /Applications "$staging/content/Applications"
cat > "$staging/content/Install Framecraft.txt" <<'NOTE'
FRAMECRAFT FOR MAC

1. Quit any running copy of Framecraft.
2. Drag Framecraft.app onto Applications beside it. Confirm Replace if updating.
3. Eject this disk image, then open Framecraft from Applications.
4. Click Capture Area to draw a rectangle on screen. Escape cancels.

This beta is ad-hoc signed. It is not Developer ID signed or notarised.
If macOS blocks it, and you trust this downloaded build, open System Settings >
Privacy & Security after trying to launch it, choose Open Anyway and confirm.
Do not disable Gatekeeper or remove quarantine attributes.

If macOS reports known malware or damage, stop and check the download/source.
Screen capture also needs permission in Privacy & Security > Screen & System
Audio Recording (Screen Recording on older macOS). Allow Framecraft and restart
it if macOS requests that. The app does not grant itself capture permission.

Documentation: https://github.com/jonathan-tipper/framecraft
NOTE
/usr/bin/hdiutil create -quiet -format UDZO -fs HFS+ -volname "Framecraft $version" -srcfolder "$staging/content" "$staging/Framecraft.dmg"
/usr/bin/hdiutil verify "$staging/Framecraft.dmg"
mv -f "$staging/Framecraft.dmg" "$output"
printf 'Created %s\n' "$output"
(cd "$project_dir/dist" && /usr/bin/shasum -a 256 "${output##*/}")
