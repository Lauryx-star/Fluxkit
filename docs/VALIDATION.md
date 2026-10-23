# Fluxkit development validation

Validation on 23 October 2026 used macOS on Apple Silicon, Node.js 24.11.0, npm 11.6.1,
and Rust 1.92.0. The native build used the installed standalone Apple Command Line Tools.
The system-wide Xcode selection and license acceptance were not changed.

## Completed checks

| Check | Result |
| --- | --- |
| `npm ci` | Installed from the repository lockfile |
| `npm run typecheck` | Passed |
| `npm run lint` | Passed with no warnings |
| `npm run format:check` | Passed |
| `npm test` | Six tests passed |
| `npm run build` | Passed |
| Cargo formatting | Passed |
| Cargo Clippy with all targets and warnings denied | Passed |
| Cargo tests | One serialization-contract test passed; binary and doc targets passed |
| Cargo native build with locked dependencies | Passed on macOS Apple Silicon |
| `npm run desktop:build -- --debug` | Passed through the documented local launcher |
| `npm run tauri -- build --debug --bundles app` | Produced a local macOS app bundle |

Cargo checks used `--manifest-path src-tauri/Cargo.toml` and, where applicable, `--locked`.
The standalone SDK was selected through a process-local `DEVELOPER_DIR` value. The npm
Tauri launcher now selects that installed SDK automatically on macOS unless the caller
has explicitly selected a developer directory.

The uncompressed debug app bundle was approximately 23 MiB. This is a build artifact size,
not a runtime memory measurement or a release-size benchmark.

## Browser interaction checks

The local Vite preview was inspected at the configured 1320 by 880 desktop start size and
at the 880-pixel minimum window width. The minimum-width preview had no horizontal overflow.
Creating and renaming a session draft, editing its URL, and editing header rows worked.
No warnings or errors appeared in the observed browser console.

Automated tests cover independent drafts, switching and closing requests, header editing,
body preservation, keyboard configuration-tab navigation, and metadata connection success
and failure. The browser preview deliberately does not invoke or emulate the native bridge.

## Native window verification

The native macOS smoke test passed on 23 October 2026 after UI automation access became
available. The rebuilt bundled app loaded from `tauri://localhost`, rendered the new
request/response arrow logo, and displayed Desktop connected with version 0.1.0.

Creating a session draft, entering a URL, switching to Headers, entering a header pair,
and closing the draft worked. Send remained disabled and the session-only notice was
visible. The synthetic test draft was removed after verification.

The logo uses a single SVG source for the sidebar and browser icon; Tauri generates the
platform icons from that source. Type checking, linting, formatting checks, six frontend
tests, the frontend production build, and the macOS app-bundle build passed after the change.

Windows and Linux builds and smoke tests, release-mode packaging, signing, startup time,
and runtime memory measurements have not been performed. Those checks remain required
before making corresponding release or performance claims.

## Milestone 2 local persistence validation

On 23 October 2026 the local project milestone passed 11 frontend tests and 13 Rust tests.
Frontend type checking, linting, formatting, and production build passed. Native formatting,
Clippy with warnings denied, tests, and macOS bundle build passed with locked dependencies.

The native bundled application was tested with synthetic data in an isolated temporary
folder. Creating a project through the native folder chooser, creating a collection,
editing a request, and saving through Cmd+S worked. The UI changed from Unsaved to Saved.
Closing the desktop window with an edited request displayed confirmation; Cancel retained
the input. After saving and fully closing the app, reopening the same project restored
the collection and exact saved request URL. The metadata connection remained available.

Automated coverage also includes nested folder creation/removal, multipart attachment
references, body preservation, project isolation, revision conflicts that preserve unsaved
input, invalid/future files, cycle/traversal rejection, portable binary copies, recoverable
removals, advisory locking, stable project identity, and rejected symlink targets.

Storage uses one manifest and one JSON file per collection, with explicit request saves.
Previous versions are backed up before atomic replacement, and deleted collections move
to local recovery trash. The guarantees and filesystem race limits are documented in
[FILE_FORMAT.md](FILE_FORMAT.md). New runtime dependencies are rfd for native dialogs,
uuid for stable identifiers, tempfile for safe staged writes, sha2 for revision checks,
and serde_json promoted from tests to runtime. No frontend persistence plugin is exposed.

Windows/Linux integration and packaging remain unverified. Forced process termination,
OS power-loss behavior on every filesystem, arbitrary external editors racing the final
revision check, and a hostile local process manipulating directories are not guaranteed
by these checks. Recent-project paths and open tabs are not restored automatically yet;
users reopen saved project folders through Open project.

## Current feature boundary

Milestones 1 through 6 implement desktop editing, local projects, project environments,
OS credential values, native HTTP execution, response views, explicit streamed downloads,
and inherited Basic/Bearer/API Key authentication, OAuth2 Client Credentials, and Custom Login.
History and import/export remain planned. Use placeholders for
credentials in saved request fields; project JSON is not a secret store.


## Milestone 3 environments and OS values

On 23 October 2026, the frontend suite passed 15 tests. The default Rust suite passed
22 tests, with the explicitly opt-in OS integration test excluded as designed. TypeScript,
ESLint, Prettier, production UI build, Rust formatting, Clippy with warnings denied,
locked Rust tests/build, and the debug macOS app bundle passed.

The OS integration test was run separately against macOS Keychain: it created one entry
with a random integration UUID, read the synthetic bytes, deleted the entry, and verified
absence. Access failed inside the restricted tool sandbox and passed when run with native
OS access. Provider errors were not exposed as diagnostic strings or stored as values.

The bundled native UI was exercised with a temporary project: added a variable and an
environment, saved a synthetic default URL in Keychain, explicitly loaded/revealed it,
selected Testserver in the header, and checked `{{baseUrl}}/health`. Resolution succeeded
using the default because the environment-specific slot was absent; no HTTP request was
sent. Saving a collection/request continued to work. The manager layout was visually
inspected at the default window size. GUI testing stopped when the user began editing the
open window; their input and the temporary project were preserved. A full bundled-app
restart test of environment selection was not performed in this pass. Selection persistence
across a fresh load and separation between project identities are covered by native tests.

Additional automated coverage includes empty overrides, missing/default/denied values,
renaming without losing credential identity, unknown and malformed definitions, stale
revision rejection before credential writes, no resolved data in files or check replies,
scoped removal, partial credential deletion with safe retries, and file-write failure after
credential deletion. UI tests verify explicit reveal/save, cleared editor after success,
input preserved after failure, deletion and discard confirmation, and explicit recovery
from an externally removed environment selection.

The new runtime dependency is keyring 3.6.3 (MIT/Apache-2.0), with native Apple/Windows
features and Linux synchronous Secret Service. Credential I/O runs off the UI thread;
values are not preloaded, and repeated references in one resolution share an ephemeral
lookup cache. No startup-time or runtime-memory improvement is claimed or measured.
Windows/Linux credential adapters, packaging, and platform-specific prerequisites still
require their own integration runs. The 2,560-byte value limit matches Windows blob storage.

## Variable editor visibility adjustment

