# Fluxkit local project format

Version 1 stores projects as readable JSON in a user-selected folder. The desktop folder
chooser authorizes the root; native commands subsequently use an opaque session handle,
not a path supplied by the frontend. No database or browser storage is involved.

## Folder layout

```text
project.fluxkit.json
environments.fluxkit.json  # optional, definitions only
collections/
    <collection UUID>.json
attachments/
    <attachment UUID>.bin
.fluxkit/
    write.lock
    backups/
        project.json
        environments.json
        <collection UUID>.json
    trash/
        <collection UUID>-<deletion UUID>.json
```

A project manifest has `schemaVersion`, `id`, and `name`. Every collection file contains
`schemaVersion`, `id`, `name`, `folders`, and `requests`. Collection filenames match their
canonical lowercase UUID. Renaming display names does not move files or change identity.
Collections are independent API documents; logical folders and requests live inside them.
A single collection mutation therefore needs only one atomic file replacement.

## Hierarchy and requests

Folders have `id`, `name`, and nullable `parentId`. Requests have `id`, `name`, nullable
`folderId`, `method`, `url`, `params`, `headers`, and `body`. Parent references stay within
the collection. Cycles, duplicate identifiers, and missing parents are invalid.

Parameter and header rows contain numeric `id`, `enabled`, `key`, and `value`. Disabled
rows are preserved. The URL is an editable template. `{{variableName}}` resolves using the project-wide
environment; see [environments and local values](ENVIRONMENTS.md). Resolved values are
never serialized in project files.

The body contains `kind`, `text`, `fields`, and nullable `file`. Supported kinds are
`none`, `text`, `json`, `urlEncoded`, `multipart`, and `file`. Form fields contain `id`,
`enabled`, `key`, `value`, and nullable `file`. An attachment reference has a generated
project-relative `path` and its original display `name`. A multipart field with a file
uses that attachment instead of its text value. URL-encoded fields use text values.
Inactive body variants remain available when switching type in the editor.

Selecting a body file streams a copy into `attachments/` and records its relative path.
No absolute source path is serialized. An attachment copied before a cancelled or unsaved
edit may remain unreferenced; automatic attachment cleanup is deferred to avoid deleting
files still needed by another request or recovery copy.

## Save behavior

Request edits are explicit: Save or Cmd/Ctrl+S saves the selected request to its collection.
Dirty markers show uncommitted request changes. Closing a dirty tab or project, reloading,
or closing the desktop window requires confirmation. A failed save preserves edited input.

Desktop window closure and the Quit menu use the same three-way dialog: Cancel returns to
the editor, Discard changes exits without saving pending edits, and Save & quit saves all
dirty request tabs across open projects plus pending authentication, variable/environment
definitions and edited OS-store values. No requests are dispatched. Request destinations
are checked before writing; session drafts need an open project and collection. Dirty
requests sharing a collection are merged into one atomic save using its expected revision.
Any subsequent authentication save uses that returned revision and preserves saved requests.

Saving disables dismissal and background editing until completion. Invalid input, external
file conflicts, missing destinations or write failures keep the app open and preserve the
remaining edits. Cancel resumes editing; retry saves only changes that remain dirty.
Successful writes are retained when a later save fails. This is not a transaction across
collections, projects and the OS credential store. Already committed structural operations
and explicit earlier saves are unaffected by Discard changes. In-progress workspace/import/
export operations must finish before quitting; normal request execution still uses the
native shutdown/cancellation behavior. Browser preview retains its browser unload warning.

Creating or renaming projects, collections, and folders is an immediate structural operation.
Removing a folder also removes its descendant requests after confirmation. The collection's
previous snapshot remains in `.fluxkit/backups/`. Removed collections move to `.fluxkit/trash/`.
Close project only removes it from the current workspace; its files remain on disk.
Closing a project does not permanently delete its folder, credential-store values, or
local history. History is managed separately through Clear history and bounded retention;
see [HISTORY.md](HISTORY.md).
Variable/environment removal explicitly confirms deletion of the associated local values;
see ENVIRONMENTS.md for non-transactional failure recovery.

Each successful update retains one previous snapshot for that project manifest or collection.
Recovery is manual in version 1: with the project closed, copy the corresponding backup to
its original manifest/collection path, or restore a trashed collection under its UUID filename.
Backups and trash can contain request content; they are local recovery data, not telemetry.
They are excluded from future exchange formats by default. Trash is not automatically pruned
until recovery and retention requirements have been agreed.

