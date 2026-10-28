# Shared values, transfer preview and encrypted secrets

Local mode continues to use the OS credential store for every reusable variable value.
Server mode stores explicitly classified ordinary values as shared project data. Personal
secret values are encrypted in Rust before transmission; other project members receive no
plaintext or decryption key. Missing `secret` metadata means secret, including older files.
This does not protect secrets from a compromised client while they are unlocked.

## Using server values

Open **Server connection** and sign in. Set up an account vault with a separate master
password (12–1,024 UTF-8 bytes), then unlock it to save or use secrets. The vault belongs to
one account across its projects. **Lock** clears its in-memory key; closing a server project,
signing out, changing accounts/server or quitting also removes available keys. Pending unlock
operations cannot restore a key after a lock/account change. Only one expensive key operation
runs at once, on a blocking worker.

Local projects keep every secret and ordinary value in the OS credential store. In server
workspaces, ordinary values are shared project data and secret values live only in the account
vault, encrypted with a key wrapped by the vault master password. In the environment manager,
new variables default to secret. Explicitly marking a variable
ordinary preserves its UUID and does not upload an OS value. Definitions are saved on the
server as soon as they are saved, so values can be stored right afterwards. When the server
still rejects a value because its variable/environment no longer exists there or its
classification differs (HTTP 400, nothing written), reload the project and try again.
Ordinary values are readable by all project members; secrets require the unlocked vault.
Reveal and request execution resolve only the requested slots in Rust. Values are limited to
2,560 UTF-8 bytes per slot and 2,000 values per batch request.

