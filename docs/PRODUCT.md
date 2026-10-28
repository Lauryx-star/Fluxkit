# Fluxkit product plan

Fluxkit is an open-source, local desktop API client for macOS, Windows, and Linux.
The first release focuses on responsive request editing, execution, authentication,
local project files, and migration from existing API clients.

## Confirmed foundations

- Rust and Tauri provide native application behavior.
- React and TypeScript provide the user interface.
- English is the primary language for code, documentation, and application text.
- Projects use readable JSON files and folders in a user-selected location, without a database.
- Personal application settings and machine-specific state live outside portable projects.
- Local mode needs no Fluxkit account, backend, synchronization, or remote telemetry.
- An optional, self-hosted server mode stores synchronized workspace data. There is no
  Fluxkit-hosted service or cloud dependency.
- In local mode, reusable variable values live in the OS credential store. In server mode,
  ordinary values are project data and visible to authorized members; secret values remain
  user-specific and are client-encrypted before upload. Project files contain definitions
  and references rather than OS credential values.

## Projects and navigation

The hierarchy is project, collection, nested folders, and requests. A collection usually
represents an API application. Requests can live directly in a collection or in folders.

On desktop startup, reopen the last active project if its saved files and identity remain
valid. Keep its path only in local app-data settings. An unavailable project produces a
recoverable warning; explicitly closing the last project leaves the next startup empty.
This restores saved project definitions, not unsaved request drafts or response payloads.

Environments belong to a project and apply to all its collections and requests. Each
project selects its own environment independently. The same variable can hold different
values for environments such as Testserver and Prodserver. A missing environment value
falls back to a shared default value; an explicitly empty value stays empty. Values use
`{{variableName}}` placeholders. Removing a variable or environment confirms deletion of
its associated local OS-store values. See [ENVIRONMENTS.md](ENVIRONMENTS.md) for the
implemented syntax, identity, persistence, and failure-recovery rules.

The initial interface has project and collection navigation on the left, request tabs
in the main area, and the response beneath the request editor. The active environment
and the source of inherited authentication must remain visible.

## Request editing and execution

Request changes use explicit Save or Cmd/Ctrl+S, with visible dirty markers and confirmation
before discarding edits. Project, collection, and folder structural operations save immediately.
See [local file format](FILE_FORMAT.md) for the initial storage contract and recovery.

A request has a name, URL, HTTP method, query parameters, headers, body, and authentication
configuration. Individual query parameters and headers can be enabled or disabled.
Support text, JSON, URL-encoded forms, multipart forms, and file bodies.

Resolve variables and effective authentication in Rust before dispatch. Missing required
values or inaccessible credentials prevent dispatch with a useful error. Requests are
cancellable, and blocking network, storage, or parsing work must not freeze the UI.

Initial network policy is defined in [HTTP.md](HTTP.md): a 60-second default timeout,
no automatic redirects or cookie session, direct connections, and native verified TLS.
Credentials are not forwarded to another origin because a redirect was returned.

### Response display

Show status, elapsed time, response headers, and response body size. Offer formatted JSON
and raw text views, explicit empty-response handling, and save-to-file for binary content
and downloads. Bound previews and stream large downloads rather than buffering entire
files in memory. Do not execute HTML or scripts returned by an API.

Distinguish received HTTP responses from transport errors, timeouts, and cancellation.

## Authentication

### Inheritance

Authentication follows this hierarchy:

```text
Project -> Collection -> Folder -> Nested folder -> Request
```

Each non-root level supports Inherit, None, or an explicit configuration. The nearest
explicit choice wins; None disables authentication for that branch. A project supports
None or an explicit configuration. Show the effective method and where it was configured.

Store credential values in the OS credential store. Keep token state separate for each
authentication configuration and environment. A different project must not accidentally
reuse another project's credentials or token state.

### Supported methods

- Basic authentication using username and password references.
- Bearer authentication using a manually supplied token reference.
- API key authentication with a configurable name and header or query placement.
- OAuth2 Client Credentials with token acquisition and automatic reacquisition.
- Custom Login using configurable requests and token extraction.

Browser-based OAuth login, Authorization Code with PKCE, browser callbacks, device flows,
and other OAuth grants are deferred. Phase 1 requires no OAuth callback listener.
Do not embed a shared Fluxkit client secret in the distributed application. Client
Credentials uses the credentials supplied by the user for their target API.

### OAuth2 Client Credentials

