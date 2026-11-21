# Labonair Architecture

**Status:** Normative target architecture
**Version:** 6
**Related:** [`product.md`](product.md), [`capabilities.md`](modules.md), [`modules.md`](modules.md), [`registries.md`](registries.md), [`repository-layout.md`](architecture.md), [`feature-lifecycle.md`](architecture.md), [`architecture/layers.md`](architecture.md), [`architecture/composition-root.md`](architecture.md), [`architecture/runtime-lifecycle.md`](architecture.md), [`architecture/dependency-rules.md`](architecture.md)

## 1. Architecture objective

Labonair is a single native Rust application built with GPUI. It is composed from independently owned feature modules. The application composition layer connects modules, but it does not contain their business logic.

The architecture optimizes for three properties:

- **Isolation:** a module owns its state, behavior, UI, persistence, and tests.
- **Consistency:** shared interaction and visual behavior comes from foundation crates.
- **Replaceability:** a module can be rewritten or removed without rewriting unrelated modules.

## 2. Terms

- **Capability:** a user-facing ability such as SSH, SFTP, terminal, themes, or transfers.
- **Module:** the complete ownership boundary of one capability.
- **Capability crate:** the canonical Rust crate for one product capability. It
  is the default starting point for the module and must not contain unrelated
  product capabilities.
- **Sibling crate:** an additional crate inside the same owning module, used
  only for a real core, UI, storage, or integration boundary.
- **Composition root:** the application startup code that constructs services and invokes registration functions.
- **Registry:** an extensible collection of metadata and providers owned by a foundation or host contract.
- **Workspace:** a runtime context containing tabs, panes, tools, and optional project/remote identity.

Every product capability has exactly one owning module and one canonical
capability crate. A module may contain sibling crates, but splitting a module
does not split ownership. Foundation crates are not product capabilities and
must have one explicit, reusable foundation responsibility.

## 3. Layer model

```text
Application composition
  labonair / labonair-shell
        ↓
Workspace orchestration
  workspace, tabs, panes, docks, sessions
        ↓
Feature modules
  terminal, editor, ssh, sftp, hosts, transfers, git, themes,
  keymap, notifications, command-palette, settings, ai
        ↓
Foundation and platform services
  ui-kit, theme-tokens, gpui-ext, filesystem, secrets, persistence
```

Dependencies point downward. A feature may depend on a foundation contract, but a feature must not depend on the shell. Cross-feature behavior uses a typed contract, registry, or event; it does not reach into another feature's private state.

`labonair-command-palette-core` is a contract-level registry crate even though
it belongs to the Command Palette module. Capability crates may depend on this
UI-free contract to contribute command metadata; they must not depend on the
palette UI crate. `labonair-keymap` may depend on the command contract to
publish keymap-owned commands, while the command contract depends only on the
identity foundation. This one-way direction keeps the graph acyclic.

`labonair-command-palette-runtime` is the GPUI-facing execution bridge. Owner
crates contribute handlers that capture their own entities and services; the
shell supplies only the active window/application context. It must not become
a product facade or contain a central feature dispatch table.

Dynamic `SubmenuAction` values are wrapped in the UI-free `PaletteAction`
contract and dispatched through
`labonair-command-palette-runtime::PaletteActionHandlerRegistry`. Capability
owners register narrow matchers that capture only their own entities/services;
`shell/src/actions.rs` forwards opaque actions and contains no feature-specific
submenu match table.

Persisted keymap action names cross into the runtime through one canonical
resolver (`keymap::runtime::command_for_action`). Platform adapters may map the
resulting `CommandId` to GPUI actions, but they must not introduce a second
string alias table. Rich GPUI context predicates remain an adapter concern;
portable keymap resolution uses typed command contexts.

Command owners also publish their default bindings on the command descriptor.
The keymap module converts those typed defaults into the default resolution
layer, then applies the user keymap as the override layer. The composition root
registers providers but does not maintain a feature-wide default-shortcut table.
The module-owned `keymap::adapter::load` boundary returns the resulting
immutable snapshot, including diagnostics; the shell only installs it into
GPUI and connects file-watch events.
The editor contract keeps raw JSONC source separate from its parsed view, so
comments, unknown commands, and malformed edits remain available for recovery
and are never lost through a deserialize/serialize round trip.

## 4. Target workspace crate map

### Foundation

| Crate | Responsibility |
|---|---|
| `labonair-gpui-ext` | GPUI helpers and small shared primitives. |
| `labonair-theme-tokens` | Design tokens (`Theme`, `RadiusScale`, `ThemeMetrics`, `ActiveTheme`, colour structs, `IconThemeContent`) and the `UiTheme` accessor contract. A leaf below `ui-kit`; the Themes *feature* crate (`labonair-theme`) depends on it, not the reverse. |
| `labonair-ui-kit` | Buttons, inputs, lists, dropdowns, dialogs, icons, badges, disclosure, sliders, tabs (classic and floating), empty states, the motion engine and exit-state helper, the generic pane bar/frame, the grouped Settings widgets (group, row, preview tiles, option cards, category chip), and other reusable components. Renders against `labonair-theme-tokens`; it does not depend on the Themes feature crate. |
| `labonair-filesystem` | Local filesystem abstractions and watchers. |
| `labonair-errors` | Structured, UI-free domain error contract and recovery metadata. |
| `labonair-events` | UI-free in-process transport primitives for adapter-level events; it owns no product event vocabulary or application state. |
| `labonair-secrets` | Keychain and secret references. |
| `labonair-persistence` | Cloneable shared SQLite connection and schema lifecycle; feature modules own stores and queries. |
| `labonair-panel` | UI-free panel, dock, and status-item contracts used by workspace-owned surfaces. |
| `labonair-mcp-core` | UI-free MCP session/grant, tab-operation, and persisted bridge-preference contracts shared by the agent-access host and the injected `labonair-mcp-server` adapter. |
| `labonair-terminal-integration` | UI-free OSC 7/133 shell-integration payloads shared by local PTY and remote SSH adapters. |