Each value is one slot with its own revision, so members editing different values never
conflict. Value saves are online operations. A conflict preserves editor input; **Reload**
obtains current revisions before an explicit retry. Removing an environment removes its values
on the server, and removing a variable removes its values. Changing a classification removes the
values of the old kind for every member (shared plaintext when a variable becomes secret, all
members' encrypted values when it becomes ordinary); enter the values again afterwards. Local
imports retain their existing OS-store behavior.

## Imports into server workspaces

An OpenAPI or Postman import into an editable server workspace writes its collection and
variable definitions to the server in one atomic batch; when the server refuses it, nothing is
kept locally either. With **Save available variable values on the server** checked, the import
then adds its values in one batch: ordinary values become shared project values (readable by all
project members), secret values are encrypted into the importer's own vault values. Secret values require an unlocked vault; otherwise the import stops
before writing anything. Only new import-owned slots are written: reused existing variables keep
their values, an identical existing value is skipped and a different shared value or an existing
secret slot is never overwritten. No OS credential store access occurs for server workspaces.

Imports classify new variables explicitly. Postman: literal auth credentials (username, password,
token, API key, client ID/secret, access token) and variables of Postman `type: secret` are
secret; all other variables (base URLs, `{{token}}` references without the secret type, path
parameters, scope/token URL) are ordinary. A secret-typed environment value for an ordinary
mapped variable is not adopted. OpenAPI: server URLs and OAuth token endpoints are ordinary,
credentials are secret. Change a classification later in the environment manager.

Each step is recorded on the open preview. When the definitions are refused, retrying the import
starts again from the beginning. When only the value upload fails, the import is already saved
on the server; **Retry import** resumes with the values. Closing the preview drops the remaining
steps: then enter values in Project variables.

A forgotten master password has no recovery/reset flow: without the password or an already
unlocked key, the account's synchronized secrets are inaccessible. **Change master password**
requires the unlocked vault and rewraps the same account key; it does not rewrite secret
ciphertexts. No master password or unwrapped key is persisted.

## Local-to-server transfer

**Review transfer** previews the saved project, definitions, environment slots and data warnings.
No value is selected automatically and opening the preview reads no OS values. Choose individual
variable/environment slots (including defaults), then explicitly prepare the transfer. Only
selected slots are read from the OS store; missing slots are listed without revealing content.
Selected secrets require an unlocked vault before any OS reads and are encrypted immediately.
A definitions-only transfer reads no values. Prepared ordinary values remain native memory
until upload, cancellation or plan disposal; they are cleared on disposal where practical.

The final upload creates a personal server copy using the original project UUID. Project rows,
selected shared values and personal ciphertexts are committed together in one request. Existing identities
return a conflict. Source files and OS values remain untouched. Plans expire after ten minutes,
are limited to two, and are bound to the signed-in account and unchanged saved source snapshot.
Expiration is checked on plan operations; expired buffers are discarded on the next such
operation or session cleanup, rather than by a background timer.
Preparation captures selected values; changing an OS value afterwards requires a new preparation.

Saved literal request content is uploaded with the requests and may itself contain sensitive
data; the preview calls this out. Local file attachments block transfer and server saves. History,
acquired authentication tokens and unsaved editors are excluded. Interrupted uploads are never
replayed automatically: check the server project list before trying a new transfer.

## HTTP and revision contract

All routes require approved active accounts and project access where applicable. JSON uses
camelCase. `environmentId` is a canonical environment UUID or the literal `default`.

| Route | Purpose and precondition |
| --- | --- |
| `GET/PUT /api/account/vault` | `{ revision, profile }`; PUT `{ profile }` with `If-Match` profile revision |
| `GET /api/projects/{id}/values` | `{ values }`, each `{ variableId, environmentId, value, revision }` |
| `PUT /api/projects/{id}/values/{variableId}/{environmentId}` | `{ value }`, `If-Match` slot revision (`0` creates) |
| `GET /api/projects/{id}/vault-values` | The caller's `{ values }`, each `{ variableId, environmentId, nonce, ciphertext, revision }` |
| `PUT /api/projects/{id}/vault-values/{variableId}/{environmentId}` | `{ nonce, ciphertext }`, `If-Match` slot revision (`0` creates) |
| `DELETE` on either slot route | `If-Match` slot revision |
| `POST /api/projects/{id}/values/batch` | `{ shared, vault }` slots with `expectedRevision` each; all-or-nothing |

A write never touches another user's encrypted values. Duplicate/unknown slots, wrong
classification, oversized values and unsupported cryptographic parameters are rejected (400).
Stale revisions return 409 and missing preconditions 428. Uploading a local project
(`POST /api/projects`) carries `sharedValues` and `vaultValues` with the project tree and applies
them in the same transaction.

## Cryptographic format, version 1

A random 32-byte account data key encrypts individual values with AES-256-GCM. An Argon2id
version-19 wrapping key derived from the master password encrypts that account key. The profile
contains `schemaVersion: 1`, `algorithm: "AES-256-GCM"`, `kdf`, `salt`, `nonce`, `wrappedKey`.
The `kdf` object has `algorithm: "Argon2id"`, `version: 19`, `memoryKiB`, `iterations`,
`parallelism`. New profiles use 65,536 KiB, three iterations, one lane and a fresh 16-byte salt.
Both client and server reject memory outside 65,536–262,144 KiB, iterations outside 3–10 and
lanes outside 1–4 before derivation. These limits are resource bounds, not latency guarantees.

All salts/nonces/ciphertexts use standard padded base64. Each encryption generates a fresh
12-byte nonce with the OS random generator. Ciphertexts append the 16-byte authentication tag;
wrapped keys are exactly 48 decoded bytes. An empty value still has a 16-byte ciphertext.
Randomness failure aborts the operation.

Authenticated associated data is the UTF-8 serialization of the following compact JSON arrays
(no whitespace), with canonical UUID strings and `default` for the default environment:

- Account-key wrapping: `["fluxkit-account-key-v1", accountUuid]`.
- Value encryption: `["fluxkit-secret-v1", projectUuid, accountUuid, variableUuid, environmentId]`.

Identity binding rejects ciphertext substitution between accounts, projects, variables and
environments. Server URL is deliberately excluded so hostname changes do not invalidate
ciphertexts; local session/cache authorization remains server-URL scoped. Wrong passwords,
modified ciphertext or unsupported parameters fail without changing values. Derived/password
buffers and data keys use zeroization where practical; this is best effort and is not a claim
that all OS, allocator or UI copies can be erased.

The runtime uses pinned RustCrypto `aes-gcm` 0.10.3 and `argon2` 0.5.3 with zeroization enabled,
and `zeroize` (locked 1.9.1). These MIT/Apache-2.0 dependencies add authenticated encryption,
KDF work and key wiping; no service is added. KDF memory/work is deliberately bounded and off
the UI thread. Bundled upstream license notices are regenerated from the locked graph with
`scripts/update-crypto-notices.py` into `assets/notices/crypto/`.

## Caches, offline access and upgrade

Account-scoped application data may contain shared ordinary plaintext, personal ciphertext and
the wrapped account-key profile. They are separate from the project files and scoped by server
URL/account UUID. There is no secret-plaintext/key/password file or browser-storage
fallback. Previously stored server-scoped OS variable entries remain dormant; they are neither
automatically uploaded nor consulted. Acquired authentication tokens keep their OS storage.

Connection failures, timeouts and server 5xx responses may use cached read data. Authorization
failures never use stale data. Cached secrets still require explicit vault unlock; an offline
unlock can use the cached profile. Offline project copies stay read-only, and writes are never
queued. After an interrupted value write, Reload checks the server outcome before retry.
A changed wrapped-profile revision locks any previously loaded key.

The normalized schema replaces the earlier snapshot schema with a new initial migration; it
does not convert databases created by earlier development builds, which must start empty.
Migrations run at startup. Back up before upgrading and test restoration. OIDC is documented in [OIDC.md](OIDC.md). Independent security review, complete
signed-in GUI/platform tests and
operator backup/restore/deployment validation remain open.
