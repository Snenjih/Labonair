# Automation (MCP)

## Automation and MCP Overview

**Related:** [`tools.toml`](automation.md), [`grants.md`](automation.md), [`limits.md`](automation.md), [`test-matrix.md`](automation.md), [`../security/threat-model.md`](security.md)

Labonair's automation boundary is the local MCP Streamable HTTP bridge. It is
disabled by default, binds to loopback, and requires bearer authentication.
Tools operate on terminal tabs that the user explicitly granted to the agent.

### Trust model

1. The user enables the bridge and supplies/retains the local bearer token.
2. The user grants a specific local or SSH tab; a grant is keyed by tab identity
   and revalidated against the current host policy on every tool call.
3. Tools act visibly through the real terminal pane or explicit tab operation.
4. Host `block_agent_access` and missing/non-interactive credentials deny access.
5. Activity and expiry events return through typed MCP contracts and the shared
   notification/event infrastructure.

The exact tools, grants, limits, side effects, and negative tests are the
structured source in [`tools.toml`](automation.md). Generated documentation is
`../generated/tools.md`.

## Automation Grants

**Related:** [`overview.md`](automation.md), [`tools.toml`](automation.md), [`../security/trust-boundaries.md`](security.md)

MCP authorization is explicit and session-scoped. A tool must not infer access
from the existence of a host, an SSH session, or a remembered previous action.

### Grant rules

- `list_sessions` returns only currently granted tabs.
- `run_command`, `read_output`, `send_keys`, and `close_tab` require a current
  grant for the requested session.
- `open_tab` requires a saved host with non-interactive stored credentials and
  `block_agent_access == false`; it creates a grant only after the visible tab
  operation succeeds.
- A host policy change revokes affected grants and emits `mcp_grant_expired`.
- Auto-revoke is configurable through the MCP capability preference; zero means
  no inactivity sweep.
- A grant is never a secret. Passwords, private keys, bearer tokens, and raw
  credential material must not be returned to the agent.

The workspace keeps a local mirror for UI state, while the MCP capability is
the authoritative grant owner. The bridge rechecks authorization at execution
time rather than trusting a stale UI snapshot.

## Automation Limits

**Related:** [`overview.md`](automation.md), [`tools.toml`](automation.md), [`../settings.md`](settings.md)

The following limits are part of the current MCP contract and must be updated
in the tool catalog and tests if they change:

| Limit | Current value | Purpose |
|---|---:|---|
| Loopback port | `47823` by default | Avoid exposing the bridge on a network interface. |
| Maximum `run_command` timeout | `300000 ms` by default | Bound waiting for terminal output. |
| Default `run_command` timeout | `30000 ms` | Keep ordinary calls responsive. |
| Default `read_output` wait | `1000 ms` | Poll live output without scrollback ownership. |
| `open_tab` wait | `15 s` | Bound visible tab/session creation. |
| `close_tab` wait | `10 s` | Bound tab operation acknowledgement. |
| Notification history | 100 records | Bound retained UI state. |
| Notification dedupe window | 2 s | Suppress equivalent bursts. |

Timeouts return partial/typed failure information where the operation may still
be running. They do not silently detach a command from the visible terminal.

## Automation Test Matrix

**Related:** [`overview.md`](automation.md), [`tools.toml`](automation.md), [`../testing/evidence-model.md`](testing.md)

Every automation tool has positive and negative test decisions in the
structured catalog. Negative tests are mandatory for missing grants, revoked
grants, blocked hosts, missing credentials, timeout behavior, and secret/path
exposure.

The matrix distinguishes:

- contract tests for stable MCP wire names and typed events;
- unit tests for grants, expiry, timeout caps, and preference persistence;
- integration tests for visible tab operations and terminal adapters;
- denial tests for unauthorized sessions, blocked hosts, unsupported
  authentication, invalid bearer tokens, and loopback/limit violations;
- evidence that no tool returns raw secret material or hidden command output.

The current repository has source-level and contract tests; the catalog marks
remaining end-to-end and negative-test gaps as `Pending` rather than implying
that MCP safety is complete.

## Tool catalog