New value input is readable by default, with an explicit Hide value / Show value switch.
Slot changes and successful saves clear the editor and restore readable new input. Stored
values still load only through Load & reveal; visibility never changes persistence or
log/history policy. The frontend suite passed 16 tests, including typing, masking,
unmasking without native reads/writes, and slot/save resets. TypeScript, ESLint, Prettier,
production UI build, and the debug macOS app-bundle build passed. No Rust behavior or file
contract changed. This adjustment was verified through interaction tests; the user's open
desktop window was left untouched rather than restarted for an additional manual smoke test.

## Variable-check feedback adjustment

Check variables now keeps a prominent result panel below the URL for progress, success,
no-placeholder feedback, and failure. Rechecking replaces its contents rather than removing
and reinserting the panel. Results are scoped to the checked request and project. No
resolved values are displayed. The frontend suite passed 18 tests, including deferred native
completion, failure feedback, empty-template feedback, and request switching. TypeScript,
ESLint, Prettier, production UI build, and the debug macOS app-bundle build passed. Native
resolution rules and IPC contracts did not change. Verification used interaction tests;
the user's open desktop session was not manipulated for an additional manual smoke test.

## Variable highlighting, completion, and explicit hover preview

The request URL, parameter/header/form inputs, and text/JSON bodies now share a native
input/textarea editor with a synchronized syntax mirror. Definitions drive coloring and
filtered completion without credential reads. Hover is an explicit reveal of one effective
value; native code verifies the project handle, definition revision, variable identity,
and selected environment before resolving it. Late UI replies are discarded after context
changes. No persistent file schema or credential adapter changed; the typed environment
action union gained preview.

The frontend suite passed 24 tests, including keyboard/mouse completion, filtering, Escape,
existing closing braces, multiline preservation, explicit hover-only reads, stale replies,
store errors, and a 501-placeholder fixture that preserves text while bounding highlighting
to 500 tokens. The default Rust suite passed 23 tests; the separate OS integration test
remains opt-in and was not repeated because the platform adapter is unchanged. New native
coverage checks the preview action contract, environment/default lookup, one-variable
access, stale scope rejection before store reads, and denied-store errors.

TypeScript, ESLint, Prettier, production UI build, native formatting, Clippy, locked native
tests/build, and the debug macOS bundle passed. A separate local browser harness with
synthetic metadata/values visually verified URL and multiline token coloring, filtered
completion/insertion, and hover display. It used a fake bridge and did not read user values.
Its temporary files, browser tab, and server on port 1421 were removed/stopped afterwards.
The user's running desktop session was not manipulated. A new bundled-native interactive
smoke test and Windows/Linux integration remain unperformed for this change. No startup,
interaction-latency, or runtime-memory performance guarantee is claimed; suggestions and
highlight targets have explicit rendering bounds documented in ENVIRONMENTS.md.

## Milestone 4 native HTTP execution

On 23 October 2026 the frontend suite passed 29 tests. The default Rust suite passed
29 tests, with the pre-existing separate OS-credential integration test excluded by design.
HTTP tests initially failed because the restricted tool sandbox denied local listener binds;
with local-network access, controlled-loopback tests passed. No public API or real credential
was used. TypeScript, ESLint, Prettier, production UI build, native formatting, Clippy with
warnings denied, locked native tests/build, and the debug macOS app bundle passed.

Native checks used a separately built app named Fluxkit HTTP Smoke with its own bundle
identifier, leaving the user's normal Fluxkit session untouched. The bundled native window
sent an unsaved session draft to a controlled loopback server, displayed HTTP 200 with the
complete 70-byte JSON response, switched between formatted JSON and Headers, and retained
an accurate response status/time/size. One observed local JSON exchange reported 5 ms.
A delayed response reported 3,006 ms after full body receipt; cancellation showed a distinct
cancelled result, and a one-second timeout reported 1,002 ms without inventing an HTTP status.
An explicitly selected file download saved 2,097,152 bytes exactly matching the synthetic
source. The download was verified and removed. A separately generated untrusted TLS
certificate was rejected by the native client, producing a redacted transport error and no
HTTP status. Neither trust settings nor TLS verification were changed.

A final controlled server on port 1433 held an explicit download open after 65,536 bytes.
The macOS predefined Quit initially bypassed application cleanup; replacing it with the
window-close path fixed that case. Cmd+Q then showed the existing unsaved-change confirmation,
and confirming exit removed the active staging file without creating the target file. The
known synthetic partial from the initial failed check was removed separately after exit.

The isolated debug native host's RSS snapshot after the 2 MiB download was 118,512 KiB
(about 115.7 MiB). This excludes WebView helper processes, is a single debug-process
snapshot, and is not total application memory or a comparison against another client.
Loopback timings are functional timing observations, not real-network performance promises.
Startup and input latency were not isolated from automation overhead. The 31 MiB-class
uncompressed debug bundle is not a release-size or runtime-memory guarantee.

Automated native tests cover variable-resolved method/query/header/JSON dispatch, disabled
rows, URL encoding, text/JSON response views, empty responses, redirects without forwarding
authentication, no cookie reuse, bounded large text, exact binary downloads, cancellation,
timeout, redacted transport errors, multipart/form/file bodies, invalid headers and schemes,
interrupted bodies retaining actual status/byte count, and changed download targets remaining
untouched. Frontend tests cover unsaved draft sending, 60-second defaults, native reservation
cancellation before dispatch, response ownership across tabs, explicit send-and-save, inert
HTML/header rendering, keyboard response tabs, and timeout/truncation/binary notices.

The new runtime dependencies are reqwest with native TLS/form/multipart/stream support,
Tokio, and tokio-util (MIT/Apache-2.0). File uploads/downloads stream; JSON formatting stays
on native workers. The client disables redirects, retries, and system/environment proxies,
with no cookie jar. Native commands expose only reservation, execute, and cancel operations
to the bundled main window. No frontend networking/filesystem plugin was introduced.

The isolated test app was closed; loopback servers on ports 1431/1432/1433 were stopped and the
temporary TLS key/certificate removed. The regular Fluxkit app bundle was rebuilt with the
normal identifier after smoke testing. Windows/Linux integration and packaging remain
unverified. Download crash recovery, storage race limits, UTF-8-only previews, lack of
automatic decompression/redirects/proxies/cookies, and subsequent explicit sends are
specified in [HTTP.md](HTTP.md). Signing, release distribution, and persistent history remain
outside this milestone.


## Milestone 5 authentication and inheritance

On 23 October 2026 the frontend suite passed 35 tests and the default Rust suite passed
33 tests. The existing opt-in real OS-store test was not repeated; the platform adapter
is unchanged. No dependency or lockfile changes were needed. TypeScript, ESLint, Prettier,
UI production build, Rust formatting, Clippy with warnings denied, locked native tests/build,
and the debug macOS app bundle passed.

New native coverage verifies Basic credentials reaching a controlled loopback server,
Bearer dispatch using selected-environment values, stable variable IDs after renaming,
nearest-folder inheritance, explicit None, API key header/query encoding, case-insensitive
manual header priority, empty manual query priority, missing and denied credentials,
redacted invalid-credential errors, stale project/collection revisions, auth file round-trips,
legacy auth-free files, recovery backups, and invalid auth fields without destructive rewrite.
A regression test proves that deleting an auth-referenced variable leaves definitions and OS
values untouched until saved references are removed. Routine tests use fake values and temp
folders. Frontend tests verify parent-scope saving while dirty requests stay open, explicit
request None, collection overrides, failure retention/discard confirmation, no implicit value
reveal, session/project isolation, source presentation, and manual priority hints.

