# Module and Crate Rules

**Status:** Normative
**Version:** 1

## 1. Ownership rule

Every capability has exactly one owning module. The owner is responsible for:

- domain models;
- state and lifecycle;
- backend operations;
- user interface;
- commands and keybindings;
- notifications it emits;
- persistence and migration;
- unit and integration tests.

No other module may duplicate this responsibility.

Each product capability also has exactly one canonical capability crate. The
crate is the default home for the capability's public contract and core
behavior. If the capability grows, sibling `-ui`, `-storage`, or
`-integration` crates may be added inside the same module; they are an
implementation split, not a new owner. A crate must never mix unrelated
product capabilities.

## 2. When to create a module

Create a module when a capability has its own user intent, lifecycle, state, or
persistence. Examples are SSH, SFTP, transfers, themes, backgrounds, hosts,
notifications, and the MCP agent bridge.

Do not create a module for a visual primitive or a one-line helper. Those belong to foundation crates.

The product-module rule is intentionally explicit:

> One product capability has one owning module and one canonical capability
> crate. More crates are allowed only as named siblings within that module and
> only when a real boundary exists.

This prevents both mixed god-crates and speculative forests of empty facade
crates. A reusable button or list is a UI-kit component, not a product crate.
Buttons, lists, badges, and other reusable controls remain in the UI kit.

## 3. When to create another crate

Split a module into multiple crates when at least one of these is true:

- the core logic can be tested without GPUI;
- UI and backend have different dependency requirements;
- persistence is an independent boundary;
- multiple consumers need a stable contract;
- compilation isolation materially improves iteration;
- a platform integration needs to be isolated.

Do not split merely to mirror a theoretical layer. The new crate must have a
single responsibility, a current consumer or independent test boundary, and a
public API that reduces coupling. Do not create an empty facade crate.

## 4. Standard module layout

```text
crates/<capability>/             # canonical capability crate
crates/<capability>-ui/          # optional GPUI views and interaction
crates/<capability>-storage/     # optional substantial persistence boundary
crates/<capability>-integration/ # optional external protocol/platform adapter
```

The canonical crate may contain the UI and backend for a small capability. As
the module grows, move a responsibility into a sibling crate without moving
ownership to another module. Every sibling must expose a narrow public API.

## 5. Public API rule

Public APIs contain domain values, commands, events, traits, and registration functions. They must not expose internal GPUI entities, storage schemas, or implementation-specific widgets unless the owning layer explicitly requires them.

## 6. Feature change workflow

Before implementation:

1. identify the owning module;
2. define the user flow and canonical entry point;
3. define the public contract and events;
4. check whether an existing registry or UI-kit component applies;
5. document new settings, commands, persistence, and notifications;
6. add a focused implementation task.

During implementation:

1. keep state and behavior in the owning module;
2. send user-visible messages through notifications;
3. use UI-kit components;
4. avoid adding shell conditionals;
5. test the contract and the user flow;
6. update the dependency allow-list if a new edge is intentional.

The composition root may call registration functions and inject concrete
services, but it must not own feature state, render feature views, or contain
feature-specific dispatch tables. A feature registers its own commands,
keybindings, panels, status items, theme entries, and notification action
handlers through the appropriate typed registry.

Before completion:

1. remove compatibility code made obsolete by the change;
2. verify all affected surfaces;
3. update the relevant normative document or ADR;
4. run the full verification gates.

## 7. Removal workflow

Removing a capability requires checking command IDs, keymap entries, settings, persistence, migrations, registries, status items, menus, documentation, and tests. Keep data migration only when it protects user data; do not keep dead UI compatibility indefinitely.

For a migration, first write down the current owner and all consumers, then
establish the target contract and owner. Move one boundary at a time, keep any
temporary adapter behind a named removal condition, and update the capability
matrix, inventory, roadmap/task, and dependency verifier together. Delete the
old registration and compatibility path once consumers have moved; do not run
two implementations in parallel indefinitely.

## Capability Matrix

**Related:** [`architecture.md`](architecture.md), [`modules.md`](modules.md), [`registries.md`](registries.md)

This matrix is the authoritative index for product capabilities. A capability
has one owning module, one canonical capability crate, one canonical user
entry point, and one composition boundary. A module may have sibling crates
for real UI, storage, or integration boundaries, but they remain under the
same owner. The `Current implementation` column is deliberately separate from
the target columns so that documentation does not turn an unfinished
migration into an architectural claim.

