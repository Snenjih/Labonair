# Automation (MCP)

The automation boundary is the local MCP Streamable HTTP bridge (`labonair-mcp-server`, wire values and preferences in `labonair-mcp-core`), served at `/mcp` on `127.0.0.1`. It is disabled by default and requires `Authorization: Bearer <token>`; the token is a random 256-bit hex string kept in the secret store (`labonair-mcp`/`bearer-token`). Threat model: [security.md](security.md).

## 1. Trust model

1. The user enables the bridge (`bridgeEnabled`, default false) and keeps the local bearer token.
2. The user grants a specific local or SSH tab. A grant is keyed by tab identity.
3. Tools act visibly through the real terminal pane or an explicit tab operation; there is no hidden shell.
4. Host `block_agent_access` and missing/non-interactive credentials deny access.
5. Activity and expiry events return through typed MCP contracts and the shared notification/event infrastructure (`notifyOnActivity`, default false).

## 2. Grants

Authorization is explicit and session-scoped; never inferred from the existence of a host, SSH session or earlier action.

- `list_sessions` returns only currently granted tabs.
- `run_command`, `read_output`, `send_keys` require a current grant for `session_id` and re-read the host's `block_agent_access` at execution time (a flag flipped after granting takes effect on the next call). `close_tab` requires a current grant but does not re-check the host flag.
- Creating a grant for a blocked host fails.
- `open_tab` needs a saved host, `block_agent_access == false`, and a non-interactive credential (auth method `none`, or a stored password/passphrase); it creates a grant only after the visible tab operation succeeds.
- Blocking agent access on a host revokes its grants and emits `mcp_grant_expired`.
- Auto-revoke: `autoRevokeMinutes` (default 0 = off). A sweeper runs every 60 s and revokes grants idle longer than the window (last grant/tool activity), emitting `mcp_grant_expired`.
- A grant is never a secret; credentials and tokens are never returned to the agent.
- The MCP capability is the authoritative grant owner; Workspace keeps a UI mirror.

## 3. Limits

Defaults; change here and in tests together.

| Limit | Value |
|---|---|
| Loopback port (`bridgePort`) | `47823` |
| `run_command` default timeout (`timeout_ms`) | 30000 ms |
| `run_command` max timeout (`maxCommandTimeoutSecs`) | 300 s; request capped to it |
| `read_output` default wait (`wait_ms`) | 1000 ms |
| `open_tab` acknowledgement | 15 s |
| `close_tab` acknowledgement | 10 s |

Timeouts return partial output with `still_running: true`; the command is not detached from the visible terminal. Notification center bounds (shared, not MCP-specific): 100 records, 2 s dedupe window. Preferences persist under the `mcp` key of `config.json`.

## 4. Tools

All six are owned by `mcp`, implemented in `crates/mcp-server/src/server.rs`, with event contracts in `contract.rs`. None takes a filesystem path or secret parameter.

| Tool | Purpose | Grant / checks | Limits | Side effects / events |
|---|---|---|---|---|
| `list_sessions` | List granted tabs (tab id, session id, label, kind). | Bridge auth only; ungranted tabs absent, no grants = empty list. | In-memory snapshot. | None; passive. |
| `run_command` | Type a command into a granted local/SSH terminal; capture output and exit code (OSC 133 prompt markers). | Grant + live host check. | 30 s default, capped; per-session lock; `still_running` on timeout. | Visible execution; activity event. |
| `read_output` | Peek at new live output without running anything; never scrollback. | Grant + live host check. | `wait_ms`, default 1000. | Touches grant activity timestamp; no activity event. |
| `send_keys` | Send raw keystrokes/control input (prompts, Ctrl-C `\u0003`). | Grant + live host check. | Single write. | Visible input; activity event. |
| `open_tab` | Open a visible SSH tab to a saved `host_id` and grant it. | Host not blocked; non-interactive credential; interactive/2FA/missing credential denied. | 15 s. | Emits `mcp_open_tab_request` and activity. |
| `close_tab` | Close a granted tab by `session_id`, without unsaved-changes prompt. | Grant. | 10 s. | Emits `mcp_close_tab_request` and activity. |

Failures (missing grant, blocked host, closed session/stream, timeout, failed tab op) are returned explicitly as tool errors. Callers must not send passwords through `run_command`/`send_keys`; input is not treated as secret-safe.

## 5. Tests

Mandatory negative cases: missing grant, revoked grant, blocked host (also after granting), missing/interactive credential, timeout cap, invalid bearer token, no secret or scrollback exposure. Kinds: contract tests for wire names/events, unit tests for grants/expiry/timeout caps/preference persistence (`mcp-core/src/preferences.rs`), integration tests for visible tab operations, denial tests. The repo currently has source-level and contract tests; end-to-end and several negative tests are still pending, so MCP safety is not claimed complete.
