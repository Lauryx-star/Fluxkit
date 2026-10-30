# Fluxkit self-hosted server

This ASP.NET Core 10 service stores projects in PostgreSQL as normalized rows (collections,
folders, requests, environments, variables, values), each with its own revision, plus a change
feed. The desktop client remains usable without it. The server supports local accounts, admin
approval, personal projects, team project membership, atomic change batches and per-user opaque
encrypted values. The desktop includes local-account sign-in, project/account administration,
saved-project upload, account-scoped offline copies, write-through saving and automatic loading
of other members' changes. Shared values, explicit transfer preview and client-encrypted
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

Database migrations run at startup. Back up the database before upgrading. The normalized
schema starts with a new initial migration and does not convert databases from earlier
development builds; start those with an empty database. The bootstrap administrator is created only when the
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

## Production deployment

[`docker-compose.prod.yml`](docker-compose.prod.yml) runs a published image at a pinned version
(see Published images), stores PostgreSQL data in a host directory and publishes the server only
on `127.0.0.1` for an nginx on the same host. The database has no host port. The server runs
with a read-only root filesystem, no Linux capabilities and a `/health/ready` healthcheck; both
containers rotate their logs.

```bash
cp .env.production.example .env.production
chmod 600 .env.production
# Replace every CHANGE_ME value, set FLUXKIT_IMAGE, FLUXKIT_VERSION and FLUXKIT_DATA_DIR.
docker compose -f docker-compose.prod.yml --env-file .env.production up -d
```

To upgrade, change `FLUXKIT_VERSION` and run the same `up -d` command; the schema migrates on
start. Back up `FLUXKIT_DATA_DIR` (or `pg_dump`) before upgrading. A minimal nginx site:

```nginx
server {
    listen 443 ssl;
    server_name fluxkit.example.org;
    # ssl_certificate / ssl_certificate_key ...
    client_max_body_size 30m;   # matches the server's 30 MB request limit

    location = /metrics {
        allow 10.0.0.0/24;   # Prometheus host(s)
        deny all;
        proxy_pass http://127.0.0.1:8080;
    }
    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;   # nginx defaults to HTTP/1.0 towards the upstream
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

## Metrics

The server serves Prometheus metrics at `/metrics` on the API port. The endpoint needs no login,
so the reverse proxy must restrict it, for example to the network of the Prometheus host:

```nginx
location = /metrics {
    allow 10.0.0.0/24;   # Prometheus host(s)
    deny all;
    proxy_pass http://127.0.0.1:8080;
}
```

| Setting                   | Default | Meaning                                                   |
| ------------------------- | ------- | --------------------------------------------------------- |
| `Metrics:Enabled`         | `true`  | `false` removes the `/metrics` endpoint.                  |
| `Metrics:SnapshotSeconds` | `15`    | Minimum age before the database counts are queried again. |

Reverse proxy: `Proxy:TrustedNetworks` (comma-separated CIDR networks, empty by default) makes
the server take the client address from the last `X-Forwarded-For` entry of requests coming from
those networks, so login rate limits apply per client instead of per proxy. The production
Compose file trusts the private ranges that Docker's gateway uses.

Desktop sessions: `Auth:AccessTokenMinutes` (default `60`), `Auth:SessionIdleDays` (default `7`,
sign-in again after that many days without use) and `Auth:SessionMaxDays` (default `90`, `0` = no
absolute limit). See [SELF_HOSTED_BACKEND.md](../docs/SELF_HOSTED_BACKEND.md#desktop-sessions).

Example scrape configuration:

```yaml
scrape_configs:
  - job_name: fluxkit
    scrape_interval: 30s
    scheme: https
    static_configs:
      - targets: ['fluxkit.example.org']
```

Exported metrics:

- Process and .NET runtime: `process_*`, `dotnet_*`, `system_runtime_dotnet_*` (GC, thread
  pool, exceptions), plus the .NET meters of ASP.NET Core (`microsoft_aspnetcore_*`: request
  duration, authentication, rate limiting), Entity Framework Core and Npgsql
  (`npgsql_db_client_*`: connection pool, command duration and failures).
- HTTP: `http_requests_received_total`, `http_request_duration_seconds` and
  `http_requests_in_progress`, labelled with method, status code and the route template
  (for example `/api/projects/{projectId}/changes`), never the concrete path.
- Usage counters: `fluxkit_logins_total{mode,result}`, `fluxkit_registrations_total{mode}`,
  `fluxkit_change_batches_total{result}`, `fluxkit_change_operations_total{kind,operation}`,
  `fluxkit_value_writes_total{scope,operation,result}`, `fluxkit_feed_reads_total{result}`,
  `fluxkit_projects_created_total{kind,source}`, `fluxkit_projects_deleted_total`,
  `fluxkit_account_changes_total{change}`.
- Usage gauges from the database: `fluxkit_accounts{status}`, `fluxkit_server_admins`,
  `fluxkit_projects{kind}`, `fluxkit_team_memberships`, `fluxkit_project_rows{kind}`,
  `fluxkit_variables{classification}`, `fluxkit_stored_values{scope}`,
  `fluxkit_account_vaults`, `fluxkit_usage_snapshot_duration_seconds`.
- `fluxkit_active_users{window="5m|1h|24h"}`: distinct accounts with an authenticated request in
  the window, counted in memory since the server started.

Labels are aggregates only: no user names, project names, identifiers, URLs or values. Counters
start at zero after each restart, as usual for Prometheus; use `rate()`/`increase()`.

An importable [Grafana dashboard and setup guide](monitoring/README.md) covers Fluxkit usage,
API traffic and process health, with selectable Prometheus data source, job and instance.
The guide explains process-local activity, shared database counts and snapshot freshness limits.

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
team access/removal, shared-value and vault isolation, value limits/classification, the
schema and its cascades, atomic batches and uploads, tree rules, the change feed (including
polling during parallel writers), missing/stale revisions, simultaneous writers of one row and
deleting empty projects. Each synthetic HTTP client gets a separate
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
