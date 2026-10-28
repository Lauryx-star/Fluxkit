# Authentication and inheritance

Milestone 5 adds None, Inherit, Basic, manual Bearer/JWT, and API Key authentication.
Milestone 6 adds OAuth2 Client Credentials and Custom Login; see
[token acquisition and lifecycle](TOKEN_AUTHENTICATION.md). A manually provided bearer token
does not acquire or renew itself.

## Editing and ownership

Use **Project authentication** in the sidebar, **Auth** beside a collection/folder, or the
request's **Auth** tab. Parent-scope dialogs use **Save authentication**; request settings
use the request's ordinary explicit Save/Cmd/Ctrl+S. Sending includes the unsaved request
snapshot. Saving parent settings does not save or discard open request edits. Dialog failures
preserve edited settings and unsaved dialog dismissal requires confirmation.

Credential fields select existing project variables by stable UUID. Create/edit their values
through project Variables. Usernames, passwords, tokens, and API key values remain in the OS
credential store; authentication editing does not fetch/reveal those values. Environment
selection and default fallback are the same as for other project variables. Renaming a
variable preserves its authentication references. No additional credential namespace or token
cache is introduced for these manually configured methods.

Removing a variable that saved project, collection, folder, or request auth settings use is
allowed after a warning that lists those settings (including configured auth in branches
currently overridden by None or a manual header). The settings keep the deleted variable's
identity: the editor shows **Deleted variable — choose a replacement**, and sending a request
that resolves to such auth stops with “Authentication uses a deleted variable. Choose another
variable in the authentication settings.” (token logins: “Token login uses a deleted
variable…”). Unsaved request settings are not listed; they become unresolved the same way. Removing an environment
still follows the explicit environment/value deletion confirmation and recovery rules in
[ENVIRONMENTS.md](ENVIRONMENTS.md).

## Precedence

The nearest explicit setting wins:

```text
Project -> Collection -> Folder -> Nested folder -> Request
```

An omitted/null `auth` inherits. At project root it means None. Explicit `{ "kind": "none" }`
stops inherited configuration for that branch. A descendant can supply a new explicit method.
The request editor displays the effective method and source without resolved values.
Literal manual overrides have a visible priority hint. Variable-derived field names are
checked authoritatively when sending. **Check variables** checks request-field placeholders;
credential references in authentication settings are validated when sending. The
parent-scope dialog also identifies what Inherit would use. A session request does not borrow
credentials from a project merely because that project is open in the sidebar.

Manual fields take priority, as selected in the product decision:

- Basic/Bearer: an enabled resolved `Authorization` header suppresses generated Authorization.
- Header API key: an enabled resolved header with the same case-insensitive name wins.
- Query API key: an existing decoded query name, matched case-sensitively, wins. This includes
  URL query text and enabled parameter rows, even an explicitly empty value.
- Disabled rows do not override. Existing duplicates are preserved as manually authored.
- Explicit None does not remove manually authored headers/query parameters.

Overridden credentials are not read from the OS store, so a missing unused credential does
not prevent the manual override from sending. All active manual fields still resolve and
validate normally. A manual placeholder with a missing value blocks sending; Fluxkit does
not silently replace it with configured auth.

## Wire behavior and safety

Basic produces Base64 of the UTF-8 username/password using the HTTP client's Basic encoder.
Username colons are rejected; password colons are allowed. Bearer adds `Bearer ` and requires
a non-empty token without whitespace. JWT values are treated as opaque tokens; this milestone
does not verify JWT signatures, infer expiry, or renew tokens. API key names are literal
configuration; values are resolved through their selected variable. Query values are URL-encoded.
Transport-managed header names cannot be used as API key headers.

Rust resolves auth under the authorized project lock. The execution scope contains
`handle`, environment definition `revision`, nullable `environment`, `projectRevision`,
nullable `collectionId`, and nullable `collectionRevision`. Rust reloads the project manifest
and only the relevant collection, validates the ancestry, and compares revisions before
credential access for auth and network dispatch. Any changed manifest/collection revision
requires a project reload. Request folder references must belong to the specified collection.
An explicit unsaved request setting overrides those saved ancestors.

