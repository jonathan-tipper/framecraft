# Install Framecraft on a Mac

Beta 6 adds capture-to-paste and shortcut recording. Beta 2 is the latest published installer. Maintainers can download the beta 6 Apple silicon DMG and its checksum from the private development draft. That copy has an ad-hoc signature and Gatekeeper may block it. For a clean install on another MacBook, use the Developer ID installer once the release issue records Apple notarisation and Gatekeeper verification. [Development draft](https://github.com/jonathan-tipper/framecraft/releases/tag/untagged-1718d416b78610f78c6f) · [Signed installer status](https://github.com/jonathan-tipper/framecraft/issues/35)

## Install or update on a MacBook

1. Choose the DMG for your Mac’s chip under Apple menu → **About This Mac**. `arm64` means Apple silicon. Read the release notes and compare the download with the accompanying SHA-256 file. The current Mac build targets macOS 13 or later. Tested configurations are listed in the [test notes](TEST-NOTES.md).
2. Quit Framecraft. Open the DMG, drag **Framecraft.app** onto **Applications**, and confirm replacement if updating. Eject the DMG and launch the copy in Applications. Saved sessions and settings live outside the app bundle.
3. Open **More Capture → Screen Access… → Which copy is running?** Confirm the release version and the path inside Applications. Opening an older copy can give a different permission result.
4. In **Capture setup**, check screen access, record an available shortcut and choose **Copy and return**. Use the [second-Mac test](TESTING.md#beta-6-shortcut-and-paste-on-another-mac) for a first capture.

Developer ID signing, Apple notarisation and Screen Recording consent are separate checks. A normally downloaded Developer ID build should launch with standard Gatekeeper settings. If that fails, record the exact alert, version and source. The published beta 2 and development DMGs have a different trust status. On managed Macs, use an approved build or ask the administrator. [Beta 2 installation guide](https://github.com/jonathan-tipper/framecraft/blob/v0.3.0-beta.2/docs/INSTALLATION.md) · [Apple’s guidance on opening Mac apps](https://support.apple.com/en-gb/102445)

## Allow capture

macOS controls access to screen pixels. In **System Settings → Privacy & Security → Screen & System Audio Recording**, allow Framecraft. Older macOS versions call this **Screen Recording**. Quit and reopen Framecraft if requested, then explicitly retry the capture. The app cannot approve this permission for you.

In beta 6, use **More Capture → Screen Access… → Request Access** to ask macOS for access from Framecraft itself. **Open System Settings** opens the relevant privacy pane. **Check Again** reads the current permission. A blocked capture opens this panel before hiding the window. Existing screenshots and edits remain available.

If Framecraft is absent from the privacy list, use the **+** button and select `/Applications/Framecraft.app`. Quit other copies, including the one on a mounted DMG, and launch the installed app. If macOS requires a restart after approval, quit and reopen Framecraft before retrying. If the switch is unavailable on a managed Mac, ask its administrator about the screen-recording policy.

An ad-hoc rebuild can acquire a different permission identity. A signed upgrade should be tested with the previously installed copy quit. Check the path and version if the switch appears enabled but the running app reports denied access.

The command error `could not create image from rect` alone does not prove denial. In the Screen Access panel, check this running copy's status. If granted, try a non-empty area on an available display; report the capture mode, macOS version and running-copy path if it still fails.

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
