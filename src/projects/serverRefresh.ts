import { fromRequest, isDirty, type Draft, type ProjectView } from './types';

export interface RefreshOutcome {
  drafts: Draft[];
  /** Names of open, edited requests whose server version changed since it was loaded. */
  changed: string[];
  /** Names of open, edited requests that were deleted on the server. */
  deleted: string[];
  /** Editor ids that were closed because their unedited request was deleted on the server. */
  closed: number[];
}

/**
 * Brings the open editors of a server workspace up to date with a freshly loaded view.
 * Unedited editors follow the server; edited editors keep their content and are flagged
 * so that saving asks before overwriting the newer server version.
 */
export function mergeRefreshedView(drafts: Draft[], view: ProjectView): RefreshOutcome {
  const located = new Map<
    string,
    {
      collectionId: string;
      request: ProjectView['collections'][number]['data']['requests'][number];
    }
  >();
  for (const collection of view.collections) {
    for (const request of collection.data.requests) {
      located.set(request.id, { collectionId: collection.data.id, request });
    }
  }
  const outcome: RefreshOutcome = { drafts: [], changed: [], deleted: [], closed: [] };
  for (const draft of drafts) {
    // Editors of other projects and never-saved requests have no server version.
    if (draft.projectHandle !== view.handle || draft.savedSnapshot === null) {
      outcome.drafts.push(draft);
      continue;
    }
    const known = draft.serverSnapshot ?? draft.savedSnapshot;
    const found = located.get(draft.requestId);
    if (!found) {
      if (!isDirty(draft)) {
        outcome.closed.push(draft.id);
        continue;
      }
      if (!draft.serverDeleted) {
        outcome.deleted.push(draft.name);
      }
      outcome.drafts.push({ ...draft, serverDeleted: true, serverChanged: false });
      continue;
    }
    const current = fromRequest(draft.id, found.request, view.handle, found.collectionId);
    if (!isDirty(draft)) {
      outcome.drafts.push(current.savedSnapshot === draft.savedSnapshot ? draft : current);
      continue;
    }
    const moved = {
      collectionId: found.collectionId,
      folderId: found.request.folderId,
      serverDeleted: false,
    };
    if (current.savedSnapshot === known) {
      outcome.drafts.push({ ...draft, ...moved });
      continue;
    }
    if (!draft.serverChanged) {
      outcome.changed.push(draft.name);
    }
    outcome.drafts.push({
      ...draft,
      ...moved,
      serverChanged: true,
      serverSnapshot: current.savedSnapshot ?? undefined,
    });
  }
  return outcome;
}

/** A user-facing summary of what a refresh did to edited editors, or null when nothing. */
export function refreshNotice(outcome: RefreshOutcome): string | null {
  const parts: string[] = [];
  if (outcome.changed.length) {
    parts.push(
      `${quoted(outcome.changed)} ${outcome.changed.length === 1 ? 'was' : 'were'} changed on the server while you were editing. Saving will ask before overwriting the server version.`,
    );
  }
  if (outcome.deleted.length) {
    parts.push(
      `${quoted(outcome.deleted)} ${outcome.deleted.length === 1 ? 'was' : 'were'} deleted on the server. Saving recreates ${outcome.deleted.length === 1 ? 'it' : 'them'}.`,
    );
  }
  return parts.length ? parts.join(' ') : null;
}

function quoted(names: string[]) {
  return names.map((name) => `“${name}”`).join(', ');
}

/** Write-through rejections caused by concurrent server changes start with this text. */
export function isServerConflict(reason: unknown): reason is string {
  return typeof reason === 'string' && reason.startsWith('Not saved:');
}
