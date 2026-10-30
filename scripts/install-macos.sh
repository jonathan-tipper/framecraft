#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_app="$project_dir/dist/Framecraft.app"
destination="/Applications/Framecraft.app"
replace=false
usage() {
  printf '%s\n' 'Usage: scripts/install-macos.sh [--replace] [--destination /path/Framecraft.app] [--source /path/Framecraft.app]' 
  printf '%s\n' 'Installs an already-built app. Quit Framecraft first. Existing installs require --replace.'
}
while [[ $# -gt 0 ]]; do
  case "$1" in
    --replace) replace=true; shift ;;
    --destination|--source)
      [[ $# -ge 2 ]] || { usage >&2; exit 2; }
      if [[ "$1" == --destination ]]; then destination="$2"; else source_app="$2"; fi
      shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done
[[ "$(uname -s)" == Darwin ]] || { printf '%s\n' 'Installation requires macOS.' >&2; exit 1; }
[[ "$destination" == /* && "${destination##*/}" == Framecraft.app ]] || {
  printf '%s\n' 'Choose an absolute destination ending in /Framecraft.app.' >&2; exit 1;
}
[[ ! -L "$destination" && ! -L "$source_app" ]] || {
  printf '%s\n' 'The source and installed app must not be symbolic links.' >&2; exit 1;
}
destination_parent="$(dirname "$destination")"
[[ -d "$destination_parent" && -w "$destination_parent" ]] || {
  printf 'Cannot write to %s. Use Finder, or choose your own Applications folder with --destination.\n' "$destination_parent" >&2
  exit 1
}
source_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$source_app/Contents/Info.plist")"
[[ "$source_id" == dev.framecraft.app && -x "$source_app/Contents/MacOS/framecraft" ]] || {
  printf '%s\n' 'The source is not a valid Framecraft bundle. Run scripts/bundle-macos.sh first.' >&2; exit 1;
}
/usr/bin/codesign --verify --deep --strict "$source_app"
if [[ -e "$destination" ]]; then
  [[ "$replace" == true ]] || { printf '%s\n' 'Framecraft is already installed. Quit it, then pass --replace to update.' >&2; exit 1; }
  installed_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$destination/Contents/Info.plist")"
  [[ "$installed_id" == dev.framecraft.app ]] || { printf '%s\n' 'Refusing to replace a different application.' >&2; exit 1; }
fi
check_stopped() {
  local result=0
  /usr/bin/pgrep -x '[Ff]ramecraft' >/dev/null || result=$?
  if [[ "$result" -eq 0 ]]; then
    printf '%s\n' 'Quit Framecraft before installing. No running app was stopped.' >&2
    return 1
  elif [[ "$result" -ne 1 ]]; then
    printf '%s\n' 'Could not check whether Framecraft is running. Installation was not attempted.' >&2
    return 1
  fi
}
check_stopped

# Serialise this installer and stage on the destination volume. The lock is
# never removed unless this invocation created it.
install_lock="$destination_parent/.framecraft-install.lock"
if ! mkdir "$install_lock" 2>/dev/null; then
  printf 'Another installation may be running. Check %s before retrying.\n' "$install_lock" >&2
  exit 1
fi
staging=""
backup=""
completed=false
cleanup() {
  local result=$?
  if [[ "$completed" != true && -n "$backup" && -d "$backup/Framecraft.app" ]]; then
    if [[ ! -e "$destination" ]]; then
      mv "$backup/Framecraft.app" "$destination_parent/" || printf 'Restore the previous app manually from %s\n' "$backup/Framecraft.app" >&2
    else
      printf 'The destination changed. The previous app is preserved at %s\n' "$backup/Framecraft.app" >&2
    fi
  fi
  if [[ -n "$staging" && -d "$staging" ]]; then rm -rf "$staging"; fi
  if [[ -n "$backup" && -d "$backup" ]]; then
    if [[ "$completed" == true ]]; then rm -rf "$backup"; else rmdir "$backup" 2>/dev/null || true; fi
  fi
  rmdir "$install_lock" 2>/dev/null || true
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
staging="$(mktemp -d "$destination_parent/.framecraft-stage.XXXXXX")"
/usr/bin/ditto "$source_app" "$staging/Framecraft.app"
/usr/bin/codesign --verify --deep --strict "$staging/Framecraft.app"
check_stopped
if [[ -e "$destination" ]]; then
  [[ "$replace" == true && ! -L "$destination" ]] || { printf '%s\n' 'Destination changed during installation. Retry explicitly.' >&2; exit 1; }
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$destination/Contents/Info.plist")" == dev.framecraft.app ]] || exit 1
  backup="$(mktemp -d "$destination_parent/.framecraft-backup.XXXXXX")"
  mv "$destination" "$backup/Framecraft.app"
fi
# -n prevents a concurrently created destination from being replaced. Check the
# source afterwards because BSD mv reports success when it skips an existing file.
mv -n "$staging/Framecraft.app" "$destination_parent/"
[[ ! -e "$staging/Framecraft.app" ]] || { printf '%s\n' 'Destination appeared during installation. Nothing was overwritten.' >&2; exit 1; }
completed=true
printf 'Installed %s\n' "$destination"
printf '%s\n' 'Open Framecraft from Applications. This beta is ad-hoc signed, not notarised; macOS may ask you to allow this specific app in Privacy & Security.'
