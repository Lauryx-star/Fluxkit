# Fluxkit implementation milestones

The first release delivers the scope in [the product plan](PRODUCT.md). Milestones are
implementation order, not permission to omit later confirmed features. Milestones 1 through 5 have
been implemented and validated on macOS; see [validation results](VALIDATION.md) for the
exact native and automated checks and their limits. Milestones 6 and 7 are implemented with Linux
validation recorded there; milestone 8 is in progress and milestone 9 remains planned.

## Milestone 1 Desktop foundation

Deliver a Rust and Tauri desktop application with React and TypeScript, bundled local UI,
and documented development commands. Establish strict checks, tests, and lockfiles.

Acceptance criteria:

- The desktop window launches on the development host without a Fluxkit account or backend.
- The application has navigation, a main editor area, and a response area.
- Frontend and native validation commands are documented and pass on the available host.
- Platform-specific prerequisites and unverified platform checks are explicit.

## Milestone 2 Projects and saved requests

Status: complete on macOS. Storage and recovery are defined in [FILE_FORMAT.md](FILE_FORMAT.md).

Define versioned JSON files, folder layout, stable identifiers, and save semantics before
implementing project, collection, nested folder, and request editing.

Acceptance criteria:

- A user can create, open, rename, edit, and remove the supported entities.
- Saved data survives restart and round-trips without losing supported fields.
- Query parameters, headers, text, JSON, forms, and file-body references can be edited.
- Invalid files, external edits, and failed writes do not silently destroy user data.
- Personal settings remain outside portable project files.

## Milestone 3 Environments and local values

Status: complete on macOS. Syntax, storage, selection, and deletion recovery are defined
in [ENVIRONMENTS.md](ENVIRONMENTS.md). Windows and Linux integration remain unverified.

Define variable syntax, value precedence, credential ownership, and selection persistence.
Implement project-wide environments and OS credential-store access before authentication.

Acceptance criteria:

- Changing the environment affects every collection in that project and no other project.
- Reusable values, including non-password variables, are stored only in the OS store.
- Missing or inaccessible values produce useful errors without plaintext fallback.
- Renaming entities does not break reference identity.
- Routine tests use isolated folders and fake credential adapters.

## Milestone 4 HTTP execution and responses

Status: complete on macOS. The network, timing, preview, and download contracts are
defined in [HTTP.md](HTTP.md). Native smoke checks used an isolated application identity
and controlled HTTP/HTTPS servers. Other platforms remain unverified.

Agree on timing, timeouts, redirects, cookies, proxy/certificate scope, and preview limits.
Implement native HTTP execution, cancellation, response views, and downloads.

Acceptance criteria:

- Requests reach a controlled local test server with expected methods, headers, and bodies.
- Values resolve for the selected environment before dispatch; missing values block sending.
- Status, timing, headers, body size, JSON/text, and empty responses display correctly.
- Binary downloads can be saved without requiring full-file buffering.
- Timeouts, cancellation, and transport errors have distinct visible outcomes.
- Large-response work does not block interaction; credentials are not leaked on redirects.

## Milestone 5 Authentication and inheritance

Status: complete on macOS. See [AUTHENTICATION.md](AUTHENTICATION.md) for storage,
reference ownership, inheritance, and native execution. Manual fields take priority over
configured authentication. Native smoke checks used an isolated app and local server.
Windows/Linux integration remains unverified.

Implement None, Inherit, Basic, Bearer, and API Key.

Acceptance criteria:

- Project, collection, nested folder, and request settings resolve consistently.
- An explicit None stops inherited authentication for that branch.
- The editor identifies the effective method and its source without revealing values.
- Token and credential references remain isolated by configuration and environment.
- Actual request execution and redaction are covered by meaningful tests.

## Milestone 6 Token acquisition and Custom Login

Status: implemented. Configuration, OS-token ownership, expiry, renewal, failure recovery,
and compatibility are defined in [TOKEN_AUTHENTICATION.md](TOKEN_AUTHENTICATION.md).
See [validation](VALIDATION.md) for executed Linux checks and remaining platform limits.

Implement OAuth2 Client Credentials and the bounded Custom Login scope. No browser login
or callback listener is part of this milestone.

Acceptance criteria:

- Client Credentials acquires tokens and reacquires them using known expiry information.
- Custom Login supports configurable input composition and extraction from text, JSON,
  or headers, with optional separate renewal.
- Unknown expiry remains visibly unknown; optional configured lifetime is respected.
- Parallel requests share token acquisition for the same configuration and environment.
- Failed extraction does not replace a usable stored token with invalid data.
- Login cannot recursively invoke itself and failed API calls are not blindly replayed.
- Token values remain outside files, diagnostics, and history.

## Milestone 7 Minimal history

Status: implemented. [HISTORY.md](HISTORY.md) defines the approved literal-host policy,
redaction, local storage, controls, retention, and failure recovery. See [validation](VALIDATION.md)
for executed Linux checks and remaining platform limits.

