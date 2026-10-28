# Project environments and local values

Every collection and request uses the environment selected for its project in the header.
**Defaults** selects shared default values without an environment override. Selection is
local to this machine and persists across restarts, independently for each project UUID.
The last active saved project reopens on startup; request tabs and unsaved drafts are not restored.

## Definitions and references

**Variables** opens the project manager. Add, rename, or remove environments and variables.
Definition changes save immediately. Variable names are case-sensitive and unique within
one project. Names start with an ASCII letter or underscore and then contain letters,
digits, underscores, dots, or hyphens. Environment display names are also unique.
Renaming retains UUIDs and OS value associations. Because templates reference variable
names, update existing placeholders when renaming a variable; no request text is rewritten.

An optional `environments.fluxkit.json` file contains `schemaVersion: 1`, `environments`,
and `variables`. Each definition has `id` (canonical UUID) and `name`. The implicit reference
for each value is `(project UUID, variable UUID, environment UUID or default)`. The service
name in OS storage is `app.fluxkit.variables.v1`; its account key joins those three
identities with `/`, using `default` for the shared value. Values are never JSON fields.
A missing file means empty definitions, so existing version-1 projects need no migration.
The manifest and collection format remain version 1. Unknown fields and future versions
are rejected without rewriting. Older Fluxkit versions cannot edit these definitions.

Definition writes use the existing project lock, revision checks, atomic replacement, and
`.fluxkit/backups/environments.json` recovery copy. Managed symlinks are rejected.
There are at most 100 environments and 1,000 variables per project. Existing JSON byte
limits still apply. A shared folder may be opened by multiple users, but each user's
credential store remains local. Copies retaining the same project UUID on the same machine
share value associations and selection; a copy is not a newly identified project.

## Value editing and resolution

Choose a variable and a slot: **Shared default** or one environment. Type a replacement and
choose **Save value**. Empty strings are valid, explicit values. Saving clears the editor.
New input is readable by default. **Hide value** masks the editor; **Show value** makes
its current contents readable again. This switch neither loads nor saves a value and does
not mark the input as changed. Changing slots, clearing, or successfully saving resets
the empty editor to readable input. No partial value prefixes are loaded automatically.
All variables continue to use OS storage and the same log/history redaction rules;
visibility is an editor preference, not a confidentiality classification.
**Load & reveal** explicitly reads that single slot and displays its value. Opening a project,
changing the selected environment, or opening the manager does not preload values.
**Clear editor** clears UI memory without removing the saved slot. Unsaved edits require
confirmation before changing slots, reloading, closing the manager, or closing the window.

Use `{{variableName}}` in URLs, enabled query/header keys and values, text/JSON bodies,
and enabled form keys and text values. Substitution is literal and single-pass, with no
recursive expansion, scripting, escaping, automatic URL encoding, or JSON escaping.
Placeholders in file contents and attachment paths are not expanded. Body type and enabled
rows determine which fields participate. Construct valid URL, JSON, and header templates;
HTTP-specific validation and encoding belong to request execution in milestone 4.

Resolution runs in Rust. For each referenced variable it reads the selected environment
slot, then the shared default only if the environment slot is absent. An empty value never
falls back. Missing definitions, malformed/unclosed placeholders, missing required values,
and denied/locked/unavailable OS access fail the check. Access denial never triggers a
fallback. A stale selection removed externally requires an explicit selection before checking.
No alternate environment is substituted. Repeated variable references share a temporary
lookup cache for that one check; it is dropped afterwards. `Check variables` checks the current draft without
returning resolved text or sending an HTTP request. Repeat after changing the draft or
selection. The check accepts at most 1 MiB of template text and bounds resolved content to
8 MiB. It does not promise the template is a valid executable HTTP request.

The result panel below the URL shows progress, success, or an actionable failure and remains
visible after completion. Requests without placeholders receive an explicit message saying
there is nothing to resolve. Results belong to the checked request and project; switching
requests does not display another request's result. Rechecking updates the panel in place.

