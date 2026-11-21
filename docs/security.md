# Security

## Automation and Remote Threat Model

**Related:** [`trust-boundaries.md`](security.md), [`secrets.md`](security.md), [`remote-access.md`](security.md), [`../automation/tools.toml`](automation.md)

This threat model covers the local MCP bridge, AI/agent access, SSH/SFTP
sessions, credentials, and filesystem-adjacent feature integrations.

### Assets

- bearer tokens and stored credentials;
- private keys, passwords, host metadata, and session identifiers;
- local filesystem content and visible terminal input/output;
- remote hosts, sessions, tunnels, and transfer targets;
- user trust that agent actions are visible, scoped, and revocable.

### Actors and boundaries

| Actor | Trust level | Boundary |
|---|---|---|
| User | Decision authority | Grants/revokes tab access and chooses hosts/actions. |
| Labonair UI | Trusted application process | Owns capability state and presents visible actions. |
| MCP client/agent | Untrusted-by-default caller | Must authenticate and hold explicit session grants. |
| Local bridge | Narrow privileged adapter | Loopback bearer endpoint; rechecks policy per tool call. |
| SSH/SFTP remote | External/untrusted system | Reached only through typed transport contracts and host policy. |
| Secret store | Sensitive platform boundary | Returns credential material only to the narrow authentication path. |

### Threats and controls

| Threat | Required control | Residual evidence |
|---|---|---|
| Unauthenticated tool call | Disabled-by-default bridge, loopback binding, bearer token. | Negative authentication test required. |
| Stale tab grant | Recheck current grant and host block flag at execution time. | Grant expiry and blocked-host tests. |
| Agent reaches blocked host | `block_agent_access` denies open and active calls; revokes existing grants. | Host/MCP integration test. |
| Secret exfiltration | No raw secrets in tool schema/output; secret files blocked by AI path guards. | Secret redaction and path-negative tests. |
| Hidden command execution | Commands are typed into the visible granted terminal. | Visible-terminal integration evidence. |
| Runaway command | Per-session lock and bounded timeout cap; partial result is explicit. | Timeout/cancellation tests. |
| Remote path or host confusion | Saved-host identity and typed SSH/SFTP contracts; no arbitrary tool host input. | Host/transport contract tests. |
| Log/notification leakage | Structured details and source; never record credentials/tokens. | Review and redaction tests. |

The bridge is not a general sandbox. The user grant, host policy, secret
boundary, visible terminal, and explicit denial behavior are the safety model;
any stronger isolation claim requires a separate architecture decision.

## Trust Boundaries

**Related:** [`threat-model.md`](security.md), [`../automation/grants.md`](automation.md), [`../architecture/composition-root.md`](architecture.md)

The following boundaries must remain explicit in code and documentation:

```text
user decision
    ├─ project/worktree trust decision
    │      ↓ typed Workspace trust state
    │   project settings / Editor language services / MCP integrations
    └─ explicit tab/host grant
           ↓
        MCP loopback + bearer token
           ↓ typed MCP contracts/events
        composition root adapters
           ↓ owner contracts
        workspace / hosts / SSH / SFTP / terminal capabilities
           ↓ narrow secret lookup
        secret store and remote transport
```

- `labonair-mcp-core` owns wire values and narrow service contracts.
- `labonair-mcp-server` owns HTTP tools, grants, timeouts, and event adapters.
- `labonair-shell` composes concrete state into `McpServerAccess`.
- Workspace mirrors grants for visible UI; it does not own the authoritative
  grant map.
- Hosts owns host policy and `block_agent_access`; SSH/SFTP own transport.
- Secrets owns keychain/secret material and is never a general data source.

### Project and worktree trust

- Workspace (`labonair-workspace`) owns the Restricted Mode decision,
  path-scoped trust policy, persisted trust state, reset behavior, and
  user-facing recovery. Settings stores values only; the global auto-trust
  preference defaults to false, and Workspace owns its effect.
- Untrusted project and single-file roots start restricted. Trust requires an
  explicit user decision for the selected path or validated parent scope. Local
  path trust and remote-host trust are isolated identities.