Use four-field history with limits of 100 entries per project and seven days,
persisted across restarts.

Acceptance criteria:

- Entries show safe URL, method, HTTP status when available, and response time.
- No resolved credentials, reusable values, bodies, or headers enter history storage.
- Failed/cancelled attempts do not receive invented HTTP status codes.
- Background token-acquisition calls are excluded.
- History survives restarts and enforces both the 100-entry and seven-day limits.
- Oldest entries are removed first; expired entries are pruned before display and on
  startup and insertion, with cleanup continuing while the app is open.
- Recording starts disabled and history can be cleared per project. Close project
  preserves history; deletion requires explicit Clear history. Disabling recording does not prevent expiration or clearing.
- An internal UTC timestamp supports expiration without storing additional request details.
- Retention tests control time deterministically and cover count and age boundaries.

## Milestone 8 Migration and exchange

Status: in progress. The first OpenAPI 3.0.x/3.1.x YAML/JSON import slice is implemented;
[OPENAPI_IMPORT.md](OPENAPI_IMPORT.md) defines mappings, compatibility blockers, internal-only
references, metadata preservation, and failure recovery. Postman Collection v2.1 and associated
environment JSON import are implemented; [POSTMAN_IMPORT.md](POSTMAN_IMPORT.md) defines their
scope mappings and limits. OpenAPI 3.1.0 YAML export is implemented for saved requests,
folder subtrees, collections and projects; [OPENAPI_EXPORT.md](OPENAPI_EXPORT.md) defines
privacy, validation, conflicts and write recovery. Native backup export/import remains planned.
See validation for executed platform checks.

Select supported input versions and the OpenAPI output version before implementation.
Implement OpenAPI import, Postman collection/environment import, OpenAPI YAML export,
and native Fluxkit export/import in independently reviewable slices.

Acceptance criteria:

- Fixture-based tests cover each explicitly supported format version.
- Users preview mappings, conflicts, missing values, and unsupported features before import.
- Collections, folders, requests, supported auth, and environments migrate predictably.
- Imported variable values go to the OS store; scripts never run during import.
- Unsupported versions and unresolved references receive actionable diagnostics.
- Request/folder/collection/project export produces validated OpenAPI documents.
- Duplicate operations and unsupported mappings are reported rather than silently lost.
- Native export/import preserves supported structure and configuration without secrets,
  history, or machine-specific state.

## Milestone 9 Cross-platform release validation

Validate the complete agreed workflows and desktop packages on all target platforms.

Acceptance criteria:

- macOS, Windows, and Linux each pass their applicable build and smoke checks.
- Native credential adapters are tested with isolated test entries on each target OS.
- Filesystem behavior, imports, cancellation, and downloads work on representative paths.
- Startup, memory use, and interaction responsiveness are measured with stated conditions.
- Setup, storage, migration limits, supported formats, and known limitations are documented.
- Signing, publishing, and automatic updates proceed only under their own explicit scope.

## Milestone 10 Optional self-hosted server

Status: in progress; implementation handoff and ordered remaining work are in
[SELF_HOSTED_BACKEND.md](SELF_HOSTED_BACKEND.md#handoff-status-2026-10-27). The initial
ASP.NET Core/PostgreSQL server project provides local registration and admin approval, personal
projects, team project membership, project snapshot revision checks, and per-user encrypted-secret
envelope storage. The desktop includes account-scoped caches and workspaces, explicit sync,
atomic server revision replacement, and refresh with a retained recovery copy for manual
merge/retry. Shared values, explicit local-to-server transfer preview and client-side secret
encryption with account-key wrapping are implemented; see [SERVER_VALUES.md](SERVER_VALUES.md).
OIDC browser login, issuer/subject mapping and administrator approval are implemented; see
[OIDC.md](OIDC.md). Full deployment/platform validation and explicit existing-account linking
remain open. A reproducible [Keycloak/HTTPS E2E suite](../server/e2e/README.md) validates the
real browser/provider/API path and native encrypted-value protocol. See [server setup](../server/README.md).

Acceptance criteria:

- The desktop can select local or server mode and authenticate against the configured server.
- Local account and OIDC modes are mutually exclusive server configurations; pending users
  cannot read project data before administrator approval.
- Approved users automatically receive a private personal project; team projects grant data
  access through explicit membership only.
- Team members can read and change every collection in their project; administrators manage
  memberships without gaining personal-project access.
- Local-to-server upload previews the data and creates a server copy. Original local project files
  remain available after a verified successful upload.
- The client keeps a read-only local snapshot for offline access and never queues offline
  writes. History remains local in both modes.
- Ordinary values are visible to project members. Secrets are encrypted client-side with
  Argon2id-derived wrapping keys and authenticated encryption; each user can decrypt only
  their own values. Forgotten master passwords without recovery permanently lose access.
- PostgreSQL migrations, account administration, backups, restore, and TLS deployment are
  documented and validated.
- OIDC client secrets remain server-side and identities use stable issuer/subject keys.
