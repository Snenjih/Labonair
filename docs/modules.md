# Module and Crate Rules

Related: [`architecture.md`](architecture.md) (layers, dependency rules, crate map), [`registries.md`](registries.md).

## 1. Ownership

Every capability has exactly one owning module, responsible for its domain models, state and lifecycle, backend operations, UI, commands and keybindings, emitted notifications, persistence and migration, and unit/integration tests. No other module duplicates this.

Each product capability has exactly one canonical capability crate, the default home of its public contract and core behavior. If it grows, sibling `-ui`, `-storage`, or `-integration` crates may be added inside the same module; they are an implementation split, not a new owner. A crate never mixes unrelated product capabilities.

> One product capability has one owning module and one canonical capability crate. More crates are allowed only as named siblings within that module and only when a real boundary exists.

This prevents both mixed god-crates and forests of empty facade crates.

## 2. When to create a module or crate

Create a **module** when a capability has its own user intent, lifecycle, state, or persistence (SSH, SFTP, transfers, themes, backgrounds, hosts, notifications, the MCP agent bridge). A visual primitive or one-line helper is not a module: reusable buttons, lists, badges, and other controls belong to `labonair-ui-kit`.

Split a module into **multiple crates** when at least one holds: core logic is testable without GPUI; UI and backend have different dependency needs; persistence is an independent boundary; several consumers need a stable contract; compilation isolation materially helps; a platform integration must be isolated. Do not split to mirror a theoretical layer. A new crate needs one responsibility, a current consumer or independent test boundary, and a public API that reduces coupling.

```text
crates/<capability>/             # canonical capability crate (may hold UI+backend for a small capability)
crates/<capability>-ui/          # optional GPUI views and interaction
crates/<capability>-storage/     # optional substantial persistence boundary
crates/<capability>-integration/ # optional external protocol/platform adapter (e.g. -ssh, -transport)
crates/<capability>-host/        # optional injected contract so consumers avoid depending on the owner
```

Every sibling exposes a narrow public API. Public APIs contain domain values, commands, events, traits, and registration functions; they do not expose internal GPUI entities, storage schemas, or implementation-specific widgets unless the owning layer requires it.

## 3. Feature change workflow

Before: identify the owning module; define the user flow and canonical entry point; define the public contract and events; check for an applicable registry or UI-kit component; document new settings, commands, persistence, and notifications.

During: keep state and behavior in the owner; send user-visible messages through notifications; use UI-kit components; add no shell conditionals; test the contract and the user flow; update `scripts/crate-graph.toml` if a new edge is intentional. A feature registers its own commands, keybindings, panels, status items, theme entries, and notification action handlers through the typed registries; the composition root only calls registration functions and injects services.

Before completion: remove compatibility code made obsolete by the change; verify all affected surfaces; update the relevant normative doc or ADR; run the verification gates ([`architecture.md`](architecture.md) §11).

## 4. Removal and migration

Removing a capability requires checking command IDs, keymap entries, settings, persistence, migrations, registries, status items, menus, docs, and tests. Keep a data migration only when it protects user data; do not keep dead UI compatibility.

For a migration, write down the current owner and all consumers, establish the target contract and owner, and move one boundary at a time. Keep any temporary adapter behind a named removal condition, update the matrix and `crate-graph.toml` together, and delete the old registration and compatibility path once consumers moved; never run two implementations in parallel indefinitely.

## 5. Capability matrix

The authoritative index of product capabilities: one owner, one canonical crate, one canonical entry point per row. Siblings remain under the same owner. The Application row makes the composition boundary visible; it has a coordination responsibility instead of a capability. Reusable foundation crates are listed in [`architecture.md`](architecture.md) §3. Reopen a deferred or removed capability only through an explicit product decision and an owner-backed contract.

