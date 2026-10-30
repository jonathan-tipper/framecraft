# Framecraft repository and source archives

The maintained repository is [jonathan-tipper/framecraft](https://github.com/jonathan-tipper/framecraft). Version 0.3.0-beta.1 adds persistent session review and local handoff. Read the [README](README.md), [developer guide](docs/DEVELOPER-GUIDE.md) and [recorded checks](docs/TEST-NOTES.md) before testing.

## Existing checkout

Fetch the beta tag without replacing local changes:

```sh
git fetch origin --tags
git switch --detach v0.3.0-beta.1
cargo test --all-targets --locked
./scripts/bundle-macos.sh
open dist/Framecraft.app
```

Commit or stash unrelated work before switching if Git requests it. For new changes, follow the repository's work-type branch convention, such as `fix/capture-recovery` or `docs/user-guide`; use a PR for review.

## Downloaded source ZIP

GitHub's source archive contains tracked files without the `.git` directory or commit history. Extract it, enter its directory and use the build steps above, omitting Git commands. Do not reinitialise an existing checkout or replace its remote.

For a separate fork with new history, initialise only inside the extracted archive and push to an empty repository you own. The included workflow runs locked tests, strict Clippy and a macOS development bundle build.

## Test distribution

The supplied binary is Apple silicon, built with a macOS 13 minimum deployment target and tested on macOS 26.6.2. Intel, other supported OS versions and a clean Mac remain unvalidated. The app is ad-hoc signed, not Developer ID signed or notarised. Use the source build if local security policy prevents opening the downloaded test bundle; do not disable system-wide protections.

The original 0.2 archive instructions are retained in Git history. Their Linux-only validation statements are historical, not the evidence for 0.3.