A separate native app named Fluxkit Auth Smoke used its own bundle identifier and a synthetic
project folder. Its real native bridge loaded inherited Bearer auth and blocked a send for a
missing local value. An unsaved manual Authorization header then reached a controlled server
on port 1434, which returned only boolean checks (HTTP 200, 50 bytes, observed 13 ms).
Saving project None, then restoring Bearer through the native auth dialog, preserved that
unsaved header. The project manifest and recovery backup were verified separately; the
collection file still contained no unsaved header. A request in a None-configured folder
returned HTTP 200 with hasAuthorization=false (52 bytes, observed 1 ms), demonstrating that
folder None stops inherited Bearer. No real credential was read/written, and no credential
entry was created for this test. The isolated app was closed with the dirty-change prompt;
the controlled server was stopped. The regular app bundle was then built with its normal
identifier. The user's normal running Fluxkit session was not manipulated.

These loopback timing observations are functional checks, not performance guarantees.
Authentication preparation reads only the selected collection rather than loading all
collections; credential values stay outside the UI until a deliberate existing reveal/edit
interaction. Startup/input latency and total memory were not independently benchmarked for
this milestone. Windows/Linux OS integration and packaging remain unverified. OAuth2 Client
Credentials, Custom Login, token renewal, history, and import/export remain subsequent work.

## Linux baseline for milestones 1 through 5

On 23 October 2026 the current milestone 1–5 baseline was checked on Linux Mint 22.3
(Ubuntu 24.04 base), x86_64, kernel 7.0.0-38-generic, in an X11 desktop session.
The working tree was clean before validation; no application source or lockfile changed.

The final checks used the repository's specified Node.js 24.11.0, npm 11.6.1, and Rust
1.92.0 with rustfmt and Clippy. Node/npm were installed under a temporary directory without
replacing the host's Node.js 24.14.1/npm 11.11.0. Rust 1.92.0 was installed through rustup.
The host provided GTK 3.24.41, WebKitGTK/JavaScriptCoreGTK 2.52.6, libsoup 3.4.4, and
OpenSSL 3.0.13 development libraries.

| Check | Result |
| --- | --- |
| `npm ci` with the pinned Node/npm versions | Passed from the committed lockfile; audit reported no known vulnerabilities |
| TypeScript, ESLint, Prettier | Passed with no project warnings |
| `npm test` | 35 tests passed across six files |
| `npm run build` | Passed |
| Cargo formatting | Passed |
| Cargo Clippy, all targets, warnings denied | Passed |
| Locked Cargo tests | 33 passed; the separate OS integration test was intentionally excluded |
| Locked Cargo native build | Passed |
| Separate `os_credential_roundtrip` test | Passed against Linux Secret Service |
| `npm run desktop:build -- --debug` | Passed; produced the native executable with bundled UI, without installer bundles |

The separate credential test used the existing randomly named integration entry, wrote and
read synthetic data, deleted the entry, and verified absence. It did not access user values.
Native checks needed access outside the restricted tool sandbox for toolchain/cache writes,
dependency downloads, loopback listeners, and desktop/session services. The frontend also
passed with the host's newer Node/npm before being repeated with the specified versions;
the initial engine-version warning does not apply to the final pinned-version run.

### Native Linux smoke test

The executable ran with isolated temporary `XDG_CONFIG_HOME` and `XDG_DATA_HOME` directories.
The bundled window displayed **Desktop connected · v0.1.0**. Creating a session request,
entering a synthetic loopback URL, and sending through the real native bridge worked.
A controlled server on `127.0.0.1:1435` returned HTTP 200 and 50 bytes of JSON. Raw text,
formatted JSON, and the response-header table displayed the expected synthetic response.
Visual inspection confirmed the native editor and response layout at the default window
size. The observed 1 ms exchange is a functional observation, not a performance guarantee.

Closing the dirty desktop window displayed the unsaved-change confirmation. Cancel kept
the window and request input; a subsequent confirmed close exited successfully. The test
server was stopped afterwards. No user project or reusable value was used for this smoke test.

Startup emitted a GStreamer plugin-scanner critical message for
`_dma_fmt_to_dma_drm_fmts` with `GST_VIDEO_FORMAT_UNKNOWN`. It did not prevent the checked
window, IPC, HTTP, or response-view workflows. Its cause was not resolved in this pass;
this is a recorded host-runtime observation, not a claim of warning-free desktop startup.

### Limits and next milestone

This pass establishes automated Linux baseline coverage, real Secret Service integration,
and the native session-request smoke test described above. Project folder dialogs, saved
project restart, environment editing, inherited-auth UI flows, TLS rejection, cancellation,
and streamed downloads were not separately repeated through the Linux GUI. Existing automated
storage, environment, authentication, HTTP, and transfer tests passed, but that does not
substitute for complete manual OS integration coverage. Linux installer bundles, release builds,
other distributions, Windows validation, startup/input latency, and total runtime memory
remain unverified.
Earlier macOS-only statements above describe their historical validation passes.

At the end of this baseline pass, milestone 6 was still unimplemented: OAuth2 Client Credentials
and bounded Custom Login were next, following [PRODUCT.md](PRODUCT.md) and [MILESTONES.md](MILESTONES.md). Their durable
configuration and IPC contracts, configuration/environment token identity, OS-store token
lifecycle and partial-failure recovery, expiry handling, and coalesced acquisition need to
be defined with the implementation. The subsequent milestone 6 pass is recorded below.

## Milestone 6 OAuth2 Client Credentials and Custom Login

On 23 October 2026 milestone 6 was implemented and checked on the Linux Mint 22.3 x86_64/X11
host described above. Final frontend checks used Node.js 24.11.0 and npm 11.6.1 from the
temporary pinned runtime. Rust checks used the pinned 1.92.0 toolchain and locked dependencies.
The earlier Linux baseline section and its results were preserved.

| Check | Result |
| --- | --- |
| TypeScript, ESLint, Prettier | Passed |
| Frontend interaction/unit tests | 44 passed across eight files |
| Production UI build | Passed |
| Rust formatting and Clippy, all targets with warnings denied | Passed |
| Default locked Rust suite | 42 passed; two explicit OS integration tests excluded as designed |
| Locked native build | Passed |
| Separate `os_token_roundtrip` integration | Passed against Linux Secret Service |
| Debug desktop build with bundled UI | Passed for the isolated smoke app and the regular application |
| Diff/content checks | Passed |

The new OS integration test wrote isolated synthetic client-variable entries, acquired a token
from a controlled loopback server, stored the structured token record through the real adapter,
then deleted its known entries and verified absence. The pre-existing variable-only OS test was
not repeated in this pass; it passed in the earlier Linux baseline. Routine tests use fake stores.

Native regression coverage includes OAuth Basic form encoding and body client authentication,
scope encoding, cached token reuse, known/unknown expiry, pre-send reacquisition, owner/project/
environment separation, stable variable references, coalesced success and failure, cancellation
of a waiter without cancelling its leader, timeouts, and manual header priority. Custom coverage
exercises composition/separators/prefix/Base64, JSON login, URL-encoded separate renewal using
stored token inputs, text/JSON/header extraction, accepted statuses, query token placement, and
renewal-value replacement. Invalid extraction/expiry, oversized bodies and credential records,
store failures, and malformed configurations do not silently replace usable tokens. HTTP 401
remains the original API response without automatic replay. Stale scope fails before OS reads.

