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
| `GET /api/auth/mode` | Discover the exclusive server login mode |
| `POST /api/auth/oidc/start`, `POST /api/auth/oidc/poll` | Start/complete an OIDC desktop handoff |
| `GET /api/auth/oidc/authorize/{id}`, `POST /api/auth/oidc/callback` | Browser challenge and verified provider callback |
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
| `GET/PUT /api/account/vault` | Read/update the caller's wrapped account key |
| `GET/PUT /api/projects/{id}/values` | Read/update shared ordinary values |
| `GET/PUT /api/projects/{id}/vault-values` | Read/update the caller's encrypted secret bundle |
| `GET/PUT/DELETE /api/projects/{id}/secrets/...` | Access only the caller's encrypted values |

Authenticated calls use bearer tokens. Project replacement requires `If-Match` with the
current numeric revision. The database compares and increments that revision in one conditional
UPDATE; simultaneous writers cannot both replace the same revision. Stale writes return HTTP 409
and are never silently applied. Missing preconditions return HTTP 428.
Snapshot payloads use `{ project, collections, environments }`; the server checks project
identity, top-level shape and size before PostgreSQL stores the JSON document.

Takeover is idempotency-by-identity: project UUIDs cannot be created twice. The client keeps the
original local files after a successful upload. Server projects use a separate local workspace;
that workspace is retained for offline read-only use and is not a destructive migration.

## Shared values and encrypted secrets

The desktop now synchronizes ordinary values separately from project snapshots and encrypts
personal secrets in Rust before upload. Local-to-server transfer has an explicit selection,
preparation and final upload preview. See [SERVER_VALUES.md](SERVER_VALUES.md) for the UI flow,
revision contract, cryptographic format, cache ownership and recovery limitations.

The legacy `/secrets/...` envelope endpoints remain available for compatibility. The desktop
uses the new account vault and per-project value bundles; legacy envelopes are not automatically
converted. Neither API derives keys or decrypts secrets.

## Account modes and current scope

Local account mode is currently implemented with password hashing, failed-login lockout,
server-admin bootstrap configuration, pending registration approval and account suspension.
OIDC is implemented as an alternative server mode. The provider client secret stays server-side;
Authorization Code with PKCE runs in the system browser, identities map by exact issuer/subject,
and new users require admin approval. Only the configured bootstrap subject becomes the initial
administrator. Local login/register routes are disabled in OIDC mode. See [OIDC.md](OIDC.md)
for setup, the desktop handoff, cancellation, mode-switch limits and validation boundaries.

The current implementation supports local-account and OIDC server modes. The desktop can configure the
server URL, register/sign in, list accessible projects, manage team membership when authorized as
a project administrator, create team projects and approve/disable accounts as a server admin, and
upload a saved local project as a new personal server copy. Permitted online projects with write
access open in a separate local workspace. Every saved change there (requests, collections,
folders, moves, project auth/name, variable/environment definitions, imports) is published
automatically as a snapshot with an `If-Match` revision check; publishes are serialized. A failed
publish (offline, revision conflict, attachments) keeps the local save, shows the reason and is
retried by the next save or by **Sync saved changes** under Server workspaces. A revision conflict
preserves local edits and rejects the sync; use Load server version to continue. Each publish
uploads the complete snapshot. Offline snapshots and restored server workspaces are read-only. Immutable snapshots are
cached by revision; authorization failures never fall back to stale project data. Upload and
workspace snapshot sync omit request history, reusable values and file attachment content;
transfer includes only explicitly selected ordinary values and client-encrypted secrets; local file attachments block these operations to avoid broken references.
Request history remains local by design.

Variable definitions carry an optional `secret` classification. New variables default to
secret; older files without the field remain secret. Local-mode values stay in the OS store.
Server ordinary values are shared project data; secret values belong to the caller and require
an unlocked account vault. Existing local values never become shared through classification alone.

## Account isolation and workspace recovery

