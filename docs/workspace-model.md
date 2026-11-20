# Workspace Model

**Status:** Normative
**Version:** 1

## Workspace types

Labonair supports two equally valid entry modes:

1. **Project workspace** — tied to a local folder, remote folder, repository, or other durable work context.
2. **Standalone workspace** — a temporary context for a terminal, editor, SSH, SFTP, or other single action.

Both use the same tabs, panes, commands, notifications, and statusbar. The difference is identity and persistence, not a separate UI architecture.

The current implementation represents this contract in
`labonair-workspace::context` as `WorkspaceIdentity` plus `WorkspaceState`,
with identity changes accepted through the typed `WorkspaceTransition`
contract.
`Workspace` owns the state and exposes one snapshot for shell surfaces. It
starts as `Standalone`; the explicit native project picker changes the
identity through the workspace API rather than inferring a project from a
terminal's current working directory. Session snapshots now persist that
identity, while legacy snapshots default to Standalone. The explicit
return-to-standalone action uses the same workspace model without changing
the tab, pane, or shell layout.

## Shared Spaces across app instances

When the Detached Host is enabled, Workspace stores Spaces, ordered tabs,
pane trees, and session references in its own `workspace.db`. Space, tab,
pane, and split-node identities are durable UUIDs; each app window has a
separate random Workspace instance ID. GPUI entities and presentation state
remain local to each process.

Startup atomically leases the most recently used unclaimed Space. If every
existing Space is leased, Workspace creates an empty Space that carries the
most recently used Space's project identity. The Spaces picker is the explicit
join path for a Space another instance already uses. Leases are renewed while
the window is alive, released on clean close, and expire after a crash. Every
shared mutation carries the current lease generation and expected Space
revision; a stale write is rejected and the client reloads or merges the
revisioned snapshot instead of reporting success.

Tab order and pane structure synchronize across participants. Active tab,
focus, window geometry, terminal size, selection, search, and scroll position
remain local. `general.sessionRestore` controls restoration after the final
live Workspace lease; it does not disable sharing while instances are open.
When restoration is disabled, releasing the final lease clears the shared
Workspace records, while Detached Host sessions continue under their own
lifecycle. Shared Editor tabs do not synchronize unsaved buffers or cursors.

A workspace is an application context, not a requirement that every action be
project-based. The context carries optional identity and shared navigation
state; the tool module still owns the terminal, editor, SSH, SFTP, or transfer
state inside it.

## Workspace contents

A workspace contains:

- optional project or remote identity;
- tabs;
- recursive split panes;
- active tool instances;
- layout state;
- scoped settings overrides where supported;
- session persistence metadata.

No tool may require a project workspace unless its operation genuinely needs a project root.

## Entry and lifetime rules

- Opening a project or remote folder creates a project workspace with a stable
  identity and eligible session persistence.
- Opening a terminal, editor, SSH connection, SFTP browser, or one-off
  transfer without a project creates a standalone workspace containing only
  the requested tool context.
- Standalone and project workspaces use the same command, tab, pane, dock,
  statusbar, and notification contracts. The shell does not maintain a second
  standalone implementation.
- A standalone workspace is temporary by default. It is not silently turned
  into a project or written into project settings; persistence is explicit and
  belongs to the workspace module.

Host selection and host management remain owned by Hosts. A selected host may
create a standalone remote workspace or attach to an existing workspace
through the SSH/SFTP contracts; Workspace only places and focuses the
  resulting tool instance.

Project identity is also the authoritative root for shell-owned project
surfaces such as Explorer and Git. A standalone workspace may use the active
terminal cwd as a convenience fallback, but changing directories in a
terminal must never change the root of an explicitly selected project.

Workspace-to-shell navigation uses `WorkspaceEvent` for requests such as
opening Hosts or presenting the native project picker. Workspace does not
store a shell callback or decide whether the destination is a palette page,
panel, or standalone window.

## Identity transition matrix

Identity changes are explicit and use the UI-free
`WorkspaceTransition` contract. The `WorkspaceContext` applies the pure state
change; `Workspace` then coordinates project-settings synchronization and UI
invalidation for a changed transition.

| Trigger | Transition | Identity effect | Layout/tool effect |
|---|---|---|---|
| User selects a folder in `Open Project…` | `OpenProject { root }` | Set the selected project root | Preserve existing tabs, panes, and shell zones |
| Session restores a persisted project | `OpenProject { root }` | Restore the persisted project root before tab replay | Recreate the saved tools and layout |
| User invokes `Return to Standalone` | `ReturnToStandalone` | Clear project scope | Preserve existing tabs, panes, and shell zones |
| Terminal reports a new CWD | No transition | Do not change workspace identity | Update terminal metadata only |

Legacy snapshots without an identity field resolve to `Standalone`. A
terminal CWD may provide a convenience root for standalone Explorer/Git
surfaces, but it can never activate project settings or convert a standalone
workspace into a project workspace.

## Tool instances

Tabs and panels reference tool instances through stable IDs and typed capabilities. The workspace orchestrates placement and focus; the owning tool module controls its state and operations.

Workspace must not inspect feature-private state to decide how a tool works.
It receives typed capabilities and lifecycle events, and delegates commands to
the owning module. Layout and focus are the only cross-tool responsibilities
that belong here.

## Empty workspace

An empty workspace is valid and remains a blank surface; the shell must not create a fallback terminal merely to fill space. Workspace actions remain available through their existing keyboard bindings and command entry points.

## Remote contexts

An SSH connection may provide a workspace root and terminal sessions. An SFTP connection may provide a remote filesystem context. Shared concepts should have common identifiers, while transport-specific behavior remains owned by SSH or SFTP.

Local and remote tools should follow the same workspace flow when their
capabilities are equivalent. Transport differences are expressed by typed
contracts, not by shell-wide conditionals or duplicated workspace views.