Frontend coverage includes both editors, UUID composition references without credential reads,
separate renewal and extraction controls, unknown/known expiry presentation, explicit token
status/renewal, failed-renewal status retention, confirmed scoped forgetting, cancellation during
reservation, and discarded replies after context changes. Two additional regressions cover
loaded native-field-order requests and nullable/absent inherited auth: serialization field order
must not falsely mark an unedited saved request dirty.

### Native token smoke test

A separate app named **Fluxkit Token Smoke**, identifier `app.fluxkit.tokensmoke`, ran with
temporary XDG app-data/config directories. Its project, variables, requests, and responses were
synthetic. The native folder chooser opened the temporary project, and the app reported
Desktop connected. Credentials were set up only under random project/variable IDs in Secret
Service; no user project or value was read or changed.

An inherited project OAuth2 configuration acquired a token and sent an authorized API request.
A second explicit API send reused it: server counters showed one OAuth acquisition and two API
calls. A request-level Custom Login extracted JSON token/renewal values, sent successfully,
and displayed **Expiry unknown**. Check token status returned the same metadata without
revealing values. Explicit renewal sent the saved renewal value to its separate endpoint,
extracted a replacement token from a response header, and applied its configured 120-second
lifetime. The next API send used the replacement, verified by a boolean-only server reply.

Confirmed Forget changed the Custom Login status to **No local token**, while the inherited
OAuth record remained usable. Selecting Testserver caused a separate OAuth acquisition while
credential inputs fell back to Defaults. Final controlled-server counters were two OAuth,
one login, one renewal, and five API requests. No token background call became an API response
or a history entry. Response timing observations were functional checks, not benchmarks.

Visual inspection found that the long form could push the toolbar out of view. The final UI
keeps token controls first and bounds the request auth panel's scrolling; auth selects also
render with a readable dark background in WebKitGTK. The rebuilt isolated app was inspected at
the default window size. Reopening the same project retained Testserver selection, and the
saved Custom Login request displayed Saved without edits. Closing then exited without a false
dirty prompt. The final regular app was rebuilt with its normal identity after this smoke test.

The test window and loopback server on port 1436 were stopped. All known synthetic GUI-test
variable/token entries were explicitly deleted and checked absent. No GUI fixture was saved
in the repository. Accessibility inspection emitted stale bus-registration warnings, but the
test app remained accessible. No GStreamer scanner warning appeared in the observed token-test
app output; this does not resolve the earlier baseline's host-runtime warning.

### Contracts and remaining checks

[TOKEN_AUTHENTICATION.md](TOKEN_AUTHENTICATION.md) defines the additive version-1 file shapes,
token identity, expiry, command metadata, bounds, HTTPS/loopback policy, process-local flight
coordination, and partial OS-write/cancellation recovery. The already locked Base64 0.22.1
package became a direct MIT/Apache-2.0 dependency; no new package version was added.

No automatic token cleanup runs on configuration/environment removal; use Forget before
removal when cleanup is desired. Separate processes may independently acquire the same token.
These limits are explicit, not atomicity guarantees. Existing manual auth and file-save behavior
remain intact. This milestone's new native behavior has not been checked on macOS or Windows.
Release packaging, other Linux distributions, total memory, startup time, and input latency
were not benchmarked. Browser OAuth, scripting, automatic replay after API rejection, history,
and exchange formats remain outside this milestone.

## Milestone 7 — local history (Linux, 2026-10-23)

History was implemented on the same Linux Mint 22.3 / Ubuntu 24.04 x86_64 X11 host,
with the pinned Node 24.11.0, npm 11.6.1, and Rust 1.92.0. Existing milestone-6 work was
preserved. The user approved literal-host visibility, redaction of all literal path/query
content, and preserving history on Close project. [HISTORY.md](HISTORY.md) defines these
choices, opt-in default, file/IPC shapes, retention, and recovery. No dependency was added.

### Automated validation

- `npm ci --offline --cache /tmp/fluxkit-npm-cache` installed the committed lockfile.
- Frontend typecheck, lint, formatting, and production build passed.
- Frontend tests passed: 51 tests in nine files.
- Rust formatting, locked Clippy for all targets with warnings denied, locked tests,
  and locked native build passed: 53 tests passed; two explicit OS-store integration
  tests remained ignored in the routine suite. Those adapters were unchanged here;
  their prior isolated Linux integration checks are recorded above.

History coverage exercises opt-in persistence, exact four-field IPC entries, payload/value
exclusion, literal URL redaction, fail-closed malformed inputs, status/time from a controlled
HTTP exchange, null status/time, equal-time ordering, exact seven-day expiration, the count
limit, future timestamps after clock rollback, closed-project cleanup, disabled recording,
independent project identity, concurrent completions, generation changes during requests,
corrupt/future/oversized files, locks, crash staging files, and unsafe symlinks. Retention
tests use a deterministic clock. UI tests cover recording controls, confirmation, clearing,
project switches and stale replies, polling cleanup, storage errors, and response preservation
on metadata failure. A dialog regression ensures loading does not focus a disabled checkbox,
so Escape can close the history panel before storage finishes.

### Native history smoke test and measurements

A separately named **Fluxkit History Smoke** app (`app.fluxkit.historysmoke`) used temporary
XDG data/config directories and a synthetic project. No real project or OS credential
entry was accessed. A loopback server on port 1437 returned synthetic headers and JSON.
A successful request before opt-in left history empty. After enabling recording, a second
success showed HTTP 200 and the actual response timing; a missing-variable attempt showed
no status or timing. The native table displayed exactly Method, URL, Status, Time.
The literal 127.0.0.1 host remained visible; the port, path, query name/value, and fragment
were absent or masked. Inspection of the private mode-0600 history file found neither the
private URL markers nor response header/body markers.

Closing the project and app preserved both entries. Restarting and reopening the project
showed both again. Disabling recording retained them and excluded the next successful
Send. Clear history presented a confirmation; cancellation preserved entries and explicit
Continue emptied them while preserving recording off. Escape closed the rebuilt panel.
Screenshots were visually checked at 1320×880: the four columns remained readable, and a
100-entry fixture scrolled inside a bounded table with controls still visible.

For a separate startup fixture, 100 current entries and one entry at the exact seven-day
boundary were written only to the closed test app's temporary history file. On restart,
the expired entry was removed before any project or history panel was opened, including
with recording disabled. The remaining 100 entries were displayed successfully.

Single observations on this development host, debug native build and bundled production UI,
warm build/filesystem caches, synthetic two-request project, 100 current history entries:

| Observation | Result |
| --- | --- |
| Process launch to accessible Desktop connected UI | 800.2 ms |
| History activation to accessible table containing 100 rows | 391.5 ms |
| Native host RSS at connected startup | 155,452 KiB |
| Native host RSS with 100-row history open | 159,696 KiB |

Timings include Linux AT-SPI traversal, remote accessibility calls, and polling overhead
(50 ms startup polling, 10 ms history polling). These are functional development observations,
not repeatable latency benchmarks or release guarantees. RSS is `/proc/<test PID>/status`
for the native host only; it excludes separate WebKit processes and is not total app memory.
No comparison with another build or operating system was performed.

