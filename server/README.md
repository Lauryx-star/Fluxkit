# Fluxkit self-hosted server

This ASP.NET Core 10 service stores project snapshots in PostgreSQL. The desktop client
remains usable without it. The initial server slice supports local accounts, admin approval,
personal projects, team project membership, revision-checked snapshots, and per-user opaque
encrypted-secret envelopes. The desktop includes local-account sign-in, project/account administration, saved-project
upload, account-scoped offline snapshots, explicit sync and workspace recovery with a retained
read-only copy for manual merge. Shared values, explicit transfer preview and client-encrypted
personal secrets are implemented; see [the value/vault contract](../docs/SERVER_VALUES.md).
OIDC browser sign-in is implemented as an exclusive alternative to local accounts; see
[OIDC setup and existing-account limits](../docs/OIDC.md). Full deployment/platform validation remains unfinished. See [the detailed contract](../docs/SELF_HOSTED_BACKEND.md).

## Local development

Install the .NET 10 SDK and PostgreSQL 17 or later. Configure these environment variables before run:

```powershell
$env:ConnectionStrings__Database = "Host=localhost;Database=fluxkit;Username=fluxkit;Password=<local secret>"
$env:Auth__SigningKey = "<unique random value, at least 32 bytes>"
$env:Bootstrap__AdminUsername = "admin"
$env:Bootstrap__AdminPassword = "<strong unique password>"
dotnet run --project server/Fluxkit.Server
```

Database migrations run at startup. Back up the database before upgrading. The shared-value
and account-vault migration is additive and preserves existing project snapshots; downgrading
that migration removes the new value tables/columns and must not be used as a recovery flow. The bootstrap administrator is created only when the
configured username does not already exist; changing the environment password does not reset
an existing account. Keep signing keys and database credentials outside source control.

## Compose deployment

Copy `server/.env.example` to `server/.env`, replace every `CHANGE_ME` value with a unique
secret, then start Compose from the `server` directory:

```powershell
Copy-Item .env.example .env
# Edit .env and replace every CHANGE_ME value before continuing.
docker compose up -d --build
```

Compose loads `.env` automatically from its project directory. From the repository root, pass
`--env-file server/.env -f server/docker-compose.yml` explicitly. Keep `.env` out of source
control. Use long random URL-safe secrets; for example, `openssl rand -hex 32` produces a
64-character value suitable for the signing key and a database password. `FLUXKIT_PORT` is
optional and defaults to `8080`.

The service binds only to loopback by default. Put it behind an operator-managed HTTPS reverse
proxy before remote clients connect. Do not expose the internal HTTP listener directly to an
untrusted network. Back up PostgreSQL with a tested restore procedure. `GET /health/ready`
reports readiness and checks database connectivity.

Set `Auth:Mode` to `local` or `oidc`. OIDC uses a confidential server-side client, browser
Authorization Code with PKCE and explicit administrator approval. Follow [OIDC.md](../docs/OIDC.md)
for provider registration, bootstrap and Compose settings. Switching modes does not automatically
link or migrate existing accounts. Existing tokens must sign in again after this upgrade.

## Backend regression tests

The test project uses xUnit and ASP.NET Core's in-process test host, with real PostgreSQL.
Tests require a dedicated PostgreSQL instance and an account allowed to create/drop databases.
Set `FLUXKIT_TEST_DATABASE` to that instance's administrative connection string. Each test run
creates a randomly named `fluxkit_test_<UUID>` database, runs the production EF migrations and
bootstrap, and drops only that database on completion. Never point this at a production instance.

```sh
dotnet restore server/Fluxkit.Server.Tests/Fluxkit.Server.Tests.csproj --locked-mode
dotnet test server/Fluxkit.Server.Tests/Fluxkit.Server.Tests.csproj --no-restore
```

Supply `FLUXKIT_TEST_DATABASE` through your shell's environment before running the commands.
An isolated local container can use PostgreSQL 17 with a synthetic password and a loopback port;
remove that test container afterwards. The tests keep all HTTP inside TestServer. They cover
pending/suspended users, personal-project isolation from members and server administrators,
team access/removal, shared-value and vault isolation, value limits/classification, additive
migration, atomic transfer, missing/stale revisions and deterministic
simultaneous writers. A test-only interceptor holds both snapshot reads before either update,
so the previous read-then-save race fails reliably. Each synthetic HTTP client gets a separate
peer address inside TestServer; production authentication rate limiting stays enabled.

Test dependencies are confined to the test project: xUnit and its Visual Studio
runner (Apache-2.0), and Microsoft's test SDK/ASP.NET Core test host (MIT). They add no server or
desktop runtime dependency. Both server and test NuGet lockfiles are committed.

## HTTPS/OIDC end-to-end suite

Run `npm run test:server:e2e` from the repository root to start an isolated PostgreSQL,
Keycloak and HTTPS proxy, exercise real browser login/approval/access/conflict checks, and
run the native Rust encryption protocol through HTTPS. The launcher cleans up its own fixtures
and never installs host trust or writes real OS credentials. See [e2e/README.md](e2e/README.md)
for prerequisites, exact coverage, the Docker-only option and remaining GUI/platform limits.
