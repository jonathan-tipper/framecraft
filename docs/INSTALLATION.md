# Install Framecraft on a Mac

Beta 6 is an Apple silicon prerelease with capture-to-paste and recorded shortcuts. The DMG is ad-hoc signed and has no Apple notarisation. The app targets macOS 13 or later and the installed upgrade was tested on macOS 26.6.2. [Download beta 6 and its checksum](https://github.com/jonathan-tipper/framecraft/releases/tag/v0.3.0-beta.6) · [Test results](TEST-NOTES.md)

## Install or update on a MacBook

1. Check **Apple menu → About This Mac**. The `arm64` DMG is for Apple silicon Macs (M-series). Download the DMG listed under **Assets** on the beta 6 release page.
2. Quit Framecraft. Open the DMG, drag **Framecraft.app** onto **Applications**, and confirm replacement if updating. Eject the DMG and launch the copy in Applications. Saved sessions and settings live outside the app bundle.
3. If macOS says it can’t verify Framecraft, dismiss the alert, open **System Settings → Privacy & Security**, and click **Open Anyway** for Framecraft. This option appears after an attempted launch. Confirm and open the app. On a managed Mac, ask your administrator if the option is unavailable. [Apple’s first-launch instructions](https://support.apple.com/en-gb/guide/mac-help/mh40617/mac)
4. Open **Capture setup**, allow Screen Recording, record an available shortcut and choose **Copy and return**. From another app, press your shortcut, drag a rectangle, then press **⌘V** at the original cursor. [MacBook test steps](TESTING.md#beta-6-shortcut-and-paste-on-another-mac)

To verify the download, put the DMG and its `.sha256` file in the same folder and run this command from that folder.

```sh
shasum -a 256 -c Framecraft-0.3.0-beta.6-macos-arm64-development.dmg.sha256
```

## Allow capture

macOS controls access to screen pixels. In **System Settings → Privacy & Security → Screen & System Audio Recording**, allow Framecraft. Older macOS versions call this **Screen Recording**. Quit and reopen Framecraft if requested, then explicitly retry the capture. The app cannot approve this permission for you.

In beta 6, use **More Capture → Screen Access… → Request Access** to ask macOS for access from Framecraft itself. **Open System Settings** opens the relevant privacy pane. **Check Again** reads the current permission. A blocked capture opens this panel before hiding the window. Existing screenshots and edits remain available.

If Framecraft is absent from the privacy list, use the **+** button and select `/Applications/Framecraft.app`. Quit other copies, including the one on a mounted DMG, and launch the installed app. If macOS requires a restart after approval, quit and reopen Framecraft before retrying. If the switch is unavailable on a managed Mac, ask its administrator about the screen-recording policy.

An ad-hoc update can change the signature macOS associates with Screen Recording. On the beta 4 to beta 6 upgrade, the Framecraft switch stayed on while macOS rejected the updated signature. If **Check Again** still reports denied access, verify the path and version under **Which copy is running?**. Quit Framecraft, remove its old entry from **Screen & System Audio Recording** with **–**, then use **+** to add `/Applications/Framecraft.app`. Allow that copy and reopen it before retrying. macOS may ask for Touch ID or your Mac password when you change the entry.

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
