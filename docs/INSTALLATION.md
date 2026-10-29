# Install Framecraft on a Mac

Framecraft 0.3.0-beta.2 is an Apple silicon test build. macOS 13 is the deployment baseline; the supported-version and hardware matrix is not fully tested. The app is ad-hoc signed, **not Developer ID signed or notarised**.

## Download and install

1. Download the `.dmg` and checksums from the [0.3.0-beta.2 release](https://github.com/jonathan-tipper/framecraft/releases/tag/v0.3.0-beta.2). Compare `shasum -a 256` for your download with the published digest.
2. Open the disk image. Quit any running copy of Framecraft, then drag **Framecraft.app** onto **Applications** in the disk image. Confirm replacement if updating an older version.
3. Eject the disk image. Open **Applications → Framecraft**. Its icon appears in the Dock while running, and its capture icon appears in the menu bar at the top of the screen.
4. Choose **Capture Area** in the app or its menu bar menu, then drag across the area you want. Press Escape to cancel. No capture session or shortcut configuration is needed for a single screenshot.

The app is installed separately from your saved captures and settings. Updating the app does not delete session folders. Keep those folders together with their manifests when making backups.

## If macOS blocks the beta

After trying to open Framecraft, open **System Settings → Privacy & Security**. If you trust the source and checksum, select **Open Anyway** for Framecraft and confirm. This approves that particular app; it does not require turning off Gatekeeper. Managed Macs may not offer this override. Follow your organisation's policy if that applies.

Do not override warnings about known malware or damaged software. Check the download and source instead. Apple's [open apps safely guidance](https://support.apple.com/en-gb/102445) explains the distinction and the supported approval flow. General installation steps are in Apple's [Mac installation guide](https://support.apple.com/en-gb/guide/mac-help/mh35835/mac).

## Allow capture

macOS controls access to screen pixels. In **System Settings → Privacy & Security → Screen & System Audio Recording**, allow Framecraft. Older macOS versions call this **Screen Recording**. Quit and reopen Framecraft if requested, then explicitly retry the capture. The app cannot approve this permission for you.

Run the installed copy consistently. Starting a separate build from Terminal or an IDE can give it a different permission identity. The application identifier is `dev.framecraft.app`.

## Build and install locally

From the repository on a Mac with Rust and Apple's command-line developer tools:

```sh
bash scripts/bundle-macos.sh
bash scripts/install-macos.sh
open /Applications/Framecraft.app
```

To update an existing install, quit Framecraft and use `bash scripts/install-macos.sh --replace`. The installer stages and verifies the new bundle before replacing the old one, and attempts rollback if publication fails. It refuses to replace another app, a symbolic link, or a running Framecraft process. It does not use `sudo`, change screen-recording permission or remove quarantine metadata.

If you cannot write to `/Applications`, create your own Applications folder and choose it explicitly:

```sh
mkdir -p "$HOME/Applications"
bash scripts/install-macos.sh --destination "$HOME/Applications/Framecraft.app"
```

The optional `--source /path/Framecraft.app` chooses an already-built bundle. To create the drag-to-Applications disk image, run `bash scripts/package-macos.sh`; `--skip-build` packages a previously built bundle only when its recorded version matches `Cargo.toml`.

Developers can run `bash scripts/test-install-macos.sh` after bundling. It uses temporary destinations and checks replacement, refusal and rollback behaviour without changing Applications. Process enumeration and a publication failure are simulated; signature and file operations use the real macOS tools.

To remove the app, quit it and move Framecraft from Applications to the Bin. This does not delete your capture folders or preferences in `~/Library/Application Support/Framecraft`.
