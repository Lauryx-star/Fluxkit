# OpenAPI import (milestone 8, first slice)

Use **Import OpenAPI…** in an open project's sidebar. Choose a UTF-8 YAML/JSON file, review
its compatibility report, then choose **Import collection**. Previewing or cancelling before
confirmation writes no collection, definition, or credential value. Imports never execute a
request, fetch a URL, read an external reference, or run scripts. A new collection is added;
existing requests, open dirty drafts, environments, and selected environment stay in place.

## Versions and reference boundary

This slice accepts OpenAPI 3.0.x and 3.1.x; fixtures exercise published 3.0.0–3.0.4 and
3.1.0–3.1.2 shapes. Swagger 2.0 and OpenAPI 3.2 are rejected. Version detection is explicit,
not inferred from a file extension. This is a bounded request mapper, not a complete OpenAPI
or JSON Schema conformance validator. Only the documented executable subset is mapped.

References used by the mapper and schema `$ref` references must be in-document JSON Pointers
beginning with `#/` (including URI percent decoding and JSON Pointer `~0`/`~1` escaping).
Missing targets and external file/HTTP references are actionable errors. Bundle such files
before importing. Raw example/default payloads are data rather than reference graphs;
selected Example Object references are resolved when used. Unselected example payloads are
omitted from preserved metadata. No externalValue example is fetched. Semantic object chains
are limited to 32 references; cyclic schema graphs are preserved without recursive expansion.
Anchor-style JSON Schema reference fragments are outside this slice.

Source files are capped at 4 MiB. YAML parsing uses serde-saphyr 2.0.0, with deserialization limits,
with duplicate-key errors, strict booleans, unsupported-tag rejection, one document, depth 64,
100,000 nodes, 300,000 events, 4 MiB total scalar/recorded-anchor bytes, and bounded anchors,
alias counts and replay work. There is no include resolver or property interpolation source.
Errors do not echo input lines, values, private paths or native parser diagnostics.

## Mapping rules

- A document becomes one new collection. An existing title receives a visible `(import N)`
  suffix. Native UUID filenames and fresh variable IDs prevent overwriting existing entities.
- Missing, null or empty operation response definitions produce a compatibility warning;
  requests are still imported, without inventing a response schema. Non-object response
  definitions are rejected. This intentionally accepts incomplete descriptions from API-client
  exports; it does not certify their OpenAPI conformance.
- GET, PUT, POST, DELETE, OPTIONS, HEAD, PATCH and TRACE operations become requests. Operation
  summary, then operationId, then method/path provide the name. Display names are bounded and
  control characters removed; full operation metadata remains available for future export.
- The first operation tag proposes a folder. Additional tags remain in metadata and are
  reported. Callbacks/webhooks remain descriptive metadata, not runnable requests.
- Server precedence is operation, path, root. The first server is selected and alternatives
  are reported. Server-variable defaults are flattened into the base URL. Each distinct base
  URL becomes a fresh project variable; absolute HTTP(S) defaults are offered for OS storage.
  Missing/relative/unsafe/unresolved server URLs require a local base URL value after import.
  URLs with embedded credentials, query strings or fragments are not adopted as base defaults.
- Path and operation parameters merge by location/name; operation parameters override the
  corresponding path parameter. Primitive query/header/form examples and defaults become
  literal, editable request values, not project variables. Optional rows start disabled.
  Header names are validated. Path examples/defaults are percent-encoded as one segment.
  Missing required values retain a persistent request review lock. Missing path values stay
  visible as `{id}` in the URL; configure them, confirm review, and save before sending.
- Only server URLs and authentication create project variables, with readable names such as
  `baseUrl` and security-scheme credential names. Existing names are reserved; actual collisions
  add `_2`, `_3`, etc. Each definition receives a fresh UUID. Distinct server URLs remain
  separate variables; requests using the same server share its variable. Existing project
  values are never read or reused to choose a binding.
- One body media type is selected: JSON/+json first, then supported text/form media, then the
  first available type. Explicit JSON/text examples become editable request content in the
  collection file. Missing body examples are reported, without invented example values.
  URL-encoded and primitive multipart properties become form fields. Other body types require
  a body file selected after import; no source attachment path is opened or copied.
- Required cookie parameters, array/object parameter serialization, non-default parameter
  styles/allowReserved/content, per-field body encoding, and binary/complex form fields are
  visible blockers in this initial slice. They are not silently converted into different wire
  semantics. Optional cookie parameters are omitted with a warning.

Security follows operation override, then root security. Empty/absent security explicitly
maps to None, so an existing project's auth cannot unexpectedly reach an imported API.
Basic, Bearer, header/query API keys and OAuth2 Client Credentials produce the existing auth
models and fresh credential-variable references. Client Credentials defaults to Basic client
authentication, with a review warning. Supply credential values and review the endpoint before
sending. Imported tokens are not acquired during import. The first security alternative is
selected and reported. Combined AND requirements and unsupported required schemes (cookie
keys, browser OAuth, OpenID Connect, mutual TLS, other HTTP schemes) block confirmation.

Imported executable literals cannot implicitly bind pre-existing Fluxkit variables. Paths,
parameter/form names and media types containing `{{…}}` are blocked; body examples containing
that syntax are omitted with a warning. Parameter/form values with that syntax are cleared
and require explicit review, so they cannot capture an existing local value. Values held in
generated variables are substituted
once, so their contents cannot recursively capture another project's local value.

## Values, privacy and compatibility