Values use the native macOS Keychain, Windows Credential Manager, or Linux Secret Service
through [keyring 3.6.3](https://docs.rs/keyring/3.6.3/). Linux needs a running/unlocked Secret
Service provider (for example GNOME Keyring or KWallet), D-Bus, and development packages
for libdbus when building. No in-memory mock or plaintext fallback is selected for the three
target platforms. Values are limited to 2,560 UTF-8 bytes, matching the Windows credential
blob limit; the adapter stores bytes rather than UTF-16 password strings. Oversized values
receive an explicit error. The library is MIT/Apache-2.0 licensed; no frontend storage plugin
is exposed. Native access runs on a blocking worker, outside the UI thread.

## Ownership, deletion, and failure recovery

A variable owns its default slot and one slot per environment in its project. Removing a
variable deletes all those local slots. Removing an environment deletes that environment's
slots for every variable and preserves defaults and other environments. **Remove value**
deletes only the selected slot. Each UI action explicitly confirms the permanent local
value deletion. Close project never deletes definitions or values. Missing entries count
as already deleted. No other project or retained tuple is deleted.

Files and OS credentials do not form an atomic transaction. Removal validates definitions
and revisions first, then deletes the affected local slots, then saves definitions. If OS
deletion fails partway, definitions remain and an error explains that some values may
already be gone; unlock the store and retry. Retrying deletion is safe. A subsequent file
write/conflict/settings failure likewise reports that values may already be deleted;
reload definitions, resolve the file problem, and retry removal. If definitions were changed
externally during that operation, inspect the reloaded definitions before retrying. Recovery
copies restore definitions only, never deleted values. Re-enter any required missing values.

Setting a value changes only an existing authorized tuple in the credential store and
checks the definition revision first. A failed OS write preserves editor input for retry;
there is no file write containing the value. A failure after a provider write may leave the
new value saved: explicitly load that slot to inspect it before retrying. External processes
may ignore advisory locks; the narrow filesystem race documented in FILE_FORMAT still
applies. Do not manually remove definitions in a text editor if you expect OS cleanup;
Fluxkit cannot discover values for definitions it no longer knows about.

Saved authentication settings reference variable UUIDs. Deleting a variable they reference
is blocked before OS deletion; replace/remove those settings first. See
[AUTHENTICATION.md](AUTHENTICATION.md). Removing an environment continues to remove only
its explicitly confirmed values.

Acquired tokens are separate records owned by effective auth configuration and environment.
Environment/variable removal does not automatically delete those records. Use Forget local
token before removing its configuration/environment when cleanup is desired; see
[token ownership and recovery](TOKEN_AUTHENTICATION.md).

## Highlighting, completion, and hover preview

URL, query/header keys and values, form fields, and text/JSON body editors highlight
well-formed `{{variableName}}` tokens. Defined names are green; unknown names have a
contrasting dashed underline. This indicates definition presence, not whether a value
exists in the OS store. Check variables remains the full validation action.

Typing `{{` opens a list of project variable names. Additional letters filter names with
a case-insensitive substring match; insertion preserves the definition's exact spelling.
Arrow keys select a suggestion, Enter or Tab inserts, Escape dismisses, and clicking a
suggestion inserts it. The list shows up to 20 matches; type more letters to narrow it.
Existing closing braces are reused and surrounding text remains intact. Composition input
is not intercepted. Suggestions use definitions only and never fetch credential values.

Hovering a highlighted token explicitly reveals that variable's effective value in a small
tooltip, using the current project environment and shared-default fallback. This includes
sensitive values: the current secret classification does not suppress explicit hover reveal. No other variables
are read. Missing values, unknown names, locked stores, and stale project/environment
state show errors. Empty values are identified explicitly. Values are rendered as text,
never HTML. Moving away, scrolling, editing the field, or switching project/environment
hides the preview; late replies are ignored. The preview is never persisted or logged.

The syntax mirror follows native input/textarea scrolling and leaves the editable value
unchanged. Highlighting and hover targets are bounded to the first 500 tokens per field;
a visible notice appears beyond that limit, with all text still editable and preserved.
These limits bound rendering work; they are not a measured latency or memory guarantee.
No new editor dependency, browser storage, or project format change is required.

## Local selection and command contract

Selection lives in the OS application-data directory under
`app.fluxkit.desktop/environment-selection/<project UUID>.json` (the exact app-data root
is supplied by Tauri). A file contains `schemaVersion: 1` and nullable `environmentId` only.
It contains no variable values or project directory paths. Malformed settings fail visibly;
the manager’s **Use Defaults** recovery button can replace a malformed selection file.

The main window has one narrow `environment_action(handle, action)` command. Handles must
have been granted by the native project chooser. Tagged actions are `load`, `select`,
`save`, `setValue`, `reveal`, `preview`, and `check`. `save` carries definitions plus their expected
revision; `setValue` carries variable/environment IDs, nullable value (null deletes), and
expected definition revision. `reveal` returns only the explicitly requested slot. Replies
contain `view` (definitions/revision/selection/stale-selection flag) and nullable `value`.
`preview` carries a variable ID, expected definition revision, and expected nullable
environment ID. Rust checks both before reading the effective value. Only reveal and
preview return a value. Errors never include resolved data or native provider details.

Routine tests use temporary folders and a fake values adapter. The separate opt-in OS test
creates a random isolated synthetic credential, verifies reading, and deletes it:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked os_credential_roundtrip -- --ignored
```

The test is excluded from the default suite because it requires OS-store access. It must
be exercised on each platform before claiming that platform's integration is verified.

## Server values and account isolation

Server workspaces resolve ordinary values from shared project values and secrets from the
caller's client-encrypted values, with no OS plaintext fallback. The account vault must be
unlocked before secret resolution. Older definitions without `secret` remain secret. Changing
classification is explicit and does not read or upload any stored value; the server removes
values of the old kind, and deleting a definition removes its values.

Server `setValue` additionally carries `expectedValueRevision`, the revision of the edited slot
(`0` when it has no value yet); replies include `view.serverValues: { slots }`, keyed by
`variableId/environmentId` (`default` for the default slot). Both the loaded definition revision
and the slot revision must match. On conflicts, input is preserved; Reload fetches current
revisions before an explicit retry. Saved definitions are already on the server, so values for
new definitions can be stored right away. Imports store values themselves
([SERVER_VALUES.md](SERVER_VALUES.md#imports-into-server-workspaces)).

Caches, environment selection and acquired tokens are scoped to verified server URL and account
UUID. Local-only projects retain their OS keys and behavior. See
[SERVER_VALUES.md](SERVER_VALUES.md) for transfer consent, encryption, offline access and limits.
