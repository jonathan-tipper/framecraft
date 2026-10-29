#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  printf '%s\n' 'Framecraft.app must be bundled on macOS.' >&2
  exit 1
fi

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-13.0}"

if [[ -f Cargo.lock ]]; then
  cargo build --release --locked
else
  cargo build --release
fi

# Respect an explicitly configured Cargo target directory.
target_dir="${CARGO_TARGET_DIR:-$project_dir/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$project_dir/$target_dir"
fi
binary_path="$target_dir/release/framecraft"
if [[ ! -f "$binary_path" ]]; then
  printf 'Expected the host release binary at %s\n' "$binary_path" >&2
  printf '%s\n' 'Unset any Cargo cross-compilation target and build on the target Mac.' >&2
  exit 1
fi

bundle_dir="$project_dir/dist/Framecraft.app"
mkdir -p "$bundle_dir/Contents/MacOS" "$bundle_dir/Contents/Resources"
cp "$binary_path" "$bundle_dir/Contents/MacOS/framecraft"
cp "$project_dir/packaging/Info.plist" "$bundle_dir/Contents/Info.plist"
cp "$project_dir/packaging/Framecraft.icns" "$bundle_dir/Contents/Resources/Framecraft.icns"
cp "$project_dir/docs/THIRD-PARTY-NOTICES.md" "$bundle_dir/Contents/Resources/THIRD-PARTY-NOTICES.md"
cp "$project_dir/docs/USER-GUIDE.md" "$bundle_dir/Contents/Resources/USER-GUIDE.md"
cp "$project_dir/docs/INSTALLATION.md" "$bundle_dir/Contents/Resources/INSTALLATION.md"
cp "$project_dir/LICENSE" "$bundle_dir/Contents/Resources/LICENSE"
awk '/^\[package\]/{package=1;next} /^\[/{package=0} package && /^version *=/{gsub(/"/, "", $3); print $3; exit}' Cargo.toml > "$bundle_dir/Contents/Resources/VERSION"
chmod +x "$bundle_dir/Contents/MacOS/framecraft"

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$bundle_dir"
  printf '%s\n' 'Applied ad-hoc signing for local use. This is not notarisation.'
else
  printf '%s\n' 'codesign is unavailable; the bundle remains unsigned.' >&2
fi

printf 'Created %s\n' "$bundle_dir"