The preview lists generated variable names and whether defaults/examples exist, without
exposing their values. By default, confirmation stores available variable values in the OS
credential store. Clear that checkbox for definitions-only import; it performs no OS access.
Values above the adapter's 2,560-byte limit are omitted and reported. No existing variable
name or local value is reused implicitly. Unconfigured values are supplied through Variables.

**Show variable definitions** includes each variable's origin: server URL or authentication
scheme/credential role. Before starting an OpenAPI
import, **Rename** and **Apply name** update that variable's executable request references
throughout the entire preview, including routes beyond the first 200 displayed requests.
Names must satisfy the existing variable syntax and remain unique across the project.
Names/UUID references in authentication keep their associations; preserved source metadata
is not rewritten. An unapplied name edit blocks confirmation. Renaming has no filesystem or
credential access, is bounded by the existing collection limit, and is disabled after import
has started so partial-import recovery keeps stable names and identities.

To configure the API's server, set the one generated server variable's value in Variables,
using shared defaults or per-environment values. Every request with that placeholder uses
the same selected value. Naming changes apply to new previews. To obtain the improved
mapping for an older completed import, import the source again as a new collection and
review its variables; existing collections, saved values and open drafts are preserved.
The existing native pre-dispatch resolution remains authoritative and blocks missing values.

Parameter/form values and body examples are saved as plain editable request content after
the explicit import action. This includes literal header/query values, which may contain
private data; the preview states this distinction. Review them before sharing a collection.
For sensitive fields, create a project variable explicitly and replace the literal with its
placeholder. Do not treat request files as a secret store or commit private payloads.

Collection version 1 gains optional `openApi: { version, document, operations }` metadata.
The document preserves API descriptions, schemas, response definitions, tags and security descriptions for
later export. The operations map links stable imported request UUIDs to original operation
JSON Pointers, so renaming a request does not lose its source association. Metadata for removed
requests can remain in this map; future export must select only live request identities.
Source example/default annotations, server declarations, tokenUrl values and
vendor extensions are removed so generated variable values are not duplicated into metadata.
Schema property names and literal enum/const constraints are preserved. This sanitized
metadata is not itself promised to be a standalone valid/lossless OpenAPI export; export must
reconstruct and validate the selected operations and report omissions. Existing supported
files load without migration or rewrite. Older builds reject the new field rather than
silently discarding it. Request editing/saving preserves collection metadata.

## Commit and failure recovery

Native preview state holds the generated plan and candidate OS values in memory. The UI gets
an opaque UUID, counts, mappings and compatibility messages, not an executable file-write
plan. A preview is bound to its authorized project handle and captured project, collection
and environment-file revisions. Native preparation/commit are serialized; at most two plans
are retained, and plans older than ten minutes are discarded on the next import interaction.
The request preview renders at most 200 rows and the expanded variable list at most 100.

On confirmation, Rust validates the generated collection/definitions and project limits
before writing. It holds the project advisory lock and checks captured revisions. It then:

1. Adds the generated variable definitions through atomic replacement and the existing
   environment backup path. This makes every later attempted OS entry discoverable.
2. Optionally stores defaults/examples under fresh project/variable/default tuples. A retry
   skips matching imported values and refuses to overwrite a locally edited value.
3. Rechecks project/collection/definition revisions, then creates the collection with
   no-clobber persistence and generated UUID paths.

This is not a multi-file/credential-store transaction. After an OS or final file failure,
added definitions and any saved OS values remain visible in Variables. The error explains
that no complete collection was created yet. Retry the same open preview to resume with the
same IDs. Closing the preview or restarting discards the in-memory plan, not those definitions
or values: inspect/remove the newly added variables listed in that preview explicitly before
starting a fresh import if they are no longer wanted. Existing unrelated values are retained.
Backups restore definitions, never deleted credential values.

External edits block commit or the final revision check. Arbitrary external editors can
ignore advisory locks; the narrow final-check/write race documented in FILE_FORMAT still
applies. Permission failures and forced termination can leave partial definitions/values,
but values have definition IDs on disk before they are attempted. No hidden plaintext
credential cache, source-file copy, response cache or import history record is created.

Commands are choose_openapi_import(handle), rename_import_variable(handle, id, oldName, name),
commit_openapi_import(handle, id, storeValues) and discard_openapi_import(id), restricted to
the bundled main window. Rename validates preview ownership, uniqueness and the pre-commit
state on a worker and returns the updated Preview without resolving any values. Native
file selection authorizes the source; frontend-supplied filesystem destinations are not used.
A commit returns the updated ProjectView plus an optional recoverable error. Open drafts stay
open while the imported collection and environment definitions refresh.

## Dependencies and remaining milestone 8 work

serde-saphyr 2.0.0 and granit-parser 2.0.0 are MIT/Apache-2.0, compatible with the pinned
Rust toolchain. The lockfile adds 11 packages (parsing, encoding and diagnostic helpers);
there is no frontend/runtime service dependency or telemetry. Upstream notices are preserved
under `assets/notices/openapi-import/` and included in
the desktop bundle. `scripts/update-import-notices.py` regenerates them from locked package
sources. The transitive encoding_rs tables also retain their BSD-3-Clause WHATWG notice.
Parsing and filesystem/OS work run on workers, not the UI thread.

Postman collection/environment import is described in [POSTMAN_IMPORT.md](POSTMAN_IMPORT.md).
OpenAPI YAML export is defined in [OPENAPI_EXPORT.md](OPENAPI_EXPORT.md).
Native Fluxkit export/import, external reference files,
additional source versions and broader executable mappings remain separate slices.

Mapping references: [OpenAPI 3.0.4](https://spec.openapis.org/oas/v3.0.4.html),
[OpenAPI 3.1.2](https://spec.openapis.org/oas/v3.1.2.html).
