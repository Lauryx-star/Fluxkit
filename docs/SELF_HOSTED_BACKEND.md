# Self-hosted backend

Fluxkit remains usable in local mode without a server. The server is a separate ASP.NET Core
10 service backed by PostgreSQL. It stores project snapshots and never receives request
history. Client migration keeps a read-only local snapshot for offline access; local history
remains separate.

## Ownership and access

- Every approved account receives one personal project, visible only to its owner.
- Team projects have no private collections. All collections in a team project inherit the
  project's member access rules.
- A server administrator approves registrations and creates team projects. A project
  administrator manages that team's memberships. A member can read and modify project data.
- Ordinary variable values are project data. A secret value is stored separately as an
  opaque envelope scoped by `(project, user, variable, environment)`.
- Server administrators and other team members do not get a secret decryption key.

The API checks active account status on each authenticated request. Personal-project access
requires ownership. Team-project access requires a membership. Server administration does not
implicitly grant access to a user's personal project.

## API contract

The initial API exposes:

| Method and path | Purpose |
| --- | --- |
| `POST /api/auth/register` | Create a pending local account |
| `POST /api/auth/login` | Issue an access token for an approved account |
| `GET /api/account` | Read the caller's username and server-admin status |
| `GET /api/admin/pending-users` | List accounts awaiting approval |
| `POST /api/admin/users/{id}/approve` | Approve an account and create its personal project |
| `GET /api/admin/users` | List user status and server-admin flag |
| `PUT /api/admin/users/{id}/status` | Disable or re-enable an approved account |
| `GET /api/projects` | List only owned or member projects |
| `POST /api/projects` | Create a blank team project as server admin |
| `GET /api/projects/{id}` | Read a permitted project snapshot |
| `POST /api/projects/{id}/takeover` | Create a project and its own encrypted-secret envelopes |
| `PUT /api/projects/{id}` | Replace a snapshot using `If-Match` revision |
| `GET /api/projects/{id}/members` | List team members as a project admin |
| `POST /api/projects/{id}/members` | Add an approved user with a project role |
| `PUT /api/projects/{id}/members/{userId}` | Change a member's project role |
| `DELETE /api/projects/{id}/members/{userId}` | Remove a member; preserve that user's secret envelopes |
| `GET/PUT/DELETE /api/projects/{id}/secrets/...` | Access only the caller's encrypted values |

Authenticated calls use bearer tokens. Project replacement requires `If-Match` with the
current numeric revision; stale writes return a conflict and are never silently applied.
Snapshot payloads use `{ project, collections, environments }`; the server checks project
identity, top-level shape and size before PostgreSQL stores the JSON document.

Takeover is idempotency-by-identity: project UUIDs cannot be created twice. The client keeps the
original local files after a successful upload. Server projects use a separate local workspace;
that workspace is retained for offline read-only use and is not a destructive migration.

## Secret envelope

The API accepts only the `AES-256-GCM` envelope label with base64 salt, ciphertext, wrapped
data key and separate nonces. It bounds field sizes and permits version-19 Argon2id parameters
with 64 to 256 MiB memory, 3 to 10 iterations and 1 to 4 lanes. The server does not derive keys or
decrypt values. The master password, derived key and plaintext are never API fields. The client
must use project, user, variable and environment identities as authenticated associated data.
Changing the master password rewraps the user's data key on the client. A forgotten master
password without a separately designed recovery mechanism permanently removes access to that
user's synchronized secrets.

The cryptographic envelope contract is a server-side validation boundary only at this stage;
the client does not yet create or unlock it. The eventual client flow must use random salt and
nonces, wipe transient key buffers where practical, and never persist the master password or
derived keys in settings, browser storage, project files, diagnostics or history.

## Account modes and current scope

Local account mode is currently implemented with password hashing, failed-login lockout,
server-admin bootstrap configuration, pending registration approval and account suspension.
OIDC is planned as an
alternative server mode, not a simultaneous login method. It must use server-held provider
credentials, authorization code with PKCE, stable `(issuer, subject)` identity mapping, and
admin approval before project access.

The current implementation supports the local-account server mode. The desktop can configure the
server URL, register/sign in, list accessible projects, manage team membership when authorized as
a project administrator, create team projects and approve/disable accounts as a server admin, and
upload a saved local project as a new personal server copy. Permitted online projects with write
access open in a separate local workspace; an explicit Sync saved changes operation publishes its
snapshot with an `If-Match` revision check. A revision conflict preserves local edits and rejects
the sync. Offline snapshots and restored server workspaces are read-only. Immutable snapshots are
cached by revision; authorization failures never fall back to stale project data. Upload and
workspace sync omit request history, OS credential-store values, encrypted secret envelopes and
file attachment content; local file attachments block these operations to avoid broken references.
Request history remains local by design.

Variable definitions now carry an optional `secret` classification. New variables default to
secret; older project files without the field are interpreted as secret. This preserves the
existing local-only handling of their OS credential values. The classification is metadata only
at this stage: ordinary values are still local and are not synchronized. Remaining work includes
OIDC login and identity linking, local-to-server transfer preview, synchronization of ordinary
variable values, client-side secret encryption/unlocking and envelope synchronization, and robust
workspace refresh/recovery after a server-side revision conflict.

## Deployment

See [server/README.md](../server/README.md) and [server/docker-compose.yml](../server/docker-compose.yml).
The Compose service binds to loopback; operators must configure TLS at a reverse proxy before
remote access. Database backups and restoration remain the operator's responsibility.

## Handoff status (2026-10-27)

The current branch contains an implementation foundation, not a complete backend release. To
continue, work through these items in order and keep this section and the Milestone 10 status in
sync:

1. **Complete ordinary-value ownership and sync.** Add a server-side shared-value model and
   authorized API, client resolution/update flows, transfer preview, and conflict handling. Do
   not include OS credential-store values in project snapshot uploads. Existing variables without
   a classification must stay secret.
2. **Complete secret encryption on the client.** Implement master-password setup/unlock, secure
   KDF parameter handling, authenticated encryption and key wrapping, per-user envelope upload
   and download, password change, and lock/clear behavior. Keep plaintext and keys out of
   browser persistence, logs, snapshots, and history; test the wrong-password and forgotten
   password cases.
3. **Add OIDC as an exclusive server authentication mode.** Keep provider credentials on the
   server, map identities by issuer and subject, and require administrator approval before data
   access.
4. **Finish server-workspace recovery.** Provide a clear refresh/merge/retry path after revision
   conflicts and cover interrupted sync and stale offline snapshots.
5. **Validate and review deployment/security.** Run the documented Rust and frontend checks,
   server build and tests, and an end-to-end local Compose workflow. Review TLS/proxy guidance,
   secret handling, authorization boundaries, database migrations, backup/restore, and upgrade
   behavior before calling the feature complete.

The variable `secret` field is currently optional for compatibility. New variables default to
secret; absent fields are interpreted as secret. The current client still stores all variable
values in the operating system credential store, and neither ordinary values nor secret
envelopes are synchronized yet. The server's secret endpoints validate/store opaque envelopes,
but do not implement client cryptography.

Last recorded validation for this handoff: `dotnet build server/Fluxkit.Server/Fluxkit.Server.csproj
--no-restore` succeeded with zero warnings and errors; `git diff --check` succeeded. Rust and
frontend checks were not run because `cargo`, `npm`, and `rustfmt` were unavailable in that
environment. Re-run checks on the next machine; do not treat the server build as validation of the
desktop client.