The desktop verifies `/api/account` after login and keeps the returned account UUID with the
access token in a version-2 OS credential entry. Tokens are bound to their configured server
URL. Cache directories and active-workspace pointers are scoped by a SHA-256 digest of server
URL and account UUID. Every native project-handle operation verifies the same owner, including
restoration through the ordinary folder chooser. Variables, acquired tokens, local history and
environment selections also have account-specific namespaces. Local-only projects keep their
existing identities and storage keys. No local OS values are automatically copied into a server
account's namespace.

Close all server projects before signing out or changing server accounts in the UI. This keeps
unsaved-change protection in the normal project-close workflow. Native account changes also
invalidate access through previously issued handles. Logout preserves saved files but does not
allow another account to reuse them. Version-1 session entries and server caches without an
account owner are not adopted automatically; sign in again and open projects from Server
connection. Legacy cache/workspace files remain on disk for manual recovery of saved edits.

After a sync conflict, choose **Load server version**, then **Keep recovery copy and load**.
Save or close unsaved editors first. Fluxkit fetches an authorized current snapshot and creates
a separate workspace generation, retaining the previous saved files as a read-only recovery
project. Both projects appear in the workspace. Copy desired changes manually into the new
workspace and use **Sync saved changes** again. This is an explicit manual merge, not an
automatic merge or a force-overwrite of server data. Local edits made while fetching abort the
refresh. Authorization failures and unavailable servers preserve the existing workspace.

Active-workspace selection is an atomic pointer to an independently staged generation. Snapshot
cache revisions are also staged before becoming discoverable. A failed operation can leave a
retained generation, but does not overwrite previous saved edits. If a sync reached the server
but its response or local revision update failed, the next explicit sync compares parsed JSON
snapshots. Identical content acknowledges the current server revision without repeating PUT;
different content at a newer revision requires the recovery flow. Offline access prefers a saved
workspace in read-only mode, preserving visibility of unsynced edits; only when none exists does
it use the latest cached server snapshot. Recovery projects are explicitly labelled in the
project picker and restore as recovery copies after restart. There is no automatic request
replay or offline write queue.

## Deployment

See [server/README.md](../server/README.md) and [server/docker-compose.yml](../server/docker-compose.yml).
The Compose service binds to loopback; operators must configure TLS at a reverse proxy before
remote access. Database backups and restoration remain the operator's responsibility.

## Handoff status (2026-10-27)

The current branch contains an implementation foundation, not a complete backend release. To
continue, work through these items in order and keep this section and the Milestone 10 status in
sync:

Implemented in this slice: shared-value ownership and revision checks, explicit transfer
preview, master-password setup/unlock/lock/change, authenticated encryption, account-key wrapping
and per-user secret synchronization. OIDC browser sign-in, stable identity mapping and approval
are also implemented. Existing local/OIDC account linking remains an explicit future migration
feature; no automatic email/name linking is performed. Validation is recorded in [VALIDATION.md](VALIDATION.md).

Remaining work:

1. **Extend platform validation.** Complete signed-in GUI workflows and OS-store integration on
   each supported platform; automatic conflict merge remains outside this slice.
2. **Review deployment/security.** Validate operator TLS/proxy configuration, backup/restore,
   upgrades and deployment load before calling the backend release complete. The isolated
   [real-provider HTTPS E2E suite](../server/e2e/README.md) now covers Keycloak, proxy, database,
   approval/access/revisions and native encrypted values; it does not validate each operator's
   production setup. Regression tests do not replace an independent security review.

Historical validation before account-isolation/recovery changes: `dotnet build server/Fluxkit.Server/Fluxkit.Server.csproj
--no-restore` succeeded with zero warnings and errors; `git diff --check` succeeded. Rust and
frontend checks were not run because `cargo`, `npm`, and `rustfmt` were unavailable in that
environment. Re-run checks on the next machine; do not treat the server build as validation of the
desktop client.

Current regression results and their platform limits are recorded in [VALIDATION.md](VALIDATION.md).
