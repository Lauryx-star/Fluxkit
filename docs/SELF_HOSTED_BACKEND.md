# Self-hosted backend

Fluxkit remains usable in local mode without a server. The server is a separate ASP.NET Core
10 service backed by PostgreSQL. It stores each project as normalized rows (collections,
folders, requests, environments, variables and values), each with its own revision, and never
receives request history. The desktop keeps a read-only local copy for offline access; local
history remains separate.

## Ownership and access

- Every approved account receives one personal project, visible only to its owner.
- Team projects have no private collections. All collections in a team project inherit the
  project's member access rules.
- A server administrator approves registrations and creates team projects. A project
  administrator manages that team's memberships. A member can read and modify project data.
- Ordinary variable values are project data. A secret value is stored separately as an
  opaque client-encrypted value scoped by `(user, variable, environment)`.
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
| `DELETE /api/admin/users/{id}` | Delete an account with its personal project, vault and encrypted values |
| `GET /api/projects` | List only owned or member projects, with creation date and collection count |
| `POST /api/projects` | Create a team project (server admin), optionally with an initial tree and values |
| `DELETE /api/projects/{id}` | Delete an empty team project as server admin, `If-Match` project revision |
| `GET /api/projects/{id}` | Read a permitted project's metadata |
| `GET /api/projects/{id}/tree` | Read the whole project as rows, with the current feed position |
| `GET /api/projects/{id}/changes?since=` | Read what changed after a feed position |
| `POST /api/projects/{id}/changes` | Apply a batch of row changes atomically |
| `GET /api/projects/{id}/members` | List team members as a project admin |
| `POST /api/projects/{id}/members` | Add an approved user with a project role |
| `PUT /api/projects/{id}/members/{userId}` | Change a member's project role |
| `DELETE /api/projects/{id}/members/{userId}` | Remove a member; preserve that user's encrypted values |
| `GET/PUT /api/account/vault` | Read/update the caller's wrapped account key |
| `GET /api/projects/{id}/values` | Read shared ordinary values with per-value revisions |
| `PUT/DELETE /api/projects/{id}/values/{variable}/{environment}` | Write one shared value, `If-Match` its revision |
| `GET /api/projects/{id}/vault-values` | Read the caller's encrypted secret values |
| `PUT/DELETE /api/projects/{id}/vault-values/{variable}/{environment}` | Write one of the caller's encrypted values |
| `POST /api/projects/{id}/values/batch` | Write many shared and own encrypted values atomically (imports) |

Authenticated calls use bearer tokens.

### Rows, revisions and the change feed

Every collection, folder, request, environment and variable is one row with its own numeric
revision; the project row holds its name and authentication. Order is a sortable `position`
string. A batch is a list of `put`/`delete` operations; every operation on an existing row
carries its `expectedRevision`. The server applies a batch all-or-nothing in one transaction:
when any row changed meanwhile, nothing is written and HTTP 409 lists each conflict with
`kind`, `id`, current `revision` (absent when deleted), `updatedAt` and `updatedBy`. Edits of
different rows by different members therefore never conflict. A delete removes the row and
everything below it (folders, requests, values); delete wins over concurrent edits of children.
The server checks the tree: parents in the same collection, no folder cycles, unique variable
and environment names, size and count limits.

Every applied change appends to the project's change feed with who and when (not the data).
`GET /changes?since=n` returns the latest state of every row changed after `n` plus the new
feed position; a client too far behind (or with an invalid position) gets HTTP 410 and reloads
the tree. Values use per-slot revisions the same way: `If-Match: 0` creates, a stale revision
returns 409, a missing precondition 428. Stale writes are never silently applied.

Uploading a local project creates a new server project in one request: the tree is applied as
put operations and the selected values are stored in the same transaction. The client keeps the
original local files. Server projects use a separate local workspace that is retained for
offline read-only use.

Only server administrators delete projects, and only empty ones (no collections, environments
or variables). The client closes open copies first and removes its local copies afterwards.

## Shared values and encrypted secrets

The desktop synchronizes ordinary values per value slot and encrypts personal secrets in Rust
before upload. Local-to-server transfer has an explicit selection,
preparation and final upload preview. See [SERVER_VALUES.md](SERVER_VALUES.md) for the UI flow,
revision contract, cryptographic format, cache ownership and recovery limitations.

The server never derives keys or decrypts secrets.

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
upload a saved local project as a new server copy. Permitted online projects with write access
open in a separate local workspace. Every change there (requests, collections, folders, moves,
project auth/name, variable/environment definitions, imports) is written to the server as it is
saved: the desktop compares the saved files with the last known server state, sends only the
changed rows with their revisions, and keeps the local save only when the server accepted it.
A rejected change rolls the workspace back to the last known server state and is reported with
what changed and by whom. Other members' changes are loaded from the change feed every 15
seconds, when the window regains focus, and on **Refresh from server**. Offline copies are
read-only. Request history and file attachment content are never uploaded; local file
attachments block upload and saves to avoid broken references.
Request history remains local by design.

Variable definitions carry an optional `secret` classification. New variables default to
secret; older files without the field remain secret. Local-mode values stay in the OS store.
Server ordinary values are shared project data; secret values belong to the caller and require
an unlocked account vault. Existing local values never become shared through classification alone.

## Managing accounts

Server administrators open **Settings → Server users** (also linked from the Administration step
of Server connection). It lists every registered account with status and registration date,
filterable by status and username, and approves pending accounts, disables or reactivates them,
or deletes them. Deleting asks for the username and permanently removes the account, its personal
project with all rows and values, its account vault, its encrypted values in every project and
its team memberships. Team projects and their content remain; rows the account last changed show
no author name afterwards. Administrators cannot disable or delete their own account, and at
least one active server administrator always remains. A deleted OIDC identity that signs in
again becomes a new pending account. The local bootstrap administrator configured on the server
is recreated at the next start if it was deleted.

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

### Concurrent edits in the desktop

Open request editors follow the server automatically while they are unedited. When a request
with unsaved edits changes on the server, the editor keeps the edits, shows a notice and asks
before saving over the newer server version; a request deleted on the server can be saved again
to recreate it. When a save is rejected because the server changed in the meantime, a dialog
offers **Load server version** (discard the edits in that request) or **Keep my version**
(load the server state, then save the edits on top of it). There is no offline write queue and
no automatic request replay.

The workspace keeps the last known server tree in `.fluxkit/server-tree.json`; the regular
project files are rendered from it and are overwritten when the server state is loaded.
Read-only copies are staged per feed position before becoming discoverable, so an open copy is
never rewritten underneath its viewer.

## Deployment

See [server/README.md](../server/README.md) and [server/docker-compose.yml](../server/docker-compose.yml).
The Compose service binds to loopback; operators must configure TLS at a reverse proxy before
remote access. Database backups and restoration remain the operator's responsibility.

Prometheus metrics (runtime, HTTP per route template and aggregated usage statistics) are served
at `/metrics` without login; the reverse proxy must restrict that route to the Prometheus
hosts. See the Metrics section of [server/README.md](../server/README.md). The metrics carry no
user or project names, identifiers or values.

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
   each supported platform; field-level merging of one request edited by two people remains
   outside this slice (the second save asks before overwriting).
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