### Capability modules and crates

The names below are the current canonical crates. They are not a request to
pre-create empty `-core` or `-ui` crates. A sibling crate is added only when
the module has a demonstrated dependency, lifecycle, storage, or platform
boundary.

| Module | Canonical capability crate | Existing sibling crates | Ownership boundary |
|---|---|---|---|
| Settings | `labonair-settings` | `labonair-settings-content`, `labonair-settings-json`, `labonair-settings-macros`, `labonair-settings-ui` | Typed values, layered persistence, Settings surface registry contract, and value-only settings UI. |
| Keymap | `labonair-keymap` | `labonair-keymap-ui` | Binding descriptors, file data, resolution, conflicts, and a presentation adapter; no feature behavior. The UI registers its Settings link through the public Settings surface contract. |
| Command palette | `labonair-command-palette-core` | `labonair-command-palette`, `labonair-command-palette-runtime` | UI-free command registry contract; the UI sibling owns search/navigation and submenu presentation, while the runtime sibling carries owner-provided GPUI handlers. |
| Notifications | `labonair-notifications-core` | `labonair-notifications` | Notification registry/state and its GPUI statusbar presentation. |
| Themes | `labonair-theme` | `labonair-theme-ui` | Built-in color and icon-theme registries, preview, and selection. `labonair-theme-ui` owns the settings→`ThemeStore` application policy, Settings-link registration, and the palette preview/activate handler so `labonair-theme` keeps no `labonair-settings` dependency. |
| Workspace | `labonair-workspace` | `labonair-workspace-storage`; panel crates are separate capabilities | Workspace identity, durable Space/Tab/Pane IDs, leases, revisioned shared structure, layout, and session orchestration. The storage sibling is GPUI-free and owns `workspace.db` transactions and migrations. |
| Backgrounds | `labonair-background` | owner UI is in `labonair-background` | Background-image settings, file catalog, persistence, decoding, and rendering; the owner UI depends on `labonair-ui-kit` for shared sliders, buttons, text fields, and segmented controls. Appearance links to this surface without duplicating its state. |

### Product modules

| Module | Canonical capability crate | Responsibility |
|---|---|---|
| Terminal | `labonair-terminal` | PTY sessions, terminal engine, renderer, and terminal commands. Sibling `labonair-terminal-integration` owns shared shell protocol payloads. |
| Editor | `labonair-editor` | Buffers, syntax, editing behavior, and editor commands. |
| SSH | `labonair-ssh` | SSH contracts plus the `labonair-ssh-transport` integration sibling for russh transport, authentication, tunnels, and jump-host execution. |
| SFTP | `labonair-sftp` | Remote filesystem browsing and SFTP contracts; the `labonair-sftp-ssh` integration sibling owns concrete session setup and adapters. |
| Hosts | `labonair-hosts` | Saved host definitions, recent hosts, import/export, and host management. Transport adapters remain capability-owned. |
| Credentials | `labonair-credentials` | Credential metadata, secret references, and generated SSH key material. |
| Transfers | `labonair-transfers` | Transfer queue, progress, cancellation, conflict resolution, and lifecycle history. The `labonair-transfers-ssh` integration sibling owns concrete SFTP execution. |
| Git | `labonair-git` | Git contracts and source-control behavior; `labonair-git-transport` owns local/remote CLI execution and contract adapters, while sibling panel crates provide Git views. |
| Explorer | `labonair-panel-explorer` | Local file navigation UI over filesystem contracts and the injected `labonair-explorer-host::RemoteExplorerService` for SSH-backed roots. The panel does not depend directly on SSH or SFTP. |
| Snippets | `labonair-snippets` | Snippet storage, execution contracts, and snippet behavior; sibling panel crate provides the view. |
| AI | `labonair-ai` | Providers, sessions, context, tools, and future UI. MCP host contracts live in `labonair-mcp-core`; the concrete HTTP bridge lives in `labonair-mcp-server`. |

The first transport split is intentionally contract-first:
`labonair-ssh` owns UI-free SSH session, PTY, trust, remote-command, tunnel,
tester, and SSH-config contracts. `labonair-sftp` owns the authenticated SFTP
session handle and remote-browser contracts. `labonair-ssh-transport` owns the
concrete russh implementation and its contract adapters; workspace and feature
views consume injected traits. No broad backend package owns transport code;
the application composition root wires the integration sibling explicitly.
The `labonair-sftp-ssh` integration sibling owns concrete SFTP session setup
and the `labonair-sftp` service adapters; it receives SSH state and the raw
event transport explicitly.
Transfers remain a separate capability and are not part of the SFTP browser
contract.

The Explorer panel consumes remote directory listings through the
`labonair-explorer-host::RemoteExplorerService` contract. The shell composition
root adapts `labonair-sftp` and `labonair-ssh` services into that contract;
`labonair-panel-explorer` must not import either transport capability directly.

