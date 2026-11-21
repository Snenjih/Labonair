# Release & Packaging

How Labonair-rust is built into a distributable artifact. macOS is the primary
target; Linux is prepared but not yet a first-class release (see below).

Unlike the original Tauri app there is no `tauri bundle`; the bundle is
assembled by a script from a plain `--release` build.

## Version — single source of truth

The version lives in **`crates/app/Cargo.toml`** (`[package] version`). Every
other place derives from it:

- the binary exposes it as `CARGO_PKG_VERSION`
  (`env!("CARGO_PKG_VERSION")` in the binary),
- `scripts/package-macos.sh` reads it into `CFBundleShortVersionString`,
- `CFBundleVersion` is set to the commit count (`git rev-list --count HEAD`).

Bump it there, add a `CHANGELOG.md` section, tag `vX.Y.Z`.

## macOS build

Prerequisites: Xcode CLT + the Metal Toolchain
(`xcodebuild -downloadComponent MetalToolchain`), stable Rust.

```sh
scripts/package-macos.sh            # -> target/release/bundle/macos/Labonair.app
scripts/package-macos.sh --dmg      # also builds Labonair_<version>_<arch>.dmg
```

To run the native Rust application locally, use an explicit repository path:

```sh
cargo run -p labonair
# or, after packaging:
target/release/bundle/macos/Labonair.app/Contents/MacOS/labonair
```

Do not use `open -a Labonair` for development or visual verification. The
legacy Tauri application may be installed under the same display name, which
would launch the wrong application.

The complete native visual-verification contract is in
[`visual-verification.md`](testing.md). In particular, packaged
checks must use the absolute Rust bundle path and an exact executable PID.

The script:

1. builds `labonair` and `labonair-host` in release mode,
2. creates `Labonair.app/Contents/{MacOS,Resources}`,
3. copies both binaries, `AppIcon.icns`, `LICENSE`, `ADDITIONAL-TERMS.md`,
   asset/vendor license texts, and a generated Cargo license report,
4. writes a version-substituted `Info.plist` and `PkgInfo`, then checks the
   plist with `plutil -lint`,
5. optionally signs, builds a dmg, and notarizes (see below).

### What's in the bundle

The bundle contains the app and host binaries, `Info.plist`, app icon, the
project license and additional terms, a generated `THIRD-PARTY-LICENSES.txt`,
and readable license/notice files for bundled assets and vendored GPUI. Fonts
(Inter, JetBrains Mono, IBM Plex Sans, and Lilex) and Tree-sitter grammars are
compiled into the binary; their corresponding font license files are copied
into `Contents/Resources/Licenses/Fonts/`.

The release workflow installs `cargo-about` 0.9.3. Local packaging requires
the same version on `PATH` (`cargo install --locked --version 0.9.3 --features
cli cargo-about`).

### Universal binary (optional)

```sh
scripts/package-macos.sh --target aarch64-apple-darwin
scripts/package-macos.sh --target x86_64-apple-darwin
lipo -create -output labonair-universal \
  target/aarch64-apple-darwin/release/labonair \
  target/x86_64-apple-darwin/release/labonair
```

## Code signing & notarization (macOS)

Optional — **never blocks CI**. Without the env vars the script produces an
unsigned bundle (Gatekeeper: right-click → Open, or `xattr -dr
com.apple.quarantine`).

| Env var | Meaning |
|---|---|
| `LABONAIR_SIGN_IDENTITY` | `"Developer ID Application: NAME (TEAMID)"`, or `"-"` for ad-hoc |
| `LABONAIR_NOTARY_PROFILE` | `xcrun notarytool` keychain profile (set up once with `notarytool store-credentials`) |

```sh
xcrun notarytool store-credentials labonair-notary \
  --apple-id you@example.com --team-id TEAMID --password <app-specific-pw>

LABONAIR_SIGN_IDENTITY="Developer ID Application: … (TEAMID)" \
LABONAIR_NOTARY_PROFILE=labonair-notary \
  scripts/package-macos.sh --dmg
```

Signing uses `--options runtime` (hardened runtime) with
`packaging/macos/Labonair.entitlements` (keychain access group
`com.labonair.rust`, sandbox disabled — needed for terminal/editor filesystem
access, network client for SSH/SFTP). Notarization zips the `.app` (or signs
the dmg), submits with `--wait`, then staples.

## Linux (prepared, not yet released)

The binary builds on Linux (`cargo build --release -p labonair`, GPUI Vulkan
renderer, needs Vulkan loader + `libxkbcommon`, `wayland`/`xcb`, `fontconfig`,
`openssl` dev libs). Packaging is intentionally not automated yet; when it is,
the intended path is an **AppImage** (bundle the binary + a `.desktop` file +
`packaging/macos/icon.png` re-used as the app icon) and/or a **Flatpak**
manifest. Keep platform packaging behind `scripts/package-<os>.sh` so the
release workflow stays a per-OS switch.

## Auto-update (T15-005)

**Decision — custom minimal updater, not Sparkle.** Sparkle would need an
`objc2`/framework binding or a Swift shim plus its own signed-appcast tooling;
the predecessor already publishes a compatible `latest.json` (T15-004) and
its updater used **minisign** signatures. Reusing that wire format keeps the
release pipeline unchanged and avoids a second signing system, so the native
updater reimplements the same four steps without any Tauri runtime dependency.

`labonair-updater` (`crates/updater/`):

- **Manifest** — predecessor-compatible `latest.json` at
  `DEFAULT_UPDATE_ENDPOINT`
  (`…/releases/latest/download/latest.json`). Shape: `{ version, notes,
  pub_date, platforms: { "<os>-<arch>": { url, signature } } }`. `signature` is
  the base64 of the whole `.minisig` file.
