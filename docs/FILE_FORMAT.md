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
in application-data storage as defined in [HISTORY.md](HISTORY.md); recent-project settings remain
planned. Reusable values are stored exclusively in the OS credential store. Credentials should use placeholders when drafting;
request fields are plain JSON, not a secret store. Personal settings and resolved reusable
values must stay outside portable files as their features are added.

No cross-project recent-path list or UI tabs are persisted yet. After restarting, use
Open project and choose the same project folder to recover saved collections and requests.
