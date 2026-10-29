<p align="center">
  <img src="assets/fluxkit.svg" alt="Fluxkit" width="96" />
</p>

<h1 align="center">Fluxkit</h1>

<p align="center">
  A fast, local-first, open-source desktop API client — a lightweight alternative to Postman and Insomnia.
</p>

---

Fluxkit is a desktop application for designing, organizing, and sending HTTP requests. It stores
everything as readable JSON files in a project folder you choose, runs completely offline by
default, and adds an optional self-hosted sync server for teams that want shared projects without
handing data to a third-party cloud.

## Why Fluxkit

- **Local-first.** No account, no backend, and no network access required to start the app, edit
  requests, or save a project. Your collections live in plain JSON files you can put under Git.
- **Fast and light.** A Rust/Tauri core keeps the installed app small and responsive, instead of
  bundling a full Chromium/Node runtime like Electron-based clients.
- **Your secrets stay yours.** Reusable values (tokens, passwords, API keys) are stored in the
  operating system's credential store, never written to project files or logs.
- **Optional self-hosted sync.** Teams that want shared projects can run their own ASP.NET Core +
  PostgreSQL server. There is no Fluxkit-hosted cloud service.
- **Open source.** Licensed under GPL-3.0 — inspect it, modify it, self-host it.

## Current status

Fluxkit is under active development and not yet a full Postman/Insomnia replacement. Implemented
so far: local projects with collections and nested folders, request editing (params, headers,
text/JSON/form/multipart/file bodies), native HTTP execution with cancellation and response
previews, project-wide environments and variables backed by the OS credential store, inherited
authentication (Basic, Bearer, API Key, OAuth2 Client Credentials, Custom Login), minimal local
request history, OpenAPI import/export, Postman collection/environment import, and an initial
self-hosted sync server. See [docs/PRODUCT.md](docs/PRODUCT.md) for the full product plan and
[docs/MILESTONES.md](docs/MILESTONES.md) for delivery status, and
[docs/VALIDATION.md](docs/VALIDATION.md) for recorded platform validation results.

## Architecture

Fluxkit has two independent parts: the desktop application (always required) and an optional
self-hosted server (only needed for team sync).

```text
┌─────────────────────────────── Desktop application ───────────────────────────────┐
│                                                                                     │
│   React + TypeScript UI  ──typed Tauri commands/events──▶  Rust application core   │
│   (src/)                                                   (src-tauri/src/)        │
│                                                                                     │
│                                                   ├─ project & file storage (JSON) │
│                                                   ├─ environment & variable rules  │
│                                                   ├─ OS credential-store adapter   │
│                                                   ├─ native HTTP execution         │
│                                                   ├─ auth (Basic/Bearer/API key/   │
│                                                   │   OAuth2 Client Credentials/   │
│                                                   │   Custom Login)                │
│                                                   ├─ OpenAPI & Postman import/export│
│                                                   └─ optional self-hosted server   │
│                                                      client adapter                │
└─────────────────────────────────────────────────────────────────────────────────┘
                                          │ HTTPS (optional, user-configured)
                                          ▼
                        ┌───────────────────────────────────┐
                        │   Self-hosted server (optional)    │
                        │   server/Fluxkit.Server             │
                        │   ASP.NET Core 10 + PostgreSQL      │
                        │   ─ local accounts or OIDC login    │
                        │   ─ versioned rows + change feed    │
                        │   ─ shared values + per-user         │
                        │     client-encrypted secret vault    │
                        └───────────────────────────────────┘
```

### Desktop application

- **UI (`src/`)** — React 19 + TypeScript. Owns presentation and draft/interaction state, and
  talks to the Rust core only through typed Tauri commands and events (`src/native.ts`).
- **Application core (`src-tauri/src/`)** — Rust, built on Tauri 2. Owns everything durable or
  security-sensitive: project file I/O, variable and environment resolution, the OS credential
  store, native HTTP execution (with timeout/cancellation), authentication inheritance, OpenAPI
  and Postman import/export, and the optional server-sync client. Tauri command handlers stay
  thin: validate input, delegate to application logic, map results.
- **Persistence** — projects are readable JSON files and folders in a location you pick; there is
  no local database. Personal/machine-specific settings (last-open project, window state) live in
  the OS app-data directory, outside the portable project files. See
  [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md).
- **Variables and environments** — environments belong to a project and apply to all its
  collections and requests. In local mode, reusable values live in the OS credential store, never
  in project JSON. See [docs/ENVIRONMENTS.md](docs/ENVIRONMENTS.md).
- **Security boundary** — remote API responses are rendered as untrusted data and never executed
  in the privileged application context; native filesystem/HTTP/credential access is only
  reachable through the narrow, validated Tauri command surface.

### Self-hosted server (`server/`)

An independent, optional ASP.NET Core 10 service backed by PostgreSQL. It is never required to
run the desktop app locally — it exists purely for teams that want shared, versioned projects:

- Local account registration with admin approval, or OIDC browser sign-in as an alternative.
- Personal and team projects stored as individually versioned rows, so members editing
  different requests never conflict; concurrent edits of the same request are detected.
- Ordinary shared values as plain project data; secret values are encrypted client-side
  (Argon2id key-wrapping + authenticated encryption) and scoped to one user — the server never
  sees plaintext secrets.
- The desktop client saves every change straight to the server, loads other members' changes
  automatically and keeps a read-only copy for offline use, with no hidden offline write queue.

