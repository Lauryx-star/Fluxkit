# OIDC server authentication

`Auth:Mode` is either `local` (default) or `oidc`. A server enables exactly one login mode.
Local desktop projects remain available without an account or server. OIDC authenticates the
Fluxkit account; the secret-vault master password remains separate and is never sent to the
identity provider.

## Operator setup

Register Fluxkit as a **confidential Web client** at your OIDC provider, using Authorization
Code, PKCE S256 and `form_post`. Register the exact redirect URI
`https://fluxkit.example.org/api/auth/oidc/callback`. Enable the `openid` and `profile` scopes.
The provider needs discovery, a token endpoint and signing-key discovery. No provider client
secret belongs in the desktop application.

Configure the server (environment variable names shown):

| Variable | Meaning |
| --- | --- |
| `Auth__Mode=oidc` | Disable local registration/password login and enable OIDC |
| `Auth__Oidc__Authority` | Exact HTTPS issuer from the provider's discovery document |
| `Auth__Oidc__PublicOrigin` | External HTTPS origin, such as `https://fluxkit.example.org` |
| `Auth__Oidc__ClientId` | Confidential client identifier |
| `Auth__Oidc__ClientSecret` | Provider-issued client secret |
| `Bootstrap__OidcSubject` | Exact stable `sub` of the intended initial server administrator |
| `Auth__SigningKey` | Existing random Fluxkit signing key; still required |
| `ConnectionStrings__Database` | Existing PostgreSQL connection string |

For Compose, the corresponding `.env` variables are `FLUXKIT_AUTH_MODE`,
`FLUXKIT_OIDC_AUTHORITY`, `FLUXKIT_PUBLIC_ORIGIN`, `FLUXKIT_OIDC_CLIENT_ID`,
`FLUXKIT_OIDC_CLIENT_SECRET` and `FLUXKIT_OIDC_ADMIN_SUBJECT`. See
[server/.env.example](../server/.env.example). Local-mode bootstrap credentials are only required
in local mode. Missing OIDC settings, example placeholders, non-HTTPS URLs, URL credentials or
a public origin with a path prefix fail startup. The public origin must serve the Fluxkit API;
subpath hosting is not supported in this slice.

Terminate HTTPS at your trusted reverse proxy and forward the API/callback paths to the
loopback-bound service. Redirect URIs are built from the configured public origin, rather than
request Host/forwarded headers. Preserve the browser's secure correlation/nonce cookies and
form POST. TLS verification remains enabled for discovery/token requests. Proxy request logs
must redact callback query/form data and cookies; never enable identity-library PII logging,
request-body logging or token logging. Provider credentials and database backups remain operator
secrets.

The configured bootstrap subject, under the configured issuer, creates an active server admin
on its first verified login. It is never inferred from an email, display name, provider group
or the first arriving user. Other new identities create pending accounts. The administrator
approves them in **Server connection**; approval creates the personal project. A pending user
must sign in again after approval. Suspending an account rejects existing Fluxkit tokens on
their next API request.

## Desktop sign-in and cancellation

Enter the Fluxkit server URL and choose **Continue**. The desktop reads the server's mode.
Local mode then shows username/password fields; OIDC opens the system browser without asking
for provider credentials inside Fluxkit. Complete provider authentication and return to the app.
The browser completion page contains no account token. **Cancel browser sign-in**, changing
server, signing out or closing the dialog cancels native waiting. Cancelled flows cannot install
a late session. Provider consent/SSO remains in the external browser.

A handoff lasts five minutes and polls every two seconds. Expiration, interruption or approval
requires a new explicit sign-in; no hidden retry or offline login queue exists. The final
Fluxkit account UUID is verified through `/api/account` before the token is written to the OS
credential store. Existing account-scoped workspaces, values and vault ownership use that UUID.
Close server projects before switching accounts. Sign-out clears the desktop session and vault;
it does not sign the browser out of the identity provider. Fluxkit sessions expire after eight
hours; no provider refresh token is requested or retained.

macOS uses `/usr/bin/open`, Linux `xdg-open`, and Windows the system URL handler through
`rundll32.exe`. These launchers receive a validated server URL as an argument without a shell.
The browser is never embedded in the privileged WebView. Linux requires a working desktop
browser/`xdg-open`; cross-platform launcher behavior still needs platform smoke checks.