The isolated app and server were stopped. The regular desktop executable was rebuilt with
its normal identity. Accessibility tooling emitted stale bus-registration warnings without
preventing the test. This milestone's native behavior remains unchecked on macOS and Windows;
release bundles and other Linux distributions remain unverified. Import/export remains the
next milestone and has not been implemented as part of history.


## Milestone 8 first slice — OpenAPI import (macOS, 2026-10-23)

OpenAPI import was implemented on the macOS Apple Silicon host with Node.js 24.11.0,
npm 11.6.1 and pinned Rust 1.92.0. The initial working tree was clean; existing milestone
6/7 implementation and Linux validation were retained. This pass does not complete
milestone 8: Postman import, OpenAPI YAML export and native export/import remain planned.
The supported mapping and recovery contract is in [OPENAPI_IMPORT.md](OPENAPI_IMPORT.md).

TypeScript, ESLint without warnings, Prettier, 55 frontend tests and the production UI build
passed. The default native suite passed 66 tests with the two pre-existing explicit OS-store
integration tests excluded by design; binary and doc targets passed. Rust formatting, Clippy
with warnings denied, locked native build and the debug macOS bundle passed. An initial
native doc-test pass conflicted with artifacts from a concurrently running Tauri build;
the complete native checks were rerun sequentially and passed without disabling any check.
Routine import credential tests use fake stores; the underlying OS adapter was not changed.

The new native fixtures cover OpenAPI 3.0.x/3.1.x YAML and JSON, selected scalar/default/body
mappings, path parameter encoding, operation overrides, tag folders, Basic/Bearer/API Key/
OAuth2 configuration shapes, internal/escaped references, recursive schemas, unsupported
security/serialization blockers, duplicate keys, unknown versions, external/broken/cyclic
references, tagged/multi-document/oversized sources, metadata preservation and stable operation
origins across request renames, and literal
placeholder capture prevention. Storage tests cover new IDs/collision names, preview-before-write,
definitions-only import without OS access, stale preview rejection, credential failure with
visible partial definitions, retry without duplicate definitions, edited OS values remaining
untouched, and symlink boundaries. Frontend tests cover compatibility/default/missing-value
presentation, inert source text, blocked confirmation, explicit value policy, cancel/discard,
partial error/retry, and preserving an open dirty request across import.

The isolated bundled app Fluxkit Import Smoke used a separate app identifier and a fresh
synthetic project. The native file picker opened the 3.1 JSON fixture. Its preview showed
one request, one tag folder and three new variables (two available defaults, one missing
credential). Cancel was verified to create no collection or environment definition file.
A second preview was confirmed with OS-value storage disabled. The new collection/folder,
URL/form placeholders and API Key configuration appeared through the real native bridge.
Editing the imported request name and using Cmd+S preserved its OpenAPI metadata. The
collection and definitions were separately inspected: three definitions, no imported default
values in JSON, retained auth/form/schema data, and a normal saved request. No OS entry or
API call was made in this desktop smoke test. The test app was closed afterwards, and the
regular app was rebuilt with its normal identifier. The user's running Fluxkit session
was not manipulated. No development server was started or left running for this pass.

A controlled debug Rust mapping measurement processed 47,232 bytes and 500 synthetic
operations in an observed 29 ms on this host, producing a 200-row preview and one shared
base-URL definition. This is a single parsing/mapping observation, not startup or interaction
latency and not a production speed guarantee. An isolated native debug-host RSS snapshot
after importing, editing and saving the small fixture was 123,744 KiB (about 120.8 MiB),
excluding WebView helper processes. It is not total app memory or a comparison to another
client. GUI automation overhead was not treated as an interaction latency measurement.
The uncompressed debug app bundle is approximately 38 MiB, not a release size guarantee.

The lockfile adds serde-saphyr 2.0.0 plus ten transitive packages. Upstream MIT/Apache-2.0
and additional encoding_rs BSD-3-Clause notices are preserved in assets/notices/openapi-import
and included as bundle resources. A regeneration helper reads the pinned Cargo metadata;
no new frontend dependency, hosted service, telemetry, parser include resolver or automatic
reference fetch was introduced. Native preparations/commits are serialized, source/graph/work
and preview rendering limits are explicit, and candidate values remain native in memory.

Windows/Linux import integration and packaging have not been run in this pass. Existing
Linux milestone 6/7 records remain valid as historical checks, not evidence for the new
importer. The parser is not a full OpenAPI/JSON Schema validator; unsupported mappings,
source preservation omissions, final-check/write races and multi-system partial recovery
are documented rather than presented as lossless migration or an atomic transaction.

## Milestone 8 second slice — Postman import (macOS, 2026-10-23)

The next implemented slice adds Collection v2.1 JSON and separate environment JSON imports,
with the user's explicit choice to associate environment source names with a previously
imported collection. Existing OpenAPI work in the working tree was retained. The mapping,
file compatibility, limits and partial-recovery contract is in
[POSTMAN_IMPORT.md](POSTMAN_IMPORT.md). Milestone 8 remains in progress: OpenAPI YAML export
and native Fluxkit backup export/import have not been implemented in this slice.

TypeScript, warning-free ESLint, Prettier, all 59 frontend tests and the production UI build
passed. The native suite passed 74 tests; the same two explicit real OS-store integration
tests remain opt-in and were not executed. Rust formatting, warning-free Clippy, locked
native build and the macOS debug desktop bundle passed. Checks were sequential around
native compilation/bundling. The environment adapter itself is unchanged; import storage
uses fake credential adapters in automated tests. An environment confirmation-label test
initially failed and exposed a real UI label error; the label was fixed and the full frontend
suite rerun. A formatting check also caught a newly added expression; rustfmt corrected it.
No checks were weakened or disabled.

Native fixtures cover Collection v2.1 JSON, explicit version rejection, strict JSON/duplicate
keys, structured/raw URLs, encoded query/path values, protocol-variable rewriting, missing
variables, hierarchy, disabled rows, raw JSON/text, URL-encoded/multipart text bodies, explicit
file reselection, Basic/Bearer/API Key and OAuth configuration, auth inheritance, unsupported
scripts/dynamic variables/auth/body/file fields, and source-value/response/script omission.
Environment tests verify new project-wide identities, collection-default fallback, matching
source-name identities, disabled/unused entries, two isolated environment imports, definitions
only without OS access, stale preview rejection and partial OS failure followed by retry.
Repeated collection imports use new names and identities. Frontend coverage adds source-to-local
mapping, explicit environment association, disabled selection before any imported collection,
the environment confirmation action and preservation of dirty drafts across Postman preview.

An isolated Fluxkit Import Smoke bundle used a separate app identifier and a new temporary
synthetic project. Native file dialogs selected the committed synthetic Collection v2.1 and
environment fixtures. The collection preview showed five requests, two nested folders,
18 variable definitions and script/runtime compatibility warnings. Cancellation created no
collection or definitions file. A subsequent definitions-only import created the collection
and displayed the folder/request tree through the native bridge.