## Consistency and limits

Native operations use a per-project advisory lock and compare SHA-256 revisions against
what the UI last loaded. External changes cause a conflict instead of a silent overwrite.
Writes use a temporary file in the destination directory, synchronize its contents, and
atomically replace the target where supported. New files never overwrite an existing file.
The previous contents are backed up before replacement, and revisions are checked again
immediately before replacement.

File locks coordinate Fluxkit processes. Arbitrary external editors may ignore those locks;
there remains a narrow race if an editor writes between the final revision check and the
atomic replacement. This is not a distributed or multi-file transaction guarantee.
Symbolic links below the selected project root are rejected for managed paths. Projects
are local user-owned data; this is not a sandbox against an attacker concurrently replacing
filesystem directories. Unknown JSON fields and unsupported schema versions are rejected
rather than discarded during rewriting.

For the initial loader, each JSON file is limited to 8 MiB, total collection JSON to 32 MiB,
a project to 1,000 collections, and a collection to 10,000 folders/requests and 32 folder
levels. These limits produce actionable errors. They do not limit streamed attachment size.
Names are non-empty, contain no control characters, and occupy at most 200 UTF-8 bytes.

Imported collections may additionally contain `openApi: { version, document, operations }`. This optional
version-1 field holds sanitized API schemas/descriptions, not source variable defaults or an
original source-file copy. See [OPENAPI_IMPORT.md](OPENAPI_IMPORT.md) for omitted annotations,
reader compatibility, new-variable ownership and multi-system partial-failure recovery.
Existing collection saves retain this field.

Imported Postman collections may contain optional `postman: { version: "2.1", variables }`
metadata. `variables` maps original source names to stable project variable UUIDs, allowing
explicitly associated environment imports after collection/request renames. It contains no
source values or scripts. Source names are bounded, UUIDs validated, and the map is limited
to 1,000 entries. Older builds reject this optional field; existing files need no migration.
See [POSTMAN_IMPORT.md](POSTMAN_IMPORT.md) for mapping and recovery rules.

Requests may contain optional `importIssues: string[]` review notes (at most 100 notes of
1,000 bytes without control characters). These are configuration notes, not source scripts or
values. Non-empty notes persist the import review lock; native execution/token operations
check both submitted and saved notes. Explicit review acknowledgement plus successful Save
clears the notes. Existing files need no migration; older readers reject the new field. See
[POSTMAN_IMPORT.md](POSTMAN_IMPORT.md).

## Compatibility and data ownership

Environment definitions are an additive optional version-1 file with their own revision.
Existing projects open without a migration or destructive rewrite. See ENVIRONMENTS.md
for its fields, implicit value references, and local selection settings.

The frontend/native contracts are mirrored in `src/projects/types.ts` and
`src-tauri/src/projects/model.rs`; Rust validates all loaded and submitted data.
Version 1 is the first durable format, so there is no earlier durable data to migrate.
Future incompatible changes must introduce a documented migration and recovery procedure.

Request definitions, selected attachments, and optional environment/variable definitions
are saved here. Authentication configuration is an optional `auth` field on project, collection, folder, and
request objects; see [AUTHENTICATION.md](AUTHENTICATION.md) for reference-only shapes,
inheritance, and compatibility with older readers. OAuth2/Custom Login definitions and local
token ownership are in [TOKEN_AUTHENTICATION.md](TOKEN_AUTHENTICATION.md). Minimal history lives
in application-data storage as defined in [HISTORY.md](HISTORY.md). Reusable values are stored exclusively in the OS credential store. Credentials should use placeholders when drafting;
request fields are plain JSON, not a secret store. Personal settings and resolved reusable
values must stay outside portable files as their features are added.

### Local startup selection

The OS application-data directory contains `workspace.json` (schema version 1):

```json
{
  "schemaVersion": 1,
  "lastProject": {
    "directory": "/absolute/local/project/path",
    "projectId": "550e8400-e29b-41d4-a716-446655440000"
  }
}
```

`lastProject` is null after explicitly closing the last open project. Opening, creating,
or activating another project updates this target; closing the active project selects the
remaining active project. App startup restores only this project's saved definitions with
the usual file validation and a fresh session handle. The recorded project identity must
still match. Missing, inaccessible, malformed or replaced projects produce an actionable
warning and leave the target unchanged for a later retry or manual Open project.

