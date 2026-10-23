# Token acquisition and Custom Login

OAuth2 Client Credentials and Custom Login follow [authentication inheritance and manual-field
priority](AUTHENTICATION.md). They require an open project. Sending uses unsaved request auth
and saved ancestors. A manual header/query override suppresses token-store access and token
acquisition for that send. Existing manual Basic, Bearer, and API Key behavior is unchanged.

## Configuration and controls

Select either method in a scope's authentication editor. Parent dialogs save their scope;
request auth uses explicit Save/Ctrl+S/Cmd+S. Literal configuration is portable JSON, not a
secret store. Use project-variable parts for credentials and reusable inputs. Token expressions
reference stable variable UUIDs, so renaming preserves references. The variable-removal guard
includes both login and renewal expressions.

The request Auth tab operates on the effective configuration and selected project environment:

- **Check token status** reads metadata without a network request or revealing values.
- **Acquire / renew token** explicitly acquires even if a usable token exists. It does not send
  the API request.
- **Forget local token** confirms deletion of only that local token/renewal record. Variables
  and other records stay available. Changing context cancels work and discards late UI replies.

Send acquires if no token exists or known expiry requires replacement. The response view shows
token metadata at API dispatch. **Expiry unknown** means no lifetime was supplied; no default
lifetime or JWT expiry is inferred. Such tokens remain usable until explicitly renewed or
forgotten. Displayed metadata is a snapshot, not a background expiry monitor.

## OAuth2 Client Credentials

