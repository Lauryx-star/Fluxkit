# OpenAPI YAML export (milestone 8)

Use **Export OpenAPI…** in an open project. Select the project, a collection, a folder
subtree or one saved request, then **Preview export**. Review the file list, compatibility
report and blockers before choosing a destination. Unsaved request changes are explicitly
reported and excluded; close the dialog and save first to include them.

The agreed output version is **OpenAPI 3.1.0**, serialized as readable YAML. A request,
folder or collection produces one document. A project produces a new `fluxkit-export-<id>`
folder under a chosen parent, with one YAML document per non-empty collection. Filenames use
sanitized collection labels and stable UUIDs, avoiding name collisions and unsafe source
paths. Empty project collections are reported and omitted; empty single selections block.
The single-document save dialog suggests `fluxkit-openapi.yml`.

## Values and descriptive mappings

Export never accesses the OS credential store, selects an environment, executes a request,
reads an attachment or changes project files. Stored variable values, credentials, acquired
tokens, token input compositions, history and response payloads are excluded. Query/header/
form values and raw body examples are also omitted, including literal values in saved files.
Enabled field names and generic string schemas describe these inputs; disabled rows are
omitted. A body retains its media type and a generic schema or applicable imported schema.
File inputs are binary schema descriptions, without filenames or paths. Repeated form names
become one schema property and produce an explicit serialization warning.

Saved literal URL hosts/paths, names, descriptions and imported schema constraints may still
contain private application information. The preview calls this out: review the generated
file before sharing. This feature is an API description, not a lossless request/workspace
backup or a payload export. It is not a secret-detection service.

HTTP(S) URL authorities become operation-level servers, and saved paths become OpenAPI path
items. `{{name}}` server/path placeholders become `{name}` with required string path
parameters. Server defaults are conspicuously synthetic (`https://example.invalid`,
`example.invalid` or `443`, depending on position), never resolved local values. Relative
paths use a synthetic server with a warning. If the entire URL is stored in one variable,
its path is unknowable without resolving that value; confirmation is blocked. Save an
explicit path outside the base variable to describe it. Dynamic/ambiguous URL schemes,
embedded credentials, unclosed/literal braces and invalid server templates are blocked.
Raw URL query values/fragments are discarded; query names join the saved parameter rows.

Collection, folder ancestry and request authentication are resolved over project auth using
the same None/inherit precedence as execution. Basic, Bearer and API Key become security
schemes without values. Client Credentials uses a synthetic token endpoint and empty scope
map, with a warning that local compositions and token lifecycle cannot be reproduced.
Custom Login automation is omitted with a clear warning, leaving no claimed automatic
security requirement. Enabled manual auth fields override configured auth; their values
are omitted and the operation does not claim the overridden requirement. Authorization,
Cookie, Accept and Content-Type are not exported as ordinary header parameters. Variable
header/query/form names and duplicate enabled query/header names block confirmation rather
than guessing names or serialization semantics.

Requests still awaiting import review produce a compatibility warning that their description
may be incomplete; export does not clear their execution lock.

Folders become operation tags. A stable request UUID supplies operationId. Duplicate method/
canonical-path pairs block export even across different servers. Equivalent path templates
with different parameter names also block rather than produce ambiguous OpenAPI paths.
No operation is silently overwritten or chosen as an example variant. Non-OpenAPI HTTP
methods such as CONNECT are reported as blockers.

## Imported descriptions and validation

Imported OpenAPI metadata retains its stable source-operation association across request
renames. Exports use current saved request names, URLs, fields, bodies and effective auth;
original response descriptions and schemas can still describe the operation. Relevant
component sections (schemas, responses, headers, parameters and requestBodies) remain in
that collection's document so internal references can be preserved. The original API version
is retained where available; newly authored collections use informational version `1.0.0`.
Examples/default annotations, callbacks, links, source server/security definitions and vendor
annotations are omitted. Unused retained components can remain in the description.

Known OpenAPI 3.0 schema nullable/type and boolean exclusive-bound forms are converted to
3.1 forms. This is a bounded transformation, not a general dialect migration. Review retained
schemas after editing request bodies; Fluxkit does not infer authoritative response schemas
from observed responses. Unknown custom dialect semantics are not certified by this exporter.

Before saving, generated documents undergo bounded YAML round-trip checks, method/path/
parameter conflict checks, internal reference checks, response status/description checks
and basic schema-shape/type checks. External/missing references or invalid retained metadata
block confirmation. This is not a complete runtime OpenAPI/JSON Schema conformance engine.
Development fixtures are additionally checked against the official OpenAPI 3.1 meta-schema
with an independent validator; that evidence does not certify arbitrary imported schemas.
Each document is limited to 4 MiB, the total to 32 MiB, and the existing project/item limits
still apply. Native mapping and file work run on workers; the file list renders at most 200
rows. YAML ordering is deterministic for the same saved input.

## Writes, authorization and recovery

`preview_openapi_export(handle, target)` accepts tagged targets `project`, `collection`,
`folder` or `request`, with their validated entity UUIDs. Native code loads the saved project
and definitions under the project lock. The UI receives an opaque preview UUID, file names,
operation counts, byte counts, warnings and blockers; no destination or write plan is accepted
from JavaScript. `save_openapi_export(handle, id)` opens the native file/folder chooser and
rechecks project/collection/definition revisions before writing. Cancelling that chooser
returns to the same preview. `discard_openapi_export(id)` releases it. Permissions cover only
the bundled main window. At most two previews are retained; previews older than ten minutes
expire on the next export interaction.

Single files use the existing synchronized temporary-file/no-clobber persistence helper.
Existing files and symlink targets are never replaced: choose a new filename. Project export
exclusively creates a new folder, then writes each new document atomically. This is not a
multi-file transaction; disk/permission failures may leave a visible partial new export
folder. Inspect it and prepare a new preview to retry. No automatic deletion or overwrite
is performed. Final revision checks cannot exclude arbitrary external writes immediately
after the check; the exported snapshot is consistent, not a continuously synchronized view.

Serialization enables the existing serde-saphyr 2.0.0 serialize feature and adds the small
MIT/Apache-2.0 nohash-hasher 0.2.0 package. Updated upstream notices ship with the desktop
bundle; `scripts/update-import-notices.py` regenerates the YAML dependency notices. Parsing
still has no include/property/network resolver. No frontend dependency or service was added.

Specification references: [OpenAPI 3.1.0](https://spec.openapis.org/oas/v3.1.0.html),
[official 3.1 validation schema](https://spec.openapis.org/oas/3.1/schema/2022-10-07).
Native Fluxkit backup export/import remains separate milestone work.