`labonair-events` is a foundation transport only. It replaces the former
backend-local event primitive, while SSH, MCP, and Transfers translate raw
adapter events into their own typed contracts at the boundary. Product code
must not add feature semantics or a second global event registry to this
crate.

The transfer capability is now split into `labonair-transfers` and
`labonair-transfers-ui`. The former owns the UI-free job values, typed worker
contracts, event translation boundary, and retained lifecycle registry. The
latter owns the queue dropdown and resolution dialogs. The
`labonair-transfers-ssh` integration sibling owns the concrete SFTP worker,
including execution, checksums, conflicts, cancellation, and reconnect
requeue behavior. The shell composition root and SFTP capability only connect
services and submit typed transfer requests. The statusbar owns the
trigger/anchor, but not transfer state.

SSH connection lifecycle and MCP bridge events follow the same contract-first
rule. `labonair-ssh` and `labonair-mcp-core` expose typed event sources and
receivers; shell composition supplies the named integration adapters, and
Workspace owns only the GPUI bridges and feature reaction. No Workspace code
subscribes to a transport adapter directly; the shared raw transport is owned
by `labonair-events` and remains hidden behind the composition boundary.

Capability UI modules register their Settings links through the typed
`labonair-settings::SettingsSurfaceRegistry` contract. The composition root
supplies the window/workspace callbacks; it does not own feature row metadata.

The current repository does not yet match every ownership boundary in this
map. The migration is tracked in [`rework-roadmap.md`](archive/rework-roadmap.md), and
the observed state is recorded in [`audits/architecture-inventory.md`](archive/audits/architecture-inventory.md).

Workspace chrome state is persisted by `labonair-workspace` in the dedicated
`workspace-layout.json` file. Dock membership, open/closed state, active panel,
size, zoom, and the primary dock edge are runtime layout state; they are not
`SettingsContent` values and must not be project settings. The workspace owner
imports legacy `workspace.sidebar*` and `workspace.dockLayout` values once,
then removes those keys from `config.json`. In Detached mode, the Workspace
owner stores shared Space, tab, pane, lease, revision, and change-log state in
`workspace.db` through `labonair-workspace-storage`; the former session JSON is
an import source rather than a concurrent writer. See [ADR 0010](adr/0010-shared-workspaces-and-multi-client-detached-sessions.md).

**Pane model (R10-002, ADR 0005).** A workspace
tab is a pane group: a `WorkspaceLayout` whose leaves are `PaneId`s of any
`PaneKind` (`pane_kind.rs`, one-to-one with `TabKind`). The model is pure data,
split into four modules that carry no GPUI types:

- `pane_kind` — `PaneKind` (icon, label, singleton and split policy),
  `PaneMeta`, and the `PaneMetaStore` that replaces the per-tab-id maps.
  Singleton kinds (git graph, diffs, keymap, hosts) cannot split but can move;
  opening one again focuses the existing pane.
- `pane_ops` — tree operations added next to the terminal-only API in
  `pane_group.rs`: strict split, close (survivors keep their ratios), move to a
  `DropZone`, swap, detach to its own tree, `insert_subtree`, `normalize`
  (splice same-axis nesting, promote one-member axes, repair flexes, flex
  floor) and `validate`. Inserting beside a same-axis sibling equalises that
  axis; `normalize` alone never discards a manual resize.
- `tiling` — pixel geometry over the tree. `solve` partitions a container into
  pane rectangles and gap strips (resize handles sit inside the gap).
  `smart_split` splits along the target's longer side (tie: stacked) and
  refuses with `SplitRejection::NoRoom` when any pane would drop below
  240x120 px (a pane already too small and not shrinking never blocks).
  `neighbor` / `cycle_focus` drive focus moves, `drop_zone` classifies the
  outer 25 % edge bands vs. centre, and `AxisGeometry::boundary_fraction`
  turns a pointer position into the fraction `set_boundary` expects, correct
  with gaps and pixel minimums.
- `pane_group_naming` — tab label (`"<active> +N"` or the custom title), shared
  kind icon vs. a generic panes icon, and the hover-list data model.

**Pane integration (live).** `Workspace` keeps one `WorkspaceLayout` per tab
(a non-terminal tab is a single pane whose id is the tab id) and a
`PaneMetaStore`; tab, pane and axis ids come from one process-wide counter
(`pane::next_id`). The former per-tab maps (`panes`, `editors`, `previews`,
`sftp_views`, `sftp_sessions`, `remote_edits`, ...) are keyed by pane id, so
moving a pane between tabs moves its resources with it. New modules:

- `pane_items/{terminal,editor,preview,sftp,singleton}` - one adapter per kind:
  `pane_spec` (title, icon, dirty, bar slot), `pane_body`, `focus_pane`,
  `spawn_pane_clone` (`split_clone`), teardown. Terminal clone: new shell in the
  same cwd; an SSH pane opens a new connection to the same host (the SSH
  service has no channel multiplexing, so password hosts ask again; failures
  are reported through the notification center, never a silent local fallback).
  Editor clone: a second *view group* of the same editor-owned document
  (`EditorView::add_group`): shared buffer, history and dirty state, independent
  selection, scroll and folds. The pane whose group is the editor's focused one
  hosts the interactive text area, the others a read-only mirror
  (`EditorView::mirror_element`, no syntax colours) until clicked; focusing a
  mirror swaps the roles. Preview / SFTP clone: a new instance at the same
  location. Singletons are not splittable and reopening focuses the existing
  pane (`find_singleton_tab`).
