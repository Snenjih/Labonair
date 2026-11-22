# Labonair Architecture

Related: [`product.md`](product.md), [`modules.md`](modules.md), [`registries.md`](registries.md), [`workspace-model.md`](workspace-model.md), [`settings.md`](settings.md), [`design-system.md`](design-system.md). Machine-readable crate graph: [`scripts/crate-graph.toml`](../scripts/crate-graph.toml).

## 1. Objective and terms

Labonair is a single native Rust application built with GPUI, composed from independently owned feature modules. The composition layer connects modules; it contains no business logic. The architecture optimizes for:

- **Isolation:** a module owns its state, behavior, UI, persistence, and tests.
- **Consistency:** shared interaction and visual behavior comes from foundation crates.
- **Replaceability:** a module can be rewritten or removed without rewriting unrelated modules.

Terms:

- **Capability:** a user-facing ability (SSH, SFTP, terminal, themes, transfers).
- **Module:** the complete ownership boundary of one capability.
- **Capability crate:** the canonical Rust crate of one product capability; the default home of its contract and core behavior. It never contains unrelated capabilities.
- **Sibling crate:** an additional crate inside the same module, only for a real core/UI, storage, or integration boundary. Splitting never splits ownership.
- **Composition root:** startup code that constructs services and invokes registration functions (`labonair` and `labonair-shell` only).
- **Registry:** an extensible collection of metadata and providers owned by a foundation or host contract (see [`registries.md`](registries.md)).
- **Workspace:** a runtime context of tabs, panes, tools, and optional project/remote identity (see [`workspace-model.md`](workspace-model.md)).

Every product capability has exactly one owning module and one canonical capability crate. Foundation crates are not product capabilities and have one explicit, reusable responsibility.

## 2. Layers and dependency rules

```text
Application composition   labonair / labonair-shell
        ↓
Workspace orchestration   workspace, tabs, panes, docks, sessions
        ↓
Feature modules           terminal, editor, ssh, sftp, hosts, transfers, git, themes,
                          keymap, notifications, command-palette, settings, ai, ...
        ↓
Foundation / platform     ui-kit, theme-tokens, gpui-ext, filesystem, secrets, persistence
```

Layer labels in `scripts/crate-graph.toml` (`application`, `product`, `integration`, `foundation`) describe a crate's role, not its owner: a product owner may have a foundation contract sibling and an integration sibling.

| Layer | Responsibility | May depend on |
|---|---|---|
| `application` | Process startup, shell composition, window actions, service wiring. | Anything, through explicit composition edges. |
| `product` | User-facing capabilities, workspace orchestration, panels, presentation. | Foundation contracts/services; other capabilities only via typed contracts, registries, or events. |
| `integration` | Concrete protocol/process/filesystem/external-service adapters. | UI-free capability contracts and foundation services. |
| `foundation` | Values, contracts, UI primitives, persistence, errors, platform services. | External libraries and lower foundation crates; never product modules. |

Rules:

1. Dependencies point downward. Foundation crates never depend on product crates, the shell, or the binary.
2. Feature crates never depend on `labonair-shell` or on another feature's private types/state. A feature may depend on another module's public contract crate when its user flow requires it, never on its private state or feature UI.
3. Integration crates implement typed contracts; they never become a second owner of product state. Concrete integration code belongs in the owning module's named sibling crate and is injected through its public contracts. No capability code is added to an application-composition crate.
4. Core crates do not depend on GPUI unless their state genuinely requires it. UI-free engines (`ai`, `editor`) stay UI-free transitively.
5. `labonair-ui-kit` contains no product-specific behavior and renders against `labonair-theme-tokens` (a foundation leaf), never the `labonair-theme` feature crate.
6. Panels never depend on another panel; the panel contract (`labonair-panel`) stays below Workspace and the shell.
7. Persistence and secrets are reached through narrow service contracts.
8. Shared discovery uses a typed registry only when there are multiple providers or consumers; a single caller gets a narrow typed contract.
9. A dependency that cannot be explained by ownership, a typed contract, a registry, or composition wiring is a boundary defect. Every new edge needs a reason (manifest entry or ADR); removed edges are removed from manifest, code, and docs.
10. The graph is acyclic.