This file stores one machine-specific canonical path and project ID, with no request tabs,
drafts, responses, credential values or history. It is bounded to 16 KiB. Invalid JSON,
unsupported versions and symlink settings are rejected without replacement. Manual project
opening remains usable when local settings fail. Writes use a local advisory lock, atomic
replacement, an external-change check, and a `workspace.previous.json` recovery copy of the
previous valid settings. A normal window close waits for queued selection writes. To recover
damaged settings, close Fluxkit and preserve the damaged file before restoring a valid
previous copy; never copy these machine-specific settings into portable project files.

IPC: `restore_last_project()` returns `ProjectView | null` or a non-sensitive error.
`remember_project(handle: string | null)` accepts only an authorized open handle or an
explicit clear; the frontend cannot submit a path. Neither command reads credential values
or sends requests. Browser preview does not restore or record projects.


## Moving requests

`move_request(handle, input)` accepts an edited request snapshot, nullable saved source
collection/revision, target collection/revision, project revision and `allowMetadataLoss`.
It is limited to the bundled main window and an already authorized project. The native
operation holds that project's write lock, validates both collections, stable IDs, the
requested folder, duplicate request identities and revisions before writing. Save & move
saves only the chosen request; other open drafts remain uncommitted. Both affected collection
views/revisions are returned. Same-collection folder moves use one atomic replacement and
preserve request ordering. Unsaved requests add one new entry to the target.

A cross-collection move writes the destination first, then removes the source request.
Each replacement has its normal previous-version backup. It rechecks the destination before
removing the source. If source removal fails, Fluxkit restores the original target only if
it still exactly matches the snapshot this move wrote; external edits are never rolled back.
Failure leaves the editor open and reports whether rollback succeeded or a copy may remain.
Both final and intermediate collection sizes must stay within the project limit, so an
interrupted move can still be loaded.

These two file replacements are **not an atomic multi-file transaction**. A process crash
between replacements can leave the request in both collections. Reload and inspect both
copies; retain the intended (possibly edited destination) request and explicitly remove the
unwanted copy. The same UUID can appear in separate collection files during recovery; a
subsequent move refuses to overwrite an existing target UUID. If necessary, inspect the
normal source/destination backups with Fluxkit closed. Never assume a failed move discarded
all writes. The residual race with editors ignoring the advisory lock is the same as other
file writes; backups preserve earlier persisted source content.

Moves preserve the UUID, explicit auth, import-review flags and project-relative attachments.
The move does not read OS variable values or acquired tokens, or add resolved values to files. Inherited authentication
follows the destination, as previewed in the dialog. Project definitions/environment selection
are unchanged; moves across projects are unsupported.

For imported OpenAPI requests, a source operation association is copied when the destination
has no OpenAPI metadata or uses the same source document/version. Existing target associations
are retained. Collections based on different documents require explicit confirmation through
the metadata-omission checkbox; executable request fields are preserved, but the original
schema association is not added to the target. Original metadata remains in the source
collection for recoverability, including orphaned operation associations as allowed by the
import contract. Postman collection/environment mappings stay with their original collection;
project variable references in the moved request remain valid.

## Machine-local server workspaces

Server mode does not change the portable project schema. Its app-data cache is scoped by
verified server URL and account UUID under `server-accounts/<SHA-256>/`. Each cached revision
has a `.fluxkit/server-cache.json` owner marker; editable workspace generations instead have
`.fluxkit/server-workspace.json` with `serverUrl`, `accountId`, `serverProjectId` and `revision`.
An atomic per-project UUID pointer selects the current generation. These markers and pointers
are machine-local metadata, excluded from project snapshot uploads.

Explicit conflict recovery retains the previous workspace files and adds a
`.fluxkit/server-recovery` marker. It opens that generation read-only and creates a separate
current workspace for manual merge/retry. `ProjectView.serverOwned` identifies server data;
`serverRecovery` identifies retained edits, independently of online status. Recovery copies
remain identifiable after restart. Native access verifies account ownership even through
previous handles or the ordinary folder chooser. Legacy server markers without `accountId`
are preserved but rejected rather than adopted by a different account. See
[server account isolation and recovery](SELF_HOSTED_BACKEND.md#account-isolation-and-workspace-recovery).
