# Isolated HTTPS and OIDC end-to-end tests

From the repository root:

```sh
npm run test:server:e2e
```

Prerequisites are Docker with Compose v2, the repository's Node/npm versions, and the pinned
Rust toolchain/native desktop build prerequisites in the root README. On macOS the launcher
selects the standalone Command Line Tools SDK when available. The server is built from its
production Dockerfile, including locked restore, release publish, normal authentication and
production database migrations. No host .NET SDK, installed Keycloak, host-name edits or host
certificate-store changes are required.

For a Docker/Node-only run, explicitly omit the native phase:

```sh
npm run test:server:e2e -- --server-only
```

The launcher reports that exclusion. Run the full command to include real Rust encryption and
password rewrapping. First builds download substantial .NET/browser images and need registry/
package network access plus several GiB of Docker disk space. Database/provider/server/runner traffic stays on an internal Docker network. The HTTPS proxy
also joins a separate ingress bridge for its loopback-only host port. These are disposable synthetic fixtures, never deployment examples.

## What runs

- PostgreSQL 17.11, with a fresh database on container tmpfs.
- Keycloak 26.8.1, importing two synthetic users and a confidential client requiring PKCE S256.
- Caddy 2.11.7, serving both the provider and Fluxkit through HTTPS with a fresh private CA.
- The actual Fluxkit ASP.NET Core server in OIDC mode, reachable through the proxy.
- Playwright 1.64.1/Chromium driving the actual Keycloak login page and automatic form POST.
- The explicitly selected Rust protocol integration test, using sessions obtained by that
  browser flow and the real server through the same HTTPS proxy.

Provider, proxy, PostgreSQL and browser base images are pinned by digest; the browser package
has a separate committed npm lockfile. The .NET images follow the production Dockerfile's
10.0 tags. Ubuntu build packages follow the runner base's repositories. This is a repeatable
workflow, not a byte-for-byte build guarantee. Keep the browser package/image versions aligned.

The checks exercise:

1. Certificate-chain validation, with an unrelated-CA negative test and a trusted-CA positive
   test; discovery must advertise the exact configured HTTPS issuer.
2. OIDC-only mode and rejection of anonymous project access/local login/local registration.
3. A wrong password rejected by the real provider, followed by successful browser login,
   bootstrap administration, secure correlation/nonce cookies and code redemption. The
   imported client requires S256, so successful login also checks provider-side PKCE enforcement.
4. Single-use handoffs and rejection of an incorrect polling secret without consuming the
   legitimate result.
5. Pending accounts, administrator approval, repeat login with stable Fluxkit identity,
   personal-project isolation and one personal project per approved user.
6. Team membership and two members editing concurrently: different requests both succeed,
   simultaneous writers of the same request get one success and one conflict naming the current
   revision, and the change feed reports every changed row. Shared values use per-slot
   revisions; stale writes conflict.
7. Atomic project upload with selected ordinary values, rejection of plaintext values for secret
   variables, duplicate identity protection and ownership checks; only server administrators
   delete projects, and only empty ones.
8. Account suspension and team membership removal revoking access.
9. Native Rust Argon2id account-key setup, AES-GCM value encryption/upload/download/decryption,
   master-password rewrapping, old-password rejection, shared-value access by another member
   and isolation of ciphertext/account-key profiles. Values stay out of the project rows.

The browser test talks to the real handoff API as the desktop would. The native phase uses the
application's existing crypto/value protocol functions without touching the OS credential store.
Full Tauri UI interaction, keyring integration, desktop offline/recovery workflows, other browsers,
other identity providers, Windows/Linux launchers, production backup/restore and deployment load
remain separate checks. The test does not claim those platform or deployment guarantees.

## Trust, credentials and cleanup

CA keys stay in the proxy's private volume. A one-shot exporter mounts that volume read-only
and copies only the public root certificate into a separate volume. The server and test runner
receive read-only access to that public certificate, never the private keys. The server trusts
it via Linux's process certificate-file configuration. The disposable runner imports it into its
own OS/NSS stores; Node also uses `NODE_EXTRA_CA_CERTS`. Chromium does not ignore HTTPS errors,
and no client disables certificate or hostname checks.

For the host Rust phase, the launcher copies only the public CA and synthetic session tokens
into a fresh private temporary directory (0700 directory/0600 session file on Unix). A
`cfg(test)`-only client hook trusts that explicit CA and resolves the reserved `fluxkit.test`
hostname to loopback on the fixture port. Production clients do not read these fixture settings.
No host DNS mapping or persistent trust is installed. The optional native fixture environment
is set only for the selected child test process.

All usernames/passwords/client secrets/signing keys in this directory are deliberately public
synthetic test data. The internal provider has no direct-grant flow or published admin port.
Only the HTTPS proxy binds a host port, on `127.0.0.1:18443`. If occupied, set
`FLUXKIT_E2E_HTTPS_PORT` to another unprivileged free port for the launcher. Browser/provider
URLs inside Docker remain on 18443; the native test uses the selected published port.

Each launcher run creates a random `fluxkit-e2e-...` Compose project and removes only its own
containers, CA volumes/network and host temporary files on success, failure or ordinary
SIGINT/SIGTERM. Built images and Docker build caches remain available for subsequent runs.
If cleanup cannot complete, the launcher reports its exact project-scoped cleanup command.
A hard kill/host failure can leave that run's fixtures behind; never use global Docker prune to
clean them. Concurrent runs need different published ports.

Browser contexts are ephemeral. Screenshots, traces, cookies, token responses and resolved
values are not saved as test reports. Synthetic sessions exist only long enough for the native
phase. Output lists named checks; failure diagnostics include a bounded tail of fixture service
logs. Keep these fixtures separate from real `.env` files, accounts and data. Production setup
remains in [the server guide](../README.md) and [OIDC.md](../../docs/OIDC.md).

Fresh-stack startup can show provider development/deprecation notices, temporary Data Protection
key warnings and the optional Kerberos library probe from the password-authenticated database
client. Migration history is initially absent. Readiness and all authenticated checks must still
pass. Kerberos authentication and persistent production Data Protection are not validated here.

The added Playwright dependency is Apache-2.0 and confined to the test runner. Keycloak/Caddy
images are Apache-2.0, and PostgreSQL uses the PostgreSQL License. These fixtures add no desktop
or server runtime package; upstream notices remain with their official images/packages.