Contract crates that keep the graph acyclic:

- `labonair-command-palette-core` is the UI-free command registry contract (it belongs to the Command Palette module). Capability crates may depend on it to contribute command metadata; they must not depend on the palette UI crate. `labonair-keymap` depends on it to publish keymap-owned commands; the contract depends on no feature.
- `labonair-command-palette-runtime` is the GPUI-facing execution bridge. Owner crates contribute handlers that capture their own entities and services; the shell supplies only the active window/application context. It must never become a product facade or hold a central feature dispatch table.
- `labonair-background`, `labonair-theme-ui`, and `labonair-keymap-ui` register Settings entry points through the public `labonair-settings::SettingsSurfaceRegistry` contract. These edges expose only surface metadata and an owner callback, not Settings storage or another feature's state. `labonair-shell` depends on `labonair-keymap-ui` to compose its registration function.

The dependency guard is `scripts/check-crate-deps.sh` (`scripts/check_crate_deps.py`). It requires a manifest entry for every workspace crate, enforces a per-crate allow-list of workspace dependencies, acyclicity, and transitive must-not-reach invariants (panels do not reach the shell or each other; the panel contract and `ui-kit` stay below product modules; UI-free engines reach no UI crate). Tolerated transitional edges are listed in `scripts/crate-graph.toml` with a removal condition, review trigger, last-reviewed date, and review interval; an expired review fails the check. A green check means "no untracked violation", not "migration complete".

## 3. Crate map

The workspace has the crates below (verify with `ls crates/` and `scripts/crate-graph.toml`). Sibling crates are added only for a demonstrated dependency, lifecycle, storage, or platform boundary; do not pre-create empty `-core`/`-ui` crates.

### Application

| Crate | Role |
|---|---|
| `labonair` (`crates/app`) | Binary and process entry point. |
| `labonair-shell` | Permanent chrome, composition wiring, native window shell. |

### Foundation

| Crate | Responsibility |
|---|---|
| `labonair-gpui-ext` | GPUI helpers and small shared primitives. |
| `labonair-theme-tokens` | Design tokens (`Theme`, `RadiusScale`, `ThemeMetrics`, `ActiveTheme`, colour structs, `IconThemeContent`) and the `UiTheme` accessor contract. Leaf below `ui-kit`; `labonair-theme` depends on it, not the reverse. |
| `labonair-ui-kit` | Reusable components: buttons, inputs, lists, dropdowns, dialogs, icons, badges, disclosure, sliders, tabs (classic and floating), empty states, motion engine and exit-state helper, generic pane bar/frame, grouped Settings widgets. See [`design-system.md`](design-system.md). |
| `labonair-filesystem` | Local filesystem abstractions and watchers. |
| `labonair-errors` | Structured, UI-free domain error contract and recovery metadata. |
| `labonair-events` | UI-free in-process transport primitives for adapter-level events. Owns no product event vocabulary or state; SSH, MCP, and Transfers translate raw events into their own typed contracts. No feature semantics or second global event registry may be added. |
| `labonair-secrets` | Keychain and secret references. |
| `labonair-persistence` | Shared SQLite connection and schema lifecycle; feature modules own their stores and queries. |
| `labonair-panel` | UI-free panel, dock, and status-item contracts for workspace-owned surfaces. |
| `labonair-mcp-core` | UI-free MCP session/grant, tab-operation, and bridge-preference contracts shared by the agent-access host and `labonair-mcp-server`. |
| `labonair-terminal-integration` | UI-free OSC 7/133 shell-integration payloads shared by local PTY and remote SSH adapters. |
| `labonair-activity` | UI-free registry of running work shown in the statusbar. |
| `labonair-*-host` (`background-host`, `explorer-host`, `hosts-host`, `snippets-host`) | Narrow injected contracts so a consumer does not depend on the providing capability (e.g. `RemoteExplorerService`, `SnippetExecutionHost`, `BackgroundHost`, `HostView`). |