| Capability | Disposition | Contract / domain crate | UI crate | Storage / integration | Canonical entry point | Current state |
|---|---|---|---|---|---|---|
| Application composition | Keep / simplify | `labonair-shell` | `labonair-shell` | named integration siblings composed by the shell | App startup | Owns construction and registration only. |
| Workspace | Redesign | `labonair-workspace` | `labonair-workspace` | `labonair-workspace-storage`: GPUI-free `workspace.db` schema, lease transactions, revisions, change log, legacy import, scrollback GC permits | Titlebar tab strip, or Tabs dock panel when `tabsLocation == "sidebar"`; Spaces popover for explicit join | Typed Empty/Standalone/Project state and `WorkspaceEvent` boundary; durable UUID Space/Tab/Pane/split-node IDs and per-window instance IDs; atomic last-used free-Space claims and explicit join leases; new empty Space when all are leased; revision-checked structural mutations and off-thread live resync; idempotent import of session v3/v2/flat snapshots; `sessionRestore = false` clears shared rows only after the last lease. Active tab, focus, geometry, terminal size, selection, search, scroll stay local; shared Editor buffers/cursors are not synchronized. |
| Backgrounds | Keep / isolate | `labonair-background`, `labonair-background-host` | `labonair-background` | local image storage, decoded cache | Themes page hosts the Background owner editor (inline surface; no modal) | Owns its settings editor and registers its Settings link through the Settings surface contract; Workspace/Terminal render through the injected `labonair-background-host::BackgroundHost` instead of a `workspace → background` dependency. |
| Terminal | Keep / isolate | `labonair-terminal-core` (GPUI-free engine, PTY, scrollback), `labonair-terminal` | `labonair-terminal` + workspace integration | PTY/process adapter; optional detached `SessionHost` (`labonair-terminal-host`, daemon `labonair-host`, [ADR 0009](adr/0009-detached-session-host.md)) | Terminal tab or standalone terminal | Detached sessions share one host-side PTY/emulator across app processes; each view has an independent output queue and snapshot/resync path. V2 input is sequence-acknowledged and ordered; one resize-controller lease owns PTY sizing. Closing a view detaches; explicit kill ends the session. V1 clients keep legacy input and get only compatible control messages. Workspace persists shared session references in its own store. |
| Editor | Keep / isolate | `labonair-editor` | `labonair-editor-ui`, hosted by Workspace | filesystem adapter; Editor-owned local language-service runtime/process boundary | Editor tab or standalone editor | Core and GPUI view are editor-owned; Workspace owns tab hosting and injects filesystem, Git, theme, and language-service adapters. File preview (images, SVG, Markdown, tables, PDF/HTML external-open) is an editor content form: `ViewMode::{Source, Preview}`, `PreviewView` and the Markdown parser live in `labonair-editor-ui`, toggled by `editor::TogglePreview` ([ADR 0015](adr/0015-preview-merged-into-editor.md)). |
| Filesystem | Keep | `labonair-filesystem` | consuming feature UI | local filesystem and watcher | Explorer/editor consumers | Extracted. |
| SSH | Keep / redesign entry points | `labonair-ssh` | SSH connection surface | `labonair-ssh-transport`: russh, auth, tunnels, jump hosts | Host action or standalone SSH | Contract and injected workspace/Hosts paths migrated; transport isolated from the composition root. |
| SFTP | Keep / isolate | `labonair-sftp` | SFTP browser | `labonair-sftp-ssh`: russh-sftp session adapters | Host action or standalone SFTP | Session/browser contract and view done; transfer queue stays separate. Browser chrome (pane label + item count, address/search/refresh/hidden toolbar, reorderable metadata headers, zebra rows, `..` row, directional drop highlight) is view-local, driven by `fileManager` `Sftp*` settings via a `SettingsStore` observer. The `>_ Term` action emits `SftpEvent::OpenRemoteTerminal`, opening an SSH terminal tab through the workspace. |
| Hosts | Redesign | `labonair-hosts`, `labonair-hosts-host` | `labonair-hosts-ui` | `labonair-persistence`, credentials, SSH/SFTP integration siblings | Titlebar global menu / palette → Settings `Hosts` page (full-page inline surface below Keymap, [ADR 0007](adr/0007-hosts-settings-page.md)) | SQLite-backed manager composed once by the shell; Hosts owns rendering, picker snapshots, recent ordering, typed SSH/SFTP requests. Settings embeds its `Entity<HostManagerView>` and stores no host data. `Workspace` keeps the same entity as the live host-data hub (`labonair-hosts-host::HostView` serves tunnel status and picker rows). |
| Credentials | Keep / isolate | `labonair-credentials` | Hosts UI consumer | `labonair-secrets`, keychain | Host management | Extracted. |
| Transfers | Redesign | `labonair-transfers` | `labonair-transfers-ui` | `labonair-transfers-ssh`: SFTP worker and adapters | Statusbar Transfers badge | Typed registry, worker, statusbar UI isolated; shell only injects the integration. Detached-host conflict dialogs are fenced by a one-use prompt ID and initiating connection; other v2 instances see read-only state and may take over after disconnect. |
| Git / source control | Keep / isolate | `labonair-git` | `labonair-panel-scm`, `labonair-panel-git-graph`, Project Diff | `labonair-git-transport`: local/remote Git execution and adapters | Source Control panel / palette | Contracts and concrete integration isolated. |
| Explorer | Redesign | `labonair-panel-explorer`, `labonair-explorer-host` | `labonair-panel-explorer` | `labonair-filesystem`; shell-injected SSH/SFTP adapter | Explorer dock panel | No `workspace`/`ssh`/`sftp` dependency edges. Open-file/terminal/preview/active-file intents and SSH-backed directory reads cross the injected `labonair-explorer-host` contracts; concrete remote services stay in shell composition. |
| Snippets | Keep / isolate | `labonair-snippets`, `labonair-snippets-host` | `labonair-panel-snippets` | persistence; `labonair-snippets-ssh` SSH executor | Snippets panel / palette | No `workspace` dependency; inject/run/SSH-session intents cross `labonair-snippets-host::SnippetExecutionHost`. |
| Settings | Redesign | `labonair-settings` | `labonair-settings-ui` | `-settings-content`, `-settings-json`, `-settings-macros` | Settings window | Value model is current state; Settings owns the typed owner-surface registry; see [`settings.md`](settings.md). |
| Keymap | Redesign | `labonair-keymap` | `labonair-keymap-ui` | keymap file, binding registry | Titlebar global menu → Keymap; Settings → Keymap; palette | UI-free keymap crate; `KeymapManagementView` renders as the workspace `Keymap` tab (`Workspace::open_keymap_tab`), not an OS window; shell keeps only platform key installation/watch wiring. The tab groups commands by section, filters All/Modified/Conflicts/Unbound, is keyboard-navigable, records keystrokes (`keymap-ui` `keystroke` module + `App::intercept_keystrokes`), edits in an anchored popover with a context picker, offers "Reset to default" (`file::remove_user_binding_override`), shows `keymap.json` diagnostics as a banner, and live-reloads on external changes (`KeymapManagementView::reload`, `settings::watch_file`). Conflict/override state is derived on `management` (`OverrideState`, `conflict_commands`, `shadowed_defaults`). |
| Command Palette | Redesign | `labonair-command-palette-core` | `labonair-command-palette` | typed providers and `PaletteActionHandlerRegistry` contributions | Titlebar global menu / global shortcut | Metadata, ordinary handlers, and dynamic submenu actions are owner-contributed; shell composes and forwards actions. |
| Activity indicator | New | `labonair-activity` | `labonair-activity-ui` | none; producers: Git, SSH connection state, Editor language servers, Updates, Explorer and SFTP listing | Statusbar activity item (left cluster) | Registry orders running work by priority (connection > language server > git > file job > failure > package > notice) with delayed reveal, keyed de-duplication, sticky failures; item shows the top entry with `+ N more` and a popover with progress and cancel. Transfers keep their own badge. |
| Notifications | Redesign | `labonair-notifications-core` | `labonair-notifications` | none; retained in registry | Statusbar dropdown plus opt-in toast overlay | One registry owns retained notifications, toast lifetime, grouped problems/activity, search, clear-all, and data-only actions ([ADR 0011](adr/0011-actionable-notification-presentations.md)). |
| Themes (color and icon) | Redesign | `labonair-theme`, `labonair-theme-ui` | `labonair-theme` (runtime + catalogs); `labonair-theme-ui` (settings→store policy, Settings surface registration, palette handler) | extension and distribution contracts not yet defined | Titlebar global menu → Themes / Icon Themes → palette submenu | App-theme and icon-theme registries are current state; extension, import, discovery, and management are open. |
| Updates | Keep / isolate | `labonair-updater` | `labonair-updater-ui` | release manifest, signed artifact verification, macOS installation | Settings/global menu update action | Logic, dialog, state view, status item, command contribution isolated from the shell UI. |
| Onboarding | Redesign | `labonair-onboarding` | `labonair-onboarding` | per-installation progress file in the data dir; consumes `labonair-ssh` `SshConfigService` | First launch (workspace tab), then palette `Onboarding: Open` | One crate owns step registry, progress, page view, built-in Start/Look/Sessions/Hosts steps, and the command; hosted as a singleton `Onboarding` tab; writes existing settings and imports hosts only through owner contracts; adds no Settings key ([ADR 0008](adr/0008-onboarding-module.md)). |
| AI | Keep | `labonair-ai` | none while the AI frontend is parked | provider/session persistence; MCP in `labonair-mcp-core` / `labonair-mcp-server` | AI panel / workspace context (frontend removed) | Backend core retained; UI and service rework open. |

