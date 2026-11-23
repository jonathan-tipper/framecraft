# Framecraft repository and source archives

Framecraft is maintained at [jonathan-tipper/framecraft](https://github.com/jonathan-tipper/framecraft). Beta 6 adds capture-to-paste and recorded shortcuts. The [README](README.md), [developer guide](docs/DEVELOPER-GUIDE.md) and [test results](docs/TEST-NOTES.md) document the release.

## Existing checkout

To build beta 6 from source, fetch and select the release tag.

```sh
git fetch origin tag v0.3.0-beta.6
git switch --detach v0.3.0-beta.6
cargo test --all-targets --locked
./scripts/bundle-macos.sh
open dist/Framecraft.app
```

Commit or stash unrelated work before switching if Git requests it. For new changes, follow the repository's work-type branch convention, such as `fix/capture-recovery` or `docs/user-guide`; use a PR for review.

## Downloaded source ZIP

GitHub's source archive contains tracked files without the `.git` directory or commit history. Extract it, enter its directory and use the build steps above, omitting Git commands. Do not reinitialise an existing checkout or replace its remote.

For a separate fork with new history, initialise only inside the extracted archive and push to an empty repository you own. The included workflow runs locked tests, strict Clippy and a macOS development bundle build.

## Test distribution

The Apple silicon development build has a macOS 13 minimum deployment target. The download and checksum of each published release are on [GitHub Releases](https://github.com/jonathan-tipper/framecraft/releases). Look for the architecture and the signing status of the exact DMG you are testing. The instructions for a Developer ID release and an ad-hoc development package are in the [installation guide](docs/INSTALLATION.md).

The original 0.2 archive instructions are retained in Git history. Their Linux-only validation statements are historical, not the evidence for 0.3.
