# Release and Packaging

How Labonair is built into a distributable artifact. macOS is the only packaged target; Linux builds from source. There is no `tauri bundle`: `scripts/package-macos.sh` assembles the `.app` from a plain `--release` build.

## 1. Version

Single source: `[workspace.package] version` in the root `Cargo.toml` (currently `1.0.0`); `crates/app` inherits it (`version.workspace = true`), and `scripts/package-macos.sh` reads it from the workspace manifest. The binary exposes it as `CARGO_PKG_VERSION`; the bundle gets `CFBundleShortVersionString` = version and `CFBundleVersion` = `git rev-list --count HEAD`. Release steps: bump the version, add a `CHANGELOG.md` section, tag `vX.Y.Z`.

## 2. macOS build

Prerequisites: Xcode CLT, Metal Toolchain (`xcodebuild -downloadComponent MetalToolchain`), stable Rust, and `cargo-about` 0.9.3 on `PATH` (`cargo install --locked --version 0.9.3 --features cli cargo-about`; the script aborts on any other version).

```sh
scripts/package-macos.sh                      # -> target/release/bundle/macos/Labonair.app
scripts/package-macos.sh --dmg                # also Labonair_<version>_<arch>.dmg
scripts/package-macos.sh --target <triple>    # explicit cross-target (aarch64-/x86_64-apple-darwin)
```

The script: builds `labonair` and `labonair-host` (package `labonair-terminal-host`, the detached session host, ADR 0009) in release mode; creates `Labonair.app/Contents/{MacOS,Resources}`; copies both binaries, `AppIcon.icns`, `LICENSE`, `ADDITIONAL-TERMS.md`, asset/vendor license texts (`Resources/Licenses/{Fonts,GPUI,Icons}`) and a `cargo-about` generated `THIRD-PARTY-LICENSES.txt`; writes a version-substituted `Info.plist` and `PkgInfo` and lints the plist; then runs the optional steps below and always emits the update tarball and `latest.json`. Fonts and Tree-sitter grammars are compiled into the binary. `packaging/macos/Info.plist` registers the `labonair` URL scheme (Settings field links); keep it aligned with the open-URL handler in `crates/app` ([settings.md](settings.md)). Bundle identifier: `com.labonair.rust`.

A universal binary is not scripted; run the two `--target` builds and `lipo -create` the binaries manually.

Run the native app locally with `cargo run -p labonair` or the absolute bundle path `target/release/bundle/macos/Labonair.app/Contents/MacOS/labonair`. Never `open -a Labonair`: the legacy Tauri app has the same display name. Visual-verification rules: [testing.md](testing.md).

## 3. Signing and notarization (conditional)

Both are optional and never block the build; without the env vars the bundle is unsigned (Gatekeeper: right-click Open, or `xattr -dr com.apple.quarantine`).

| Env var | Effect |
|---|---|
| `LABONAIR_SIGN_IDENTITY` | `codesign --force --deep --options runtime --timestamp` with `packaging/macos/Labonair.entitlements`, then verify; also signs the dmg. `"-"` = ad-hoc. |
| `LABONAIR_NOTARY_PROFILE` | With a signing identity: `xcrun notarytool submit --wait` + `stapler staple` on the dmg if `--dmg`, else on a zip of the `.app`. Profile is a keychain profile from `notarytool store-credentials`. |
| `LABONAIR_UPDATER_KEY` / `LABONAIR_UPDATER_KEY_PASSWORD` | Path to a minisign secret key and its password; signs the update tarball (needs `minisign` on PATH). |

The entitlements file declares keychain access group `com.labonair.rust` and disables the app sandbox (terminal/editor filesystem access, SSH/SFTP networking).  The update tarball is built from the `.app` after these steps (stapled only if notarization ran on the `.app`, i.e. without `--dmg`).

## 4. Auto-update (`crates/updater`, UI in `crates/updater-ui`)

Custom minimal updater, not Sparkle; it reuses a Tauri-compatible `latest.json` and minisign signatures.