## Identity and existing installations

External identities map exclusively by the exact `(issuer, subject)` pair, hashed separately
with SHA-256 into the existing ASP.NET Identity login-provider/key columns. Email and preferred
username never link accounts or grant roles. A readable sanitized provider-name prefix plus an
internal UUID forms a unique Fluxkit username; later display-name changes preserve account ID,
memberships, projects and encrypted-vault identity.

No OIDC database migration is needed: the existing Identity user-login table stores mappings.
Switching an existing installation from local to OIDC does **not** merge local accounts by name
or email. Local users, projects and secret envelopes remain stored, but their passwords cannot
log in while OIDC mode is enabled. There is no account-linking/data-migration UI in this slice.
Plan the mode change with a backup; switching back restores local login. Account-vault ciphertext
is bound to its original Fluxkit UUID and cannot simply be reassigned to a new OIDC account.

New Fluxkit tokens include an `auth_mode` claim. The API checks it against the active server
mode as well as the account's active status. Older tokens without that claim must sign in
again after this upgrade; tokens from the alternative mode are rejected.

## Protocol and implementation limits

| Route | Purpose |
| --- | --- |
| `GET /api/auth/mode` | Anonymous discovery of `local` or `oidc` |
| `POST /api/auth/oidc/start` | Create an opaque handoff ID, separate random 256-bit polling secret and expiry |
| `GET /api/auth/oidc/authorize/{id}` | Claim the ID once and challenge the configured provider |
| `POST /api/auth/oidc/callback` | Framework-owned code exchange and validation |
| `POST /api/auth/oidc/poll` | Authenticate with ID/secret; receive waiting, complete, pendingApproval, denied, failed or expired |

The start, authorize and poll routes are available only in OIDC mode; local login/register
routes are available only in local mode. The desktop constructs the browser destination from
its configured server and the typed handoff UUID. The polling secret is kept in native memory,
never placed in a browser URL, settings or frontend state. Terminal poll results are consumed
once, so a lost final response requires another login. Responses carrying handoffs/sessions
use `Cache-Control: no-store`.

ASP.NET Core's maintained OpenID Connect handler owns code redemption, S256 PKCE, protected
state, secure correlation/nonce cookies and ID-token signature/issuer/audience/lifetime/nonce
validation. Enrollment runs only after all protocol checks complete. It never persists
provider access/ID/refresh tokens or a browser login cookie. The server issues its own token
only after checking the mapped account is currently active. Provider denial/failure details
are not reflected to the browser or desktop.

Handoffs are bounded to 256 in memory and expire after five minutes; expired entries are
removed on the next operation. Restarts invalidate unfinished handoffs. This slice supports one
server process; multi-instance deployment requires a deliberately shared handoff store and
Data Protection configuration, which are outside scope. Start/authorize are limited to ten
requests per peer/minute and polling to 120. Behind a reverse proxy the peer is currently the
proxy, so those limits are shared unless the deployment provides separate trusted peer routing;
untrusted forwarded headers are not accepted automatically. Rate limits do not replace proxy
connection/body bounds.

The only added runtime dependency is Microsoft's MIT-licensed
`Microsoft.AspNetCore.Authentication.OpenIdConnect` 10.0.13 and its locked protocol dependency.
It adds server-side discovery/code validation, with no new desktop runtime dependency.
The behavior follows [ASP.NET Core OIDC](https://learn.microsoft.com/aspnet/core/security/authentication/configure-oidc-web-authentication)
and the external-browser guidance in [RFC 8252](https://www.rfc-editor.org/rfc/rfc8252).

## Reproducible HTTPS/provider validation

[The isolated end-to-end suite](../server/e2e/README.md) runs a real Keycloak provider and
Caddy HTTPS proxy, drives browser login and approval, checks server permissions/revisions,
and exercises the native encryption protocol. Start it with `npm run test:server:e2e`.
Its private CA is trusted only by disposable containers and the explicitly selected native
test process; no host trust or DNS settings are installed. Full Tauri GUI and OS-store
integration remain separate platform checks.
