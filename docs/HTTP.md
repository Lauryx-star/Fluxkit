# HTTP execution and response handling

## Sending and cancellation

**Send** executes the active draft without saving it. Project requests use that project's
selected environment, including shared-default fallback. Session requests can send literal
HTTP/HTTPS URLs without a project; placeholders require project definitions. Inherited
authentication follows [AUTHENTICATION.md](AUTHENTICATION.md). Manually configured headers,
including Authorization and Cookie, can use variable placeholders. Token acquisition precedes
API dispatch; its separate deadline and lifecycle are in [TOKEN_AUTHENTICATION.md](TOKEN_AUTHENTICATION.md).

The UI captures the draft, project identity, environment ID, and definition revision at
Send. Rust validates that scope, resolves only active fields, and opens required attachments
before dispatch. Missing variables, denied credential access, invalid headers, inaccessible
attachments, or an externally changed environment stop dispatch with an actionable error.
Each variable tuple is read at most once during preparation. Editing the draft, changing
the environment, or switching tabs does not mutate an already prepared request. The response
identifies its environment at dispatch and belongs to the last send of that draft.

**Cancel** covers preparation and transfer. Closing its request/project or reloading the
project cancels abandoned work; late replies never replace another draft's response.
An OS credential call already running on a worker may finish after cancellation, but its
result is not dispatched. Cancellation cannot undo effects the API server already processed.
No requests are automatically retried, including POST or failed downloads.

There are at most eight registered requests. Native reservations are issued before execute,
so cancelling while registration is pending cannot accidentally start a request. Reservations
can execute once; cancel is idempotent. Unused reservations expire after ten minutes when
new work is registered. Request slots are released on success, failure, or cancellation.

## Network policy

- Default timeout: **60 seconds**, configurable per send from **1 to 300 seconds** in the
  session UI. The timeout is not saved into project files.
- A 3xx is returned as a response. Redirects are not followed automatically, so authentication,
  cookies, and variable-derived URL/header values cannot be forwarded to another origin.
- No automatic Cookie header, cookie jar, or persisted cookie session. Set-Cookie is visible
  in response headers; a manually configured Cookie header is sent as requested.
- Connections are direct. Environment/system proxies are disabled. Proxy configuration,
  client certificates, custom CA selection, and TLS-relaxation controls are deferred.
- TLS verification and hostname checks use the native platform TLS implementation and
  system trust. There is no bypass switch. HTTP is also supported for selected APIs.
- No automatic Referer, compression negotiation/decompression, or speculative retries.
  Servers' compressed bodies are treated as binary. Manually supplied Accept-Encoding is
  allowed, but does not add a decompressor. Protocol negotiation is delegated to the client.

Only HTTP/HTTPS URLs are accepted. Credentials embedded in URL user-info are rejected;
use an Authorization header. URL fragments are not transmitted. Enabled, non-empty-key
query rows append to the URL's existing query, with standard URL encoding. The URL field shows
the URL together with the enabled query rows, like Postman: a query typed or pasted into the
field becomes parameter rows (decoded, replacing the enabled rows in order; disabled rows stay),
and editing the rows updates the field. Requests store the URL without those rows. Enabled headers
preserve repeated values. Fluxkit controls framing/connection/proxy headers: Content-Length,
Transfer-Encoding, Connection, Upgrade, TE, Trailer, Proxy-Authorization, and Proxy-Connection
must not be manually set. POST, PUT and PATCH without body content send `Content-Length: 0`,
because servers such as IIS reject unframed requests with 411 Length Required. CONNECT
tunnelling is unsupported.

The URL, query parameter and header fields and the text/JSON body editor each have their own
undo history per open tab: Ctrl+Z (⌘Z) undoes, Ctrl+Shift+Z (⌘⇧Z) or Ctrl+Y redoes. Fast typing
is grouped into one step; a pause, a new line or switching between typing and deleting starts the
next. The histories live only in memory and end when the tab closes. A field's history starts over
when its text is replaced from outside, such as by a server refresh or by the URL and parameter
table updating each other.

