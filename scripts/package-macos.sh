#!/usr/bin/env bash
set -euo pipefail

fail() { printf '%s\n' "$*" >&2; exit 1; }
project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
mode="${1:---development}"
case "$mode" in
  --development|--skip-build|--developer-id) ;;
  *) fail 'Usage: package-macos.sh [--development | --skip-build | --developer-id]' ;;
esac
[[ $# -le 1 && "$(uname -s)" == Darwin ]] || fail 'Pass one packaging mode on macOS.'

if [[ "$mode" == --developer-id ]]; then
  identity="${FRAMECRAFT_SIGN_IDENTITY:-}"
  profile="${FRAMECRAFT_NOTARY_PROFILE:-}"
  [[ -n "$identity" && "$identity" != '-' ]] || fail 'Set FRAMECRAFT_SIGN_IDENTITY to a Developer ID Application identity in Keychain.'
  if ! /usr/bin/security find-identity -v -p codesigning | awk -v wanted="$identity" '
    /"Developer ID Application: / {
      name=$0; sub(/^[^"]*"/, "", name); sub(/"[^"]*$/, "", name)
      if ($2 == toupper(wanted) || name == wanted) found=1
    }
    END {exit !found}'; then
    fail 'No matching valid Developer ID Application identity with a private key is available.'
  fi
  [[ -n "$profile" ]] || fail 'Set FRAMECRAFT_NOTARY_PROFILE to your notarytool Keychain profile name.'
  revision="$(git rev-parse --verify HEAD)"
  [[ -z "$(git status --porcelain)" ]] || fail 'Build the distribution DMG from a clean checkout of the candidate commit.'
fi

dist_dir="$project_dir/dist"
mkdir -p "$dist_dir"
stage="$(mktemp -d "$dist_dir/.framecraft-dmg.XXXXXX")"
mount_dir=""
mounted=false
release_lock=""
cleanup() {
  if [[ "$mounted" == true ]]; then
    /usr/bin/hdiutil detach "$mount_dir" -quiet || { printf 'Could not eject %s. Staging files retained.\n' "$mount_dir" >&2; return; }
  fi
  [[ -z "$release_lock" ]] || rmdir "$release_lock"
  rm -rf "$stage"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

notarise() {
  local artefact="$1" receipt="$2" status request_id
  /usr/bin/xcrun notarytool submit "$artefact" --keychain-profile "$profile" --wait --output-format plist > "$receipt"
  status="$(/usr/bin/plutil -extract status raw -o - "$receipt")"
  request_id="$(/usr/bin/plutil -extract id raw -o - "$receipt")"
  printf 'Apple notarisation submission %s returned %s.\n' "$request_id" "$status"
  [[ "$status" == Accepted ]] || fail "Inspect the notarytool log for submission $request_id."
}

bundle="$dist_dir/Framecraft.app"
manifest="$project_dir/Cargo.toml"
if [[ "$mode" == --developer-id ]]; then
  # Sign a snapshot of the named commit even if the working tree changes
  # while Cargo or Apple's notarisation service is running.
  mkdir "$stage/source"
  git archive "$revision" | /usr/bin/tar -xf - -C "$stage/source"
  manifest="$stage/source/Cargo.toml"
  target_dir="${CARGO_TARGET_DIR:-$project_dir/target}"
  [[ "$target_dir" == /* ]] || target_dir="$project_dir/$target_dir"
  CARGO_TARGET_DIR="$target_dir" bash "$stage/source/scripts/bundle-macos.sh" --output-dir "$stage/bundle"
  bundle="$stage/bundle/Framecraft.app"
elif [[ "$mode" == --development ]]; then
  bash scripts/bundle-macos.sh
fi
version="$(awk '/^\[package\]/{package=1;next} /^\[/{package=0} package && /^version *=/{gsub(/"/, "", $3); print $3; exit}' "$manifest")"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]] || fail 'Unexpected package version.'
[[ "$(cat "$bundle/Contents/Resources/VERSION")" == "$version" ]] || fail 'Bundle and Cargo versions differ. Rebuild before packaging.'
/usr/bin/codesign --verify --deep --strict "$bundle"
architecture="$(/usr/bin/lipo -archs "$bundle/Contents/MacOS/framecraft")"
case "$architecture" in
  arm64|x86_64) ;;
  'x86_64 arm64'|'arm64 x86_64') architecture=universal ;;
  *) fail "Unsupported bundle architecture: $architecture" ;;
esac
name="Framecraft-$version-macos-$architecture"
mkdir "$stage/content" "$stage/publish"
if [[ "$mode" == --developer-id ]]; then
  directory="$dist_dir/releases/$name"
  mkdir -p "$dist_dir/releases"
  [[ ! -e "$directory" && ! -L "$directory" ]] || fail "Release artefacts already exist at $directory."
  mkdir "$directory.lock" || fail 'Another process holds the release packaging lock.'
  release_lock="$directory.lock"
  printf '%s\n' "$revision" > "$bundle/Contents/Resources/BUILD_COMMIT"
  requirement='anchor apple generic and identifier "dev.framecraft.app" and certificate leaf[field.1.2.840.113635.100.6.1.13] exists'
  /usr/bin/codesign --force --options runtime --timestamp --sign "$identity" "$bundle"
  /usr/bin/codesign --verify --deep --strict -R="$requirement" "$bundle"
  /usr/bin/ditto -c -k --keepParent "$bundle" "$stage/app.zip"
  notarise "$stage/app.zip" "$stage/app-notary.plist"
  /usr/bin/xcrun stapler staple "$bundle"
  /usr/bin/xcrun stapler validate "$bundle"
else
  name="$name-development"
fi

/usr/bin/ditto "$bundle" "$stage/content/Framecraft.app"
ln -s /Applications "$stage/content/Applications"
cat > "$stage/content/Install Framecraft.txt" <<'NOTE'
FRAMECRAFT FOR MAC

1. Quit Framecraft. Drag Framecraft.app onto Applications beside it.
2. Eject the disk image, then open the installed copy from Applications.
3. Open Capture setup. Allow Screen Recording when macOS asks, then reopen
   Framecraft if requested. Record an available shortcut and choose Copy and return.
4. In a rich-text TextEdit document, use the shortcut, select a small area and
   press Command+V. The image should appear at the original insertion point.

Documentation: https://github.com/jonathan-tipper/framecraft
NOTE
if [[ "$mode" != --developer-id ]]; then
  printf '\nDevelopment test build. Ad-hoc signed; Apple notarisation is unavailable for this file.\n' >> "$stage/content/Install Framecraft.txt"
fi
dmg="$stage/publish/$name.dmg"
/usr/bin/hdiutil create -quiet -format UDZO -fs HFS+ -volname "Framecraft $version" -srcfolder "$stage/content" "$dmg"
if [[ "$mode" == --developer-id ]]; then
  /usr/bin/codesign --timestamp --sign "$identity" "$dmg"
  notarise "$dmg" "$stage/dmg-notary.plist"
  /usr/bin/xcrun stapler staple "$dmg"
  /usr/bin/xcrun stapler validate "$dmg"
  /usr/bin/codesign --verify --strict "$dmg"
  /usr/sbin/spctl --assess --type open --context context:primary-signature --verbose=4 "$dmg"
fi
/usr/bin/hdiutil verify "$dmg"
if [[ "$mode" == --developer-id ]]; then
  mount_dir="$stage/mounted"
  mkdir "$mount_dir"
  /usr/bin/hdiutil attach -readonly -nobrowse -quiet -mountpoint "$mount_dir" "$dmg"
  mounted=true
  shipped_app="$mount_dir/Framecraft.app"
  /usr/bin/codesign --verify --deep --strict -R="$requirement" "$shipped_app"
  /usr/bin/xcrun stapler validate "$shipped_app"
  /usr/sbin/spctl --assess --type execute --verbose=4 "$shipped_app"
  cmp "$bundle/Contents/MacOS/framecraft" "$shipped_app/Contents/MacOS/framecraft"
  [[ "$(cat "$shipped_app/Contents/Resources/BUILD_COMMIT")" == "$revision" ]] || fail 'Mounted app revision differs.'
  /usr/bin/codesign -dv --verbose=4 "$shipped_app" > "$stage/signature.txt" 2>&1
  /usr/bin/grep -Eq '^CodeDirectory .*flags=.*runtime' "$stage/signature.txt" || fail 'Hardened runtime missing from the shipped app.'
  /usr/bin/grep -q '^Timestamp=' "$stage/signature.txt" || fail 'Secure signing timestamp missing from the shipped app.'
  {
    printf 'Version %s\nCommit %s\nArchitecture %s\n' "$version" "$revision" "$architecture"
    printf 'Bundle build %s\n' "$(/usr/bin/plutil -extract CFBundleVersion raw -o - "$shipped_app/Contents/Info.plist")"
    printf 'Minimum macOS %s\n' "$(/usr/bin/plutil -extract LSMinimumSystemVersion raw -o - "$shipped_app/Contents/Info.plist")"
    /usr/bin/grep -E '^(Identifier=|TeamIdentifier=|Authority=|Timestamp=|CodeDirectory )' "$stage/signature.txt"
    printf 'App notarisation %s\n' "$(/usr/bin/plutil -extract id raw -o - "$stage/app-notary.plist")"
    printf 'DMG notarisation %s\n' "$(/usr/bin/plutil -extract id raw -o - "$stage/dmg-notary.plist")"
  } > "$stage/publish/BUILD-INFO.txt"
  /usr/bin/hdiutil detach -quiet "$mount_dir"
  mounted=false
fi
(cd "$stage/publish" && /usr/bin/shasum -a 256 "$name.dmg" > "$name.dmg.sha256")
if [[ "$mode" == --developer-id ]]; then
  mv "$stage/publish" "$directory"
  printf 'Verified Developer ID release: %s\n' "$directory/$name.dmg"
else
  for file in "$name.dmg" "$name.dmg.sha256"; do
    [[ ! -L "$dist_dir/$file" && ! -d "$dist_dir/$file" ]] || fail "Unexpected development output path: $dist_dir/$file"
    mv -f "$stage/publish/$file" "$dist_dir/$file"
  done
  printf 'Development image: %s\n' "$dist_dir/$name.dmg"
fi