### Explicit product dispositions

Surfaces, strategies, or predecessor behaviors that are not independent ownership boundaries:

| Item | Decision | Consequence |
|---|---|---|
| Duplicate Settings management surfaces for Hosts, Themes, Icon Themes, Shortcuts | Remove | Settings may link to the Keymap owner editor and embed the Hosts owner's full-page surface (ADR 0007); owners keep state and persistence. |
| Toast notifications | Integrate | Selected events use the retained notification's opt-in transient presentation; toast and dropdown share one record and actions ([ADR 0011](adr/0011-actionable-notification-presentations.md)). |
| Feature-local operation-error banners | Remove | Failures publish notifications; dialogs and field validation remain only where a decision or correction is needed. |
| Jump-host primary menu/badge | Remove surface, keep capability | Jump hosts stay in SSH connection configuration/execution; no statusbar item. |
| Remote theme/icon-theme downloads | Open | Owner, trust model, and service boundary must be defined before implementation. |
| User theme-file import/export (local `.json`) | Open | Existing code stays dormant until an owner exposes a defined workflow. |
| Static shell-wide command tables | Remove | Commands and submenus are contributed by owners through the command registry. |

### Rules for changing the matrix

1. Add a row before adding a product capability, with exactly one owner and one canonical crate; name siblings in the same row.
2. A new crate needs one responsibility, a current consumer or test boundary, and a real dependency/lifecycle/storage/platform reason; visual primitives go to `labonair-ui-kit`.
3. Record the canonical entry point first; alternate entry points delegate to it and never duplicate behavior.
4. Update the matrix and `scripts/crate-graph.toml` in the same change as a new internal edge or ownership move.
5. Do not delete a row because a feature is parked; mark its disposition. Remove it only on an explicit `remove` decision with data/command migration complete.