- `pane_view` - renders the tree with flex (siblings share `extent - gaps` by
  their flexes, identical to `tiling::solve`), gap-aware resize handles whose
  drag uses `AxisGeometry::boundary_fraction`, `pane_frame` + `pane_bar` from
  two panes on (a single pane stays full-bleed, no bar),
  the zoom overlay, and the `DraggedPane` payload (the bar is the drag source
  with a mini-chip ghost, `PaneDragChip`, shared with tab drags). Each pane is
  wrapped in the key context `Pane`.
- `pane_dnd` / `pane_drop` / `pane_menu` - tab and pane drag and drop (below)
  and the pane entries of the tab context menu.
- `tab_hover` - the hover list of a pane-group tab (below).
- `pane_actions` - split (`Right`/`Down` fall back to the other axis when the
  requested one has no room, `Left`/`Up` do not), close (last pane closes the
  tab; a dirty editor asks only when it is the last view of its document;
  focus goes to the spatial neighbour), focus by direction / cycle, zoom, move
  (swap), `sync_tab_kind` (a tab's kind follows its first pane).
- `zoom` - runtime-only per-tab zoom state.

**Drag and drop (`pane_dnd`, `pane_drop`).** `pane_dnd` is pure: zone
hit-testing on the solved pane rectangles (`tiling::pane_drop_target`; a point
in a gap hits nothing, the pane bar always counts as the centre zone), the
highlight rectangle (`tiling::drop_preview`, the half the dropped pane will
occupy), the guard (`drop_allowed`: same Space only, no tab onto itself, a
pinned tab is never merged away) and the plans, each computed on clones so a
rejection changes nothing:

| Payload | Target | Result |
|---|---|---|
| pane | pane zone, same tab, edge | `move_pane` semantics: re-inserted next to the target, `NoRoom` checked |
| pane | pane zone or bar, same tab, centre | the two panes swap |
| pane | tab block of another tab | detached (source tab closes only if it had one pane), smart split of that tab's focused pane |
| pane | empty strip / sidebar area | detached into a new tab at the end of its Space (`plan_detach`); a lone pane is a no-op |
| tab | pane zone, edge | whole pane group merged as a nested axis (`insert_subtree`), source tab removed |
| tab | pane zone or bar, centre | same, along the target's longer axis, other axis as fallback |
| tab | tab block | reorder (`TabStore::reorder`, pinned and Space guards unchanged) |

Feedback while dragging. Every hover dry-runs the plan on the layouts
(`DropHover::verdict`, `tab_join`): a fitting drop is highlighted in the primary
colour, one that would fall below the 240 x 120 px minimum in the warning colour
with "No room", and a drop that would do nothing gets no highlight. The dry run
repeats only when the pane or zone changes and uses throwaway pane ids. Resting a
drag on another tab block of the same Space for 500 ms opens that tab
(`tab_drag_spring`, `spring_candidate`), so the dragged tab or pane can be
dropped into its panes; the active tab, the drag's own tab and other Spaces never
spring. The rows of the hover list are `DraggedPane` sources (the list closes
once the drag is active).

`pane_drop` owns the hover state (`PaneDnd`), registers the typed handlers
(`on_drag_move` / `on_drop` for `DraggedPane` and `DraggedTab` on the pane
area, `DraggedPane` on tab blocks and the strip), paints the zone overlay and
applies a plan: layouts of both tabs, `SshTab.tab_id` of the moved panes,
`sync_tab_kind` / `sync_tab_dirty` for both tabs (a lone non-terminal pane also
sets the tab title and path), zoom of both tabs cleared, then the target tab is
selected and the moved pane focused. Resources keyed by pane id (views,
sessions, editor bindings, dirty state) need no move; an emptied source tab is
removed without retiring any pane. A rejected move (no room for the 240 x 120
px minimum) is reported through the notification center. Handlers are typed,
carry no occluding hitbox and the overlay is input-transparent, so
`ExternalPaths` / `DraggedPaths` drops still reach terminals and editors. Esc
during a drag is handled in the capture phase on the workspace root
(`cancel_drag_on_escape`: `stop_active_drag` and state reset); GPUI 0.2.2
itself only cancels on mouse-up. Tab strips scroll while a drag is near an end
(`edge_scroll`, driven by pointer moves, not a timer). While a tab is zoomed
its pane area accepts no drops (the zoomed pane covers the zones).

**Hover list (`tab_hover`).** `HoverIntent` is a generation counter: each
enter/leave bumps it and a delayed open (350 ms, 80 ms when another list is
already open) or close (220 ms grace so the pointer can reach the list) acts
only if its generation is still current. The one shared `PresenceSlot` renders
a `popover_with_exit` of `ListItem` rows (`pane_group_naming::pane_list`);
a click runs `activate_pane`. It is suppressed through `hover_list_allowed`
while a context menu, popover, rename, dialog or drag is active and closed as
soon as one appears. Tabs with two or more panes show it instead of the
tooltip.

**Tab context menu.** `pane_menu` adds Split Right, Split Down (disabled with
the reason for singleton kinds), Move Pane to New Tab and Close Other Panes.
They call the same methods as the `pane::SplitRight`, `pane::SplitDown`,
`pane::MoveToNewTab` and `pane::CloseOthers` commands (the last two are new
`CommandId`s registered by the workspace provider). Close Other Panes keeps a
pane that is the last view of a dirty document and says so.
- `session_io` - session v3 capture / restore (below).

Zoom keeps the tree laid out and replaces the zoomed pane's slot with an empty
frame; the pane is painted once, as an overlay, so siblings never resize. GPUI
clips to rectangles, so the pane body is inset by the frame corner's sagitta in
the comfortable layout to keep square content inside the rounded frame.

**Session v3.** `session_v3` defines the snapshot: a tab is `{title, tree}` and
a leaf is `{kind, state}` with the state kept as opaque JSON on the wire and
decoded per kind (`PaneState`: terminal, editor, preview, SFTP; transient
kinds are not persisted). `session::load_snapshot` reads v3, v2 and the
pre-Spaces flat file through `parse_any` (v2 terminal layouts become terminal
groups, other kinds single leaves) and `save_snapshot` writes v3; an
unreadable file is left untouched. `restore_tree` mints fresh pane ids, drops
unknown or invalid leaves and anything the caller's `keep` check rejects (a
missing file or host) and collapses the tree around them. Editor panes of one
file inside one tab restore as one document with several view groups, and every
editor pane carries its editor-owned recovery payload, so a dirty editor in a
background tab survives a window close. SSH panes reconnect eagerly on restore,
one connection per pane behind its own loading screen. User file contents are
never discarded by layout recovery.