See [server/README.md](server/README.md) and [docs/SELF_HOSTED_BACKEND.md](docs/SELF_HOSTED_BACKEND.md)
for the full contract, and [docs/SERVER_VALUES.md](docs/SERVER_VALUES.md) /
[docs/OIDC.md](docs/OIDC.md) for the value-sync and login details.

## Repository layout

```text
src/            React UI, draft/interaction state, typed native interface (native.ts)
src-tauri/      Rust application core: storage, HTTP, auth, credentials, server client
server/         Optional self-hosted sync server (ASP.NET Core + PostgreSQL) and its tests
docs/           Product scope, milestones, and durable file/API/security contracts
scripts/        Dev tooling (Tauri wrapper, server e2e runner, notice generation)
assets/         Application icon source and third-party notices
AGENTS.md       Engineering rules and required validation for contributors (human or AI)
```

## Getting started (desktop app)

### Requirements

- [Node.js](https://nodejs.org/) `24.11.0` and npm `11.6.1` (pinned in `.nvmrc` / `package.json`).
- [Rust](https://www.rust-lang.org/) `1.92.0` with `rustfmt` and `clippy` (pinned in
  `rust-toolchain.toml`).
- Native [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS:
  - **macOS** — Xcode command-line tools.
  - **Windows** — Visual Studio C++ build tools and WebView2.
  - **Linux** — WebKitGTK and the other documented build dependencies.
  - All platforms also need OpenSSL development packages for native HTTP/TLS.

No Fluxkit account, backend, database, or telemetry service is required to run or develop the
desktop app. The Tauri CLI itself is a local dev dependency — no global install needed.

> **macOS note:** if a raw `cargo` command fails because Xcode is waiting on a license prompt, and
> you have the standalone Command Line Tools SDK installed, prefix the command with
> `DEVELOPER_DIR=/Library/Developer/CommandLineTools`. The npm desktop scripts already do this
> automatically unless `DEVELOPER_DIR` is already set.

### Run it

```sh
npm ci
npm run desktop:dev
```

This launches the Tauri desktop shell with a live-reloading frontend. For a UI-only preview
without native capabilities (no filesystem, HTTP, or credential access), run `npm run dev` and
open the printed local URL — the preview footer identifies it as a browser preview.

### Build

```sh
npm run desktop:build
```

Builds the frontend and the native host executable. Run `npm run tauri -- build` for full
installer bundles once you need distributable packages — signing, notarization, and auto-update
are separate, not-yet-implemented scope. The bundle identifier `app.fluxkit.desktop` is a
development placeholder.

## Getting started (self-hosted server)

The server is entirely optional — skip this section if you only want the local desktop app.

### Local development

Install the .NET 10 SDK and PostgreSQL 17+, then:

```sh
export ConnectionStrings__Database="Host=localhost;Database=fluxkit;Username=fluxkit;Password=<local secret>"
export Auth__SigningKey="<unique random value, at least 32 bytes>"
export Bootstrap__AdminUsername="admin"
export Bootstrap__AdminPassword="<strong unique password>"
dotnet run --project server/Fluxkit.Server
```

### Docker Compose

```sh
cd server
cp .env.example .env
# edit .env and replace every CHANGE_ME value with a unique secret
docker compose up -d --build
```

The service binds to loopback only by default; put it behind your own HTTPS reverse proxy before
exposing it to real clients. Full details, including OIDC setup, are in
[server/README.md](server/README.md).

## Testing and validation

Frontend:

```sh
npm run typecheck
npm run lint
npm run format:check
npm test
npm run build
```

Rust:

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo build --manifest-path src-tauri/Cargo.toml --locked
```

Server (requires a disposable PostgreSQL instance, see [server/README.md](server/README.md)):

```sh
dotnet restore server/Fluxkit.Server.Tests/Fluxkit.Server.Tests.csproj --locked-mode
dotnet test server/Fluxkit.Server.Tests/Fluxkit.Server.Tests.csproj --no-restore
```

Full server/Keycloak/HTTPS end-to-end suite (builds disposable containers, cleans up after
itself):

```sh
npm run test:server:e2e
```

Run the checks relevant to what you changed; see [AGENTS.md](AGENTS.md) for the full required
validation matrix and [docs/VALIDATION.md](docs/VALIDATION.md) for recorded platform results.

## Contributing

Contributions are welcome — bug reports, fixes, documentation, and small, focused features.

1. Read [AGENTS.md](AGENTS.md) first. It is the binding engineering contract for this repository:
   scope discipline, architecture boundaries, security/privacy rules, and the validation every
   change must pass before it's considered done.
2. Check [docs/PRODUCT.md](docs/PRODUCT.md) and [docs/MILESTONES.md](docs/MILESTONES.md) so new
   work fits the agreed product scope instead of duplicating or contradicting it.
3. Keep changes small and cohesive: implement the active request completely, avoid speculative
   abstractions or unrelated refactors, and update the relevant doc under `docs/` alongside any
   contract change.
4. Add or update tests for behavior you change (Vitest/React Testing Library on the frontend,
   Rust tests in `src-tauri`, xUnit in `server/Fluxkit.Server.Tests`), and run the validation
   commands above for whatever you touched.
5. Never commit real credentials, tokens, or private project data — tests and examples must use
   synthetic values only.
6. Open a pull request describing the change, the reasoning behind non-obvious decisions, and
   which validation steps you ran (and which you couldn't, and why).

If you're planning something larger than a small fix, please open an issue or discussion first so
the approach can be agreed before significant work goes in.

## License

Fluxkit is licensed under [GPL-3.0](LICENSE).
