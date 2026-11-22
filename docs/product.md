# Labonair Product Contract

## Product identity

Labonair is a fast, keyboard-first Dev-Op workspace for local and remote work, combining terminal, SSH, SFTP, editor, Git, and AI capabilities without becoming a terminal emulator or a traditional IDE. The primary unit is a workspace: a project, a remote environment, or a temporary collection of tools ([`workspace-model.md`](workspace-model.md)). Every tool also supports standalone use without project context.

## Goals

1. Make local and remote development tasks fast to start and easy to finish.
2. Give terminal and remote workflows first-class status.
3. Keep the default interface quiet and intentional; make power features discoverable by keyboard.
4. Let features evolve independently without inconsistent UI or cross-module coupling.
5. One interaction model, one visual language, one location per responsibility.

## Non-goals

- Labonair owns its scope, interaction model, implementation, and assets. Product behavior is specified by this contract, the capability matrix ([`modules.md`](modules.md)), and the design system.
- Marketplace, extension, collaboration, and hosted-service workflows need an explicit product decision, capability owner, service contract, and security and distribution review first.
- Settings keeps typed value ownership; capability management stays with its owner and is exposed through its canonical entry point ([`settings.md`](settings.md)).
- No feature is preserved merely because it existed in the Tauri predecessor.

## Principles

- **Simple by default.** Only the active task occupies permanent UI; secondary actions use the command palette, contextual menus, or focused dialogs.
- **Keyboard-first, mouse-complete.** Every user-invokable action exposes a stable command ID; frequent navigation and workflow actions ship with a default binding, rare or destructive ones stay reachable through the palette. No important workflow is mouse-only.
- **One responsibility, one home.** Each feature has one canonical owner (a module with one canonical capability crate, plus siblings only for real boundaries) and one primary entry point; alternates are allowed only for discoverability without duplicated logic. Users experience one coherent feature.
- **Local and remote parity.** Equivalent workflows share workspace concepts; differences live in the owning module, not in shell-wide special cases.
- **Progressive replacement.** Existing implementation is reused only if it satisfies this contract; compatibility code is temporary with a removal plan.
- **Deliberate scope.** The capability matrix defines feature scope; every retained capability has an owner, entry point, contract, and acceptance evidence. Deferred or removed work stays out of scope until a product decision reopens it.

## Product surfaces

The permanent chrome has four zones and one overlay layer:

1. **Titlebar** — tabs and one global menu button.
2. **Workspace** — active tab content and split panes.
3. **Docks** — registered panels on the left, right, or bottom.
4. **Statusbar** — panel controls on the left; global status and information items on the right.
5. **Overlay layer** — command palette, dialogs, focused pickers, other explicitly modal surfaces.

No feature adds a new permanent global strip, badge row, or shell toolbar without an ADR showing existing surfaces cannot host it.

## Product decisions

- One universal tab bar manages all tab content (Editor, Terminal, Source Control, Git Graph, DiffView, Keymap, Onboarding, feature views).
- Themes and icon themes are selected through command-palette submenus with live preview.
- Hosts are selected through a command-palette submenu; `Enter` opens SSH, `Shift+Enter` opens SFTP.
- The titlebar global menu exposes Keymap, Themes, Icon Themes, and Hosts. Host management is the Hosts owner's full-page surface on the Settings `Hosts` page (below Keymap, [ADR 0007](adr/0007-hosts-settings-page.md)); host selection stays a palette submenu.
- Notifications are retained entries in the statusbar dropdown. Producers may opt into a brief toast for selected events; the toast and dropdown share the same entry and actions ([ADR 0011](adr/0011-actionable-notification-presentations.md)).
- Transfers are a statusbar item with progress and history.
- Jump hosts are part of connection configuration and execution, not a standalone menu.
- AI and agent workflows follow the same owner-first design as other capabilities.