### Capability modules

| Module | Canonical crate | Siblings | Ownership boundary |
|---|---|---|---|
| Settings | `labonair-settings` | `-settings-content`, `-settings-json`, `-settings-macros`, `-settings-ui` | Typed values, layered persistence, the Settings surface registry contract, value-only settings UI. |
| Keymap | `labonair-keymap` | `-keymap-ui` | Binding descriptors, file data, resolution, conflicts, presentation adapter; no feature behavior. |
| Command palette | `labonair-command-palette-core` | `-command-palette`, `-command-palette-runtime` | UI-free command registry; UI sibling owns search/navigation/submenu presentation; runtime sibling carries owner GPUI handlers. |
| Notifications | `labonair-notifications-core` | `-notifications` | Notification registry/state, grouped statusbar dropdown, and opt-in toast presentation ([ADR 0011](adr/0011-actionable-notification-presentations.md)). |
| Activity | `labonair-activity` | `-activity-ui` | Running-work registry and statusbar activity item. |
| Themes | `labonair-theme` | `-theme-ui` | Built-in color and icon-theme registries, preview, selection. `-theme-ui` owns settings→`ThemeStore` policy, Settings-link registration, and the palette preview/activate handler, so `labonair-theme` has no `labonair-settings` dependency. |
| Workspace | `labonair-workspace` | `-workspace-storage` | Identity, durable Space/Tab/Pane IDs, leases, revisioned shared structure, layout, session orchestration. The storage sibling is GPUI-free and owns `workspace.db` transactions and migrations. |
| Backgrounds | `labonair-background` | `-background-host` | Background-image settings, catalog, persistence, decoding, rendering, and its own settings editor. |
| Terminal | `labonair-terminal` | `-terminal-core` (GPUI-free engine/PTY), `-terminal-host` (detached session host, daemon `labonair-host`, [ADR 0009](adr/0009-detached-session-host.md)), `-terminal-integration` | PTY sessions, engine, renderer, terminal commands. |
| Editor | `labonair-editor` | `-editor-ui` | Buffers, syntax, editing behavior, commands; the UI sibling holds the GPUI view. |
| SSH | `labonair-ssh` | `-ssh-transport` | UI-free session, PTY, trust, remote-command, tunnel, tester, and SSH-config contracts; the sibling owns russh transport, auth, tunnels, jump hosts. |
| SFTP | `labonair-sftp` | `-sftp-ssh` | Authenticated session handle and remote-browser contracts; sibling owns concrete session setup and service adapters. |
| Hosts | `labonair-hosts` | `-hosts-ui`, `-hosts-host` | Saved hosts, recent hosts, import/export, management UI. |
| Credentials | `labonair-credentials` | — | Credential metadata, secret references, generated SSH key material. |
| Transfers | `labonair-transfers` | `-transfers-ui`, `-transfers-ssh` | UI-free job values, worker contracts, event translation, lifecycle registry; UI owns queue dropdown and resolution dialogs; sibling owns the concrete SFTP worker. |
| Git | `labonair-git` | `-git-transport`, `-panel-scm`, `-panel-git-graph` | Git contracts and source-control behavior; transport sibling runs local/remote CLI; panels are the views. |
| Explorer | `labonair-panel-explorer` | `-explorer-host` | Local file navigation over filesystem contracts plus injected `RemoteExplorerService` for SSH-backed roots. Does not depend on SSH or SFTP. |
| Snippets | `labonair-snippets` | `-panel-snippets`, `-snippets-host`, `-snippets-ssh` | Storage, execution contracts; panel is the view; sibling runs snippets over SSH. |
| Updates | `labonair-updater` | `-updater-ui` | Manifest, signed-artifact verification, macOS install; dialog/status item in the UI sibling. |
| Onboarding | `labonair-onboarding` | — | Step registry, progress, page view, command ([ADR 0008](adr/0008-onboarding-module.md)). |
| AI | `labonair-ai` | `-mcp-server` | Providers, sessions, context, tools. MCP host contracts live in `-mcp-core`; the concrete HTTP bridge in `-mcp-server`. |