Configure the token endpoint, client identity, credential references, scopes where needed,
and the provider's client authentication method. Store acquired tokens in the OS
credential store. Use expiry metadata to acquire a replacement before sending a request
when needed. Coalesce concurrent token acquisitions for the same configuration and
environment. Client Credentials reacquires a token using client credentials rather than
assuming the provider issues a refresh token.

A manually supplied bearer token alone cannot be renewed automatically. Token acquisition
failures must remain visible; do not silently fall back to another environment or identity.

### Custom Login

Support one configurable login request and, optionally, one separate renewal request.
The login may also be repeated to renew a token. Arbitrary multi-step workflows and a
JavaScript scripting engine are outside the first release.

Configure:

- URL, HTTP method, query parameters, headers, and body using variable references.
- Value composition, separators, prefixes, and Base64 transformation where needed.
- Accepted success status codes and a required token extraction result.
- Token extraction from the complete response text, a selected JSON field, or a header.
- Token placement and optional prefix in subsequent API requests.
- Optional expiry extraction or an explicitly configured validity duration.
- Optional renewal request inputs and extraction of replacement values.

When no lifetime is available, show Expiry unknown instead of inventing an expiration.
Allow manual renewal. Define configurable rejection detection before adding automatic
renewal after a failed API request. Preserve the original failure response, bound renewal
attempts, and do not silently replay requests that could repeat side effects.

Login and renewal requests must not recursively trigger their own authentication flow.
Only accept extracted values after the response passes the configured success conditions.
Keep credentials and extracted values out of project JSON, logs, and diagnostic exports.

## Optional self-hosted server

Local-only operation remains complete without a server. An operator may deploy a separate
ASP.NET Core and PostgreSQL service and configure the desktop client with its URL. Approved
users receive a personal project; personal data is visible only to its owner. Team projects
share all collections among members. Project administrators manage team membership. There are
no private collections in a team project.

The server stores projects as individually versioned rows (collections, folders, requests,
environments, variables), ordinary project values, memberships and per-user encrypted secret
values. Request history always remains local. Changes in a server workspace are saved on the
server as they are made; members editing different requests never conflict, and a save over a
request someone else changed asks first. Other members' changes appear automatically. Offline
copies are read-only. Local project files remain after upload. Local OS credential-store values
are uploaded only when explicitly selected in the transfer preview.

Local accounts require administrator approval before accessing project data. OIDC is an
alternative server authentication mode, not a simultaneous option. The provider secret stays
on the server. OIDC identities are linked by `(issuer, subject)` and new registrations require
administrator approval. See [SELF_HOSTED_BACKEND.md](SELF_HOSTED_BACKEND.md) for the data,
authorization, sync, encryption, and deployment contract and its implementation status.

## Minimal request history

History contains only these visible fields:

- URL, represented safely without exposing resolved reusable values or embedded credentials.
- HTTP method.
- Response status code, when a response exists.
- Response time, using the same definition as the response view.

Keep bodies, headers, cookies, authentication data, and response payloads out of history.
Keep history outside portable project files and exports. Do not add OAuth or Custom Login
background calls as ordinary user-request history entries.

A URL is potentially sensitive: do not persist the executed URL verbatim. Prefer the
unresolved request template with sensitive components removed. Variable-derived values
remain placeholders. Preserve literal hosts, including internal hostnames, locally.
Redact all literal path and query content, including query names; strip embedded credentials
and fragments and mask literal ports. See [HISTORY.md](HISTORY.md) for the agreed rules.

When no HTTP response exists, do not fabricate a status code. Use a clear absent-status
representation for failed or cancelled attempts. History must not silently trigger
request execution or act as a full request/response archive.

### Retention and controls

Persist history locally across application restarts, with a maximum of 100 entries per
project and a maximum age of seven days. Both limits apply: discard the oldest entries
when the count is exceeded and remove expired entries regardless of the count.

Store an internal UTC timestamp for ordering and expiration; it does not add another
visible history detail. Prune expired entries on startup, before displaying history,
and when inserting entries. Run expiration cleanup while the app is open; when it is
closed, cleanup resumes on the next launch.

Provide a history recording toggle and Clear history per project. Recording starts disabled.
Disabling recording stops new entries; existing entries still expire and can be cleared.
Close project preserves history, including across restarts. Only explicit Clear history
deletes entries apart from retention cleanup. Keep the count and age limits fixed in the first release;
user-configurable limits are deferred.

## Import and export

Import and export are part of the first-release scope. Show a preview and compatibility
report before writing imported data. Do not silently overwrite existing user work or
silently discard unsupported features.

### OpenAPI import

Accept OpenAPI documents in YAML and JSON. Create requests from operations, including
methods, URLs, parameters, bodies, examples, and applicable security definitions. Propose
folder organization from tags. Mark values still requiring user configuration.