The environment picker offered the imported collection. Its preview showed a new project-wide
environment, three mapped values, disabled-entry/unused-key notices and original-to-local
variable names without candidate values. Definitions-only confirmation added one environment
and one standalone variable (19 project variables total), retained the previous Defaults
selection, and left the collection file byte-for-byte unchanged. The new environment appeared
in the project selector and could be selected. Opening a migrated request showed Basic auth
and its JSON body; changing its name and saving with Cmd+S retained all three Postman source
variable associations. File inspection confirmed that source credentials, source environment
values, saved responses and scripts were absent from persisted project JSON. No API call or
OS credential write was made in the GUI smoke test. The preview was visually checked at a
1320×900 window size with expanded variable mappings and visible confirmation controls.
The isolated app was closed afterwards; the regular bundle was rebuilt with its normal identity.
No development server was started or left running by this pass.

A single controlled debug Rust parsing/mapping observation processed 38,468 bytes and 500
synthetic requests in 30 ms on this macOS Apple Silicon host with warm build/filesystem caches,
pinned Rust 1.92.0 and fake/no credential I/O. The preview contained 200 rows and one shared
base variable. This measures mapper work, not startup, GUI latency or total application memory;
it is not a production performance guarantee. No new dependency was added for this slice.

Windows/Linux Postman integration and packaging have not been checked. Real credential-store
permission prompts are covered by existing opt-in adapter integration workflows rather than
claimed as part of this definitions-only desktop smoke. The importer intentionally does not
reproduce the complete Postman runtime, scripts or all serialization/auth behaviors; the
compatibility report and documented subset are the supported migration boundary.

## Milestone 8 third slice — OpenAPI YAML export (macOS, 2026-10-23)

The user selected OpenAPI 3.1.0 and saved-state-only export, with no OS value access.
Request, folder-subtree, collection and project selection are implemented; project export
creates a new directory with one document per non-empty collection. Existing import work
was retained. [OPENAPI_EXPORT.md](OPENAPI_EXPORT.md) defines mapping, privacy, conflicts,
metadata conversion, validation boundaries and file recovery. Native Fluxkit backup/export
restoration remains the unfinished part of milestone 8.

TypeScript, ESLint without warnings, Prettier, all 62 frontend tests and the production UI
build passed. The full locked native suite passed 83 tests, with the same two opt-in real
OS credential-store integration tests not run. Rust formatting, warning-free Clippy, locked
native build and the macOS debug bundle passed. The separate desktop smoke instance and
normal bundle were built sequentially with the tests. A schema-shape regression test caught
an overly broad early type check that confused security-scheme types with JSON Schema types;
the check was scoped to schemas, and all tests passed after correction. No checks were weakened.

Nine new Rust tests cover values/payload exclusion, saved identities and all four selection
scopes, inherited Basic/Bearer/API Key/OAuth descriptions, explicit None and manual overrides,
Custom Login omission, duplicate/equivalent paths, unsupported methods and opaque URL shapes,
metadata response/schema retention after rename, known 3.0 nullable/bound conversion, invalid
response/reference blockers, deterministic 500-operation mapping, new-file/no-clobber writes,
symlink targets and stale preview revisions. Three frontend tests cover selection identities,
unsaved-state notices, blockers/retryable errors, native chooser cancellation and preserving
an unsaved draft after a successful export.

Independent validation used Python jsonschema 4.25.1, PyYAML 6.0.3 and the official OpenAPI
3.1 schema dated 2022-10-07 in a separate temporary virtual environment. All six generated
native-test examples and three desktop-generated documents passed its OpenAPI structure
checks; retained component schemas also passed Draft 2020-12 schema checks. This validates
the fixtures, not every possible dialect or arbitrary imported schema. The desktop app uses
bounded structural/reference/round-trip checks rather than bundling this Python validator.

To reproduce the independent check, create a temporary virtual environment, install those
pinned validation packages, and obtain the official schema explicitly from
`https://spec.openapis.org/oas/3.1/schema/2022-10-07`. The developer helper itself disables
remote schema retrieval and prints no payloads. Set `FLUXKIT_EXPORT_FIXTURE_DIR` to a temporary
directory while running `cargo test --manifest-path src-tauri/Cargo.toml --locked exports::tests`,
then run the virtual environment's Python with
`scripts/validate-openapi-export.py --schema <downloaded-schema.json> <fixture-directory>`.
Use only synthetic fixtures in this workflow. These Python packages are not app/build dependencies.

The isolated Fluxkit Import Smoke bundle used a separate app identifier and a new synthetic
project with two collections, three saved requests, nested folders, Bearer inheritance and
synthetic query/header/body canaries. A request name was changed without saving. Export
showed the saved-state warning; cancelling the folder chooser retained the preview. Confirming
project export created exactly two YAML files in a new folder and left the dirty draft open.
All source JSON hashes remained identical. The output contained the original saved request
name and excluded the unsaved name plus the query/header/body canaries. The project export
preview was visually inspected at 1320×900, with readable file/mapping notices and controls.

The test draft was reset to its original name without saving. The Request scope selector
and native Save dialog then produced one standalone YAML file. All three GUI-created files
passed the independent schema check. No OS value read/write, attachment read or API request
was performed by export. The test app was closed, and the regular app was rebuilt with its
normal identity. No development server was started or left running.

A single controlled debug observation mapped and serialized 500 synthetic saved operations
into 149,386 YAML bytes in 138 ms on this Apple Silicon host, pinned Rust 1.92.0, warm build/
filesystem caches, no credential/network I/O. This includes bounded YAML round trips and
preview counting; it is not startup, interactive latency, total memory or a release guarantee.
The uncompressed debug bundle is about 40 MiB, not a release size claim. Serialization enables
the existing serde-saphyr feature and adds only nohash-hasher 0.2.0 to the lockfile relative
to the importer; both upstream MIT/Apache notices are present in the normal app resources.

Windows/Linux export integration and packaging are unverified. The exporter intentionally
uses synthetic OAuth/server configuration, omits payload examples and does not certify
arbitrary schema dialect conversions. Multi-file destination failures can leave a partial
new export folder; the UI/documentation report this instead of claiming transactionality.

## Import compatibility fixes and persisted review (macOS, 2026-10-23)

A user-reported OpenAPI 3.0.0 migration failed because missing/empty operation responses
were rejected even though request execution does not require a response schema. Missing,
null and empty response definitions now produce one compatibility warning and still create
requests without invented response schemas. Malformed non-object definitions remain errors.
Regression coverage exercises all three incomplete forms across 3.0.0, 3.0.4 and 3.1.2 and
confirms definitions-only persistence.

The user's Postman compatibility report identified dynamic variables, unsupported auth and
multipart file fields. The user explicitly selected importing those requests with a mandatory
review lock instead of rejecting the entire collection. Optional version-1 `importIssues`
notes now persist this state; older readers reject the new field. Source dynamic variables
have no generated values, source attachment paths are never opened and unsupported auth is
inactive. Inherited review requirements propagate to descendant requests; unrelated sibling
requests stay unlocked. Multipart file placeholders still need a local attachment or an
explicit field/value change even after review acknowledgement. Other unsupported mappings
continue to block import, with the reason beside the disabled confirmation button and a
keyboard-accessible action to focus/scroll to the blocker report.