Transport and integration splits are contract-first:

- `labonair-ssh` and `labonair-sftp` own UI-free contracts; `labonair-ssh-transport` and `labonair-sftp-ssh` own concrete implementations and receive SSH state and the raw event transport explicitly. The composition root wires them; no broad backend package owns transport code.
- Explorer consumes remote listings through `RemoteExplorerService`; the shell adapts `labonair-sftp`/`labonair-ssh` services into it.
- Transfers are separate from the SFTP browser contract. The shell and SFTP only connect services and submit typed transfer requests; the statusbar owns the trigger/anchor, not transfer state.
- SSH lifecycle and MCP bridge events follow the same rule: `labonair-ssh`/`labonair-mcp-core` expose typed event sources; shell composition supplies adapters; Workspace owns only the GPUI bridges and feature reaction and never subscribes to a transport adapter directly.

## 4. Composition root

Only `labonair` and `labonair-shell` may know all concrete feature modules. They:

- initialize platform services; construct the GPUI application, window shell, and workspace entities;
- load settings, themes, keymap, persistence, secrets;
- construct integration adapters (SSH, SFTP, Git, transfers, MCP) and inject them into owner-defined contracts;
- call owners' registration functions (panels, commands, status items, notifications, palette providers, Settings surfaces) and connect typed events;
- select the initial workspace and recoverable startup state.

They must not: contain SSH/transfer logic, theme definitions, settings field definitions, or feature-specific rendering; store feature state or duplicate feature persistence; contain a feature-name switch for command, panel, status, or notification behavior; expose a backend facade every feature calls instead of its owner; add a second settings, notification, command, or surface registry; reinterpret an owner's domain events; reach into private module state when a typed contract exists; or serialize a feature's private state into Settings.

Concretely: `shell/src/actions.rs` forwards opaque `PaletteAction`s to `PaletteActionHandlerRegistry` and has no feature-specific match table; the shell installs the keymap snapshot returned by `keymap::adapter::load` into GPUI and wires file watching, nothing more.

For each injected service, the contract (owner or shared foundation crate), adapter (integration or composition boundary), construction site, and failure behavior must be identifiable. If construction needs a new permanent shell surface, write an ADR first and show that no existing surface can host it.

```text
configuration and platform services → typed capability contracts
  → owner registration and feature views → typed events back through the root
```

## 5. Communication, commands, and persistence

Use the smallest mechanism that fits: (1) a direct trait for a synchronous request; (2) a typed event for an asynchronous state change; (3) a registry for discoverable multi-provider contributions; (4) a shared identifier/value type only with stable domain meaning. Stringly typed global events, arbitrary global mutable state, and cross-module entity mutation are prohibited in new code. The event bus is a transport boundary: each module translates external events into its own typed state, and user-visible messages go to the notification center.

Persistence follows the same ownership: the owner writes its own file or store through a narrow boundary. The root may order migrations and provide services only.

Command and keymap plumbing (details in [`registries.md`](registries.md)):

- Dynamic `SubmenuAction` values are wrapped in the UI-free `PaletteAction` and dispatched via `labonair-command-palette-runtime::PaletteActionHandlerRegistry`; owners register narrow matchers capturing only their own entities.
- Persisted keymap action names cross into the runtime through one resolver (`keymap::runtime::command_for_action`). Platform adapters may map the resulting `CommandId` to GPUI actions but must not add a second string alias table; rich GPUI context predicates remain an adapter concern, portable resolution uses typed command contexts.
- Command owners publish default bindings on the command descriptor; the keymap module turns them into the default layer and applies the user keymap as override. No feature-wide default-shortcut table exists.
- The keymap editor keeps raw JSONC source separate from its parsed view so comments, unknown commands, and malformed edits are never lost in a round trip.