The titlebar is a shell surface, not a feature owner. Its single global-menu
button publishes `TitlebarEvent` intent. The composition root connects that
intent to Settings, a Command Palette page, or `Workspace::open_keymap_tab` (the keymap surface is a workspace tab, an
occasional-use tab, never a startup tab) or the Settings `Hosts` page
(`WorkspaceEvent::OpenHosts`, ADR 0007).
The menu uses the shared `popover_menu` primitive and anchors it in window
coordinates directly below the clicked button; it must not implement a second
menu or feature-specific behavior.

The workspace tab strip renders in the titlebar by default. When
`tabsLocation == "sidebar"` it renders instead in the Tabs dock panel, which
`labonair-workspace` owns and registers (the tab list and all its behaviour
stay with the tab owner; the panel is a thin wrapper). `Workspace` reconciles
that panel's dock membership with the setting; the titlebar simply omits the
strip. This is not a new product surface — it is the same tab strip in the
existing dock chrome.

`labonair-workspace::context` owns the UI-free distinction between workspace
identity (`Standalone` or `Project`) and activity (`Empty` or `Active`). Tool
tabs do not choose a second layout model for standalone use. Cross-surface
requests the workspace can satisfy itself (opening the Keymap tab)
are plain methods on `Workspace`; only requests for a platform-specific
surface it can't present itself (the native project-folder picker, the
Settings window) are
emitted as a typed `WorkspaceEvent` for the shell to compose. Identity
mutation uses the single typed
`WorkspaceTransition` contract (`OpenProject` or `ReturnToStandalone`); a
terminal current-working-directory event is never a workspace transition. The
pure `WorkspaceContext` applies that transition; GPUI workspace state only
coordinates the resulting settings and invalidation side effects.

## 5. Composition root

Only the `labonair` application package and `labonair-shell` may know all concrete feature modules. Their responsibilities are limited to:

- initialize platform services;
- create the application and workspace entities;
- call feature registration functions;
- connect typed events and callbacks;
- compose the permanent shell.

They must not contain SSH logic, transfer logic, theme definitions, settings field definitions, or feature-specific rendering.

The composition root may construct concrete implementations and pass them to
owners, but it may not reinterpret their domain events or duplicate their
registries. Registration calls belong to the owning module; the root only
assembles registrations and connects typed interfaces.

## 6. Communication rules

Use the smallest communication mechanism that fits:

1. A direct trait for a synchronous capability request.
2. A typed event for an asynchronous state change.
3. A registry for discoverable, multi-provider contributions.
4. A shared identifier/value type only when it has stable domain meaning.

Stringly typed global events, arbitrary global mutable state, and cross-module entity mutation are prohibited for new code.

The application event bus is a transport boundary, not a business-logic layer. Each module translates external events into its own typed state changes. User-visible messages are forwarded to the notification center.

Persistent state follows the same ownership rule as runtime behavior: the
owner writes its own file or store through a narrow persistence boundary. The
composition root may order migrations and provide services, but it must not
serialize a feature's private state into Settings as a shortcut.

## 7. Dependency rules

- Foundation crates do not depend on product modules. `labonair-ui-kit`
  renders against `labonair-theme-tokens` (a foundation leaf), never the
  `labonair-theme` feature crate; the dependency verifier enforces this.
- `labonair-ui-kit` contains no product-specific behavior.
- Feature modules do not depend on `labonair-shell`.
- Feature modules do not import private types from another feature.
- Core crates do not depend on GPUI unless their state genuinely requires it; pure domain logic stays UI-free.
- Persistence and secrets are accessed through narrow service contracts.
- The shell is the only all-feature composition layer.
- Every new dependency edge requires a reason in the module manifest or an ADR.
- A CI allow-list must verify the dependency graph and reject cycles or forbidden edges.
- A feature crate may depend on another module's public contract crate when its
  user flow requires it, but not on that module's private state or feature UI.
