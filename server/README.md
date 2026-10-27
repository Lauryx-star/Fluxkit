# Fluxkit self-hosted server

This ASP.NET Core 10 service stores project snapshots in PostgreSQL. The desktop client
remains usable without it. The initial server slice supports local accounts, admin approval,
personal projects, team project membership, revision-checked snapshots, and per-user opaque
encrypted-secret envelopes. It does not yet include the desktop server-mode UI, local snapshot
cache, migration/takeover UI, OIDC, or collection-level APIs.

## Local development

Install the .NET 10 SDK and PostgreSQL 17 or later. Configure these environment variables before run:

```powershell
$env:ConnectionStrings__Database = "Host=localhost;Database=fluxkit;Username=fluxkit;Password=<local secret>"
$env:Auth__SigningKey = "<unique random value, at least 32 bytes>"
$env:Bootstrap__AdminUsername = "admin"
$env:Bootstrap__AdminPassword = "<strong unique password>"
dotnet run --project server/Fluxkit.Server
```

Database migrations run at startup. The bootstrap administrator is created only when the
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

The only authentication mode currently implemented is `local`. OIDC setup, user linking and
approval remain planned before production use.
