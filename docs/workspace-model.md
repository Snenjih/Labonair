# Workspace Model

Related: [`architecture.md`](architecture.md) §7 (pane model, drag and drop, session snapshot), [`modules.md`](modules.md).

## Workspace types

Labonair has two equally valid entry modes:

1. **Project workspace** — tied to a local folder, remote folder, repository, or other durable work context.
2. **Standalone workspace** — a temporary context for a terminal, editor, SSH, SFTP, or other single action.

Both use the same tabs, panes, commands, notifications, docks, and statusbar; the difference is identity and persistence, not a separate UI architecture, and the shell keeps no second standalone implementation. A workspace is an application context, not a requirement that every action be project-based: it carries optional identity and shared navigation state while each tool module owns its own state inside it. No tool requires a project workspace unless its operation genuinely needs a project root.

A workspace contains: optional project or remote identity; tabs; recursive split panes; active tool instances; layout state; scoped settings overrides where supported; session persistence metadata.

## Identity and state

`labonair-workspace::context` models this as `WorkspaceIdentity` (`Standalone`/`Project`) plus `WorkspaceState` (`Empty`/`Active`); `WorkspaceContext` applies identity changes in pure form. `Workspace` owns the state, exposes one snapshot for shell surfaces, and starts `Standalone`. Identity changes only through the typed `WorkspaceTransition`; `Workspace` then coordinates project-settings synchronization and UI invalidation. Session snapshots persist the identity; legacy snapshots without one resolve to `Standalone`.

| Trigger | Transition | Identity effect | Layout/tool effect |
|---|---|---|---|
| User selects a folder in `Open Project…` | `OpenProject { root }` | Set the project root | Preserve tabs, panes, shell zones |
| Session restores a persisted project | `OpenProject { root }` | Restore the root before tab replay | Recreate saved tools and layout |
| User invokes `Return to Standalone` | `ReturnToStandalone` | Clear project scope | Preserve tabs, panes, shell zones |
| Terminal reports a new CWD | none | Identity unchanged | Update terminal metadata only |

The native project picker changes identity through the workspace API; a project is never inferred from a terminal's cwd.

## Entry and lifetime rules

- Opening a project or remote folder creates a project workspace with a stable identity and eligible session persistence.
- Opening a terminal, editor, SSH connection, SFTP browser, or one-off transfer without a project creates a standalone workspace containing only that tool context.
- A standalone workspace is temporary by default. It is never silently turned into a project or written into project settings; persistence is explicit and belongs to the workspace module.
- Project identity is the authoritative root for shell-owned project surfaces such as Explorer and Git. A standalone workspace may use the active terminal cwd as a convenience root for Explorer/Git, but that never activates project settings, converts the workspace into a project, or changes the root of an explicitly selected project.
- Host selection and management stay with Hosts. A selected host may create a standalone remote workspace or attach to an existing one through the SSH/SFTP contracts; Workspace only places and focuses the resulting tool instance.
- Workspace-to-shell navigation uses `WorkspaceEvent` (opening Hosts, presenting the native project picker). Workspace stores no shell callback and does not decide whether the destination is a palette page, panel, or window.
- An empty workspace is valid and stays a blank surface; the shell never creates a fallback terminal to fill space. Workspace actions stay reachable through their keyboard bindings and command entry points.

## Tool instances

Tabs and panels reference tool instances through stable IDs and typed capabilities. Workspace orchestrates placement and focus; the owning tool module controls state and operations. Workspace never inspects feature-private state to decide how a tool works: it receives typed capabilities and lifecycle events and delegates commands to the owner. Layout and focus are its only cross-tool responsibilities.

## Remote contexts

An SSH connection may provide a workspace root and terminal sessions; an SFTP connection a remote filesystem context. Shared concepts share identifiers, while transport-specific behavior stays with SSH or SFTP. Local and remote tools follow the same workspace flow when their capabilities are equivalent; differences are expressed by typed contracts, not shell-wide conditionals or duplicated workspace views.

## Shared Spaces across app instances

When the Detached Host is enabled, Workspace stores Spaces, ordered tabs, pane trees, and session references in its own `workspace.db` ([ADR 0010](adr/0010-shared-workspaces-and-multi-client-detached-sessions.md)). Space, tab, pane, and split-node identities are durable UUIDs; each app window has a random instance ID. GPUI entities and presentation state stay local to each process.

- Startup atomically leases the most recently used unclaimed Space. If every Space is leased, Workspace creates an empty Space carrying the most recently used Space's project identity. The Spaces picker is the explicit join path for a Space another instance uses.
- Leases renew while the window is alive, release on clean close, and expire after a crash. Every shared mutation carries the current lease generation and expected Space revision; a stale write is rejected and the client reloads or merges the revisioned snapshot instead of reporting success.
- Tab order and pane structure synchronize across participants. Active tab, focus, window geometry, terminal size, selection, search, and scroll stay local. Shared Editor tabs do not synchronize unsaved buffers or cursors.
- `general.sessionRestore` controls restoration after the final live lease and does not disable sharing while instances are open. When disabled, releasing the final lease clears the shared Workspace records; Detached Host sessions continue under their own lifecycle.
