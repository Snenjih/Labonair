# Security

Scope: the local MCP bridge, AI/agent access, SSH/SFTP sessions, credentials, the detached session host, and filesystem-adjacent integrations. Tool-level grants, limits and the tool list live in [automation.md](automation.md).

## 1. Assets and actors

Assets: bearer token, stored credentials, private keys, passwords, host metadata, session identifiers, local file content, visible terminal I/O, remote hosts/sessions/tunnels/transfers, and the user's trust that agent actions are visible, scoped and revocable.

| Actor | Trust | Boundary |
|---|---|---|
| User | Decision authority | Grants/revokes tab access; chooses hosts and actions. |
| Labonair UI | Trusted process | Owns capability state, presents actions. |
| MCP client/agent | Untrusted by default | Must present the bearer token and hold an explicit session grant. |
| MCP bridge | Narrow adapter | Loopback endpoint; rechecks policy on each tool call. |
| SSH/SFTP remote | Untrusted external | Reached only via typed transport contracts and host policy. |
| Secret store | Sensitive | Returns material only to the authentication path. |

The bridge is not a sandbox. The safety model is: user grant, host policy, secret boundary, visible terminal, explicit denial. Any stronger isolation claim needs its own ADR.

## 2. Ownership of boundaries

```text
user decision -> explicit tab/host grant
  -> MCP loopback + bearer token
  -> typed MCP contracts/events
  -> composition root adapters
  -> owner contracts: workspace / hosts / SSH / SFTP / terminal
  -> narrow secret lookup -> secret store, remote transport
```

- `labonair-mcp-core`: wire values, preferences, narrow service contracts.
- `labonair-mcp-server`: HTTP tools, grants, timeouts, event adapters; authoritative grant map.
- `labonair-shell`: composes concrete state into `McpServerAccess`.
- Workspace mirrors grants for UI only.
- `labonair-hosts` owns host policy and `block_agent_access`; `labonair-ssh*` / `labonair-sftp*` own transport.
- `labonair-secrets` owns secret material; it is never a general data source.

A change crossing one of these boundaries needs an owner, a typed contract, a negative-test decision, and an update here.

## 3. Threats and controls

| Threat | Control | Evidence |
|---|---|---|
| Unauthenticated tool call | Bridge disabled by default, binds `127.0.0.1` only, `Authorization: Bearer <token>` required (401 otherwise). | Negative auth test still wanted. |
| Stale tab grant | `block_agent_access` is re-read from the hosts DB at execution time (`ensure_grant_still_authorized`). | Blocked-host and expiry tests wanted. |
| Agent reaches blocked host | `open_tab` and grant creation refuse blocked hosts; blocking a host revokes its grants (`revoke_agent_access`, emits `mcp_grant_expired`). | Host/MCP integration test wanted. |
| Secret exfiltration | No secret values in tool schemas/outputs; `open_tab` only checks that a credential exists. AI tools additionally run path guards (see §5). | Redaction/path-negative tests. |
| Hidden command execution | `run_command`/`send_keys` write into the visible granted terminal. | Visible-terminal integration evidence. |
| Runaway command | Per-session lock; timeout capped by `max_command_timeout_secs`; timeout returns partial output with `still_running: true`. | Timeout/cancel tests. |
| Host/path confusion | `open_tab` takes only a saved `host_id`; typed SSH/SFTP contracts. | Contract tests. |
| Log/notification leakage | Structured details, never credentials/tokens. | Review, redaction tests. |

## 4. Secrets

Secrets: passwords, private keys, bearer tokens, credential payloads, sensitive host/session values.

- Stored only via `labonair-secrets` (and the `labonair-credentials` owner). Host records hold references/metadata, not raw secrets. Lookups use `(service, account)` pairs, e.g. `labonair-cred`/`<credential id>`, `labonair-app`/`<host id>`, and `labonair-mcp`/`bearer-token` for the bridge token.
- Storage is a local file in the app data dir, not the OS keychain: `secrets.json` (plain, relies on file permissions) by default, or `secrets.enc` (AES-256-GCM) when encryption is enabled in settings. The app-managed key `enc_key.bin` is generated once (mode 0600 on Unix); there is no master password. `SecretsState::in_memory()` is a RAM-only store.
- Never place secrets in source, logs, notifications, SQLite fixtures, tests, docs, screenshots or commits. Test fixtures use synthetic credentials.
- MCP tools may check that a credential exists, never return it. Tool schemas use host/session IDs, not raw password/key parameters.
- Error details must not echo command input or credential material.
- The bearer token and the update-signing private key are operator/CI secrets; only their storage contract belongs in the repo.

## 5. AI path and command guards

`crates/ai/src/tools/security.rs` (a defense layer, not a sandbox; user confirmation of write/exec is the real net): blocks reads of secret-like files (`.env*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.asc`, `*.gpg`, `id_rsa*`, `.aws/credentials`, `.ssh/`, `.git/`, ...), blocks writes into those plus system directories, offers symlink-resolving variants (`check_readable_resolved` / `check_writable_resolved`), and rejects catastrophic shell commands (`rm -rf /`, `chmod 777`, `dd` to block devices, disk formatting, commands referencing secret files). These guards apply to AI tool calls, not to MCP `run_command`, which is bounded by the grant instead.

## 6. Remote access

Hosts owns saved-host identity and policy; SSH/SFTP own connection and session execution. Transfers, snippets, Explorer, Workspace and MCP consume typed contracts or composition adapters, never transport-private state.

- Use stable host IDs and typed `HostOpenRequest`/session contracts.
- Jump hosts are SSH connection configuration, not a separate product surface.
- SFTP browsing/transfer lifecycle stays separate from SSH transport internals (Explorer consumes `RemoteExplorerService`; the shell injects the SFTP adapter).
- Connection, trust, authentication, timeout and transfer failures are structured owner notifications with safe details.
- Cancellation, reconnect and stale-session behavior must be explicit in the owning task and tests.
- Remote safety needs allowed and denied integration cases, not a compile-only check.

## 7. Detached session host

- Local to one OS user: private data directory (0700), Unix socket, and a peer-UID check (`peer_cred`) are the trust boundary. No TCP, no remote pairing.
- Workspace leases (`labonair-workspace-storage`) carry a generation that fences stale mutation, renewal, switch and release calls from a prior claim.
- Detached SSH secrets stay in host memory; they are not written to SQLite or event logs. Workspace persistence stores no credentials.
- Host-key and transfer decisions are bound to the connection that started the operation; takeover by another client issues a new prompt ID (design in [ADR 0010](adr/0010-shared-workspaces-and-multi-client-detached-sessions.md); one-use prompt-ID enforcement not re-verified in code for this revision).

## 8. Project trust

Target (not yet true): a Workspace-owned Restricted Mode where untrusted project/single-file roots start restricted, trust is an explicit per-path decision (local and remote identities separate, never implying SSH host-key acceptance or MCP tab access), and project settings, language services, Agent, Git and MCP integrations query a typed trust contract before project-controlled settings, skills, tools or workers take effect; failed trust writes surface through notifications and reopen in Restricted Mode. No `WorkspaceTrust`/Restricted-Mode code exists today. What is true: project settings files are treated as untrusted input (`crates/settings/src/project.rs` warns that keys from a cloned repo must not be trusted blindly).
