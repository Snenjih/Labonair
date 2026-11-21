# Labonair

**A native workspace for local and remote development and operations.**

> Based on Labonair by Snenjih — https://labonair.dev

Labonair brings terminals, remote connections, file management, code editing, and Git workflows together in one keyboard-first desktop application. It is built in Rust with GPUI and has a native interface without a WebView or JavaScript frontend.

## What it can do

- **Workspaces and terminals:** project or standalone workspaces, tabs, split panes, local PTY terminals, and optional detached sessions. With the detached session host enabled, sessions can outlive a window and workspace structure can be shared between app instances.
- **Remote work:** saved hosts, SSH terminals, jump hosts, an SFTP browser, file transfers, and local or SSH-backed file browsing.
- **Editing and Git:** code editing with language-service support, source control, Git history, and project diffs.
- **Workflow and appearance:** command palette, editable key bindings, color and icon themes, backgrounds, settings, activity status, and persistent notifications.
- **Local automation:** an optional MCP Streamable HTTP bridge can operate explicitly granted local or SSH terminal tabs. It is disabled by default and binds to loopback. A grant allows commands and keystrokes in the visible terminal; read the [automation contract](docs/automation.md) and [security model](docs/security.md) before enabling it.

The AI provider and tool backend remain in the workspace, but the user-facing AI frontend is currently parked and is not available as an app panel. Labonair also has no in-app web or URL preview; URLs open in the system browser.

## Platform status

| Platform | Current status |
|---|---|
| macOS | Primary platform and designated packaged release target. |
| Linux | Source builds are available; there is no packaged Linux release. Linux CI is informational. |
| Windows | Not currently supported. |

The macOS bundle script currently has a known version-reading issue: the app manifest inherits its version from the workspace, while the script expects a version in the app manifest. See [release and packaging](docs/release.md) for the details and current release workflow.

## Build and run from source

The Rust toolchain is pinned to **1.95.0** in `rust-toolchain.toml`.

On macOS, install Xcode Command Line Tools and the Metal Toolchain:

~~~sh
xcodebuild -downloadComponent MetalToolchain
~~~

Linux builds need the Vulkan loader, libxkbcommon, Wayland/XCB, Fontconfig, and OpenSSL development libraries.

Run the native app from the repository root:

~~~sh
cargo run -p labonair
~~~

For local development on macOS, use the Cargo command above or an explicit Rust binary path. `open -a Labonair` may launch the legacy Tauri app, which shares the same display name.

Run the workspace checks with:

~~~sh
scripts/check.sh
~~~

The script runs formatting, workspace checks, Clippy, tests, and crate dependency checks. More build and release details are in [release and packaging](docs/release.md).

## Project documentation

- [Documentation index](docs/README.md)
- [Product scope and interaction model](docs/product.md)
- [Architecture and dependency rules](docs/architecture.md)
- [Capability ownership matrix](docs/modules.md)
- [Workspace model](docs/workspace-model.md)
- [Settings contract](docs/settings.md)
- [Security model](docs/security.md)
- [MCP automation contract](docs/automation.md)
- [Release and packaging](docs/release.md)

## License and project name

Labonair source code is licensed under **GPL-3.0-or-later**, with the additional terms in [ADDITIONAL-TERMS.md](ADDITIONAL-TERMS.md). The Labonair name, logo, and app icon are trademarked or reserved brand assets; the source license does not grant permission to use them in a fork. See [LICENSE](LICENSE) and [TRADEMARK.md](TRADEMARK.md).
