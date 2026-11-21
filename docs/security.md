# Security model

This document records current trust boundaries, controls, and known gaps in the repository. It is not a claim that Labonair is a sandbox or has undergone an independent security audit. MCP tool names and limits are documented in [automation.md](automation.md).

## Scope and trust boundaries

Sensitive assets include stored credentials and API keys, private-key material and paths, the MCP bearer token, host and session metadata, local files, terminal input and output, and remote sessions and transfers.

| Component | Trust boundary |
|---|---|
| User | Chooses hosts and actions, and grants or revokes access to individual terminal tabs. |
| Labonair app | Trusted local process that owns UI state and composes capability services. |
| MCP client or agent | Untrusted until authenticated and explicitly granted a tab. |
| MCP bridge | Local Streamable HTTP adapter; bearer token and tab grants gate access. It is not a sandbox. |
| Detached session host | Local daemon reached through a Unix socket; same-OS-user boundary. |
| SSH/SFTP server | External system reached through the saved-host and transport contracts. |
| Secret stores | Sensitive sources; values should be read only by the authentication path that needs them. |

Security ownership follows the capability boundaries: `labonair-mcp-core` owns MCP contracts and preferences, `labonair-mcp-server` owns the HTTP server and grants, and `labonair-shell` composes terminal and SSH access. `labonair-hosts` owns host policy; `labonair-credentials` owns credential metadata; `labonair-secrets` owns local secret values; SSH and SFTP modules own their transports. See the [capability matrix](modules.md).

Processes running as the same OS user can generally access the app data directory and should be treated as within the trusted local boundary. Local and remote terminal grants carry the authority of the corresponding shell account.

## MCP bridge

The bridge is disabled by default. When enabled, it listens on `127.0.0.1` (default port `47823`) and requires an `Authorization: Bearer …` header on every request. A random 256-bit token is generated on first enable and stored through `labonair-secrets`, outside SQLite. The listener does not expose a remote network endpoint or provide TLS; a local client that obtains the token can use the bridge.

Access is granted per terminal tab. The bridge supports listing granted sessions, sending commands and raw keystrokes, reading new live output, opening a visible SSH tab to a saved host, and closing a granted tab. `run_command` and `send_keys` write to the real visible PTY. A grant does not ask for confirmation before each command. `close_tab` closes the granted tab without an unsaved-changes prompt. `run_command` and `read_output` return terminal output, so a command can expose data readable by that shell. Do not send passwords, API keys, or other secrets through commands or keystrokes.

For SSH tabs, grants and `open_tab` reject hosts with `block_agent_access` enabled. `open_tab` also requires a saved host and non-interactive stored authentication. The host policy is re-read before `run_command`, `read_output`, and `send_keys`; `close_tab` checks the grant but does not re-read the host flag. Blocking access revokes existing grants. Disabling the bridge stops the listener and clears grants. Automatic revocation is optional and defaults to off (`autoRevokeMinutes = 0`).

**Known gap:** `host_blocks_agent_access` currently treats a missing host row or a failed database query as “not blocked” (`unwrap_or(0)`). A stale grant may therefore pass the host-policy check when that query fails. Do not rely on this check as fail-closed until the implementation changes.

**Sensitive activity data:** MCP action events include the command text or raw keystroke data in their `detail` field. The bridge does not provide a secret-safe channel for shell input or output. Consumers must not persist or display these values as though they were redacted.

See [automation.md](automation.md) for tool behavior and limits. The current source includes preference and OSC 133 parser tests, but no MCP server-level tests were found for bearer authentication, grants, host-policy rechecks, or terminal-action integration.

## Credentials and local data

SSH credential metadata and host records are stored separately from secret values. `labonair-secrets` stores passwords, passphrases, and the MCP token in the app data directory:

- By default, values are written to `secrets.json` as plain JSON. On Unix, files written by the store use mode `0600`.
- Optional at-rest encryption writes `secrets.enc` using AES-256-GCM. The app generates `enc_key.bin` and stores it alongside the data; on Unix the key file also uses mode `0600`. There is no master password or separate user-held encryption key.
- The encrypted file and its app-managed key share the same data directory. This is not an independent key-management boundary and does not protect against a compromised same-user account or a copied data directory containing both files.
- The detached host receives only the secrets needed for a connection and keeps them in memory. Its secret entries redact values from debug output.
- AI provider API keys use the OS keychain through the `keyring` crate in `labonair-ai`. That crate's user-facing AI frontend is currently parked.
- Local and detached terminal scrollback may be persisted as compressed ANSI text under the app data directory. Compression is not encryption, and output is not automatically scrubbed for secrets.

Treat the app data directory and terminal output as sensitive. Never commit secrets or include them in logs, notifications, fixtures, documentation, or screenshots. Synthetic values belong in tests.

## Update integrity

The macOS updater verifies downloaded artifacts against a compiled-in minisign public key before applying them. Missing or invalid keys and signatures fail closed; a failed bundle swap attempts to restore the previous app. The corresponding private signing key and password belong in release CI secrets and must never be committed. macOS code signing and notarization are optional packaging steps, so a release can be unsigned when those credentials are not configured. The updater is macOS-only.

## AI tool guards

The AI backend has path and command guards in [AI tool security](../crates/ai/src/tools/security.rs), but the AI frontend is parked and the backend is not currently wired into the app shell. These checks do not apply to MCP tools.

The path checks reject a set of secret-like names and directories. Resolved-path variants canonicalize paths that exist; if a target cannot be canonicalized, only the literal path is checked. The checks are defense-in-depth, not a filesystem sandbox.

`check_destructive_command` returns warning labels for patterns such as recursive deletion, SQL drops, hard Git resets, force pushes, and `chmod 777`; it does not itself block them. The separate `check_shell_command` uses string heuristics to reject a smaller set of commands, including recursive deletion of the filesystem root, disk-formatting commands, writes to block devices, and commands referencing guarded secret paths. It is not a shell parser and can be bypassed by shell syntax. Mutating AI tools are designed to require user approval if that frontend is restored.

## Detached session host and remote access

When enabled, the detached host uses a private per-user data directory (`0700` on Unix), a Unix-domain socket (`0600`), and a peer-UID check. It does not accept TCP connections or support remote pairing. These controls protect the local IPC boundary between processes running as different OS users; they do not isolate processes running as the same user.

Workspace leases fence stale writes between app instances. They do not provide a confidentiality boundary between those instances or between applications under the same user account. The host can persist terminal scrollback for session recovery, as described above.

SSH host-key prompts and transfer conflict decisions are tied to the owning connection. Current protocol code checks the prompt owner and rejects a mismatched request ID; v2 transfer resolution requires a request ID. A resolved prompt is removed, and disconnecting its owner clears or rejects pending decisions. Remote safety still depends on explicit user trust decisions and correctly scoped host configuration.

## Project trust

There is no general Workspace Trust or Restricted Mode today. Project settings are treated as untrusted input in [project settings parser](../crates/settings/src/project.rs), but there is no typed trust gate covering all project-controlled settings, language services, Git, agent tools, and other integrations. Opening a repository does not sandbox its files or tools.

## Security-sensitive changes

Changes to MCP authorization, secret handling, remote authentication, detached-host IPC, or project trust should update this document and include both allowed and denied cases at the owning contract boundary. In particular, MCP server authentication and grant-policy integration tests remain an open gap.