- No new capability code may be added to an application-composition crate.
  Concrete integration code belongs in the owning module's named sibling crate
  and is injected through its public capability contracts.

## 8. UI ownership

Feature crates own feature-specific views. `labonair-ui-kit` owns reusable visual and interaction primitives. A feature must use the kit for buttons, dropdowns, lists, menus, inputs, badges, popovers, dialogs, and standard rows.

Feature-specific components may compose kit components, but may not fork their styling locally. New shared behavior is added to the kit first, with a documented API and a component test.

`modal_overlay` and `dialog_surface` are the shared modal scrim and dialog
card frame. They standardize placement and visual treatment only; focus,
dismissal, validation, and decisions stay with the dialog's owning feature.

The UI kit is a component boundary, not a product module. It may provide
generic buttons, inputs, lists, menus, dropdowns, dialogs, badges, tabs,
disclosure, scrolling, and focus behavior, but it must not know about hosts,
themes, transfers, or any other product capability.

## 9. Runtime lifecycle

Initialization is explicit and ordered:

1. platform/runtime services;
2. persistence, secrets, settings, and keymap stores;
3. theme registry and active theme;
4. notification center and transfer service;
5. command, panel, status-item, and theme registrations;
6. workspace and shell;
7. optional feature workers.

I/O is asynchronous. The GPUI foreground thread only performs bounded state updates and rendering. No startup or render path may perform blocking filesystem, network, process, or database work.

## 10. Adding and migrating a capability

Every feature change follows the same boundary-first sequence:

1. Record the user flow, disposition (`keep`, `redesign`, `defer`, or
   `remove`), owner, canonical entry point, and canonical capability crate in
   the capability matrix and roadmap.
2. Define the owning module's domain state, public typed contract, events,
   persistence, commands/keybindings, notifications, and required UI-kit
   components. Choose a direct trait, typed event, or registry based on the
   number of providers and consumers; do not introduce a registry by default.
3. Implement state, behavior, UI, and tests inside the owner. Register
   commands, panels, status items, themes, or notifications through their
   typed registration APIs; do not add a second central table.
4. Wire concrete implementations only in the composition root. Consumers use
   public contracts or immutable snapshots and never mutate another module's
   state directly.
5. During migration, move the contract first, then adapters and consumers.
   Mark each compatibility path with a removal condition, update the
   inventory and dependency verifier, and remove the old path before declaring
   the migration complete.

Do not introduce a new abstraction, permanent surface, or sibling crate
without a current consumer and a documented boundary. If a capability has no
current workflow, remove it or park it outside the permanent product surface.

## 11. Verification

Architecture changes require:

- `cargo fmt --check`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- dependency graph verification
- a visual check for any UI or layout change

The dependency verifier currently permits only the explicit transitional edges
listed in [`audits/architecture-inventory.md`](archive/audits/architecture-inventory.md).
It must be kept strict while those edges are removed. A green dependency check
therefore means "no untracked violation", not that the migration is complete.
Each tolerated edge is also recorded in `architecture/graph.toml` with a
bounded review/removal task, a review trigger, a last-reviewed date, and a
machine-checked review interval. An expired review fails the architecture
scope until the edge is re-evaluated.

## Repository Layout


This document defines where product code, contracts, documentation, and
migration work belong. It is a structural rule, not a description of every
file that currently exists. The current exceptions are recorded in the
[architecture inventory](archive/audits/architecture-inventory.md).

### Top-level layout

```text
Labonair-rust/
├── crates/
│   ├── app/                    # binary and process entry point
│   ├── shell/                  # permanent chrome and composition wiring
│   ├── <capability>/           # canonical product capability crate
│   ├── <capability>-ui/        # optional sibling UI boundary
│   ├── <capability>-storage/   # optional sibling persistence boundary
│   ├── <capability>-integration/ # optional platform/protocol boundary
│   └── foundation crates       # reusable contracts and infrastructure
├── docs/                       # normative contracts, ADRs, audits, reports
│   ├── agents/                 # agent orientation, routes, templates, gates
│   ├── architecture/           # decomposed architecture contracts + graph source
│   ├── product/                # command, surface, and menu catalog sources
│   ├── testing/                # evidence and visual verification contracts
│   ├── evidence/               # structured scorecard source
│   ├── automation/             # MCP tools, grants, limits, test matrix
│   ├── security/               # threat, secret, trust, remote contracts
│   ├── settings/               # settings lifecycle + catalog source
│   ├── performance/            # budgets + measurement catalog
│   ├── release/                # packaging, rollback, support + checklist
│   ├── generated/              # checked-in readable projections; never hand-edit
│   └── capabilities/            # capability-specific contracts and inventories
├── ideas/                      # non-normative proposals only
├── tasks/rework/               # active migration queue
├── tasks/archive/              # historical task records
├── reference/              # frozen predecessor reference; read-only
└── scripts/                    # repository checks and tooling
```

`target/`, generated output, and local runtime data are build or machine
state; they are not product source and must not become architectural
dependencies.

### Crate placement

Every product capability has one canonical crate under `crates/`. The crate
owns the capability even when it is later split into siblings. For example,
`themes` may contain `labonair-theme` and a future
`labonair-<capability>-ui` sibling, but there is still one Themes owner and one
public capability contract.

Create a sibling crate only for a real boundary: UI versus UI-free logic,
independent persistence, platform integration, or a stable contract consumed
by multiple modules. Do not create empty `core`, `api`, or `facade` crates to
make the tree look layered.

