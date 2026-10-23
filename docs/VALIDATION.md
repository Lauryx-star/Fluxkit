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

Milestones 1 through 5 implement desktop editing, local projects, project environments,
OS credential values, native HTTP execution, response views, explicit streamed downloads,
and inherited Basic/Bearer/API Key authentication. Token acquisition, history, and import/export
remain planned. Use placeholders for
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
