# Registry Contracts

Registries are the extension mechanism for a capability with multiple providers or consumers that needs discovery. A registry owns discovery, stable metadata, and lifecycle; the owning module owns the behavior behind each entry. With a single provider and consumer, prefer a direct typed trait or event.

## Common contract

Every registry documents: owner and lifecycle (startup, workspace-scoped, or long-lived); stable ID namespace and duplicate-ID behavior; static vs. asynchronous providers; snapshot, refresh, and invalidation semantics; action context and result/error handling; persistence and migration, if any; focused contract tests and a removal path for temporary adapters.

Registration rules:

- Registration is explicit at application startup, performed by the owning module through a typed API. The composition root invokes these functions and connects services but is never a second registry owner or parallel list.
- IDs are stable and unique within their registry; duplicate registration is a descriptive startup error.
- Consumers receive immutable snapshots or typed handles and never mutate another module's registry-owned state.
- Registries are testable without launching the application.
- Adding an entry never requires editing an unrelated central feature table.

## Command registry

The command-palette module owns one command registry; `CommandId` in `labonair-command-palette-core` is the single command identity. The contract is UI-free: capability crates depend on `labonair-command-palette-core` to publish metadata, the palette UI renders its snapshots, and executable owner callbacks use the separate `labonair-command-palette-runtime` bridge (knows GPUI, not the shell or any product module). Keymap depends on the contract, never the reverse.

A command registers: stable namespaced action ID (opaque to the view); title and searchable aliases; category or submenu; applicable contexts; default keybinding metadata; execution callback or typed action payload. The palette owns search, filtering, navigation, preview, and selection, but no feature state or behavior, no giant dispatch table, and no static shell command table. Execution is resolved by the owner after selection.

Default bindings follow the same ownership: a provider attaches them with `CommandDescriptor::with_default_binding`; the keymap module materializes them into its built-in default layer. The shipped JSONC asset remains a compatibility layer and the user keymap the override layer.

Submenus are registered providers. Dynamic entries (hosts, themes, tabs) are immutable snapshots with typed primary and secondary actions; the capability owns loading and action semantics, the palette only common filtering, focus, preview, and picker interaction. Snapshot builders live in the workspace, hosts, editor, theme, snippets, and Git providers; the root supplies live values. Hidden status-bar state and labels are owned by the workspace status registry using the same snapshot contract.

`PaletteAction` values are dispatched by `labonair-command-palette-runtime::PaletteActionHandlerRegistry`. Each owner registers one narrow contribution; `labonair-shell::actions` only forwards the active window/application context and has no submenu dispatch table.

## Keymap registry

Every command-capable action may register a keybinding descriptor. The keymap module owns keymap file loading and persistence, the versioned JSONC document shape and built-in default assets, contexts and precedence, conflict detection, display formatting, and user overrides. It contains no feature behavior: a feature owns its action and stable command ID; keymap only maps that ID to input. The keymap editor is a keymap surface, not a Settings category.

- `keymap::adapter::load` is the loading boundary: it takes the command registry, builds the known-action vocabulary and owner-default layer, and returns one immutable snapshot with effective bindings and diagnostics. A platform shell installs the snapshot into GPUI but does not reimplement loading, validation, recovery, or layering.
- The editor-facing `KeymapDocument` keeps the original user source authoritative and exposes parsing/validation as derived state. Saving writes the source unchanged, so invalid or unknown entries are diagnosed, never silently normalized.
- Persisted action names resolve once through `keymap::runtime::command_for_action` into `CommandId`, then map to a concrete platform action. GPUI context predicates stay in the adapter; no duplicate action-name aliases.
- Palette and other surfaces consume effective bindings by `CommandId`. The old `ShortcutId`/`SHORTCUTS` cheat-sheet model is gone; owner descriptors are the only default source and the keymap runtime owns resolution and conflicts.
- `labonair-keymap-ui` is the presentation sibling: it takes a `KeymapManagementSnapshot`, provides filtering, diagnostics, and an explicit raw-JSONC action; it does not read files on the GPUI thread, resolve commands, or keep a second shortcut registry. `KeymapManagementView` is hosted by `labonair-workspace` as the single `Keymap` workspace tab; the titlebar global menu and the `Open Keymap` command (`CommandId::OpenKeymapJson`) both route to `Workspace::open_keymap_tab`.
- Compatibility aliases: `settings::OpenShortcuts` is accepted only for existing keymap files and resolves to `Open Keymap` (`OpenKeymapJson`, historical name kept for file compatibility); it has no palette entry, menu, or Settings page. `connections::OpenHostSettings` is likewise an alias of `connections::OpenHosts`, which opens the Settings `Hosts` page (ADR 0007). New actions use the command/keymap contracts.