Foundation crates may be shared by product modules but may not contain
product-specific state or depend upward on a capability. `labonair-ui-kit`
contains reusable interaction and visual primitives; it does not know about
Hosts, Themes, Transfers, or Workspaces.

`labonair-shell` and `labonair` are the only all-feature composition layers.
They may construct concrete services, invoke registration functions, and
connect typed events. They must not become the home of feature state,
feature-specific rendering, or a second command/panel/status registry.

### Capability crate contents

A small capability may remain a single `src/lib.rs`. A larger capability may
organize private implementation into files such as:

```text
crates/<capability>/src/
├── lib.rs          # public contract and module exports
├── domain.rs       # stable values and identifiers
├── state.rs        # lifecycle and state transitions
├── contract.rs     # traits and typed events for consumers
├── registry.rs     # only when discovery has multiple providers/consumers
├── commands.rs     # command/keymap contributions owned by the capability
├── view.rs         # GPUI view when UI belongs in this crate
├── storage.rs      # capability-owned persistence when needed
└── integration.rs  # platform/protocol adapter when needed
```

These names are optional organizational tools, not a requirement to create
files without a current responsibility. The owner keeps domain state,
behavior, UI, persistence, commands, notifications, and tests together unless
a sibling boundary is justified.

### Cross-module placement

- Stable domain values, traits, and typed events belong in the owning
  capability's public contract or a clearly named foundation contract.
- Feature behavior belongs to the owning capability, never to `shell`,
  `workspace`, or `backend` merely because those crates can reach it.
- Reusable controls belong in `ui-kit`; feature views compose them.
- Platform and storage implementations are injected at the composition root
  through narrow contracts.
- Compatibility adapters stay next to the boundary they bridge and must have
  a documented removal condition.

### Documentation placement

- `docs/*.md`: current normative contracts and implementation roadmap.
- `docs/architecture/`: decomposed architecture contracts and the machine-
  readable dependency/ownership policy.
- `docs/agents/`: agent onboarding, structured request routes, change routing,
  task, handoff, and finish gate guidance.
- `docs/generated/`: deterministic projections generated from canonical sources;
  update the generator or source, never the projection directly.
- `docs/capabilities/`: capability-specific detail that is indexed by the
  capability matrix and must not redefine ownership elsewhere.
- `docs/adr/`: accepted architectural decisions and their consequences.
- `docs/audits/`: evidence about the current, unfinished tree.
- `docs/reports/`: research and comparisons; never the source of authority.
- `docs/archive/`: superseded documents retained for traceability.
- `ideas/`: proposals that are not binding until accepted into `docs/` or an
  ADR.
- `tasks/rework/`: the only active implementation queue for the architecture
  rework.
- `docs/reform/`: the repository goal and traceability contract for the
  documentation/agent knowledge system. It is not a second task queue.

The frozen predecessor source is read-only and is not a product dependency. It
may be consulted for migration evidence, but it must not be imported into the
native application or treated as current architecture authority.

When a document changes an ownership or dependency rule, update the relevant
normative contract, capability matrix, migration task, and inventory in the
same change.

## Architecture Layers

**Related:** [`../architecture.md`](architecture.md), [`graph.toml`](../scripts/crate-graph.toml), [`dependency-rules.md`](architecture.md)

This document defines the allowed direction of the Labonair crate graph. The
machine-readable edge and ownership source is [`graph.toml`](../scripts/crate-graph.toml); this
document explains how agents should interpret it.

### Layers

| Layer | Responsibility | May depend on |
|---|---|---|
| `application` | Process startup, native shell composition, window actions, and service wiring. | Product, integration, contract, and foundation crates through explicit composition edges. |
| `product` | User-facing capabilities, workspace orchestration, panels, and feature presentation. | Foundation contracts/services and other capabilities only through typed contracts, registries, or events. |
| `integration` | Concrete protocol, process, filesystem, or external-service adapters. | UI-free capability contracts and foundation services. |
| `foundation` | Reusable values, contracts, UI primitives, persistence, errors, and platform services. | External libraries and lower-level foundation crates; never product modules. |

The layer label describes the crate's architectural role, not the ownership
module. A product owner may therefore have a foundation contract sibling and
an integration sibling while retaining one canonical capability owner.

### Direction rules

1. Dependencies point from application to product/integration/foundation and
   from product or integration toward foundation contracts.
2. Foundation crates never depend on product crates, the shell, or the binary.
3. Product crates never depend on `labonair-shell` or reach into another
   feature's private state.
4. Integration crates implement typed contracts; they do not become a second
   owner of product state.
5. `labonair-ui-kit` is reusable foundation infrastructure. It may consume
   design tokens, but it must not know product modules.
6. A dependency that cannot be explained by the owning capability, a typed
   contract, a registry, or composition wiring is a boundary defect.

### Current graph

The generated projection at
[`../generated/architecture.md`](architecture.md) lists every
workspace crate and direct internal edge. Run
`python3 scripts/verify.py --scope architecture` after changing a Cargo
dependency or ownership record.

## Dependency Rules

**Related:** [`../architecture.md`](architecture.md), [`layers.md`](architecture.md), [`graph.toml`](../scripts/crate-graph.toml), [`../audits/remaining-boundaries.md`](archive/audits/remaining-boundaries.md)