The editor preserves notes while editing and requires **I reviewed and configured this
request** plus successful Save. Native request preparation checks submitted and persisted
review state before variable-value access, HTTP execution or token acquisition, so omitting
the draft flag does not bypass a saved scoped request's lock. A regression test verifies zero
credential reads for blocked preparation and checks token preparation and successful review
persistence. Frontend coverage verifies the disabled action's accessible reason, focus on
blockers, a failed review save retaining the lock and a successful save lifting it.

All 64 frontend tests, TypeScript, lint, formatting and the production UI build passed.
The full native suite passed 86 tests with the two existing opt-in real-store tests not run;
Clippy with warnings denied, Rust formatting and locked native build passed. The underlying
OS adapter was not changed and fake adapters cover review/value behavior.

In a separate Fluxkit Import Smoke identity and synthetic project, the real native picker
imported a 3.0.0 YAML operation with `responses: {}`. Its preview warned and allowed import;
one GET request appeared, and file inspection confirmed no response definition was invented.
A second synthetic Postman collection combined all three reported compatibility cases in
one request plus one supported sibling. The preview allowed import and reported one locked
request. Both requests appeared; the affected editor showed all three actionable notes and
disabled Send and Send & save response. Its layout was visually checked at 1320×900.
For this synthetic test, the URL was replaced by an explicit synthetic URL, the dynamic query
and upload field deliberately disabled, and inactive auth was retained intentionally. Checking
the review checkbox alone kept sending disabled; saving removed the notes and enabled the
button. No API request was sent. Persisted JSON retained both requests, kept the sibling
unlocked and excluded the synthetic source attachment path. Both imports were definitions
only, with no OS value writes. The test app was closed; the normal app was rebuilt. No dev
server was started. Windows/Linux integration remains unchecked for this change.

## Response presentation (2026-10-23)

New formatted JSON responses select JSON automatically; manual selection stays stable for
the same result. Dispatch environment/token metadata is initially collapsed in Details,
with keyboard activation and Escape to close. HTTP status badges distinguish 2xx, 3xx and
4xx/5xx while preserving readable numeric codes. Errors and truncation remain visible.

All 74 frontend tests passed, including new response-selection, disclosure and status-range
coverage. TypeScript, lint, formatting and the production UI build passed. A debug macOS
bundle was exercised under the separate Fluxkit Import Smoke identity against a loopback
server with synthetic responses: JSON opened automatically, a manual Raw selection reset
to JSON on the next send, text selected Raw, and 200/302/500 badges appeared green/yellow/red.
Details opened as an overlay without consuming response space and closed with Escape.
The test app and server were stopped. Native behavior was unchanged; native tests were not
repeated for this UI change. Windows/Linux UI integration was not exercised.

## Themed selection menus (2026-10-23)

All application selects now use one themed combobox/listbox, including HTTP method, body,
project/environment, authentication, import and export controls. The popup is positioned
within the viewport, scrolls for long lists, and stays inside its dialog's accessibility
scope. Focus remains on the trigger, including after a macOS mouse click. Arrow keys,
Home/End, prefix search, Enter/Space, Escape, Tab, disabled options and outside dismissal
are covered. A visually hidden native select preserves form values and required-field
validation; only the first invalid control receives focus, and hidden controls are excluded
from the dialog's focus trap. No dependency or native/storage contract was added.

TypeScript, lint, formatting, all 79 frontend tests and the production UI build passed.
Workflow tests now exercise real menu clicks; exclusion checks inspect open lists so that
unsupported choices remain tested. Five component tests cover pointer/keyboard behavior,
disabled/empty lists, dialog Escape handling, form values and required-field focus.
In the separate macOS test app, the HTTP list displayed the dark theme without a native
popup; clicking then using Down/Enter selected POST, and typing G/Enter selected GET.
The body menu selected JSON by typing. An export menu inside a synthetic project's dialog
was visually checked; Escape closed its menu while keeping the dialog open. No requests,
exports or credential operations were performed. The test app was closed and the normal
macOS app rebuilt. Native tests were not repeated for this frontend change; Windows/Linux
UI integration remains untested.

## Last active project restoration (2026-10-23)

Desktop startup restores the last active project's saved definitions using a versioned,
bounded `workspace.json` in OS app-data storage. Only an authorized open handle can update
the target. Stored project identity and ordinary project validation are checked before
accepting a restore. Missing or invalid project/settings data produces an actionable error
without clearing the previous target or overwriting malformed settings. Writes are ordered,
atomic, locally locked and recoverable through a previous valid copy. Explicitly closing
the last project clears the startup target. Normal window closure waits for queued writes.

All 85 frontend tests passed, including StrictMode restoration, pending-startup controls,
missing-project recovery, activation/close persistence, settings-write failure and queued
write ordering before a clean close. TypeScript, lint, formatting and production UI build
passed. Rust formatting, Clippy with warnings denied, the locked native build and full test
suite passed: 91 tests passed; the two existing opt-in real credential-store tests were not
run. Five new native tests cover startup round trips/clear, invalid/oversized settings,
unknown handles, replaced/missing projects, Unicode paths, symlinks and backup-write failure.

In the separate macOS test identity, a synthetic project was created in a folder containing
Unicode and spaces, with a saved Synthetic API collection. After quitting and relaunching,
both project and collection appeared automatically. Temporarily moving only this synthetic
folder produced the expected warning and usable empty workspace; restoring its original
location allowed the next startup to reopen it, confirming the target survived the failure.
Explicit Close project followed by quit/relaunch stayed empty. The temporary move was
reversed and the test app closed. No API requests or credential operations were performed.
The normal app was rebuilt. Windows/Linux startup integration remains untested.

## Save before quitting (2026-10-23)

Desktop Quit and window closure offer Cancel, Discard changes and Save & quit. Saving
groups dirty request tabs by project and collection, preserves expected-revision checks,
and then saves pending authentication/variable editor changes. Collection auth saves use
the revision returned by request saves, avoiding overwrites or self-conflicts. Background
editing is inert during the dialog, repeated quit requests cannot duplicate writes, and
buttons are disabled while saving. Failure keeps the app open with an actionable error;
successful partial saves remain clean and retry addresses the remaining edits. Cancel and
Escape return to editing; a failed save restores keyboard focus to Cancel.

TypeScript, lint, formatting, all 94 frontend tests and the production UI build passed.
Nine added tests cover cancel/Escape/discard, delayed completion and repeated close events,
all tabs in one collection, missing destinations, collection-auth revision ordering, invalid
authentication, partial failure/retry, cloned collection IDs in separate projects, and
pending definitions followed by an OS value save or a simulated credential-store failure.
Routine value tests use only fake adapters and synthetic values. Native behavior/contracts
were unchanged; the Rust suite was not repeated for this frontend change.

In the separate macOS test app, Cmd+Q showed all three actions with Cancel initially
focused. Escape retained both dirty requests. Save & quit saved both before closing;
direct inspection of the synthetic collection and a relaunch confirmed both requests.
Editing and then choosing Discard changes left both saved URLs unchanged. A synthetic
external file edit produced a real revision conflict: Save & quit kept the app open,
displayed the conflict and preserved the draft and external file contents. After exiting
the test, the synthetic external edit was reversed using its original saved bytes. No
API requests or OS credential operations occurred during the desktop test. The test app
was closed and the regular macOS app rebuilt. Windows/Linux UI integration remains untested.


## Readable OpenAPI imports (2026-10-23)

