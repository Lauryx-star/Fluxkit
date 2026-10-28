# Postman migration (milestone 8)

Use **Import Postman…** in an open project. Choose **Choose collection JSON…** for a
Postman Collection v2.1 export. To import an environment, select an already imported Postman
collection in **Collection for environment mapping**, then **Choose environment JSON…**.
Review the compatibility report and source-name to local-variable mapping before confirming.
Cancelling before confirmation writes no project definitions, collection or OS values.
In a server workspace the import is synced to the server and can store its values there; see
[SERVER_VALUES.md](SERVER_VALUES.md#imports-into-server-workspaces) for the classification of
secret and ordinary variables.

## Supported sources and mapping

Only Collection v2.1 JSON is accepted, identified by its official `info.schema` URL. The
ordinary `schema.getpostman.com/json/collection/v2.1.0/collection.json`, draft-07 schema ID
and `schema.postman.com` draft-07 URL are accepted literally, never fetched. Other versions,
YAML, account integrations, bulk archives and globals exports are unsupported. Environment
exports need a string `name`, a `values` array and `_postman_variable_scope: "environment"`.
Both inputs are UTF-8 JSON, at most 4 MiB, with duplicate keys rejected. The bounded parser
used by OpenAPI also limits nesting, nodes, scalar bytes and work. This is an executable
subset mapper, not a complete Postman schema/runtime implementation.

Collections receive new UUIDs and numbered names on title collisions. Folder nesting,
request names/methods, URL strings and structured URLs, query/header rows, disabled rows,
raw JSON/text, URL-encoded forms and text multipart fields map to existing Fluxkit models.
Raw query pairs are decoded and represented as query rows; a structured query list is
preferred over the duplicate raw query. Repeated keys stay repeated. `:path` variables with a
value are inserted into the URL, percent-encoded as one segment; `:path` variables without a
value become new variables named after the parameter that need a value before sending. URL
fragments are omitted. Body-file requests require explicit attachment selection
after import; no source path is opened or copied. Inline file data is not adopted.

Collection/folder/request Basic, Bearer and header/query API Key auth map to native auth
models. Missing auth inherits, with explicit None at collection root to isolate an imported
API from an existing project's auth. `noauth` stays explicit None. OAuth2 Client Credentials
maps token URL, client ID/secret, scope and Basic/body client authentication. Review these
settings before sending. Other OAuth flows with an explicit saved access token become manual
Bearer auth with a warning; no original browser login/renewal flow is reproduced. Unsupported
auth without a usable manual token leaves authentication inactive and marks affected requests
for mandatory import review. Import never acquires tokens.

## Variable scopes and privacy

Postman variable names are kept. A source name that already exists as a project variable
reuses that variable: imported requests reference its ID and its existing default and
environment values stay unchanged (the Postman default is not stored for it). This is
deliberate, so several collections can share e.g. one base-URL variable. It also means an
imported request can send an existing value to the URLs the collection defines; review reused
variables (marked **existing project variable**) before importing third-party collections.
Other source names become new variables with that name. Names that are not valid Fluxkit
variable names are sanitized (invalid characters become `_`, a leading `_` is added where
needed). Undefined collection/global references follow the same rules; new ones need a local
value. Scalar collection defaults of new variables can be stored in the OS credential store
after confirmation. Disabled definitions receive no default. Compound values require manual
setup.

Literal URLs and literal header, query, form and `:path` values stay editable request content
and are saved in collection JSON after the explicit confirmation, like literal raw body text.
Review them before sharing a collection; for sensitive fields, create a project variable and
replace the literal with its placeholder. Recognized credentials (Basic username/password,
Bearer and API Key values, OAuth2 client ID/secret, token URL and scope) are still represented
by new OS-held variables named after their role (`password`, `clientSecret`, …). Generated
names never reuse an existing variable; a collision receives a numeric suffix (`password_2`).
Original scripts, saved responses, cookies, source files and source credential payloads are
not persisted. This is a request migration, not a lossless copy of a Postman collection.

**Show variable definitions** lists each variable with its source name. Before starting a
collection import, **Rename** and **Apply name** change a variable's name throughout the
preview, as for OpenAPI. For Postman, entering the name of an existing project variable
reuses that variable instead (auth references and the stored source-name mapping move to its
ID; its values stay unchanged). Renaming a reused variable to a free name creates a new
import-owned variable instead. Names already used by another new variable of the same import
are rejected. Renaming has no filesystem or credential access and is disabled for environment
previews and after import has started.

Environment import creates a new **project-wide** environment, with a numbered name on
conflict; the selected environment does not change. Matching source names use the selected
collection's saved variable IDs. The new environment ID guarantees that no existing
collection default or other environment value is overwritten. Disabled entries are omitted,
so normal Fluxkit default fallback remains effective. Environment keys not referenced by the
collection use the existing project variable with that name (only its value in the new
environment is written) or receive new standalone variables, reported as not connected to
requests. They can be used in any collection manually. Other collections do not independently select an environment.

The optional collection field `postman: { version: "2.1", variables: { sourceName: variableId } }`
retains only the mapping, never source values. It survives request/collection edits and
renames. A removed mapped variable makes a later environment import fail with an actionable
message; values are not silently redirected by display name. Older builds reject this new
field rather than discard it. Existing files without it need no migration or rewrite.

## Compatibility report and recovery

The report warns about omitted descriptions, scripts/tests, saved responses, protocol/runtime
settings, body-language metadata and file selection. Scripts are never executed, fetched or
translated into Custom Login. Dynamic placeholders (`{{$…}}`), unsupported authentication and multipart file fields now
import with mandatory review notes. Dynamic variables become unconfigured local variables;
file fields retain their name with an unconfigured placeholder until an attachment is chosen
or the field is deliberately removed/disabled. Source file paths are never read. Unsupported
auth is inactive; select the intended supported authentication in Fluxkit. An inherited auth
review applies to descendant requests; unaffected sibling requests keep their normal behavior.

Affected requests persist optional `importIssues` notes. Their editor shows a review panel and
disables Send, Send & save response, and token operations. Native execution checks both draft
notes and the saved request before resolving values or sending. Configure the open items,
check **I reviewed and configured this request**, then **Save**. Checking the box alone or a
failed save does not lift the saved lock. Explicit acknowledgement is a user review, not an
automatic proof of API correctness; missing dynamic/file-placeholder values continue to block
normal execution until configured, removed or disabled. Review notes survive renames, edits
and restarts. Older builds reject the new field instead of silently discarding this state.

Folder/request-scoped variable definitions, nested stored variable values, composed
Basic/Bearer/API Key credentials, unsupported body modes, non-empty per-field Content-Type
mappings and embedded URL credentials still block import. The preview explains the disabled
import action beside its button and offers **Review import blockers**. There is no hidden
TLS relaxation, redirect/session/proxy configuration or request execution.

Values above the OS adapter's 2,560-byte limit become unconfigured. Unsupported nested or
oversized environment values are omitted with a warning; existing defaults can then apply.
The checkbox allows definitions-only import, with no credential-store access. Variable values
never enter project files, local browser storage, diagnostics or preview payloads.

The [OpenAPI import recovery contract](OPENAPI_IMPORT.md#commit-and-failure-recovery) also
applies: an in-memory preview is bound to authorized project/revisions, preparation and
commit are serialized, and writes validate limits/revisions under the project lock. Definitions
are saved before attempting OS entries. Collection import then creates one new collection;
environment import writes only definitions and new environment-value slots, never the source
collection. This spans independent systems and is not an atomic transaction.

After a partial failure, retry the same open preview to resume without duplicate entities.
Closing/restarting drops only the in-memory plan. Added definitions/values remain visible.
For an abandoned environment import, inspect/remove the newly created environment through
Variables and its explicit value-deletion confirmation; retain existing mapped variables
that requests still use. For an abandoned collection import, inspect the newly added variables
listed in the preview (reused existing variables are not changed). Locally edited imported values are never overwritten on retry. Existing unrelated
values are not deleted. Arbitrary external editors can ignore advisory locks; the existing
final-check/write race remains documented in [FILE_FORMAT.md](FILE_FORMAT.md).

Native command `choose_postman_import(handle, collectionId)` opens the file chooser;
`collectionId: null` selects collection import, a validated local UUID selects environment
mapping. It shares opaque preview IDs and the existing `rename_import_variable` /
`commit_openapi_import` / `discard_openapi_import` command lifecycle with OpenAPI. Preview
variables carry an `existing` flag for reused project variables. Only the bundled main window has
permission. The frontend cannot submit import paths, write plans, environment IDs or candidate
values. No dependency or service was added for this slice.

Format references: [official Collection v2.1 schema](https://schema.postman.com/collection/json/v2.1.0/draft-07/collection.json),
[Postman export documentation](https://learning.postman.com/docs/getting-started/importing-and-exporting/exporting-data/).

OpenAPI YAML export is described in [OPENAPI_EXPORT.md](OPENAPI_EXPORT.md).
Native Fluxkit backup export/import remains separate milestone work.