## 6. UI ownership

Feature crates own feature-specific views; `labonair-ui-kit` owns reusable visual and interaction primitives. Features use the kit for buttons, dropdowns, lists, menus, inputs, badges, popovers, dialogs, and standard rows, and may compose but not fork its styling. New shared behavior is added to the kit first, with a documented API and a component test. The kit is a component boundary, not a product module: it may provide generic buttons, inputs, lists, menus, dropdowns, dialogs, badges, tabs, disclosure, scrolling, and focus behavior, but knows nothing of hosts, themes, transfers, or other capabilities.

`modal_overlay` and `dialog_surface` are the shared modal scrim and dialog card; they standardize placement and visuals only — focus, dismissal, validation, and decisions stay with the owning feature.

## 7. Workspace shell and pane model

### Chrome

Zones: titlebar, workspace content, docks (left/right/bottom), statusbar (panel toggles left, info items right), and an overlay layer (see [`product.md`](product.md)). Workspace chrome state (dock membership, open/closed, active panel, size, zoom, primary dock edge) is runtime layout state persisted by `labonair-workspace` in `workspace-layout.json`; it is not `SettingsContent` and never project settings. The owner imports legacy `workspace.sidebar*`/`workspace.dockLayout` once and removes them from `config.json`. In Detached mode, shared Space/tab/pane/lease/revision/change-log state lives in `workspace.db` via `labonair-workspace-storage` (the former session JSON is an import source only); see [ADR 0010](adr/0010-shared-workspaces-and-multi-client-detached-sessions.md).

- **Titlebar** is a shell surface, not a feature owner. Its single global-menu button publishes `TitlebarEvent` intent; the composition root routes it to Settings, a palette page, `Workspace::open_keymap_tab` (an occasional-use tab, never a startup tab), or the Settings `Hosts` page (`WorkspaceEvent::OpenHosts`, [ADR 0007](adr/0007-hosts-settings-page.md)). The menu uses the shared `popover_menu`, anchored directly below the button; no second menu or feature behavior.
- **Tab strip** renders in the titlebar by default. With `tabsLocation == "sidebar"` it renders in the Tabs dock panel, owned and registered by `labonair-workspace` (a thin wrapper; the tab list and behavior stay with the tab owner); `Workspace` reconciles dock membership with the setting and the titlebar omits the strip. Same strip, existing chrome, not a new surface.
- **Workspace context.** `labonair-workspace::context` owns the UI-free distinction of identity (`Standalone`/`Project`) and activity (`Empty`/`Active`). Cross-surface requests the workspace can satisfy itself (opening the Keymap tab) are plain `Workspace` methods; requests needing a platform surface it cannot present (native project picker, Settings window) are typed `WorkspaceEvent`s for the shell. Identity changes use the single `WorkspaceTransition` (`OpenProject` / `ReturnToStandalone`); a terminal cwd event is never a transition. Details in [`workspace-model.md`](workspace-model.md).

### Pane model ([ADR 0005](adr/0005-universal-panes.md))

A workspace tab is a pane group: a `WorkspaceLayout` whose leaves are `PaneId`s of any `PaneKind` (Terminal, Editor, Preview, Sftp, GitGraph, Diff, CommitDiff, AiDiff, Keymap, Onboarding; one-to-one with `TabKind`). The model is pure data, split into four GPUI-free modules:

- `pane_kind` — `PaneKind` (icon, label, singleton/split policy), `PaneMeta`, `PaneMetaStore`. Singleton kinds (git graph, diffs, keymap, onboarding) cannot split but can move; opening one again focuses the existing pane (`find_singleton_tab`).
- `pane_ops` — strict split, close (survivors keep ratios), move to a `DropZone`, swap, detach, `insert_subtree`, `normalize` (splice same-axis nesting, promote one-member axes, repair flexes, flex floor), `validate`. Inserting beside a same-axis sibling equalises that axis; `normalize` alone never discards a manual resize.
- `tiling` — pixel geometry: `solve` partitions a container into pane rectangles and gap strips (resize handles sit in the gap); `smart_split` splits along the target's longer side (tie: stacked) and refuses with `SplitRejection::NoRoom` when any pane would fall below 240x120 px (an already-too-small, non-shrinking pane never blocks); `neighbor`/`cycle_focus`; `drop_zone` (outer 25 % edge bands vs. centre); `AxisGeometry::boundary_fraction` for resize.
- `pane_group_naming` — tab label (`"<active> +N"` or custom title), kind icon vs. generic panes icon, hover-list data.

**Live integration.** `Workspace` keeps one `WorkspaceLayout` per tab (a non-terminal tab is a single pane whose id is the tab id) and a `PaneMetaStore`; tab, pane, and axis ids come from one counter (`pane::next_id`). Per-pane resources (views, sessions, editor bindings, dirty state) are keyed by pane id, so moving a pane moves its resources with it. Modules:

- `pane_items/{terminal,editor,preview,sftp,singleton}` — one adapter per kind (`pane_spec`, `pane_body`, `focus_pane`, `spawn_pane_clone`, teardown). Clone semantics: terminal = new shell in the same cwd (SSH pane = new connection to the same host; no channel multiplexing, so password hosts ask again; failures go to the notification center, never a silent local fallback); editor = a second *view group* of the same editor-owned document (`EditorView::add_group`: shared buffer, history, dirty state; independent selection, scroll, folds; the group that is the editor's focused one hosts the interactive text area, others a read-only mirror until clicked); preview/SFTP = new instance at the same location.
- `pane_view` — renders the tree with flex (siblings share `extent - gaps`, identical to `tiling::solve`), gap-aware resize handles, `pane_frame` + `pane_bar` from two panes on (a single pane stays full-bleed), zoom overlay, `DraggedPane` payload (bar is the drag source; `PaneDragChip` ghost shared with tab drags). Each pane has key context `Pane`.
- `pane_actions` — split (`Right`/`Down` fall back to the other axis when out of room, `Left`/`Up` do not), close (last pane closes the tab; a dirty editor asks only when it is the last view of its document; focus goes to the spatial neighbour), focus by direction/cycle, zoom, move (swap), `sync_tab_kind` (tab kind follows its first pane).
- `pane_menu` — Tab context menu entries Split Right, Split Down (disabled with reason for singletons), Move Pane to New Tab, Close Other Panes (keeps a pane that is the last view of a dirty document, and says so), backed by `pane::SplitRight`, `SplitDown`, `MoveToNewTab`, `CloseOthers` commands registered by the workspace provider.
- `zoom` — runtime-only per-tab zoom. Zoom keeps the tree laid out and replaces the zoomed pane's slot with an empty frame; the pane is painted once as an overlay so siblings never resize. The body is inset by the frame corner's sagitta in the comfortable layout so square content stays inside the rounded frame.
- `tab_hover`, `pane_dnd`, `pane_drop`, `session_io` — below.

**Drag and drop.** `pane_dnd` is pure: zone hit-testing on solved rectangles (`tiling::pane_drop_target`; a gap hits nothing, the pane bar counts as centre), highlight rectangle (`tiling::drop_preview`), guard (`drop_allowed`: same Space only, no tab onto itself, a pinned tab is never merged away) and plans computed on clones so a rejection changes nothing:

| Payload | Target | Result |
|---|---|---|
| pane | pane zone, same tab, edge | `move_pane` semantics: re-inserted next to target, `NoRoom` checked |
| pane | pane zone or bar, same tab, centre | the two panes swap |
| pane | tab block of another tab | detached (source tab closes only if it had one pane), smart split of that tab's focused pane |
| pane | empty strip / sidebar area | detached into a new tab at the end of its Space (`plan_detach`); a lone pane is a no-op |
| tab | pane zone, edge | whole pane group merged as nested axis (`insert_subtree`), source tab removed |
| tab | pane zone or bar, centre | same, along the target's longer axis, other axis as fallback |
| tab | tab block | reorder (`TabStore::reorder`; pinned and Space guards unchanged) |

Feedback: every hover dry-runs the plan on throwaway ids (`DropHover::verdict`, `tab_join`), repeating only when pane or zone changes: a fitting drop highlights in the primary colour, one below the 240x120 px minimum in the warning colour with "No room", a no-op gets no highlight. Resting a drag 500 ms on another tab block of the same Space opens it (`tab_drag_spring`; the active tab, the drag's own tab, and other Spaces never spring). Hover-list rows are `DraggedPane` sources.

`pane_drop` owns hover state (`PaneDnd`), registers typed `on_drag_move`/`on_drop` handlers (`DraggedPane` and `DraggedTab` on the pane area, `DraggedPane` on tab blocks and the strip), paints the zone overlay, and applies a plan: layouts of both tabs, `SshTab.tab_id` of moved panes, `sync_tab_kind`/`sync_tab_dirty` for both tabs (a lone non-terminal pane also sets title and path), zoom of both cleared, then the target tab is selected and the moved pane focused. An emptied source tab is removed without retiring any pane. A rejected move is reported through the notification center. Handlers carry no occluding hitbox and the overlay is input-transparent, so `ExternalPaths`/`DraggedPaths` drops still reach terminals and editors. Esc during a drag is handled in the capture phase on the workspace root (`cancel_drag_on_escape`; GPUI 0.2.2 only cancels on mouse-up). Tab strips scroll while a drag is near an end (`edge_scroll`, pointer-driven, no timer). A zoomed tab accepts no drops.

**Hover list (`tab_hover`).** `HoverIntent` is a generation counter: each enter/leave bumps it; a delayed open (350 ms, 80 ms when another list is open) or close (220 ms grace) acts only if its generation is current. One shared `PresenceSlot` renders a `popover_with_exit` of `ListItem` rows (`pane_group_naming::pane_list`); a click runs `activate_pane`. `hover_list_allowed` suppresses it while a context menu, popover, rename, dialog, or drag is active. Tabs with two or more panes show it instead of the tooltip.

**Session v3.** `session_v3` defines the snapshot: a tab is `{title, tree}`, a leaf is `{kind, state}` with state as opaque JSON decoded per kind (`PaneState`: terminal, editor, preview, SFTP; transient kinds are not persisted). `session::load_snapshot` reads v3, v2, and the pre-Spaces flat file through `parse_any` (v2 terminal layouts become terminal groups) and `save_snapshot` writes v3; an unreadable file is left untouched. `restore_tree` mints fresh pane ids, drops unknown/invalid leaves and anything the caller's `keep` check rejects (missing file or host), and collapses the tree around them. Editor panes of one file in one tab restore as one document with several view groups; every editor pane carries its editor-owned recovery payload so a dirty background editor survives window close. SSH panes reconnect eagerly, one connection per pane behind its own loading screen. User file contents are never discarded by layout recovery.

## 8. Runtime lifecycle

Initialization is explicit and ordered:

1. platform/runtime services;
2. persistence, secrets, settings, keymap stores;
3. theme registry and active theme;
4. notification center and transfer service;
5. command, panel, status-item, and theme registrations;
6. workspace and shell;
7. optional feature workers.

I/O is asynchronous. The GPUI foreground thread only performs bounded state updates and rendering; no startup or render path does blocking filesystem, network, process, or database work (use async tasks or `spawn_blocking`, return descriptive errors for predictable failures).

A feature is integrated only when traceable through these states:

| State | Owner | Required evidence |
|---|---|---|
| `construct` | Composition root | Concrete service and owner contract wired without a duplicate owner. |
| `register` | Owning module | Commands, panels, status items, notifications, palette providers use canonical registries. |
| `load` | Owner plus injected services | Settings, persistence, or remote state loads asynchronously; failure is typed. |
| `render` | Owning UI crate | View uses `labonair-ui-kit`, preserves shell zones, has native visual evidence when UI changes. |
| `interact` | Owning module | User actions update owner state and emit typed events. |
| `persist` | Owning module | Values and lifecycle state written through the capability's persistence boundary. |
| `shutdown` | Root and service owner | Tasks, sessions, watchers, windows release resources without blocking the GPUI thread. |

For a lifecycle change, record: owner and crate; construction site and injected contract; entry point and affected shell zones; state transitions, persistence, notifications, events; failure, retry, cancellation, shutdown behavior; and source, test, or native visual evidence for each claim. A successful build proves only the paths it covers, not visual layout, ownership, or remote runtime behavior.

## 9. Repository layout

```text
Labonair-rust/
├── crates/                 # one folder per crate (see §3)
│   ├── app/                # binary and process entry point
│   ├── shell/              # permanent chrome and composition wiring
│   ├── <capability>/       # canonical capability crate
│   ├── <capability>-ui/    # optional sibling UI boundary
│   ├── <capability>-host/  # optional injected consumer contract
│   ├── <capability>-ssh/   # optional transport/integration boundary
│   └── ...                 # foundation crates
├── docs/                   # normative contracts (this tree)
│   ├── adr/                # accepted decisions
│   ├── archive/            # superseded docs, old roadmap and audits (historical only)
│   ├── ideas/              # non-normative proposals; binding only once accepted into docs/ or an ADR
│   └── tasks/              # open task queue and archived task records
├── reference/              # gitignored frozen predecessor copy; read-only, never a dependency
├── scripts/                # dependency guard, crate-graph.toml, packaging, capture tooling
├── packaging/  vendor/
└── target/                 # build output; never an architectural dependency
```

Placement rules:

- Every product capability has one canonical crate; siblings only for a real boundary (UI vs. UI-free logic, independent persistence, platform integration, or a stable contract consumed by several modules). No empty `core`/`api`/`facade` crates.
- Stable domain values, traits, and typed events live in the owner's public contract or a clearly named foundation contract. Feature behavior belongs to the owner, never to `shell`, `workspace`, or a backend merely because those can reach it. Reusable controls go to `ui-kit`. Platform and storage implementations are injected at the composition root. Compatibility adapters sit next to the boundary they bridge with a documented removal condition.
- A small capability may be a single `lib.rs`; a larger one may use optional files `domain.rs`, `state.rs`, `contract.rs`, `registry.rs` (only with multiple providers/consumers), `commands.rs`, `view.rs`, `storage.rs`, `integration.rs`. These are not a requirement to create files without a responsibility.
- The frozen predecessor is consulted for migration evidence only; it is not imported or treated as architecture authority.
- When an ownership or dependency rule changes, update the relevant doc, [`modules.md`](modules.md), and `scripts/crate-graph.toml` in the same change.

## 10. Adding and migrating a capability

1. Record the user flow, disposition (`keep`, `redesign`, `defer`, `remove`), owner, canonical entry point, and canonical crate in the capability matrix ([`modules.md`](modules.md)).
2. Define the owner's domain state, public typed contract, events, persistence, commands/keybindings, notifications, and UI-kit components. Choose a direct trait, typed event, or registry by number of providers/consumers; no registry by default.
3. Implement state, behavior, UI, and tests inside the owner; register through typed registration APIs, never a second central table.
4. Wire concrete implementations only in the composition root; consumers use public contracts or immutable snapshots.
5. In a migration, move the contract first, then adapters and consumers. Mark each compatibility path with a removal condition, update the matrix and `crate-graph.toml`, and remove the old path before calling the migration complete.

No new abstraction, permanent surface, or sibling crate without a current consumer and documented boundary. A capability with no current workflow is removed or parked outside the permanent surface.

## 11. Verification

Architecture changes require `cargo fmt --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/check-crate-deps.sh` (all run by `scripts/check.sh`), and a visual check for any UI or layout change ([`testing.md`](testing.md)).