- Manifest at `DEFAULT_UPDATE_ENDPOINT` (`https://github.com/Snenjih/Labonair/releases/latest/download/latest.json`): `{ version, notes, pub_date, platforms: { "<os>-<arch>": { url, signature } } }`; `signature` is the base64 of the whole `.minisig` file.
- Check: `fetch_manifest()`; an update is offered only for a strictly newer SemVer that has an artifact for this `UPDATE_TARGET`. Auto-check every `CHECK_INTERVAL` (6 h), last check tracked in `<config_dir>/updater-last-check`, gated by the `checkForUpdates` preference.
- Download with progress, then `verify_update()`: minisign signature against `UPDATE_PUBLIC_KEY` (set in `crates/updater/src/install.rs`). Empty key or signature is a hard failure; updates are never applied unverified.
- Apply (`apply_macos_update`): unpack `<name>.app.tar.gz`, atomically swap the running `.app` (old bundle moved aside, rolled back on failure), `relaunch()` via `open`.
- UI: `UpdaterView` dialog (available/downloading/ready), a "Check for Updates..." app-menu/palette entry, failures through the notification system.
- Signing the artifact: keypair via `minisign -G`; public key goes into `UPDATE_PUBLIC_KEY`; the secret key and password are the CI secrets `LABONAIR_UPDATER_PRIVATE_KEY` / `LABONAIR_UPDATER_KEY_PASSWORD`. The script writes `latest.json` next to the artifacts with the signature inline, or an empty signature if `LABONAIR_UPDATER_KEY` is unset (the app then refuses that update). Key rotation requires shipping a build with the new public key.
- Each script run emits a single-arch manifest (`darwin-aarch64` or `darwin-x86_64`); a manifest serving both architectures must be merged by hand before upload.

## 5. CI and release workflows

- `.github/workflows/ci.yml`: macOS job runs `scripts/check.sh` (fmt, check, clippy `-D warnings`, test, `scripts/check-crate-deps.sh`); Linux job is `cargo check --workspace --all-targets`, informational.
- `.github/workflows/release.yml`: on `v*` tags or manual dispatch, on `macos-latest`: installs `cargo-about` 0.9.3 and `minisign`, writes the updater key from secret `LABONAIR_UPDATER_PRIVATE_KEY` if present (otherwise the artifact stays unsigned), then runs `scripts/smoke-test.sh && scripts/package-macos.sh --dmg` with optional secrets `LABONAIR_SIGN_IDENTITY`, `LABONAIR_NOTARY_PROFILE`, `LABONAIR_UPDATER_KEY_PASSWORD`, and attaches `*.dmg`, `*.app.tar.gz`, `latest.json` to the release on tag pushes.

## 6. Verifying a release

```sh
scripts/smoke-test.sh                          # builds the bundle, checks structure, runs `cargo test -p labonair --test smoke`
LABONAIR_SMOKE_LAUNCH=1 scripts/smoke-test.sh  # also launches the bundled app by absolute path for 5 s
```

Structure checks: bundle dir, executable, Info.plist (present, lints, version substituted, identifier `com.labonair.rust`), icon, PkgInfo, and the signature when `LABONAIR_SIGN_IDENTITY` is set. Release evidence records the exact target, artifact digest, smoke command, signing result and any intentional unsigned state. A Linux container build never closes macOS packaging or visual evidence.

## 7. Artifacts and platforms

| Platform | Artifact | Status |
|---|---|---|
| macOS Apple Silicon | `Labonair_<version>_arm64.dmg` | primary |
| macOS Intel | `Labonair_<version>_x86_64.dmg` | via `--target` build |
| macOS | `Labonair_<version>_<arch>.app.tar.gz`, `latest.json` | update payload and manifest |
| Linux | none | builds from source (`cargo build --release -p labonair`; needs Vulkan loader, `libxkbcommon`, wayland/xcb, `fontconfig`, openssl dev libs); no packaging, no updater. Candidate formats AppImage/Flatpak behind a future `scripts/package-<os>.sh`. |
| Windows | none | out of scope |

Limitations vs. the original Tauri app: no in-app web/URL preview (GPUI cannot embed a WebView; markdown rendering and open-in-system-browser instead); no Windows; auto-update macOS only.

## 8. Rollback and support

The updater fails closed on a bad key/signature and rolls back a failed bundle swap. For a bad release: stop distributing the artifact/manifest; keep logs and digests (no secrets); restore the previous signed bundle or republish the last known-good `latest.json`; verify launch, settings/session recovery and updater state; record the incident and next gate. A release is supportable only when platform, packaging, visual, smoke, security and rollback evidence is explicit; unmeasured states stay `Pending`/`N/A`. Support handoffs include version, target, artifact, commit, known limitations, recovery path and next action, never user data or credentials. Never commit signing keys, notarization profiles, updater private keys or bearer tokens.