- Project trust does not grant SSH host-key acceptance or MCP tab access. Each
  remains a separate user decision with its existing owner and typed contract.
- Project settings, Editor language services, Agent, Git, and MCP integrations
  query a narrow typed Workspace trust contract before project-controlled
  settings, skills, tools, repository workers, or services take effect.
  Consumers do not read Settings, a shell trust table, or Workspace private
  state directly.
- A failed trust write must not be presented as durable. Report persistence
  failures through the notification center, provide retry or recovery, and
  return an unpersisted path to Restricted Mode when it is reopened.

### Detached Host clients and decision prompts

- The Detached Host remains local to one OS user. Its private data directory,
  Unix socket permissions, and peer-UID check are the trust boundary; the
  multi-instance protocol does not add TCP or remote pairing.
- Shared Workspace leases fence revisions and releases by Workspace instance
  ID and lease generation. A stale or expired lease cannot mutate a Space or
  release a later owner's claim.
- Host-key and transfer decisions are bound to the socket connection that
  started the operation. V2 responses must carry the current one-use prompt
  ID; the host rejects stale and wrong-owner responses and serializes
  concurrent answers. V1 transfer responses may omit the ID but remain
  connection-fenced and one-use. Other v1 clients do not receive another
  client's conflict prompt.
- A parked transfer decision may be claimed by another v2 client only after
  the original connection releases its owner lease. The host issues a new
  prompt ID on takeover and exposes only read-only state to the other clients.
- Workspace persistence stores no credentials or secret material. Detached
  SSH secrets stay in memory in the host and are not written to SQLite or
  event logs.

An implementation change crossing one of these boundaries requires an owner,
typed contract, negative-test decision, and scorecard evidence update.

## Secret Handling

**Related:** [`threat-model.md`](security.md), [`remote-access.md`](security.md), [`../automation/tools.toml`](automation.md)

Secrets include passwords, private keys, bearer tokens, keychain references,
credential payloads, and sensitive host/session values.

### Rules

- Store secret material only through `labonair-secrets` and the credential
  owner; host records may store references and metadata, not raw secrets.
- Never place secrets in source, logs, notifications, SQLite fixtures, tests,
  generated documentation, screenshots, or commits.
- MCP tools may check that a credential exists, but must not return its value.
- Tool schemas must use host/session IDs and typed references instead of raw
  password/key parameters.
- AI path guards reject obvious secret filenames/directories and protected
  system paths; symlink-aware checks are required where a path is resolved.
- Error details must be useful without echoing command input or credential
  material.
- Test fixtures use synthetic credentials and must be redacted before sharing
  artifacts.

The bearer token and update-signing private key are operator/CI secrets and do
not belong in repository documentation. Only their storage contract and
redaction behavior are documented here.

## Remote Access Safety

**Related:** [`threat-model.md`](security.md), [`secrets.md`](security.md), [`../capabilities.md`](modules.md), [`../audits/remaining-boundaries.md`](archive/audits/remaining-boundaries.md)

Hosts owns saved-host identity and policy. SSH/SFTP own connection and session
execution. Transfers, snippets, Explorer, Workspace, and MCP consume typed
contracts or composition adapters; they must not reach into transport-private
state.

### Required behavior

- Use stable host IDs and typed `HostOpenRequest`/session contracts.
- Keep jump hosts as SSH connection configuration, not a duplicate product
  surface or status item.
- Keep SFTP browsing and transfer lifecycle separate from SSH transport
  implementation details.
- Reject blocked-agent hosts for MCP open and active tool calls.
- Treat connection, trust, authentication, timeout, and transfer failures as
  structured owner notifications with safe details.
- Make cancellation, reconnect, and stale-session behavior explicit in the
  owning task and tests.
- Never claim remote safety from a compile-only check; include allowed and
  denied integration cases.

The Explorer remote-directory contract is the current example of the desired
boundary: the panel consumes `RemoteExplorerService`, while the shell injects
the SFTP adapter.