- **Check** — `fetch_manifest()` GETs it; `UpdateManifest::available()` returns
  an `AvailableUpdate` only for a strictly-newer `SemVer` *with* an artifact for
  this `UPDATE_TARGET`. Auto-cadence: `CHECK_INTERVAL` (6 h), tracked in
  `~/.config/labonair/updater-last-check`.
- **Download** — `download_update()` streams the artifact with progress
  callbacks.
- **Verify** — `verify_update()` checks the minisign (Ed25519, pre-hashed)
  signature against `UPDATE_PUBLIC_KEY`. An empty key or empty signature is a
  hard failure — the update is **never** applied unverified.
- **Apply** — `apply_macos_update()` unpacks the `<name>.app.tar.gz` and
  atomically swaps the running `.app` bundle (moves the old one aside, rolls
  back on failure); `relaunch()` re-opens it via `open` and exits.

UI: `crates/updater-ui/src/` (`UpdaterView`) — the native GPUI dialog
(available / downloading / ready), auto-check at startup when the
`checkForUpdates` preference is on, and a **Check for Updates…** entry in the
app menu and command palette. Failures go through the notification system.

### Signing the update artifact

1. Generate a minisign keypair once: `minisign -G -p updater.pub -s updater.key`.
2. Put the **public** key's second line into
   `crates/updater/src/install.rs::UPDATE_PUBLIC_KEY`.
3. Store the **secret** key + its password as the CI secrets
   `LABONAIR_UPDATER_PRIVATE_KEY` / `LABONAIR_UPDATER_KEY_PASSWORD`.

`scripts/package-macos.sh` then emits `Labonair_<version>_<arch>.app.tar.gz`,
signs it (`<tarball>.minisig`) and writes `latest.json` with the base64
signature inline. Until step 2 is done, `UPDATE_PUBLIC_KEY` is empty and the
in-app updater refuses every update (safe default).

> A universal `latest.json` needs both `darwin-aarch64` and `darwin-x86_64`
> entries; the per-arch runs each produce a single-arch manifest — merge them
> (or run the arm64 build last and hand-add the x86_64 block) before upload.

## Distributed artifacts

| Platform | Artifact | Notes |
|---|---|---|
| macOS (Apple Silicon) | `Labonair_<version>_arm64.dmg` | primary |
| macOS (Intel) | `Labonair_<version>_x86_64.dmg` | via `--target` |
| macOS | `Labonair_<version>_<arch>.app.tar.gz` (+ `.minisig`) | auto-update payload |
| macOS | `latest.json` | update manifest |
| Linux | — | prepared, not released (see above) |

## Known limitations vs. the original app

- **No in-app web/URL preview** — GPUI cannot embed a WebView. Replaced by
  native markdown rendering + "open in system browser".
- **macOS / Linux only** — no Windows.
- Auto-update: macOS only. Linux update (AppImage/Flatpak/repo) is still TODO.
- Linux has no packaged release yet (builds from source).

## Verifying a release

```sh
scripts/smoke-test.sh        # builds the bundle, structurally verifies it,
                             # runs `cargo test -p labonair --test smoke`
LABONAIR_SMOKE_LAUNCH=1 scripts/smoke-test.sh   # also opens the app for 5s
```

## Packaging Contract

**Related:** [`README.md`](README.md), [`../RELEASE.md`](release.md), [`../testing/evidence-model.md`](testing.md)

The package source of truth is `crates/app/Cargo.toml` for the version and
`scripts/package-macos.sh` for the native bundle. The release path is:

```text
cargo build --release -p labonair
→ Labonair.app (binaries, Info.plist, icon, compiled resources, license texts)
→ optional signing/notarization
→ optional DMG and signed updater artifact
→ smoke test and artifact verification
```

Never commit signing keys, notarization profiles, updater private keys, or
bearer tokens. Package evidence records the exact target, artifact digest,
smoke command, signing result, and any intentional unsigned state.

The macOS bundle registers the `labonair` URL scheme for Settings field links;
keep that declaration in `packaging/macos/Info.plist` aligned with the native
open-URL handler in `crates/app` and the route contract in `docs/settings.md`.

## Platform Policy

**Related:** [`README.md`](README.md), [`../RELEASE.md`](release.md), [`../visual-verification.md`](testing.md)

- macOS Apple Silicon is the primary packaged target.
- macOS Intel is supported through an explicit cross-target build.
- Linux is build-prepared but not a first-class packaged release until its
  AppImage/Flatpak decision and smoke evidence are accepted.
- Windows is outside the current supported product scope.

Platform claims require a native build, launch/smoke result, and support
decision. A successful Linux container build does not close macOS visual or
packaging evidence.

## Release Rollback

**Related:** [`README.md`](README.md), [`../RELEASE.md`](release.md), [`../automation/limits.md`](automation.md)

The macOS updater verifies a minisign signature before applying an artifact
and atomically swaps the app bundle with rollback on failure. An empty or
invalid public key/signature must fail closed and never apply an update.

For a failed release:

1. stop distribution of the affected artifact/manifest;
2. retain logs and artifact digests without secrets;
3. restore the previous signed bundle or publish the last known-good manifest;
4. verify launch, settings/session recovery, and updater state;
5. record the incident and next release gate in the release evidence record.

## Support Policy

**Related:** [`README.md`](README.md), `platforms.md`, [`../documentation-governance.md`](README.md)

A release is supportable only when its platform, packaging, visual, smoke,
security, and rollback evidence is explicit. Unsupported or unmeasured states
remain visible as `Pending` or `N/A`; they are not described as verified.

Support handoff must include version, target, artifact, commit, known
limitations, recovery path, and the next evidence action. User data and
credentials never belong in a handoff artifact.
