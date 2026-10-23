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
folders save immediately. **Close project** keeps its files on disk. After restarting,
choose **Open project** to reopen the folder; recent paths and tabs are not yet restored.

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
[token authentication](docs/TOKEN_AUTHENTICATION.md). Exchange formats remain planned in
[the product plan](docs/PRODUCT.md) and [milestones](docs/MILESTONES.md).
Browser preview supports draft editing; native sending and project files require the desktop app.

Open **Request history** for a project and enable **Record request history** to opt in.
History stores up to 100 entries for seven days, with literal hosts visible locally and
path/query content redacted. Closing a project preserves history; **Clear history** explicitly
clears it. Bodies, headers, and resolved values are excluded. See [the history contract](docs/HISTORY.md).

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

Native project commands accept session handles issued by the folder chooser; the frontend
cannot submit arbitrary filesystem paths. Only the bundled main window receives their
permissions. Window event/destroy permissions support the unsaved-change close guard.
No filesystem, shell, HTTP, or credential plugins are exposed to the frontend. Production content security policy allows bundled
resources and native IPC; development adds only the loopback development server.

## Platform validation

macOS, Windows, and Linux are target platforms. Record actual host build and smoke-test
results in [development validation](docs/VALIDATION.md); do not infer support for another
platform from a successful local build. Cross-platform CI and release validation are
planned for the release milestone.

## License

GPL-3.0; see [LICENSE](LICENSE).
