# Fluxkit

A local, open-source desktop API client built with Rust, Tauri, React, and TypeScript.

## Current state

Milestones 1 through 7 provide desktop editing, local projects, environments, native HTTP execution,
inherited authentication, OAuth2 Client Credentials, Custom Login, and minimal local history.
Create or open project folders, organize collections and nested folders, and edit requests
with parameters, headers, text/JSON/form bodies, and portable file attachments.

Use **Save** or **Cmd/Ctrl+S** to save the selected request. Unsaved changes are marked;
closing a dirty request, reloading/closing its project, or closing the desktop window
requires confirmation. Structural operations such as creating collections and renaming
folders save immediately. **Close project** keeps its files on disk. The last active project
reopens automatically on desktop startup; request tabs and unsaved drafts are not restored.

Quitting with unsaved changes offers **Cancel**, **Discard changes**, and **Save & quit**.
Save & quit saves dirty request tabs across all open projects and pending authentication,
variable definitions and edited variable values before closing. Missing destinations,
invalid fields and save failures keep Fluxkit open; successful partial saves remain saved.

See [local file format and recovery](docs/FILE_FORMAT.md) for storage, conflict handling,
limits, backups, and removal behavior. User request fields are plain JSON; use placeholders
such as `{{apiToken}}` for reusable values, stored through **Variables** in the OS credential store.

Select project-wide environments in the header. Environment values override shared defaults;
**Check variables** resolves the active draft without sending it or exposing resolved values.
Request fields highlight placeholders and suggest variable names after `{{`; hover a
highlighted token to explicitly preview its effective value.
See [environments and OS values](docs/ENVIRONMENTS.md) for syntax, limits, and deletion recovery.

**Send** executes the active draft with Cancel, a 60-second default timeout, and Raw/JSON/
Headers response views. **Send & save response…** streams a new request's response to a
chosen file. Previews are limited to 1 MiB. See [HTTP behavior and downloads](docs/HTTP.md).
Configure Basic, Bearer/JWT, API Key, OAuth2 Client Credentials, or Custom Login in the request **Auth** tab or through project, collection,
and folder authentication settings. Credential fields reference project variables; manual headers
and query parameters take priority. See [authentication and inheritance](docs/AUTHENTICATION.md).
Token acquisition, expiry, manual renewal, and local token removal are described in
[token authentication](docs/TOKEN_AUTHENTICATION.md). Native backup export/import remains planned in
[the product plan](docs/PRODUCT.md) and [milestones](docs/MILESTONES.md).
Browser preview supports draft editing; native sending and project files require the desktop app.