The architecture graph is an explicit policy, not a suggestion. Every internal
Cargo edge must be declared in [`graph.toml`](../scripts/crate-graph.toml), and every temporary
edge must have a removal condition, a bounded removal task, and a review
interval.

### Rules

1. Every product capability has one owner and one canonical capability crate.
2. Sibling crates are allowed only for a real contract, UI, storage, or
   integration boundary under that same owner.
3. Foundation crates do not depend on product modules.
4. Feature crates do not depend on `labonair-shell` or private state from
   another feature.
5. Composition roots may wire concrete implementations but must not become
   feature god objects.
6. Shared discovery uses a typed registry only when there are multiple
   providers or consumers; one caller gets a narrow typed contract.
7. UI-free engines remain UI-free transitively.
8. Panels never depend on another panel, and the panel contract stays below
   Workspace and the shell.
9. New dependencies require an ownership explanation, a task, and a passing
   architecture check.
10. A removed edge must be removed from the manifest, code, audit, and task
    once no consumer needs it.
11. A transitional edge whose review interval has expired fails the
    architecture check until its owner records a fresh review.

`labonair-background`, `labonair-theme-ui`, and `labonair-keymap-ui` may use
the public `labonair-settings::SettingsSurfaceRegistry` contract to register
their Settings entry points. These feature-level edges expose only Settings
surface metadata and an owner callback; they do not grant access to Settings
storage or another feature's private state. The composition root invokes those
owner registration functions and injects the live window/workspace callback.
`labonair-shell` therefore depends directly on `labonair-keymap-ui` to compose
its registration function.

### Verification

Run the narrow check while editing boundaries:

```text
python3 scripts/verify.py --scope architecture
```

This validates the manifest against Cargo metadata, checks acyclicity, and
retains the transitive UI/panel invariants. Regenerate the readable projection
with:

```text
python3 scripts/gen_architecture.py
```

The full repository verifier is `python3 scripts/verify.py --scope all`.

## Composition Root Contract

**Related:** [`../architecture.md`](architecture.md), [`runtime-lifecycle.md`](architecture.md), [`../feature-lifecycle.md`](architecture.md)

`labonair` and `labonair-shell` are the only all-feature composition roots.
They assemble concrete services and connect typed contracts; they do not own
feature behavior.

### Responsibilities

- construct the GPUI application and native window shell;
- load settings, themes, keymap, persistence, and platform services;
- construct integration adapters such as SSH, SFTP, Git, transfers, and MCP;
- inject those adapters into owner-defined contracts;
- register panels, commands, status items, notifications, and palette
  providers through their canonical registries;
- connect typed events between independently owned modules;
- select the initial workspace and recoverable startup state.

### Prohibited responsibilities

The composition roots must not:

- store feature state or duplicate feature persistence;
- render feature-specific views or implement capability business logic;
- contain a feature-name switch for command, panel, status, or notification
  behavior;
- expose a backend facade that every feature calls instead of its owner;
- add a second settings, notification, command, or surface registry;
- reach into private module state when a typed contract or event is required.

### Construction rule

For every injected service, the owning task and capability record must identify
the contract, implementation adapter, construction site, and failure behavior.
The adapter belongs in the integration or composition boundary; the contract
belongs to the owning capability or a deliberately shared foundation crate.

The preferred flow is:

```text
configuration and platform services
        ↓
typed capability contracts
        ↓
owner registration and feature views
        ↓
typed events back through the composition root
```

If construction requires a new permanent shell surface, first create or
update an ADR and prove that an existing product surface cannot host it.

## Runtime Lifecycle

**Related:** [`../architecture.md`](architecture.md), [`composition-root.md`](architecture.md), [`../visual-verification.md`](testing.md)

This is the canonical lifecycle vocabulary for implementation tasks, audits,
and agent handoffs. A feature is not considered integrated because its crate
compiles; it must be traceable through the relevant lifecycle states.

### Lifecycle states

| State | Owner | Required evidence |
|---|---|---|
| `construct` | Composition root | Concrete service and owner contract are wired without a duplicate owner. |
| `register` | Owning module | Commands, panels, status items, notifications, and palette providers use canonical registries. |
| `load` | Owning module plus injected services | Settings, persistence, or remote state is loaded asynchronously and failure is typed. |
| `render` | Owning UI crate | The view uses `labonair-ui-kit`, preserves shell zones, and has native visual evidence when UI changes. |
| `interact` | Owning module | User actions update owner state and emit typed events for other modules. |
| `persist` | Owning module | Values and lifecycle state are written through the capability's persistence boundary. |
| `shutdown` | Composition root and service owner | Tasks, sessions, watchers, and windows release resources without blocking the GPUI thread. |

### Agent checklist

For a lifecycle change, record:

1. the owning module and canonical crate;
2. the construction site and injected contract;
3. the user entry point and all affected shell zones;
4. state transitions, persistence, notifications, and emitted events;
5. failure, retry, cancellation, and shutdown behavior;
6. source, test, or native visual evidence for each claim.

I/O, process execution, network work, and database operations must not block
the GPUI foreground thread. Use async tasks or `spawn_blocking` and return
descriptive errors for predictable failures.

### Evidence vocabulary

Use `Verified`, `Partial`, `Pending`, or `N/A` in audits. A successful build
proves only the paths covered by that build. It does not prove visual layout,
ownership, or remote runtime behavior without corresponding evidence.