Text and JSON bodies send their literal resolved UTF-8 content, without JSON rewriting or
an obligatory JSON-validity check (invalid payloads can be useful API tests). Default media
types are text/plain with UTF-8 and application/json; a manually chosen Content-Type wins.
URL-encoded forms encode text fields; multipart streams text/file parts and generates its
own boundary. Remove manual Content-Type for multipart. File bodies stream with a native
Content-Length and default application/octet-stream. All attachment reads are restricted
to validated regular files under the authorized project's attachments directory.

Request definitions are bounded to 8 MiB serialized JSON and resolved strings to 8 MiB
combined, excluding streamed attachment bytes and form-encoding expansion. The resulting
URL/query is at most 64 KiB, and configured request headers at most 64 KiB/1,000 entries.
These are resource limits, not a general promise that every server accepts that size.

## Timing and response views

Elapsed time begins immediately before network execution and ends after the complete
response body has been received. It includes connection setup, TLS, upload, server latency,
body transfer, and download-write backpressure. It excludes variable/credential preparation,
file chooser time, final file synchronization/commit, and JSON formatting. For exchanges that fail before full receipt, it shows elapsed time until failure/cancellation
rather than inventing a complete response time. File-finalization failures after receipt
retain the measured body-receive time. The request deadline covers transfer, including staged download writes and
synchronization; final atomic commit is outside the transfer deadline.

The UI shows the actual HTTP status when received, elapsed milliseconds, and received body
bytes. 4xx/5xx are HTTP responses, not transport failures. Cancellation, timeout, network
failure, invalid input, and file failure have distinct outcomes. A broken body after headers
keeps the received status/headers and partial byte count; no successful completion is invented.
Partial failed body previews are discarded. Header displays are bounded to 64 KiB/1,000 rows.

Raw displays text as inert text, including HTML or script content. Headers are an inert
table. JSON formatting happens on a Rust worker, with raw fallback for invalid/deeply nested
JSON. Both raw and formatted previews are at most 1 MiB; a visible notice marks truncation.
The stream is still drained to EOF for accurate full size/time, subject to cancellation and
the deadline. Formatting may be unavailable when expanded JSON exceeds the preview bound.
No response payload is put into browser storage, logs, or project JSON.

Each new response opens the JSON tab when a formatted preview is available; otherwise it
opens Raw. Manual tab selection is preserved while inspecting the same result. Dispatch
environment and token metadata are available in the initially collapsed Details disclosure;
errors and truncation notices remain visible. HTTP status badges use green for 2xx, yellow
for 3xx, red for 4xx/5xx, and a neutral style for other codes.

Text previews support UTF-8 text, JSON, XML, and common textual media types. Binary media,
attachment responses, compressed content, and invalid/non-UTF-8 text have no text preview.
Empty bodies are explicitly identified. Non-ASCII response header bytes are displayed using
a byte-preserving Latin-1 character mapping. A truncated UTF-8 boundary is trimmed to a
complete code point rather than misclassifying otherwise valid text as binary.

Images (PNG, JPEG, GIF, WebP, BMP, AVIF, ICO, SVG) and PDFs get a Preview tab, also when sent
as attachments. The body is kept in memory up to 20 MiB and is never persisted; larger bodies
show a hint to save the response instead. Images render only through `<img>` from a `data:` URL,
so SVG scripts do not run. PDFs render with the bundled pdf.js (loaded on first use, in a
worker) into canvases, at most 50 pages; PDF scripts and XFA forms are not executed. Compressed
(Content-Encoding) bodies have no preview because Fluxkit does not decompress responses.

At most eight response-detail sets are retained in UI memory, each with bounded preview
and headers. Older details are released with a visible notice while status/size metadata
can remain for open drafts. Closing a draft releases its details; restarting loses all
responses. Separately, opt-in [request history](HISTORY.md) persists only sanitized URL,
method, status, and elapsed time. Execution results may include an optional `historyWarning`
when metadata storage fails; the original HTTP result remains visible and is never retried.

## Explicit downloads and failure recovery

**Send & save response…** is a new execution, not a replay hidden behind a Save button.
It opens a native destination chooser before dispatch and streams the entire body to that
selected file. Cancelling the chooser sends nothing. Previously received binary bodies are
not cached for a later save; choose this action before dispatch when the full body is needed.
Do not assume a second POST is safe: every send is an explicit user action.