Preserve meaningful imported schemas and metadata needed for later export. Resolve
references within documented file-access boundaries. Remote reference retrieval must be
explicit; importing a file must not silently make network calls or execute requests.

The initial implemented slice accepts OpenAPI 3.0.x/3.1.x and internal JSON Pointer references
only. New imports use new collections and variable identities rather than overwriting existing
ones. Only server URLs and authentication produce project variables, using readable names
and numeric suffixes for conflicts. Primitive query/header/path/form examples remain editable
request literals. Missing required values require review before sending. Preview renaming
updates all imported references together. [OPENAPI_IMPORT.md](OPENAPI_IMPORT.md) defines the mapping subset, preview, compatibility
blockers, stored metadata and partial-failure recovery. Unsupported versions receive an error.

### Postman migration

Import collection JSON with folders, requests, supported bodies, authentication, and
variables. Also import environment JSON and map it to project environments. Put reusable
values and recognized credentials in the OS credential store, leaving references in files.

Preview scope mappings and naming conflicts: Postman variable scopes may differ from
Fluxkit's project model. Imported scripts must not execute. Report tests, pre-request
scripts, dynamic variables, auth flows, and other unsupported behavior explicitly.
Do not claim to translate Postman scripts into Custom Login automatically.

The implemented slice accepts Collection v2.1 JSON and environment JSON with an explicit
environment scope marker. [POSTMAN_IMPORT.md](POSTMAN_IMPORT.md) defines identity mappings,
project-wide environment association, compatibility limits and partial-failure recovery.
Migration fixtures must cover real format shapes using synthetic values. Bulk archives
and live account integrations are not assumed to be part of the initial importer.

### OpenAPI YAML export

Allow export of a request, folder, collection, or project:

| Selection | Result |
| --- | --- |
| Request | One OpenAPI YAML document containing the selected operation |
| Folder | One document containing the selected subtree's operations |
| Collection | One document containing the collection's operations |
| Project | An export directory with one document per collection |

Resolve inherited authentication into the exported API description without exporting
credentials. Handle duplicate path and method combinations explicitly, offering example
variants or a reported conflict rather than silently overwriting an operation.

OpenAPI describes an API rather than the complete Fluxkit workspace. Folder hierarchy,
Custom Login automation, and environment value storage do not have a lossless standard
mapping. Preview omissions and transformations. Do not infer an authoritative response
schema merely from a single observed response.

The implemented export uses OpenAPI 3.1.0 YAML and the saved project state only.
[OPENAPI_EXPORT.md](OPENAPI_EXPORT.md) defines synthetic placeholders, excluded values,
compatibility blockers, metadata conversion, validation boundaries and destination recovery.

### Native Fluxkit export

Provide a native export for preserving project structure, environment definitions,
variable references, and authentication configuration. Support the corresponding native
import for restoration. Do not include OS credential-store values, history, local caches,
or machine-specific paths. A restored project requires local values to be supplied.
Define the package layout and compatibility rules before implementing this format.

## Delivery and release expectations

Implement in usable milestones rather than one large feature batch. Maintain responsive
editing and cancellation as data and response sizes grow. Test filesystem and credential
failure paths, environment isolation, and redaction as well as successful execution.

macOS, Windows, and Linux are target platforms. Validate native integration and packaging
on each before claiming release support. A build on one host does not prove another OS.

## Deferred capabilities

- A Fluxkit-hosted service, cloud synchronization, and remote telemetry.
- Browser-based OAuth and OAuth grants beyond Client Credentials.
- Arbitrary scripting and multi-step workflow automation.
- Full request/response history and automatic payload archival.
- Non-HTTP protocols and automatic execution of API collections as test suites.

## Decisions before the relevant milestone

- Manual header/query conflicts are resolved in favor of the manual field; see [AUTHENTICATION.md](AUTHENTICATION.md).
- History retains literal hosts and masks literal path/query content; see [HISTORY.md](HISTORY.md).
- Exact import versions, external reference policy, and collision resolution.
- OpenAPI output version and native export package format.

These decisions block their affected implementation, not unrelated milestones.

## Format references

- [OpenAPI specification](https://spec.openapis.org/oas/v3.2.1.html)
- [Postman schemas](https://schema.postman.com/)
- [Postman export documentation](https://learning.postman.com/v11/docs/getting-started/importing-and-exporting/exporting-data)
- [OAuth2 Client Credentials](https://www.rfc-editor.org/rfc/rfc6749.html#section-4.4)