Missing/denied/locked credentials, removed variables, invalid configurations, stale scope,
and invalid resolved header content produce redacted actionable errors before sending.
Auth contributes to existing resolved-text, URL, and header limits. There is no auth retry,
redirect forwarding, additional network call, credential echo, or automatic persistence of
resolved values. Existing HTTP policy is unchanged; see [HTTP.md](HTTP.md).

## Durable compatibility and commands

Project, collection, folder, and request objects gain an optional `auth` field in version 1:

```json
{"kind":"none"}
{"kind":"basic","username":"<variable UUID>","password":"<variable UUID>"}
{"kind":"bearer","token":"<variable UUID>"}
{"kind":"apiKey","name":"X-API-Key","value":"<variable UUID>","placement":"header"}
```

API key placement is `header` or `query`. Unknown methods/fields, malformed UUID references,
and invalid API key names are rejected. No credential literal is a valid reference. Missing
references are retained for portable cloned projects and visibly need configuration before
use. New readers open old files without rewriting them or running a migration. Absent auth
is omitted when serializing. Older Fluxkit builds that do not support `auth` reject these
files rather than dropping settings; use a compatible build. Existing backups and revision
checks protect writes, and credential values never enter those backups.

`save_project_auth(handle, auth, expected)` replaces only the project manifest's auth field
through its existing atomic write/backup/revision path and returns ProjectView. Collection
and folder settings use the existing save_collection command. Only the bundled main window
receives the new command permission. No secret-reveal or broad filesystem capability is added.

## Explicit authentication tests

**Test authentication** is available in project, collection and folder settings and in the
request Auth tab. It uses the editor's current settings, selected environment and shared
variable fallback without saving configuration. Inherit tests the parent scope, including
when replacing a saved override with unsaved inheritance. Native revision and import-review
checks still apply. Changing authentication/environment or closing the test cancels abandoned
work; no automatic retries occur.

Basic, Bearer and API Key use an explicitly entered **Test URL** and GET, without the API
request's body, manual headers or query rows. The URL may contain project placeholders and
its own query. Redirects remain disabled. A complete 2xx response reports success; other
statuses and transport failures report failure. A public endpoint can return 2xx without
checking authentication, so choose a protected endpoint such as `/me`.

OAuth2 Client Credentials and Custom Login execute a fresh configured token/login request.
A test succeeds only if its configured response/extraction rules produce a usable token.
Tests do not read, renew, replace or forget stored tokens. Credential variables are read
through the normal OS adapter; the acquired test token is discarded in native memory.
Neither response payloads nor tokens cross the test IPC boundary. Tests do not add history
or save files. Cancellation cannot undo an endpoint's already processed effects.

`test_authentication(id, input, tokenOnly)` consumes a normal cancellable HTTP reservation.
It returns `{success, status, message}` only; a token test has no API status and describes
successful acquisition rather than API authorization. Basic/Key/Bearer tests enforce GET,
no body, manual headers, extra query rows or download in Rust. Token tests use the same
validated scope and endpoint rules as acquisition, with a write-discarding value adapter.

Both unsaved and saved requests use **Move…** beside Save or the clickable breadcrumb
location. Choose the destination collection and full nested folder path, then confirm
**Save & move**. This immediately saves open request edits and relocates within the same
project. Dragging a request tab or saved tree request onto a collection/folder opens the
same confirmation. Cancelling the dialog writes nothing; failed moves retain the draft.

The dialog describes effective authentication at the destination. Inherit uses the new
collection/folder immediately after success; explicit request authentication stays attached.
The request UUID, variable references, selected environment and attachment paths remain
unchanged. Existing project references cannot be moved across projects by this action.
Request-owned tokens retain identity; inherited tokens follow the destination's owner and
configuration, with the same environment isolation. No credentials are accessed during moves.
Native file drops are disabled on the main WebView so internal HTML drag-and-drop can work;
attachment selection continues through explicit file dialogs. Native mouse-drag behavior
still requires platform validation; the destination dialog provides a keyboard/mouse fallback.
See [FILE_FORMAT.md](FILE_FORMAT.md) for cross-file recovery and OpenAPI metadata decisions.