A private staged file in the chosen destination directory allows atomic replacement and
removal of partial output. Such a file exists only for an explicitly requested download;
there is no automatic response cache. Transfer failure/cancellation removes staged output
and preserves an existing destination. Synchronize the staged data before committing.
A new destination uses no-clobber persistence. For an existing target, native save-dialog
confirmation authorizes replacement; a changed size/mtime or unsafe symlink blocks commit.
As with project writes, an external process can race the final check and replacement;
there is no filesystem-wide transaction. Once final commit starts, cancellation may arrive
too late to prevent completion. The final native result reports whether saving succeeded.

Normal application exit cancels active transfers and gives their cleanup up to two seconds
before removing any remaining known staging files. On macOS, Cmd+Q uses the same window-close
path as the close button so unsaved-change confirmation runs before exit and download cleanup.
Filesystem permission or device failures can still prevent removal.

A crash/forced termination can leave a randomly named staged file in that user-selected
directory; its payload is private. With Fluxkit closed, inspect/remove only the known partial
file. Fluxkit does not scan arbitrary download directories and delete unknown files at
startup. Full-download size is not capped, so disk capacity and timeout still matter.
No output path, request URL, body, header, or native transport error is logged automatically.

## IPC and implementation boundary

Only the bundled main window receives begin_http_request, execute_http_request, and
cancel_http_request permissions. No frontend HTTP/filesystem plugin or Fetch access to
remote origins is added. Native networking uses reqwest 0.13.5, Tokio, and tokio-util
(MIT/Apache-2.0). On Linux the native TLS backend requires OpenSSL libraries/development
packages; on macOS/Windows it uses platform TLS. File and credential preparation and JSON
formatting run on workers. Network and file streaming use asynchronous I/O.

begin_http_request returns a native UUID reservation. execute_http_request accepts that
ID and an ExecutionInput: request snapshot, nullable scope (authorized handle/environment/
revision, projectRevision, collectionId, collectionRevision), timeoutMs, and saveResponse.
Authentication is defined in [AUTHENTICATION.md](AUTHENTICATION.md). cancel_http_request accepts the reservation ID.
ExecutionResult mirrors src/http/types.ts: outcome, nullable status/elapsedMs, headers,
byteCount, nullable text/jsonText, contentKind, truncated, saved, and nullable safe message.
There is no resolved-request echo. Rust validates transport inputs independently of React.

Routine Rust tests use fake credential values, temporary folders, and controlled loopback
servers. Their local bind permission must be available; the restricted agent sandbox blocks
those ports, so the suite is run with local-network access. Native OS-store integration is
separate. Windows/Linux packaging and integration require their own platform checks.

## Request header inspection

API and token/login requests default to `User-Agent: Fluxkit/<application version>` and
`Accept: */*`; explicit headers take priority. Content-Type follows the selected body type
as described above, and is omitted for an empty body unless explicitly configured.
No automatic Origin, Referer, cookies, or compression negotiation is added.

In the request Headers tab, **Automatic and sent headers** offers an explicit local
**Inspect headers** action. Rust prepares and validates the current draft without network
I/O or token acquisition. It reads only active request/auth variable values, never preloads
an environment, and returns redacted metadata. A pending acquired token is represented by
a masked header description. Query-placed authentication does not add a header.

After actual dispatch, `ExecutionResult.requestHeaders` contains a redacted header snapshot
from the prepared request after token placement. The UI distinguishes **Draft at inspection**
from **Last dispatch**; inspecting again refreshes a draft snapshot after edits. A dispatch
snapshot describes an attempted send and does not prove delivery to the server. Multipart
boundaries can differ between inspection and dispatch. HTTP Host/authority and transport
framing generated later by the HTTP implementation are explicitly outside this list.

All manual and authentication header values are masked, including nonstandard API key
names, Cookie, variable-derived values and manual overrides of defaults. Variable-derived
header names are masked too. Only generated default values are shown in clear. The rows
include `name`, `value` and `source` (`manual`, `authentication`, `automatic`). These snapshots
remain in bounded UI response memory, with no history, diagnostics or portable-file writes.
`preview_http_headers(id, input)` consumes a cancellable reservation and returns these rows.
Its custom permission, like `test_authentication`, is limited to the bundled main window.
