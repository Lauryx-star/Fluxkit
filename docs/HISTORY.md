# Minimal local request history

History is optional, project-scoped machine state, outside portable project files and
exports. Open **Request history** beside a project to inspect or control it. Recording
starts off. Each explicit project-scoped Send or Send and save response captures the
unresolved request template before native preparation; completion stores only method,
sanitized URL, received HTTP status, and the response view's elapsed milliseconds.
Failed/cancelled attempts have null status/time when unavailable. Unsaved session drafts
without a project and background authentication/renewal calls are excluded. Entries are
inert metadata: they cannot replay requests. No request/response bodies, headers, cookies,
authentication data, resolved values, environment IDs, or response payloads are recorded.

## Approved URL policy

The user explicitly approved keeping literal hosts, including internal hostnames, locally
visible. This is a conscious exception to hiding all potentially sensitive URL components.
History uses the unresolved template, never the resolved/executed URL.

- Keep HTTP/HTTPS scheme and canonical literal host, or valid `{{variable}}` placeholders.
- Strip embedded user-info and fragments entirely. Mask literal ports; keep port placeholders.
- Replace every literal run in path and query with `[redacted]`, including query names.
  Keep only `/`, `?`, `&`, `=` structure and valid variable placeholders there.
- A base-URL placeholder stays a placeholder. Separately configured query parameters are
  not appended to history, so neither their names nor their resolved values enter it.
- Malformed/unsupported inputs fail closed as `[URL redacted]`. Controls, backslashes,
  invalid authorities, and excessive inputs are not copied into history. Input scanning
  is bounded at 65,536 bytes and stored URL output at 2,048 bytes.

Examples:

```text
https://user:password@api.example.test:8443/private/item?key=secret#fragment
https://api.example.test:[redacted]/[redacted]/[redacted]?[redacted]=[redacted]

{{baseUrl}}/private/{{id}}?key={{token}}
{{baseUrl}}/[redacted]/{{id}}?[redacted]={{token}}
```

Variable names and literal hosts remain visible. Do not put secret values in variable
names or assume this policy hides internal hostnames. The history table renders URLs as
plain text without navigation or HTML execution.

## Controls and retention

Recording choice is persisted per stable project UUID. Disabling stops new entries while
preserving existing ones. **Close project preserves history**, including when reopened or
after restart; closing removes only the workspace entry. The user explicitly chose this
behavior. There is no destructive project-folder deletion operation. Use **Clear history**
with confirmation to remove that project's entries; it preserves a readable recording
choice. No other project's history is changed.

Both fixed limits apply: at most 100 entries per project, and age strictly less than seven
days. Completion time is an internal UTC Unix timestamp in seconds, not a fifth visible
field. Oldest entries are removed first; equal timestamps preserve completion order.
Future-dated entries are discarded conservatively after a backwards clock change.
Cleanup runs on startup, once per minute while open, before display, and on recording
operations/insertion. It works for closed projects and disabled recording. While the app
is closed, expiration resumes on next launch. An open panel refreshes after Sends and
once per minute. No project files or credential values are loaded for periodic cleanup.

Clear and recording changes replace a generation UUID. Requests begun under an older
generation cannot recreate cleared entries or record after disable/re-enable. A request
that never obtains an authorized project ticket does not create an entry.

## Storage, recovery, and IPC

Native `history_action(handle, action)` accepts an authorized open-project handle and
`{kind:"load"}`, `{kind:"setRecording",enabled:boolean}`, or `{kind:"clear"}`. Rust resolves
the project UUID from that handle, validates files, and returns
`{recording,entries,warning}`. Each entry has exactly `url`, `method`, `status`, `elapsedMs`;
no timestamps or private paths cross this boundary.

The OS application-data directory contains `history/<project-UUID>.json`, with version 1:

```json
{
  "schemaVersion": 1,
  "projectId": "synthetic-project-uuid",
  "generation": "synthetic-generation-uuid",
  "recording": true,
  "entries": [
    {
      "recordedAt": 1800000000,
      "summary": {
        "url": "https://api.example.test/[redacted]",
        "method": "GET",
        "status": 200,
        "elapsedMs": 12
      }
    }
  ]
}
```

Example UUID labels above are explanatory; actual identities must be valid UUIDs. Files
are limited to 256 KiB, validated with unknown fields rejected, and replaced atomically
from private synced temporary files. There are no history backups or payload caches.
Unix-created files use private permissions. UUID-only filenames and symlink checks reject
unsafe targets. A private advisory lock serializes mutations across processes; worker
threads wait at most one second for contention. This retries metadata locking only, never
API execution. Crash-left `.history-stage-*.json` files are removed during cleanup under
the lock; unrelated files are untouched.

Invalid/unsupported history is preserved without silent rewrite. The panel reports an
actionable error and offers explicit confirmed Clear to reset unreadable records with
recording off. Permission, disk, unsafe-path, or lock failures remain visible and may
prevent clearing. Background cleanup failures surface as a generic warning, retried on
the next minute tick. No path or private value appears in these errors. History work runs
on native blocking workers; a storage failure adds `historyWarning` to the HTTP result
without losing that result, changing its status, or sending the API request again.

History is local metadata, not a credential store or archive. Limits are fixed; payload
history, replay, history export, and configurable retention are outside this milestone.
