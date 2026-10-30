import { fromRequest, requestData, type Draft, type ProjectView, type SavedRequest } from './types';
import type { SessionInput } from '../native';

/** The share of the request/response area taken by the request; the response gets a quarter. */
export const DEFAULT_REQUEST_PANE_RATIO = 0.75;

/** What the next start should reopen: projects, tabs in order and the layout. */
export function sessionInput(
  projects: ProjectView[],
  activeHandle: string | null,
  drafts: Draft[],
  activeId: number | null,
  requestPaneRatio: number | null,
  treeExpanded: string[] | null = null,
): SessionInput {
  const index = drafts.findIndex((item) => item.id === activeId);
  return {
    projects: projects.map((item) => item.handle),
    activeProject: activeHandle,
    tabs: drafts.map((draft) => ({ project: draft.projectHandle, draft })),
    activeTab: index < 0 ? null : index,
    requestPaneRatio,
    treeExpanded,
  };
}

/** Collection and folder ids of the open projects, in tree order. */
export function treeIds(projects: ProjectView[]) {
  return projects.flatMap((project) =>
    project.collections.flatMap((collection) => [
      collection.data.id,
      ...collection.data.folders.map((folder) => folder.id),
    ]),
  );
}

/** Expands branches (for example ones the user just created) and keeps them most recent last. */
export function withExpanded(expanded: string[] | null, ids: string[]) {
  return expanded === null ? null : [...expanded.filter((item) => !ids.includes(item)), ...ids];
}

/**
 * Expands or collapses one branch. Without a remembered state every branch is expanded, so the
 * first toggle starts from all ids of the open projects. Expanded ids are kept most recent last;
 * ids of branches that are not open right now are kept for when their project opens again.
 */
export function toggleExpanded(expanded: string[] | null, id: string, projects: ProjectView[]) {
  const current = expanded ?? treeIds(projects);
  return current.includes(id) ? current.filter((item) => item !== id) : [...current, id];
}

/** Tabs as the user would find them after discarding unsaved edits. */
export function withoutEdits(drafts: Draft[]): Draft[] {
  return drafts.flatMap((draft) => {
    if (draft.savedSnapshot === null) {
      return [];
    }
    if (draft.savedSnapshot === JSON.stringify(requestData(draft))) {
      return [draft];
    }
    const saved = JSON.parse(draft.savedSnapshot) as SavedRequest;
    return [
      {
        ...fromRequest(draft.id, saved, draft.projectHandle ?? '', draft.collectionId ?? ''),
        projectHandle: draft.projectHandle,
        collectionId: draft.collectionId,
      },
    ];
  });
}

/** Tabs after a successful save of all of them. */
export function asSaved(drafts: Draft[]): Draft[] {
  return drafts.map((draft) => ({
    ...draft,
    savedSnapshot: JSON.stringify(requestData(draft)),
    serverChanged: false,
    serverDeleted: false,
    serverSnapshot: undefined,
  }));
}

function isDraft(value: unknown): value is Draft {
  if (!value || typeof value !== 'object') {
    return false;
  }
  const draft = value as Record<string, unknown>;
  return (
    typeof draft.requestId === 'string' &&
    typeof draft.name === 'string' &&
    typeof draft.method === 'string' &&
    typeof draft.url === 'string' &&
    typeof draft.body === 'string' &&
    typeof draft.bodyFormat === 'string' &&
    Array.isArray(draft.params) &&
    Array.isArray(draft.headers) &&
    Array.isArray(draft.fields) &&
    (draft.savedSnapshot === null || typeof draft.savedSnapshot === 'string')
  );
}

/**
 * Rebuilds tabs from a restored session with fresh editor ids. `handles` maps the handles the
 * session was restored with to the handles now open (server projects reopen online); tabs whose
 * project is no longer open keep their content without a project.
 */
export function restoredDrafts(
  tabs: { project: string | null; draft: unknown }[],
  handles: Map<string, string>,
): { drafts: Draft[]; ids: (number | null)[] } {
  const drafts: Draft[] = [];
  const ids: (number | null)[] = [];
  for (const tab of tabs) {
    if (!isDraft(tab.draft)) {
      ids.push(null);
      continue;
    }
    ids.push(drafts.length + 1);
    const handle = tab.project ? (handles.get(tab.project) ?? null) : null;
    drafts.push({
      ...tab.draft,
      id: drafts.length + 1,
      projectHandle: handle,
      ...(handle ? {} : { collectionId: null, folderId: null }),
    });
  }
  return { drafts, ids };
}
