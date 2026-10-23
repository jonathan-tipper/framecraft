# Create your Framecraft repository

This is a Git archive of the Framecraft 0.2.0 source snapshot, exported on 23 October 2026. It includes the Rust source, tests, Cargo lockfile, MIT licence, macOS build script, GitHub Actions workflow and project documentation.

A standard `git archive` contains tracked project files, without the `.git` directory or commit history. The archive was generated from a local snapshot commit. You can create your own repository and history as follows.

## Extract and initialise

Run these commands in the directory containing the downloaded ZIP:

```sh
unzip Framecraft-0.2.0-git-archive.zip
cd framecraft
git init -b main
git add .
git commit -m "Initial import of Framecraft 0.2.0"
```

Git uses your configured author name and email. If either is missing, follow Git's instructions to configure them before committing.

## Push to a remote repository

Create an empty repository on your preferred Git host, then replace `YOUR_REPOSITORY_URL` with its SSH or HTTPS URL:

```sh
git remote add origin YOUR_REPOSITORY_URL
git push -u origin main
```

The included GitHub Actions workflow runs macOS checks and builds a development application bundle when used on GitHub. No remote repository has been created or configured by this export.

## Build on your Mac

Install a stable Rust toolchain and Apple's command-line developer tools. The application targets macOS 13 or newer.

```sh
cargo run --release --locked
```

To create a local app bundle:

```sh
./scripts/bundle-macos.sh
open dist/Framecraft.app
```

The build script is executable in this archive. If your extraction tool drops Unix permissions, run `chmod +x scripts/bundle-macos.sh` once. The bundle script uses ad-hoc signing for local development; it does not produce a notarised release.

## Current state

Read [README.md](README.md) for the workflow and [docs/TEST-NOTES.md](docs/TEST-NOTES.md) for the recorded checks and remaining Mac validation. The previous implementation run recorded 65 passing tests and Apple silicon target type-checking on Linux. Those application tests were not rerun for this packaging-only export. Capture, OCR, global shortcuts and the linked app still require testing on a real Mac.

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) describes the implementation. [docs/PRODUCT.md](docs/PRODUCT.md) covers limitations and next milestones. Build outputs and local environment files are excluded by the supplied `.gitignore`.