## Theme registries

The themes module owns separate color-theme and icon-theme registries. Each entry has a stable ID, display name, metadata, and a complete or layered definition. Preview is transactional: navigation applies a temporary preview, confirmation persists the ID. The global menu exposes both through the command palette; neither is a Settings management page.

Built-in themes ship with the app. The icon-theme catalog contains Labonair (`default`) and Material Icon Theme (`material-icon-theme`), bundled with their SVGs and selected via the same preview/commit flow. Icon themes may keep color data embedded in SVGs; monochrome themes use the Explorer foreground tint. Downloaded and user-authored theme extensions are deferred until a concrete workflow and owner exist and are not part of this contract.

## Settings owner-surface registry

`labonair-settings` owns the startup-scoped `SettingsSurfaceRegistry` for capability-owned Settings links. Each contribution has a stable `SettingsSurfaceId`, searchable row metadata, Settings page placement, and an open callback. IDs are unique in the Settings surface namespace; duplicate IDs, empty metadata, and conflicting page metadata are rejected. Contributions are static for the application lifetime and consumers receive an immutable snapshot used by navigation and search. Opening delegates to the canonical capability surface; Settings owns no linked feature's state or persistence, and the registry has no persistence or refresh. The root invokes each owner's registration function and injects the live window/workspace callback; it keeps no parallel row metadata, and adding a contribution does not touch the Settings renderer.

## Panel and status-item registries

Panels register identity, title, icon, supported docks, and a view factory. Status items register identity, placement metadata, badge behavior, and a view factory. The shell hosts the surface; feature modules own content and actions. Contracts live in `labonair-panel`.

## Activity registry

`labonair-activity::ActivityRegistry` is the sole owner of "work is running" state shown in the statusbar. A feature announces work with `ActivityCenter::begin` and clears it with `finish`; a failure with no other surface begins as `ActivityKind::Failure` and stays until dismissed. Entries may carry a `key` (one language server shared by many editors announces once), a `show_after` delay (short runs never flash the bar), progress, and data-only cancel/click action ids interpreted by the owner via `ActivityActionEvent`; closures are never stored. Failures that already publish a notification do not also publish an activity. Transfers are excluded and keep their badge.

## Notification center

`labonair-notifications-core` is the single user-message registry. It accepts structured notifications (kind, title, summary, optional details, source, timestamp, actions, optional toast presentation, deduplication metadata), keeps records until dismissed or cleared, and exposes read state for the statusbar badge. It renders no UI and runs no timers. The GPUI adapter stores no closures: owner actions return later as stable action IDs, while typed external-URL targets are handled by the notifications adapter.

Publishing a notification is the complete user-message path: no feature-local duplicate banner for the same event. A notification may opt into a short-lived toast presentation; its retained record remains available in the dropdown after the toast expires or is closed. The notifications module owns both presentations, and the shell only composes its toast overlay. Toast timers affect transient visibility only. Producers without an explicit toast policy continue to appear only in the dropdown.

The dropdown groups warnings and errors as Problems and other entries as Activity. Search, clear-all, severity cues, and notification actions are owned by the notifications UI. Actions remain data-only: owner actions emit a stable action ID, while a typed external-URL target is opened by the notifications adapter.

This is a direct notification builder and center contract, not a separate toast-provider registry: there is one registry owner and no discovery or multi-provider lifecycle to manage. See [ADR 0011](adr/0011-actionable-notification-presentations.md).

Transfer UI publishes a retained terminal-state notification with a toast and a `Show transfers` action only while its queue is closed. It owns the action and opens its existing queue; progress and recovery state remain in the transfer registry ([ADR 0012](adr/0012-transfer-notifications.md)).

Explorer file-operation notifications may offer `Open destination`. Explorer retains the destination by notification ID, loads it when the action is activated, and emits `ExplorerEvent::OpenDestination`; the shell forwards this typed event to Workspace solely to open the Explorer dock ([ADR 0013](adr/0013-explorer-notification-navigation.md)).

## Transfer registry

`labonair-transfers::TransferRegistry` is the sole owner of transfer lifecycle state. It lives for the application session and retains queued, active, paused, failed, cancelled, and completed records until the user clears terminal history. It consumes typed `TransferEvent`s and exposes immutable `TransferSnapshot`s. `TransferService` is the action contract (enqueue, cancel, conflict/file-error resolution); `TransferEventSource` is the read-only event boundary. `labonair-transfers-ssh` translates the raw adapter event bus once; no workspace or statusbar code decodes raw event names. The statusbar-hosted `labonair-transfers-ui` view is the canonical presentation and emits only a typed completion signal for the SFTP pane refresh. History is in-memory; durable history needs a separate persistence decision and schema.
