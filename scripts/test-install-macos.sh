#!/usr/bin/env bash
# Exercise installer file operations in temporary folders. Never writes to Applications.
set -euo pipefail
project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_app="${1:-$project_dir/dist/Framecraft.app}"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/framecraft-install-test.XXXXXX")"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/scripts" "$fixture/apps"
printf '#!/bin/sh\nexit 1\n' > "$fixture/process-check"
chmod +x "$fixture/process-check"
# Only process enumeration is substituted. This allows the user to keep the
# real application open while testing stage, replace and rollback behaviour.
sed "s@/usr/bin/pgrep@$fixture/process-check@" "$project_dir/scripts/install-macos.sh" > "$fixture/scripts/install-macos.sh"
installer="$fixture/scripts/install-macos.sh"
destination="$fixture/apps/Framecraft.app"
printf 'unrelated\n' > "$fixture/apps/keep.txt"
bash "$installer" --source "$source_app" --destination "$destination"
/usr/bin/codesign --verify --deep --strict "$destination"
cmp "$source_app/Contents/MacOS/framecraft" "$destination/Contents/MacOS/framecraft"
if bash "$installer" --source "$source_app" --destination "$destination" > "$fixture/refusal.log" 2>&1; then exit 1; fi
grep -q 'already installed' "$fixture/refusal.log"
bash "$installer" --replace --source "$source_app" --destination "$destination"
/usr/bin/codesign --verify --deep --strict "$destination"
test "$(cat "$fixture/apps/keep.txt")" = unrelated
test "$(find "$fixture/apps" -maxdepth 1 -name '.framecraft-*' | wc -l | tr -d ' ')" = 0

mkdir -p "$fixture/other/Framecraft.app/Contents"
cp "$project_dir/packaging/Info.plist" "$fixture/other/Framecraft.app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier test.unrelated.app' "$fixture/other/Framecraft.app/Contents/Info.plist"
if bash "$installer" --replace --source "$source_app" --destination "$fixture/other/Framecraft.app" > "$fixture/other.log" 2>&1; then exit 1; fi
grep -q 'different application' "$fixture/other.log"
mkdir "$fixture/symlink"
ln -s "$destination" "$fixture/symlink/Framecraft.app"
if bash "$installer" --replace --source "$source_app" --destination "$fixture/symlink/Framecraft.app" > "$fixture/symlink.log" 2>&1; then exit 1; fi
grep -q 'symbolic links' "$fixture/symlink.log"

# Simulate a running process and assert that the existing app stays untouched.
printf '#!/bin/sh\nexit 0\n' > "$fixture/process-check"
if bash "$installer" --replace --source "$source_app" --destination "$destination" > "$fixture/running.log" 2>&1; then exit 1; fi
grep -q 'Quit Framecraft before installing' "$fixture/running.log"
/usr/bin/codesign --verify --deep --strict "$destination"
printf '#!/bin/sh\nexit 1\n' > "$fixture/process-check"

# Force publication to fail after the old app moves to its backup. The normal
# rollback must restore the old signed bundle and clean up this invocation's lock.
mkdir "$fixture/fail-publication"
cat > "$fixture/fail-publication/mv" <<'MOCK'
#!/bin/sh
if [ "$1" = -n ]; then exit 1; fi
exec /bin/mv "$@"
MOCK
chmod +x "$fixture/fail-publication/mv"
printf 'prior version\n' > "$destination/Contents/Resources/rollback-marker"
/usr/bin/codesign --force --deep --sign - "$destination"
if PATH="$fixture/fail-publication:$PATH" bash "$installer" --replace --source "$source_app" --destination "$destination" > "$fixture/rollback.log" 2>&1; then exit 1; fi
test "$(cat "$destination/Contents/Resources/rollback-marker")" = 'prior version'
/usr/bin/codesign --verify --deep --strict "$destination"
test "$(find "$fixture/apps" -maxdepth 1 -name '.framecraft-*' | wc -l | tr -d ' ')" = 0
printf '%s\n' 'Installer checks passed: install, explicit replacement, unrelated-file retention, wrong-app/symlink refusal, running-process refusal and failed-publication rollback.'
printf '%s\n' 'Process enumeration and the publication-failure case were simulated; signature and file operations were real.'