The application-composition and foundation rows make non-product boundaries
visible for dependency auditing. The one-capability/one-capability-crate rule
applies to product rows; composition and foundation crates have one explicit
coordination or reusable-service responsibility instead.

The dispositions below describe the current implementation and product
decisions. Reopen a deferred or removed capability only through an explicit
product decision and an owner-backed contract.

| Capability | Disposition | Owning module | Contract / domain crate | UI crate | Storage / integration | Canonical entry point | Current implementation state |
|---|---|---|---|---|---|---|---|
| Application composition | Keep / simplify | `application` | `labonair-shell` | `labonair-shell` | named integration siblings composed by `labonair-shell` | App startup | Composition root owns construction and registration only |
| Workspace orchestration | Redesign | `workspace` | `labonair-workspace` | `labonair-workspace` | `labonair-workspace-storage` owns the GPUI-free `workspace.db` schema, lease transactions, revisions, committed change log, legacy import, and scrollback GC permits | Workspace surface (titlebar tab strip, or the Tabs dock panel when `tabsLocation == "sidebar"`); Spaces popover for explicit join | Typed Empty/Standalone/Project state and WorkspaceEvent boundary; durable UUID Space/Tab/Pane/split-node IDs and per-window instance IDs; atomic last-used free-Space claims and explicit join leases; new empty Space allocation when all existing Spaces are leased; revision-checked structural mutations and off-thread live resync; idempotent import of session v3/v2/flat snapshots; `sessionRestore = false` clears shared Workspace rows only after the last lease. Active tab, focus, geometry, terminal size, selection, search, and scroll remain local. Shared Editor buffers/cursors are not synchronized. |
| Backgrounds | Keep / isolate | `backgrounds` | `labonair-background`, `labonair-background-host` | `labonair-background` | local image storage and decoded cache | Themes page hosts the Background owner editor (inline surface; the former options modal is removed) | Capability extracted from workspace; Background owns its settings editor and registers its Settings link through the Settings surface contract; Workspace/Terminal render through the injected `labonair-background-host::BackgroundHost` contract instead of the `workspace → background` dependency (R07-005) |
| Terminal | Keep / isolate | `terminal` | `labonair-terminal-core` (GPUI-free engine, PTY session, scrollback), `labonair-terminal` | `labonair-terminal` and workspace integration | PTY/process adapter; optional detached session host (`SessionHost`, daemon `labonair-host`, [ADR 0009](adr/0009-detached-session-host.md)) | Terminal tab or standalone terminal | Detached sessions share one host-side PTY/emulator across multiple app processes; each view has an independent output queue and snapshot/resync path. V2 input is sequence-acknowledged and ordered; one explicit resize-controller lease owns PTY sizing. Closing a view detaches it; explicit kill ends the session. V1 clients keep legacy input and receive only compatible control messages. Workspace persists shared session references in its own store. |
| Editor | Keep / isolate | `editor` | `labonair-editor` | `labonair-editor-ui`, hosted by Workspace | filesystem adapter plus Editor-owned local language-service runtime/process boundary | Editor tab or standalone editor | Core and GPUI view are editor-owned; Workspace owns tab hosting and injects filesystem, Git, theme, and language-service adapters |
| Filesystem | Keep | `filesystem` | `labonair-filesystem` | Consuming feature UI | local filesystem and watcher | Explorer/editor consumers | Extracted |
| SSH | Keep / redesign entry points | `ssh` | `labonair-ssh` | SSH connection surface | `labonair-ssh-transport` owns russh, auth, tunnels, jump hosts | Host action or standalone SSH | Contract and injected workspace/Hosts paths migrated; concrete transport is isolated from the composition root |
| SFTP | Keep / isolate | `sftp` | `labonair-sftp` | SFTP browser | `labonair-sftp-ssh` owns russh-sftp session adapters | Host action or standalone SFTP | Authenticated session/browser contract and view migration complete; transfer queue remains separate. Browser chrome (pane label + item count, address/search/refresh/hidden toolbar, reorderable metadata column headers, zebra rows, `..` row, directional drop highlight) is view-local, driven by the `fileManager` `Sftp*` settings via a `SettingsStore` observer. The `>_ Term` action emits `SftpEvent::OpenRemoteTerminal`, opening an SSH terminal tab through the workspace. |
| Hosts | Redesign | `hosts` | `labonair-hosts`, `labonair-hosts-host` | `labonair-hosts-ui` | `labonair-persistence`, credentials, SSH/SFTP integration siblings | Titlebar global menu / Command Palette → the Settings `Hosts` page (full-page inline surface below Keymap, ADR 0007) | Canonical SQLite-backed manager is composed once by the shell; Hosts owns rendering, picker snapshots, recent ordering, and typed SSH/SFTP requests; Settings embeds its `Entity<HostManagerView>` as a full-page inline surface registered by Hosts (ADR 0007) and stores no host data. `Workspace` keeps the same entity as the live host-data hub (the `labonair-hosts-host::HostView` contract serves tunnel status and picker rows). |
| Credentials | Keep / isolate | `credentials` | `labonair-credentials` | Hosts UI consumer | `labonair-secrets`, keychain | Host management | Extracted |
| Transfers | Redesign | `transfers` | `labonair-transfers` | `labonair-transfers-ui` | `labonair-transfers-ssh` owns the SFTP worker and adapters | Statusbar Transfers badge | Typed registry, worker, and statusbar UI are isolated; shell only injects the concrete integration. Detached-host conflict dialogs are fenced by a one-use prompt ID and initiating connection; other v2 instances see read-only state and may explicitly take over after disconnect. |
| Git / source control | Keep / isolate | `git` | `labonair-git` | `labonair-panel-scm`, `labonair-panel-git-graph`, Project Diff | `labonair-git-transport` owns local/remote Git execution and adapters | Source Control panel / palette | Contracts and concrete integration are isolated |
| Explorer | Redesign | `explorer` | `labonair-panel-explorer`, `labonair-explorer-host` | `labonair-panel-explorer` | `labonair-filesystem`; shell-injected SSH/SFTP adapter | Explorer dock panel | Canonical Explorer capability/UI crate; `workspace`, `ssh`, and `sftp` implementation edges are removed (R07-004). Open-file/terminal/preview/active-file intents and SSH-backed directory reads cross the injected `labonair-explorer-host` contracts; concrete remote services remain in shell composition. |
| Snippets | Keep / isolate | `snippets` | `labonair-snippets`, `labonair-snippets-host` | `labonair-panel-snippets` | persistence and injected SSH executor | Snippets panel / palette | Contracts and panel isolated; the `workspace` dependency is removed (R08-003) — inject/run/SSH-session intents cross the injected `labonair-snippets-host::SnippetExecutionHost` contract |
| Settings | Redesign / reference parity | `settings` | `labonair-settings` | `labonair-settings-ui` | `labonair-settings-content`, `labonair-settings-json`, `labonair-settings-macros` | Settings window | Existing value model remains current state; Settings owns the typed owner-surface registry contract; category, field, navigation, and control parity is specified by R07-000 |
| Keymap | Redesign | `keymap` | `labonair-keymap` | `labonair-keymap-ui` | keymap file and binding registry | Titlebar global menu → Keymap; Settings → Keymap; quick access through palette | UI-free keymap crate is active; `labonair-keymap-ui` registers its Settings link through the Settings surface contract and its `KeymapManagementView` renders as a workspace `Keymap` tab (`Workspace::open_keymap_tab`), not an OS window; shell retains only platform key installation/watch wiring. The tab groups commands by section, filters by All/Modified/Conflicts/Unbound, is keyboard-navigable, records keystrokes instead of requiring hand-typed chords (`keymap-ui`'s `keystroke` module + `App::intercept_keystrokes`), edits in an anchored popover with a context picker for new bindings, "Reset to default" (`file::remove_user_binding_override`), surfaces `keymap.json` diagnostics as an in-tab banner, and live-reloads on external file changes (`KeymapManagementView::reload`, wired to `settings::watch_file` by the workspace). Conflict/override state is derived data on `management` (`OverrideState`, `conflict_commands`, `shadowed_defaults`). |
| Command Palette | Redesign | `command-palette` | `labonair-command-palette-core` | `labonair-command-palette` | typed providers and `PaletteActionHandlerRegistry` contributions from feature modules | Titlebar global menu / global shortcut | Metadata, ordinary command handlers, and dynamic submenu actions are owner-contributed; shell only composes and forwards actions |
| Activity indicator | New | `activity` | `labonair-activity` | `labonair-activity-ui` | none; producers are Git, SSH connection state, Editor language servers, Updates, Explorer and SFTP remote listing | Statusbar activity item (left cluster) | UI-free registry orders running work by priority (connection > language server > git > file job > failure > package > notice), supports delayed reveal, keyed de-duplication and sticky failures; the status item shows the top entry with `+ N more` and a popover listing all with progress and cancel. Transfers keep their own badge. |
| Notifications | Redesign | `notifications` | `labonair-notifications-core` | `labonair-notifications` | none; retained in registry | Statusbar notification dropdown | Registry and statusbar presentation are capability-owned; migrated operation errors use structured details/source/deduplication; no toast surface |
| Themes (color and icon) | Redesign / reference parity | `themes` | `labonair-theme`, `labonair-theme-ui` | `labonair-theme` (runtime + catalogs); `labonair-theme-ui` (settings→store policy + Settings surface registration + palette handler) | extension and distribution contracts pending R07-000 | Titlebar global menu → Themes / Icon Themes → palette submenu | Existing app-theme and icon-theme registries remain current state; extension, import, discovery, and management behavior is crosswalked against the pinned baseline |
| Updates | Keep / isolate | `updater` | `labonair-updater` | `labonair-updater-ui` | release manifest, signed artifact verification and macOS installation | Settings/global menu update action | Capability logic, dialog, state view, status item, and command contribution are isolated from the shell UI |
| Onboarding | Redesign | `onboarding` | `labonair-onboarding` | `labonair-onboarding` | per-installation progress file in the data dir; consumes `labonair-ssh` `SshConfigService` | First launch (workspace tab), then palette `Onboarding: Open` | One crate owns the step registry, progress, page view, built-in Look/Start/Hosts steps, and the command; the workspace hosts the page as a singleton `Onboarding` tab; writes existing settings and imports hosts only through existing owner contracts; adds no Settings key (ADR 0008) |
| AI | Keep / reference parity | `ai` | `labonair-ai` | `labonair-ai` until a real UI boundary requires a sibling crate | provider/session persistence; MCP integration in `labonair-mcp-server` | AI panel / workspace context | Core retained; UI and service rework are open and must be crosswalked against the pinned baseline |

### Explicit product dispositions

These items are deliberately not capability rows because they are surfaces,
implementation strategies, or predecessor behaviors rather than independent
product ownership boundaries. Earlier keep/defer/remove decisions are subject
to the pinned-reference crosswalk; only independent architecture rules remain
binding without that review:

| Item | Decision | Consequence |
|---|---|---|
| Duplicate Settings management surfaces for Hosts, Themes, Icon Themes, and Shortcuts | Remove | Settings may expose the pinned Keymap page as a link to its canonical owner editor and the Hosts owner's management view as a full-page inline surface (ADR 0007); owners retain state and persistence, and other management flows remain with their owners. |
| Toast notifications | Remove | Passive messages are retained and displayed only by the notification registry and statusbar dropdown. |
| Duplicate feature-local operation-error banners | Remove | Operation failures publish notifications; actionable dialogs and field validation remain only where a decision or correction is required. |
| Jump-host primary menu/badge | Remove as a separate surface; keep the capability | Jump hosts remain part of SSH connection configuration and execution; no dedicated statusbar item is registered. |
| Remote theme/icon-theme downloads | Crosswalk against the pinned baseline | Extension and theme distribution workflows are in the parity inventory; the owner, trust model, and service boundary are defined before implementation. |
| User theme-file import/export (local `.json`) | Crosswalk against the pinned baseline | Existing code remains dormant until the parity inventory decides whether the pinned reference has an equivalent workflow and which owner exposes it. |
| Static shell-wide command tables | Remove | Commands and submenus are contributed by owning modules through the command registry. |
| Complete Labonair workflows | Deliver through owner-backed capabilities | Use the product contract, capability matrix, workflow catalog, and release evidence to define and verify the supported experience. |

### Rules for changing this matrix

1. Add a row before adding a new product capability.
2. Record exactly one owning module and one canonical capability crate per
   row. Sibling crates must be named in the same row and must not claim
   ownership.
3. A new crate must have one responsibility, a current consumer or independent
   test boundary, and a real dependency, lifecycle, storage, or platform
   reason. Visual primitives belong in `labonair-ui-kit` instead.
4. Record the canonical entry point before implementation. Alternate entry
   points may delegate to it but must not duplicate behavior.
5. Update the owning module's task, this matrix, the inventory, and the
   dependency verifier in the same change as a new internal edge or ownership
   move.
6. Do not delete a row because a feature is temporarily parked. Mark its
   disposition and migration state instead; remove it only when the product
   decision is explicitly `remove` and its data/command migration is complete.