OpenAPI imports create project variables only for distinct server URLs and authentication.
Names are readable, existing project names are reserved, and numeric suffixes handle actual
collisions. Normal primitive parameter/form examples and defaults stay in request files;
optional rows remain disabled. Missing required values and unsafe source placeholders produce
persistent request review locks. Preview renaming updates all executable references while
preserving UUID credential associations, source metadata, and the plan on validation failure.

TypeScript, lint, formatting, all 98 frontend tests and production UI build passed. Rust
formatting, Clippy with warnings denied and all 95 routine tests passed; two explicit OS
credential integration tests remained ignored. Native tests used synthetic data and fake
credential adapters, with loopback servers for HTTP checks. New regression cases cover 60
routes with only a shared server variable, collision suffixes, independent literal values,
missing/unsafe fields, preview rename validation, stable auth IDs and bounded rename growth.
UI tests cover preview rename/apply/rejection, pending edits and filtering beyond 100 entries.

The separate macOS test app imported a synthetic 60-route document as definitions only.
The preview showed one variable; renaming baseUrl to apiServer updated route URLs together.
Direct inspection of live project files confirmed all 60 saved URLs, inline query/header/path
examples, exactly one variable definition and the existing two-request collection. No API
requests or OS credential operations occurred. The test app was closed and the regular
macOS app rebuilt. Windows/Linux desktop integration remains untested.


## Authentication tests, request assignment and header inspection (2026-10-23)

Authentication tests use current editor settings and project-wide environment selection.
Basic/Bearer/API Key use an explicit GET test URL; OAuth2/Custom Login acquire and discard a
fresh token without reading or replacing stored token records. Settings, response payloads
and history are not written by tests. Explicit header inspection is local and native,
masking all manual/auth values and templated header names. Dispatch results carry a separate
redacted snapshot after token placement. Defaults include versioned User-Agent and Accept;
manual/auth fields take priority. Content-Type still follows the body type. Internal draft
drops and a destination picker assign loose unsaved requests without writing until Save.

Frontend validation passed with 109 tests, strict types, ESLint, formatting and production
build. Added interaction coverage includes explicit GET vs token tests, safe status/error
feedback, late registration cancellation, unmount cancellation, draft vs dispatch header
views, parent-scope tests for unsaved inheritance, auth-method switches, assignment/save,
cross-project guards and collection/folder drops. Rust validation includes fake-store login
tests (success, rejection, timeout/cancel, no token record reads/writes), wire-level default
and override checks, redaction and reserved token-header names. All 99 routine Rust tests,
Clippy with warnings denied and Rust formatting passed. The two opt-in OS credential
integration tests were not run; routine tests use synthetic values and isolated loopback
servers. No real user credentials were accessed.

A separate macOS test app opened a synthetic project, assigned a session request through the
collection picker and verified immediate inherited Custom Login. Its test returned success;
an unsaved collection login endpoint change to a rejecting local route reported HTTP 401
without saving that change. Header inspection showed Fluxkit/0.1.0, Accept and a masked
pending Authorization. Switching request auth to None left exactly one GET test; it reported
HTTP 204. A separate actual send returned 204 and displayed its last-dispatch header table.
Captured synthetic server requests confirmed versioned User-Agent and Accept on login and
API calls. Project-file inspection confirmed the saved request and unchanged collection
login endpoint. No OS credential entries were read or written in this desktop workflow.
Drag-and-drop has interaction-test coverage; native mouse-drag behavior and Windows/Linux
integration still require platform checks. The test app was closed and the local test server
stopped before rebuilding the regular macOS app.

## Saved request: Test → Prod → Test (2026-10-23)

Reviewed environment selection propagation, native preparation, environment-specific variable
resolution, token account identity and acquisition coalescing. No production-code change was
needed. Added a saved-collection/request regression through native preparation, real loopback
HTTP acquisition/dispatch and fake credential storage. It covers OAuth2 Client Credentials
and Custom Login, each with successful or rejected Prod login, distinct Test/Prod client IDs,
secrets, token/API URLs, and deliberately unusable shared defaults. Wire assertions verify
both login credentials and each API's Authorization header. Returning to Test uses its stored
record even with a fresh token manager; Prod failure leaves Test unchanged and sends no Prod
API request. An old environment selection is rejected before variable/token reads.

A second native regression overlaps Test and Prod acquisitions with one manager and confirms
separate HTTP credentials, returned tokens and records. A frontend workflow sends the same
saved inherited-OAuth request in Test, Prod and Test, verifies the three IPC environment scopes
and confirms that no collection save is needed. All values are synthetic; no private projects,
real OS credential entries or external Test/Prod APIs are accessed. These are automated UI
interaction and native HTTP tests, not a new desktop or OS credential-store integration run.

Validation passed: 110 frontend tests, TypeScript, ESLint, Prettier and production UI build;
101 routine Rust tests, Rust formatting, Clippy with warnings denied and native macOS build.
The two opt-in OS credential integration tests remained ignored. Application behavior and
file/IPC contracts were unchanged; only regression tests and supporting documentation changed.

## Saved request relocation and location picker (2026-10-23)

Move… next to Save and the clickable breadcrumb location open one confirmation dialog for
saved or unsaved requests. It shows full nested folder paths and destination authentication;
Save & move persists open request edits and moves within the same project. Saved tree rows
and request tabs can supply drag data to the same dialog. No file write occurs merely from
choosing a destination or dropping. The main workspace is inert while the dialog is open;
the dialog is outside that inert subtree and becomes inert when the quit dialog is shown.

The new native move_request command validates project/source/target revisions, IDs, target
folders, duplicate request UUIDs, sizes and schema-association compatibility under the
project lock. Same-collection moves replace one file without reordering other requests.
Cross-collection moves copy first, recheck the destination, then remove the source; source
failure restores only the exact destination snapshot written by this operation. A competing
external edit is preserved and receives an explicit partial-copy recovery message. This is
not an atomic multi-file transaction; interrupted-move recovery is documented in FILE_FORMAT.
Source metadata is retained; different OpenAPI documents require explicit association omission.

All 114 frontend tests, strict TypeScript, ESLint, Prettier and production UI build passed.
All 108 routine Rust tests, Rust formatting and Clippy with warnings denied passed; two opt-in
OS credential integration tests remained ignored. Seven native relocation regressions cover
same/different collections, stable identity/auth, edited content, backups, stale revisions,
invalid folders/IDs, target collisions, source-write rollback, external target edits and
OpenAPI association preservation/confirmation. UI regressions cover save/move, cancel, retry,
full paths, inherited auth, native revision inputs, saved-tree drops, metadata confirmation
and the modal's exclusion from the inert workspace. Other dirty drafts remain open.

A separate macOS test app moved a saved synthetic request from Source API to Destination API
/ Reports / 2026 and saved an edited URL in that operation. The UI showed its new path, saved
state and destination auth. Clicking the breadcrumb and choosing Collection root successfully
moved it again within the destination. Direct inspection of live project JSON confirmed the
empty source, exactly one destination request, unchanged UUID, saved URL and root folder.
No API requests or OS credential operations occurred. Native mouse-drag attempts through the
UI automation did not reliably trigger a move dialog, so native drag behavior is unconfirmed;
the location picker is the verified workflow. The test app was closed and the regular macOS
app rebuilt. Windows/Linux integration and native mouse dragging require platform checks.