The desktop app reopens the last active project on startup. Its location is remembered in
local app-data settings; unsaved drafts and responses are not restored. Closing the last
project explicitly leaves the next startup empty. If its folder moved or is unavailable,
use **Open project** to locate it again. See [local startup settings](docs/FILE_FORMAT.md#local-startup-selection).

Selection menus use the Fluxkit theme on every platform. Use arrow keys or Home/End to
navigate, type a name prefix to find an option, and press Enter or Space to select it.
Escape closes without changing the value; Tab closes and moves to the next control.

Open **Request history** for a project and enable **Record request history** to opt in.
History stores up to 100 entries for seven days, with literal hosts visible locally and
path/query content redacted. Closing a project preserves history; **Clear history** explicitly
clears it. Bodies, headers, and resolved values are excluded. See [the history contract](docs/HISTORY.md).

The first milestone 8 slice adds **Import OpenAPI…** for OpenAPI 3.0.x/3.1.x YAML and JSON.
Review the mappings and compatibility report before adding a new collection. Defaults/examples
can be stored as new OS variable values or imported as definitions only. Internal references
are supported; external references are blocked without network access. See
[OpenAPI import and partial-failure recovery](docs/OPENAPI_IMPORT.md).

An initial self-hosted server foundation is available under [server/](server/README.md):
ASP.NET Core with PostgreSQL, local account registration and admin approval, personal/team
project access, revision-checked project snapshots, and per-user encrypted-secret envelopes.
It is not yet connected to the desktop client. OIDC, local-to-server migration, offline
snapshot handling, and end-to-end server mode remain in progress; see
[the server architecture and API notes](docs/SELF_HOSTED_BACKEND.md).

Only OpenAPI server URLs and authentication create project variables, with readable names
such as `baseUrl`. Normal parameter values remain editable at each request. The variable
list explains their origins. Use
**Rename** in the import preview to change a name once across all imported request references.
Set the server variable's value once in **Variables**, optionally per environment.

**Import Postman…** accepts Collection v2.1 JSON and separate environment exports. Choose the
imported collection whose variable mapping an environment should use. Review new names and
unsupported scripts/runtime features before confirming. Dynamic values, unsupported auth
and multipart files can be imported with a persistent request review lock: configure them,
confirm the review and Save to enable sending. See [Postman migration](docs/POSTMAN_IMPORT.md).

**Export OpenAPI…** previews the saved project, a collection, a folder subtree or one request
as OpenAPI 3.1.0 YAML. Project export creates one document per non-empty collection. Local
values and payload examples are excluded; conflicts block saving. See [OpenAPI export](docs/OPENAPI_EXPORT.md).

## Requirements

- Node.js 24.11.0 and npm 11.6.1, recorded in `.nvmrc` and `package.json`.
- Rust 1.92.0 with rustfmt and Clippy, pinned in `rust-toolchain.toml`.
- Native [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for the host OS:
  Xcode command-line tools on macOS, the Visual Studio C++ build tools and WebView2 on
  Windows, and the documented WebKitGTK and build dependencies on Linux, plus OpenSSL
  development packages for native HTTP TLS.

On macOS, the npm desktop commands prefer the standalone Command Line Tools SDK when
installed, unless `DEVELOPER_DIR` is already set. This does not change the system-wide
Xcode selection or accept any license. If raw Cargo commands fail because Xcode awaits
license acceptance, prefix them with `DEVELOPER_DIR=/Library/Developer/CommandLineTools`
when that standalone SDK is installed. Otherwise complete Xcode setup yourself.

The Tauri CLI is a local development dependency; no global CLI installation is needed.
No Fluxkit backend, account, database, CDN, or telemetry service is required. Dependency
installation needs internet access or an appropriately populated package cache.

## Development

```sh
npm ci
npm run desktop:dev
```

For UI-only browser preview, run `npm run dev` and open the printed local URL. Browser
preview does not emulate native capabilities; its footer identifies it as a preview.
The development server binds to loopback on port 1420 and fails if the port is occupied.
The production application loads bundled files, without a frontend development server.

## Validation

```sh
npm run typecheck
npm run lint
npm run format:check
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo build --manifest-path src-tauri/Cargo.toml --locked
```

Frontend tests cover HTTP dispatch/cancellation/response views, project/collection/folder workflows, explicit saving, conflicts, form
attachments, project isolation, unsaved-change protection, and native connection outcomes.
Rust tests cover durable round-trips, invalid files, revision conflicts, locks, stable identity,
portable attachments, recovery copies, filesystem boundaries, and HTTP exchanges against
controlled local servers (including redirects, failures, upload, and streamed downloads).
Rust HTTP tests require permission to bind loopback ports.

Build the host desktop executable, including the frontend:

```sh
npm run desktop:build
```

Generate host installer bundles with `npm run tauri -- build` once release packaging is
in scope. Signing, notarization, distribution, and automatic updates are separate work.
The bundle identifier `app.fluxkit.desktop` is a development identifier; confirm ownership
before publishing. The icon source is `assets/fluxkit.svg`; regenerate platform assets
with `npm run tauri -- icon assets/fluxkit.svg`.

## Repository layout

- `src/`: React UI, draft interaction state, and typed native interface.
- `src-tauri/`: Rust host, validated local storage, native dialogs, commands, and security setup.
- `docs/`: agreed product scope and milestones.
- `AGENTS.md`: engineering rules and required checks.

## Self-hosted backend

An initial ASP.NET Core and PostgreSQL backend foundation is in `server/`. To configure
Docker Compose, copy `server/.env.example` to `server/.env`, replace all placeholder values,
and follow [the server setup guide](server/README.md). The desktop client can configure a server,
register/sign in to local accounts, upload a saved local project copy, and open server snapshots
in read-only mode with offline cache fallback. Online editing and ongoing sync are still in
progress. See [the backend scope and remaining work](docs/SELF_HOSTED_BACKEND.md).

Native project commands accept session handles issued by the folder chooser or validated
last-project restoration; the frontend
cannot submit arbitrary filesystem paths. Only the bundled main window receives their
permissions. Window event/destroy permissions support the unsaved-change close guard.
No filesystem, shell, HTTP, or credential plugins are exposed to the frontend. Production content security policy allows bundled
resources and native IPC; development adds only the loopback development server.

## Platform validation

macOS, Windows, and Linux are target platforms. Record actual host build and smoke-test
results in [development validation](docs/VALIDATION.md); do not infer support for another
platform from a successful local build. Cross-platform CI and release validation are
planned for the release milestone.

Use **Test authentication** in scope settings or the request Auth tab to verify current
settings without saving them. Basic/Bearer/API Key send GET to a chosen protected test URL;
OAuth2/Custom Login test their configured login and discard the test token. Unsaved session
and saved requests can use **Move…** next to Save, or the clickable location in the
breadcrumbs. Choose a collection/folder and confirm **Save & move** to save edits and relocate
within the project. Drag a request tab or saved tree request onto a collection/folder to open
the same confirmation. In **Headers → Automatic and sent headers**, inspect
masked current headers locally or review the last dispatch. Fluxkit sends a versioned
User-Agent and Accept by default; manual values take priority.

## License

GPL-3.0; see [LICENSE](LICENSE).