Configure a token endpoint composition, client ID and secret variable UUIDs, optional scope
composition, and `basic` or `body` client authentication. POST uses URL-encoded form fields
`grant_type=client_credentials` and optional `scope`. Basic form-encodes each credential before
Base64, following [RFC 6749 section 2.3.1](https://www.rfc-editor.org/rfc/rfc6749.html#section-2.3.1).
Body authentication sends `client_id` and `client_secret` as form fields instead.

HTTP 200 must contain JSON string `access_token` and case-insensitive Bearer `token_type`.
Optional `expires_in` must be a positive integer number of seconds (an integer string is also
accepted). Invalid supplied expiry is an error, not an unknown-expiry success. Tokens must be
non-empty without whitespace. The API receives `Authorization: Bearer <token>`. Replacement
repeats Client Credentials; returned refresh tokens are ignored. No browser grant, callback
listener, or application-wide client secret is introduced.

## Custom Login

Configure one login request and optionally one separate renewal request. Without renewal,
reacquisition repeats login. A separate renewal is used when a previous record exists,
including an expired record. Renewal failure never silently switches to login; explicitly
Forget to start with login again.

Each request has an endpoint composition, HTTP method, named query/header entries, and no
body, text, JSON, or URL-encoded form. There are no attachments, multipart, scripts, inherited
auth, or recursive token acquisition inside login requests. Accepted HTTP statuses are an
explicit set from 200–599, default 200. A configured redirect status can support header
extraction, but redirects are never followed. Accepted status and required extraction must
both succeed.

Token extraction supports complete UTF-8 response text with surrounding whitespace trimmed,
a JSON Pointer string field (for example `/session/token`), or exactly one named response
header. JSON arrays and escaped pointer segments use JSON Pointer syntax. Optional renewal
value extraction is required if configured; when omitted on renewal, the previous renewal
value is retained. Expiry can be extracted as a positive integer lifetime in seconds from
text, a JSON integer/string field, or a header, or configured as a fixed duration. These two
options are exclusive. Lifetimes are at most ten years. Missing configured extraction fails.

Composition joins ordered literal/variable parts with a separator, optionally Base64-encodes
the joined UTF-8 inputs, then adds prefix/suffix. This supports `Basic ` followed by encoded
username/password inputs separated by `:`. Renewal may additionally use the stored
access token or renewal value. Missing stored inputs fail visibly. Literals do not expand
`{{name}}`; select a variable part instead. Composition does not JSON-escape inputs: author
valid JSON and quote/escape as needed. Query/form values receive normal URL encoding, and
headers are validated. Extracted tokens are non-empty without control characters. API token
placement is a named header or query parameter with an optional prefix, default Authorization
with `Bearer `. Manual fields still win.

## Identity, expiry, and concurrency

The OS service remains `app.fluxkit.variables.v1`. Separate token account keys are:

```text
tokens/v1/<project UUID>/<owner>/<environment UUID or default>/<configuration SHA-256>
```

Owner is `project`, `collection/<UUID>`, `folder/<UUID>`, or `request/<UUID>` for the nearest
explicit setting. Requests inheriting one scope share a token; identical definitions in other
scopes do not. The hash uses deterministic serialized auth, including variable IDs, never
resolved values. Auth edits use a different record; owner/variable renames retain identity.
Tokens never fall back between environments, including Defaults. Copies retaining project and
scope IDs on the same machine share associations, as with variable values. Rotating credential
values does not change auth identity: explicitly renew an otherwise usable token afterwards.

Known expiry starts conservatively immediately before token dispatch. Replacement is due
within the smaller of 30 seconds or one tenth of its lifetime (whole seconds). Expired tokens
and tokens acquired after a backwards system-clock change are not sent. If acquisition itself
outlasts the lifetime, it fails without saving the result.

One process coalesces concurrent acquisition for the exact record key, sharing success or a
redacted failure. Waiting is cancellable and bounded by the token deadline. Cancelling a waiter
does not cancel its leader. A leader's cancellation/timeout can fail its concurrent waiters;
retry explicitly. Separate Fluxkit processes may independently acquire/replace the same entry;
there is no cross-process token transaction or lock. Coordination uses weak references, without
a resident plaintext cache; flight secrets remain only in ephemeral native memory.

## Storage and failure recovery

One OS secret contains version-1 JSON: `schemaVersion`, `token`, nullable `renewalToken`,
`acquiredAt`, and nullable `expiresAt` (UTC Unix seconds). Expiry metadata also stays in the OS
store. The complete UTF-8 record is limited to 2,560 bytes for Windows blob compatibility.
Oversized results fail before writing rather than truncating. No plaintext cache/settings file,
browser storage, project backup, history entry, or diagnostic export contains tokens.

Input values are fetched on a worker only when acquisition is needed, using existing variable
environment/default rules. An inaccessible store blocks use. A malformed record requires an
explicit Forget. Accepted status, complete bounded response, extraction, placement, lifetime,
and size are checked before one replacement OS write. Failures before that write retain the
old record. Providers offer no transaction with files or requests: a write may complete before
reporting failure or before cancellation. The API is not sent after such a failure; check
status and retry explicitly. Metadata cannot prove which secret a partial provider failure kept.

Auth edits, scope removal, project close, and environment removal do not automatically delete
tokens. This avoids deleting entries associated with saved/recovery definitions. Use Forget
before changing/removing config or environments when cleanup is wanted. Otherwise old entries
remain locally; restore the original config/owner/environment to Forget through Fluxkit, or
use the OS credential manager. Variable removal still deletes only its own variable slots.

## Network, bounds, and commands

Token endpoints require HTTPS with verified native TLS. Numeric loopback HTTP (`127.0.0.0/8`
and `::1`) permits controlled local development. Other HTTP endpoints and embedded URL
credentials are rejected. There are no redirects, retries, proxies, automatic cookies, or
recursive auth. Failed API calls retain their original responses and are never replayed.
Login response headers/bodies never cross IPC or enter files/logs. Errors expose safe context
and, where useful, HTTP status, without URLs, credentials, bodies, or provider diagnostics.

Auth definitions are at most 256 KiB, composed inputs at most 1 MiB combined, URLs at most
64 KiB, and headers at most 64 KiB/1,000 entries. Custom requests support 200 total named
query/header/form entries and 100 parts per composition. A token response must fit completely
within 1 MiB body and 64 KiB/1,000 headers; oversized responses fail instead of extracting from
a prefix. Parsing and OS access run on workers; network reads are asynchronous.

The selected 1–300 second timeout (default 60) bounds the token phase including flight wait,
credential preparation, network, parsing, and store write. The API transfer has its own selected
timeout. API elapsed time excludes acquisition. Send & save asks for a destination first;
cancelling the chooser sends neither login nor API requests. Cancellation cannot undo login
side effects or an OS call already running.

Only the bundled main window gains `authentication_action(id, input, action)`. It consumes a
begin_http_request reservation and the same ExecutionInput/scope as sending; actions are
`status`, `renew`, and `forget`. Rust checks handles, ancestry/environment revisions, and auth
before store access. The API URL need not be valid for these actions. Replies are metadata
`{state, expiresAt}`, with state `missing`, `valid`, `renewalDue`, `expired`, or `unknownExpiry`.
execute_http_request gains optional `tokenStatus` metadata for the dispatched token. No tokens
or resolved inputs are returned. Existing execution result fields remain unchanged.

## File compatibility and dependencies

Version-1 optional auth gains `{kind: "oauth2", config: ...}` and
`{kind: "customLogin", config: ...}`. Exact typed definitions are mirrored in `src/auth/types.ts`
and `src-tauri/src/projects/token_config.rs`. Old manual/auth-free files load without rewriting
or migration. Older readers reject new methods instead of dropping settings. Unknown fields
remain errors. Saves retain existing revision checks, atomic replacement, and recovery copies.
Backups contain definitions/literals, never acquired tokens or resolved inputs.

The already locked `base64` 0.22.1 crate is now a direct dependency (MIT/Apache-2.0), adding
no new package version. Routine tests use fake stores and controlled loopback servers. The
separate ignored `os_token_roundtrip` test creates and removes random synthetic variable/token
entries and must be run explicitly for each platform before claiming integration there.