```toml
version = 1
server_crate = "labonair-mcp-server"
transport = "Streamable HTTP on loopback with bearer authentication"

[[tool]]
id = "list_sessions"
owner = "mcp"
purpose = "List local/SSH terminal tabs that the user explicitly granted to the agent."
required_grant = "Authenticated bridge; output is limited to currently granted tabs."
allowed_targets = "Granted terminal tabs only."
path_restrictions = "No filesystem path argument."
secret_behavior = "Returns tab/session labels and kind, never credentials or terminal secrets."
timeouts_limits = "Single in-memory grant snapshot; no process execution."
side_effects = "None."
confirmation = "User grant is required before a tab appears."
failure_behavior = "Returns an authenticated bridge error; absent grants are an empty list."
audit_behavior = "No activity event for passive listing."
source_files = ["crates/mcp-server/src/server.rs", "crates/mcp-server/src/mod.rs", "crates/mcp-core/src/lib.rs"]
positive_tests = ["Granted local and SSH tabs are listed", "Revoked tabs are omitted"]
negative_tests = ["Invalid bearer token is rejected", "No ungranted session appears"]

[[tool]]
id = "run_command"
owner = "mcp"
purpose = "Type a command into a visibly granted local or SSH terminal and capture bounded output."
required_grant = "Current grant for session_id plus live host block-agent-access check."
allowed_targets = "Granted local or SSH terminal session identified by session_id."
path_restrictions = "No direct filesystem path API; command runs in the visible terminal context."
secret_behavior = "Command/output are not persisted by the tool; callers must not send secrets."
timeouts_limits = "Default 30s; capped by max_command_timeout_secs (300s default); returns still_running on timeout."
side_effects = "Visible terminal process execution and MCP activity event."
confirmation = "Explicit tab grant; no hidden background shell."
failure_behavior = "Denied grant, blocked host, closed session, stream interruption, or bounded timeout is returned explicitly."
audit_behavior = "Emits activity metadata; notification preference may surface it."
source_files = ["crates/mcp-server/src/server.rs", "crates/mcp-server/src/mod.rs", "crates/mcp-core/src/lib.rs"]
positive_tests = ["Run a command in a granted local tab", "Run a command in a granted SSH tab", "Return partial output for a long-running command"]
negative_tests = ["Reject missing grant", "Reject host blocked after grant", "Cap requested timeout", "Never execute an ungranted session"]

[[tool]]
id = "read_output"
owner = "mcp"
purpose = "Read new live output from a granted terminal without executing a command."
required_grant = "Current grant for session_id plus live host policy check."
allowed_targets = "Granted local or SSH terminal session."
path_restrictions = "No filesystem path argument and no scrollback history access."
secret_behavior = "Returns only live terminal bytes captured after the call subscribes."
timeouts_limits = "Default wait 1000ms; bounded by wait_ms."
side_effects = "Touches grant activity timestamp; no command execution."
confirmation = "Explicit tab grant."
failure_behavior = "Denied grant or closed stream returns an explicit error/empty bounded result."
audit_behavior = "Passive read does not emit action activity."
source_files = ["crates/mcp-server/src/server.rs"]
positive_tests = ["Read live output from a granted tab", "Return empty output after bounded wait"]
negative_tests = ["Reject missing grant", "Reject host blocked after grant", "Do not expose prior scrollback"]

[[tool]]
id = "send_keys"
owner = "mcp"
purpose = "Send raw keys/control input to an explicitly granted visible terminal."
required_grant = "Current grant for session_id plus live host policy check."
allowed_targets = "Granted local or SSH terminal session."
path_restrictions = "No filesystem path argument."
secret_behavior = "Input is not logged as a secret-safe value; callers must not send passwords."
timeouts_limits = "Single write operation; terminal/session transport limits apply."
side_effects = "Visible terminal input and MCP activity event."
confirmation = "Explicit tab grant; user sees the target terminal."
failure_behavior = "Denied grant or closed session is returned explicitly."
audit_behavior = "Emits action metadata without inventing a second audit store."
source_files = ["crates/mcp-server/src/server.rs"]
positive_tests = ["Send control-C to a granted session", "Send an interactive response to a visible prompt"]
negative_tests = ["Reject missing grant", "Reject host blocked after grant", "Do not accept a session not in the grant map"]

[[tool]]
id = "open_tab"
owner = "mcp"
purpose = "Open a visible SSH terminal tab to a saved host and grant the resulting session."
required_grant = "Authenticated bridge, host block-agent-access false, and non-interactive stored credential."
allowed_targets = "Saved host_id only; no arbitrary host/address input."
path_restrictions = "No remote path argument in the current tool."
secret_behavior = "Reads credential availability only; never returns password/private-key material."
timeouts_limits = "Visible operation acknowledgement bounded to 15s."
side_effects = "Creates a visible SSH tab, session grant, and activity event."
confirmation = "Host policy and stored non-interactive credential are required; interactive auth is denied."
failure_behavior = "Blocked host, missing/interactive credential, timeout, or failed tab operation is explicit."
audit_behavior = "Emits mcp_open_tab_request and activity metadata."
source_files = ["crates/mcp-server/src/server.rs", "crates/mcp-server/src/contract.rs", "crates/hosts/src/lib.rs"]
positive_tests = ["Open a saved host with stored non-interactive credentials", "Return the created tab/session IDs"]
negative_tests = ["Reject block_agent_access", "Reject missing credential", "Reject interactive/2FA auth", "Timeout and clean pending operation"]

[[tool]]
id = "close_tab"
owner = "mcp"
purpose = "Close an existing granted terminal tab by session identity."
required_grant = "Current grant for session_id."
allowed_targets = "Granted local or SSH tab only."
path_restrictions = "No filesystem path argument."
secret_behavior = "No secret input or output."
timeouts_limits = "Visible tab operation acknowledgement bounded to 10s."
side_effects = "Closes a visible tab and emits activity/event metadata. No unsaved-changes prompt is added by the tool."
confirmation = "Grant is required; caller is responsible for ensuring closure is safe."
failure_behavior = "Missing grant, failed operation, or timeout is explicit."
audit_behavior = "Emits mcp_close_tab_request and activity metadata."
source_files = ["crates/mcp-server/src/server.rs", "crates/mcp-server/src/contract.rs"]
positive_tests = ["Close a granted local/SSH tab", "Acknowledge successful tab operation"]
negative_tests = ["Reject missing grant", "Timeout and remove pending operation", "Do not close an ungranted tab"]
```
