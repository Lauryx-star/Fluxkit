import { useCallback, useEffect, useEffectEvent, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import * as native from '../native';
import {
  emptyDraft,
  folderDescendants,
  fromRequest,
  isDirty,
  requestData,
  type Attachment,
  type Collection,
  type CollectionView,
  type Draft,
  type Folder,
  type ProjectView,
  type SavedRequest,
} from './types';
import { isServerConflict, mergeRefreshedView, refreshNotice } from './serverRefresh';
import { asSaved, restoredDrafts, sessionInput, withoutEdits } from './session';

/** How often open server workspaces pull other members' changes. */
export const SERVER_POLL_MS = 15_000;

interface Startup {
  projects: ProjectView[];
  active: string | null;
  drafts: Draft[];
  activeId: number | null;
  requestPaneRatio: number | null;
  treeExpanded: string[] | null;
  problems: string[];
}

/** Reopens a read-only server copy online; the copy is closed either way. */
async function online(view: ProjectView) {
  if (!view.readOnly || view.serverRecovery) {
    return view;
  }
  try {
    const refreshed = await native.openServerProject(view.data.id);
    if (refreshed.handle !== view.handle) {
      await native.closeProject(view.handle);
    }
    return refreshed;
  } catch (reason) {
    await native.closeProject(view.handle);
    throw reason;
  }
}

async function restore(): Promise<Startup> {
  const session = await native.restoreSession();
  if (!session) {
    // No session saved yet (first start after upgrading): the last active project only.
    const view = await native.restoreLastProject().then((view) => (view ? online(view) : null));
    return {
      projects: view ? [view] : [],
      active: view?.handle ?? null,
      drafts: [],
      activeId: null,
      requestPaneRatio: null,
      treeExpanded: null,
      problems: [],
    };
  }
  const projects: ProjectView[] = [];
  const handles = new Map<string, string>();
  const problems = session.unavailable.map(
    (directory) => `The project in ${directory} could not be reopened.`,
  );
  for (const view of session.projects) {
    try {
      const opened = await online(view);
      projects.push(opened);
      handles.set(view.handle, opened.handle);
    } catch (reason) {
      problems.push(
        typeof reason === 'string' ? reason : `“${view.data.name}” could not be reopened.`,
      );
    }
  }
  const restored = restoredDrafts(session.tabs, handles);
  // Unedited tabs follow the saved requests; edited ones are flagged if those changed meanwhile.
  let drafts = restored.drafts;
  for (const view of projects) {
    drafts = mergeRefreshedView(drafts, view).drafts;
  }
  const activeId = session.activeTab === null ? null : (restored.ids[session.activeTab] ?? null);
  return {
    projects,
    active:
      (session.activeProject && handles.get(session.activeProject)) ?? projects[0]?.handle ?? null,
    drafts,
    activeId: drafts.some((item) => item.id === activeId) ? activeId : (drafts.at(-1)?.id ?? null),
    requestPaneRatio: session.requestPaneRatio,
    treeExpanded: session.treeExpanded ?? null,
    problems: problems.length
      ? [...problems, 'Their open requests were kept without a project.']
      : [],
  };
}

export function useWorkspace(
  additionalChanges = false,
  saveAdditionalChanges?: () => Promise<boolean>,
  additionalBusy = false,
  requestPaneRatio: number | null = null,
  treeExpanded: string[] | null = null,
  /** Called with the collection/folder ids (and their parents) the user just created. */
  onBranchesCreated?: (ids: string[]) => void,
) {
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [nextId, setNextId] = useState(1);
  const [projects, setProjects] = useState<ProjectView[]>([]);
  const [activeHandle, setActiveHandle] = useState<string | null>(null);
  const [selection, setSelection] = useState<{
    handle: string;
    collectionId: string;
    folderId: string | null;
  } | null>(null);
  const [restoring, setRestoring] = useState(() => native.isDesktop());
  const [busy, setBusy] = useState(() => native.isDesktop());
  const startup = useRef<Promise<Startup> | null>(null);
  const [restoredRatio, setRestoredRatio] = useState<number | null>(null);
  /** Set once after the session was read: the remembered tree expansion (null = all). */
  const [restoredTree, setRestoredTree] = useState<{ expanded: string[] | null } | null>(null);
  // Set once the quit flow wrote the final session, so no pending autosave overwrites it.
  const sessionClosed = useRef(false);
  const remembered = useRef<string | null>(null);
  const rememberQueue = useRef<Promise<void>>(Promise.resolve());
  const [error, setError] = useState<string | null>(null);
  const [quitOpen, setQuitOpen] = useState(false);
  const [quitSaving, setQuitSaving] = useState(false);
  const [quitError, setQuitError] = useState<string | null>(null);
  const quitInFlight = useRef(false);
  // Auth saves following request saves must use the revisions just returned by native code.
  const quitCollections = useRef<Map<string, CollectionView> | null>(null);
  const [confirmation, setConfirmation] = useState<{
    message: string;
    resolve: (yes: boolean) => void;
  } | null>(null);
  const [conflict, setConflict] = useState<{ message: string; draftId: number } | null>(null);
  // Server refreshes never overlap other workspace operations, so views cannot arrive out of order.
  const busyRef = useRef(false);
  const refreshing = useRef<Promise<unknown>>(Promise.resolve());
  const [nameDialog, setNameDialog] = useState<{
    title: string;
    initial: string;
    submit: (name: string) => void;
  } | null>(null);
  const project = projects.find((item) => item.handle === activeHandle);
  const draft = drafts.find((item) => item.id === activeId);
  const desktop = native.isDesktop();
  const hasChanges = additionalChanges || drafts.some(isDirty);

  const requestWindowClose = useEffectEvent(async (event: { preventDefault: () => void }) => {
    event.preventDefault();
    if (quitInFlight.current) {
      return;
    }
    if (busy || additionalBusy) {
      setError('Wait for the current operation to finish before quitting Fluxkit.');
      return;
    }
    if (hasChanges) {
      setQuitOpen(true);
    } else {
      await finishQuit('discard');
    }
  });

  useEffect(() => {
    if (!desktop) {
      return;
    }
    let disposed = false;
    // StrictMode replays effects: reuse the same native restore instead of registering twice.
    startup.current ??= restore();
    void startup.current
      .then(
        (restored) => {
          if (!disposed) {
            remembered.current = restored.active;
            setProjects(restored.projects);
            setActiveHandle(restored.active);
            setDrafts(restored.drafts);
            setActiveId(restored.activeId);
            setNextId(restored.drafts.length + 1);
            setRestoredRatio(restored.requestPaneRatio);
            setRestoredTree({ expanded: restored.treeExpanded });
            if (restored.problems.length) {
              setError(restored.problems.join(' '));
            }
          }
        },
        (reason: unknown) => {
          if (!disposed) {
            setError(
              typeof reason === 'string'
                ? reason
                : 'The last project could not be reopened. Use Open project to choose it again.',
            );
          }
        },
      )
      .finally(() => {
        if (!disposed) {
          setRestoring(false);
          setBusy(false);
        }
      });
    return () => {
      disposed = true;
    };
  }, [desktop]);

  useEffect(() => {
    if (!desktop || restoring || remembered.current === activeHandle) {
      return;
    }
    remembered.current = activeHandle;
    let disposed = false;
    // Preserve activation order even if a user switches projects faster than disk writes finish.
    const pending = rememberQueue.current.then(() => native.rememberProject(activeHandle));
    rememberQueue.current = pending.catch(() => {});
    void pending.catch((reason: unknown) => {
      if (!disposed) {
        setError(
          typeof reason === 'string'
            ? reason
            : 'Could not remember the active project for the next app start. Your project remains open.',
        );
      }
    });
    return () => {
      disposed = true;
    };
  }, [desktop, restoring, activeHandle]);

  // Remember open projects, tabs (with unsaved edits) and the layout for the next start.
  useEffect(() => {
    if (!desktop || restoring || quitInFlight.current) {
      return;
    }
    const timer = window.setTimeout(() => {
      if (quitInFlight.current || sessionClosed.current) {
        return;
      }
      void native
        .saveSession(
          sessionInput(projects, activeHandle, drafts, activeId, requestPaneRatio, treeExpanded),
        )
        .catch(() => {});
    }, 400);
    return () => window.clearTimeout(timer);
  }, [
    desktop,
    restoring,
    projects,
    activeHandle,
    drafts,
    activeId,
    requestPaneRatio,
    treeExpanded,
  ]);

  const ask = useCallback(
    (message: string) =>
      new Promise<boolean>((resolve) => {
        setConfirmation((old) => {
          if (old) {
            old.resolve(false);
          }
          return { message, resolve };
        });
      }),
    [],
  );

  useEffect(() => {
    if (!hasChanges) {
      return;
    }
    function guard(event: BeforeUnloadEvent) {
      event.preventDefault();
      event.returnValue = '';
    }
    window.addEventListener('beforeunload', guard);
    return () => window.removeEventListener('beforeunload', guard);
  }, [hasChanges]);

  useEffect(() => {
    if (!desktop) {
      return;
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested((event) => requestWindowClose(event))
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch(() =>
        setError('Window-close protection is unavailable. Save your changes before closing.'),
      );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [desktop]);

  async function perform(operation: () => Promise<void>) {
    if (busy || busyRef.current) {
      return;
    }
    setError(null);
    setBusy(true);
    busyRef.current = true;
    const handle = activeHandle;
    let conflicted = false;
    try {
      await refreshing.current;
      await operation();
    } catch (reason: unknown) {
      conflicted = isServerConflict(reason);
      setError(
        typeof reason === 'string' ? reason : 'The operation failed. Your edits are still open.',
      );
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
    // A rejected write leaves the workspace at the server state it knew; load what changed.
    if (conflicted && handle && projects.find((item) => item.handle === handle)?.serverSync) {
      await refreshServerWorkspace(handle, true);
    }
  }

  function acceptProject(view: ProjectView) {
    setProjects((items) =>
      items.some((item) => item.handle === view.handle) ? items : [...items, view],
    );
    setActiveHandle(view.handle);
    if (draft?.projectHandle !== null) {
      setActiveId(drafts.filter((item) => item.projectHandle === view.handle).at(-1)?.id ?? null);
    }
  }
  function updateCollection(handle: string, view: CollectionView) {
    quitCollections.current?.set(JSON.stringify([handle, view.data.id]), view);
    setProjects((items) =>
      items.map((item) =>
        item.handle !== handle
          ? item
          : {
              ...item,
              collections: item.collections.some(
                (collection) => collection.data.id === view.data.id,
              )
                ? item.collections.map((collection) =>
                    collection.data.id === view.data.id ? view : collection,
                  )
                : [...item.collections, view],
            },
      ),
    );
  }
  function createProject() {
    setNameDialog({
      title: 'Create project',
      initial: '',
      submit: (name) => {
        void perform(async () => {
          const view = await native.chooseProject(name);
          if (view) {
            acceptProject(view);
          }
        });
      },
    });
  }
  function openProject() {
    void perform(async () => {
      const view = await native.chooseProject();
      if (view) {
        acceptProject(view);
      }
    });
  }
  async function openServerProject(projectId: string) {
    const view = await native.openServerProject(projectId);
    acceptProject(view);
  }
  /**
   * Loads other members' changes into an open server workspace. Unedited editors follow the
   * server; edited ones keep their content and are flagged. Returns the refreshed view.
   */
  function refreshServerWorkspace(handle: string, quiet = false) {
    const run = refreshing.current.then(async () => {
      const view = await native.refreshServerWorkspace(handle);
      setProjects((items) => items.map((item) => (item.handle === view.handle ? view : item)));
      const outcome = mergeRefreshedView(drafts, view);
      setDrafts((items) => mergeRefreshedView(items, view).drafts);
      if (outcome.closed.length) {
        setActiveId((id) => (id !== null && outcome.closed.includes(id) ? null : id));
      }
      const notice = refreshNotice(outcome);
      if (notice) {
        setError(notice);
      }
      return view;
    });
    refreshing.current = run.catch(() => {});
    return run.catch((reason: unknown) => {
      if (!quiet) {
        throw reason;
      }
      return null;
    });
  }
  const pollServer = useEffectEvent(() => {
    if (busyRef.current || quitInFlight.current || document.visibilityState === 'hidden') {
      return;
    }
    for (const item of projects) {
      if (item.serverSync) {
        void refreshServerWorkspace(item.handle, true);
      }
    }
  });
  const polling = projects.some((item) => item.serverSync);
  useEffect(() => {
    if (!desktop || !polling) {
      return;
    }
    const timer = window.setInterval(() => pollServer(), SERVER_POLL_MS);
    const focus = () => pollServer();
    window.addEventListener('focus', focus);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener('focus', focus);
    };
  }, [desktop, polling]);
  /** Deletes an empty server project after closing every open copy of it on this device. */
  async function deleteServerProject(projectId: string, revision: number) {
    const open = projects.filter((item) => item.data.id === projectId);
    const handles = new Set(open.map((item) => item.handle));
    if (
      drafts.some(
        (item) => item.projectHandle && handles.has(item.projectHandle) && isDirty(item),
      ) &&
      !(await ask('Close this project and discard its unsaved request changes?'))
    ) {
      return false;
    }
    for (const item of open) {
      await native.closeProject(item.handle);
    }
    if (open.length) {
      const remaining = projects.filter((item) => !handles.has(item.handle));
      setProjects(remaining);
      setActiveHandle((current) =>
        current && handles.has(current) ? (remaining[0]?.handle ?? null) : current,
      );
      setDrafts((items) =>
        items.filter((item) => !item.projectHandle || !handles.has(item.projectHandle)),
      );
      setActiveId(null);
    }
    await native.deleteServerProject(projectId, revision);
    return true;
  }
  function renameProject() {
    if (!project) {
      return;
    }
    setNameDialog({
      title: 'Rename project',
      initial: project.data.name,
      submit: (name) => {
        void perform(async () => {
          const view = await native.renameProject(project.handle, name, project.revision);
          setProjects((items) =>
            items.map((item) =>
              item.handle === project.handle
                ? { ...item, data: view.data, revision: view.revision }
                : item,
            ),
          );
        });
      },
    });
  }
  async function reloadProject() {
    if (!project) {
      return;
    }
    if (
      drafts.some((item) => item.projectHandle === project.handle && isDirty(item)) &&
      !(await ask('Reload this project and discard its unsaved request changes?'))
    ) {
      return;
    }
    void perform(async () => {
      const view = await native.reloadProject(project.handle);
      setProjects((items) => items.map((item) => (item.handle === view.handle ? view : item)));
      const remaining = drafts.filter((item) => item.projectHandle !== view.handle);
      setDrafts(remaining);
      setActiveId(null);
    });
  }
  async function closeProject() {
    if (!project) {
      return;
    }
    if (
      drafts.some((item) => item.projectHandle === project.handle && isDirty(item)) &&
      !(await ask(
        'Close this project and discard its unsaved request changes? Project files will remain on disk.',
      ))
    ) {
      return;
    }
    void perform(async () => {
      await native.closeProject(project.handle);
      const remaining = projects.filter((item) => item.handle !== project.handle);
      setProjects(remaining);
      setActiveHandle(remaining[0]?.handle ?? null);
      setDrafts((items) => items.filter((item) => item.projectHandle !== project.handle));
      setActiveId(null);
    });
  }
  async function saveAuthentication(
    target: import('../auth/types').AuthTarget,
    auth: import('../auth/types').Auth | null,
  ): Promise<boolean> {
    if (!project || busy) {
      return false;
    }
    let saved = false;
    await perform(async () => {
      if (!target.collectionId) {
        const view = await native.saveProjectAuth(project.handle, auth, project.revision);
        setProjects((items) =>
          items.map((item) =>
            item.handle === project.handle
              ? { ...item, data: view.data, revision: view.revision }
              : item,
          ),
        );
      } else {
        const collection = findCollection(target.collectionId);
        if (!collection) {
          throw new Error('Collection is no longer open.');
        }
        const data = target.folderId
          ? {
              ...collection.data,
              folders: collection.data.folders.map((folder) =>
                folder.id === target.folderId ? { ...folder, auth } : folder,
              ),
            }
          : { ...collection.data, auth };
        updateCollection(
          project.handle,
          await native.saveCollection(project.handle, data, collection.revision),
        );
      }
      saved = true;
    });
    return saved;
  }
  function createCollection() {
    if (!project) {
      return;
    }
    setNameDialog({
      title: 'Create collection',
      initial: '',
      submit: (name) => {
        void perform(async () => {
          const data: Collection = {
            schemaVersion: 1,
            id: crypto.randomUUID(),
            name,
            folders: [],
            requests: [],
          };
          const view = await native.saveCollection(project.handle, data, null);
          updateCollection(project.handle, view);
          onBranchesCreated?.([data.id]);
          setSelection({ handle: project.handle, collectionId: data.id, folderId: null });
        });
      },
    });
  }
  function findCollection(id: string) {
    return (
      quitCollections.current?.get(JSON.stringify([project?.handle, id])) ??
      project?.collections.find((item) => item.data.id === id)
    );
  }
  function createFolder(id: string, parentId: string | null) {
    const collection = findCollection(id);
    if (!project || !collection) {
      return;
    }
    setNameDialog({
      title: 'Create folder',
      initial: '',
      submit: (name) => {
        void perform(async () => {
          const folder = { id: crypto.randomUUID(), name, parentId };
          const view = await native.saveCollection(
            project.handle,
            { ...collection.data, folders: [...collection.data.folders, folder] },
            collection.revision,
          );
          updateCollection(project.handle, view);
          onBranchesCreated?.([id, ...(parentId ? [parentId] : []), folder.id]);
          setSelection({ handle: project.handle, collectionId: id, folderId: folder.id });
        });
      },
    });
  }
  function renameEntity(id: string, folder?: Folder) {
    const collection = findCollection(id);
    if (!project || !collection) {
      return;
    }
    setNameDialog({
      title: folder ? 'Rename folder' : 'Rename collection',
      initial: folder?.name ?? collection.data.name,
      submit: (name) => {
        void perform(async () => {
          const data = folder
            ? {
                ...collection.data,
                folders: collection.data.folders.map((item) =>
                  item.id === folder.id ? { ...item, name } : item,
                ),
              }
            : { ...collection.data, name };
          updateCollection(
            project.handle,
            await native.saveCollection(project.handle, data, collection.revision),
          );
        });
      },
    });
  }
  async function removeEntity(id: string, folder?: Folder, request?: SavedRequest) {
    const collection = findCollection(id);
    if (!project || !collection) {
      return;
    }
    const name = request?.name ?? folder?.name ?? collection.data.name;
    if (
      !(await ask(
        `Remove “${name}”${folder ? ' and everything in this folder' : ''}? Any open edits to removed requests will be discarded. A recovery copy stays in the project folder.`,
      ))
    ) {
      return;
    }
    void perform(async () => {
      const removedFolders = folder
        ? folderDescendants(collection.data, folder.id)
        : new Set<string>();
      const removedRequests = new Set(
        collection.data.requests
          .filter((item) =>
            request
              ? item.id === request.id
              : folder
                ? !!item.folderId && removedFolders.has(item.folderId)
                : true,
          )
          .map((item) => item.id),
      );
      if (!folder && !request) {
        await native.removeCollection(project.handle, id, collection.revision);
        setProjects((items) =>
          items.map((item) =>
            item.handle === project.handle
              ? { ...item, collections: item.collections.filter((c) => c.data.id !== id) }
              : item,
          ),
        );
      } else {
        const data = {
          ...collection.data,
          folders: collection.data.folders.filter((item) => !removedFolders.has(item.id)),
          requests: collection.data.requests.filter((item) => !removedRequests.has(item.id)),
        };
        updateCollection(
          project.handle,
          await native.saveCollection(project.handle, data, collection.revision),
        );
      }
      setDrafts((items) =>
        items.filter(
          (item) =>
            !(
              item.projectHandle === project.handle &&
              item.collectionId === id &&
              (removedRequests.has(item.requestId) ||
                (!folder && !request) ||
                (!!item.folderId && removedFolders.has(item.folderId)))
            ),
        ),
      );
      setSelection(null);
    });
  }
  function addDraft(collectionId?: string, folderId: string | null = null) {
    const targetId =
      collectionId ??
      (selection?.handle === project?.handle ? selection?.collectionId : null) ??
      project?.collections[0]?.data.id ??
      null;
    const targetFolder = collectionId
      ? folderId
      : selection?.handle === project?.handle && selection?.collectionId === targetId
        ? (selection?.folderId ?? null)
        : null;
    setDrafts((items) => [
      ...items,
      emptyDraft(nextId, project?.handle ?? null, targetId, targetFolder),
    ]);
    setActiveId(nextId);
    setNextId(nextId + 1);
  }
  function openRequest(collectionId: string, request: SavedRequest) {
    if (!project) {
      return;
    }
    const existing = drafts.find(
      (item) =>
        item.projectHandle === project.handle &&
        item.collectionId === collectionId &&
        item.requestId === request.id,
    );
    if (existing) {
      setActiveId(existing.id);
      return existing.id;
    }
    setDrafts((items) => [...items, fromRequest(nextId, request, project.handle, collectionId)]);
    setActiveId(nextId);
    setNextId(nextId + 1);
    return nextId;
  }
  function updateDraft(patch: Partial<Draft>) {
    setDrafts((items) =>
      items.map((item) => (item.id === activeId ? { ...item, ...patch } : item)),
    );
  }
  async function closeDraft(id: number) {
    const item = drafts.find((item) => item.id === id);
    if (item && isDirty(item) && !(await ask('Discard unsaved changes to this request?'))) {
      return;
    }
    const remaining = drafts.filter((item) => item.id !== id);
    setDrafts(remaining);
    if (activeId === id) {
      const selected = remaining.at(-1);
      setActiveId(selected?.id ?? null);
      if (selected?.projectHandle) {
        setActiveHandle(selected.projectHandle);
      }
    }
  }
  function assignDraft(id: number, collectionId: string, folderId: string | null = null) {
    const item = drafts.find((entry) => entry.id === id);
    const collection = project?.collections.find((entry) => entry.data.id === collectionId);
    if (busy || !item || item.savedSnapshot || item.collectionId || !project || !collection) {
      setError('Only unsaved requests without a collection can be assigned here.');
      return;
    }
    if (item.projectHandle && item.projectHandle !== project.handle) {
      setError('Choose a collection in this request’s project to keep variable references valid.');
      return;
    }
    if (folderId && !collection.data.folders.some((folder) => folder.id === folderId)) {
      setError('This folder no longer exists.');
      return;
    }
    setDrafts((items) =>
      items.map((entry) =>
        entry.id === id
          ? {
              ...entry,
              projectHandle: project.handle,
              collectionId,
              folderId,
            }
          : entry,
      ),
    );
    setActiveId(id);
    setSelection({ handle: project.handle, collectionId, folderId });
    setError(null);
  }
  async function moveDraft(
    id: number,
    collectionId: string,
    folderId: string | null,
    allowMetadataLoss: boolean,
  ) {
    const item = drafts.find((entry) => entry.id === id);
    const owner = projects.find((entry) => entry.handle === (item?.projectHandle ?? activeHandle));
    const destination = owner?.collections.find((entry) => entry.data.id === collectionId);
    const source = item?.savedSnapshot
      ? owner?.collections.find((entry) => entry.data.id === item.collectionId)
      : undefined;
    if (!item || !owner || !destination || (item.savedSnapshot && !source) || busy) {
      setError('Choose an existing collection in this request’s project before moving.');
      return false;
    }
    let saved = false;
    await perform(async () => {
      const request = { ...requestData(item), folderId };
      const views = await native.moveRequest(owner.handle, {
        request,
        sourceCollectionId: source?.data.id ?? null,
        sourceRevision: source?.revision ?? null,
        targetCollectionId: destination.data.id,
        targetRevision: destination.revision,
        projectRevision: owner.revision,
        allowMetadataLoss,
      });
      for (const view of views) {
        updateCollection(owner.handle, view);
      }
      setDrafts((items) =>
        items.map((entry) =>
          entry.id === id
            ? {
                ...entry,
                projectHandle: owner.handle,
                collectionId,
                folderId,
                savedSnapshot: JSON.stringify(request),
              }
            : entry,
        ),
      );
      setActiveId(id);
      setActiveHandle(owner.handle);
      setSelection({ handle: owner.handle, collectionId, folderId });
      saved = true;
    });
    return saved;
  }
  function target(item: Draft) {
    const owner = projects.find((p) => p.handle === (item.projectHandle ?? activeHandle));
    const collection =
      owner?.collections.find((c) => c.data.id === item.collectionId) ??
      (item.collectionId
        ? undefined
        : (owner?.collections.find((c) => c.data.id === selection?.collectionId) ??
          owner?.collections[0]));
    if (!owner || !collection) {
      throw new Error('Open a project and create a collection before saving or attaching files.');
    }
    return { owner, collection };
  }
  /** Writes one editor into its collection and marks it saved. */
  async function writeDraft(item: Draft, owner: ProjectView, collection: CollectionView) {
    const data = requestData(item);
    const exists = collection.data.requests.some((r) => r.id === data.id);
    const requests = exists
      ? collection.data.requests.map((r) => (r.id === data.id ? data : r))
      : [...collection.data.requests, data];
    const view = await native.saveCollection(
      owner.handle,
      { ...collection.data, requests },
      collection.revision,
    );
    updateCollection(owner.handle, view);
    setDrafts((items) =>
      items.map((entry) =>
        entry.id === item.id
          ? {
              ...entry,
              projectHandle: owner.handle,
              collectionId: collection.data.id,
              savedSnapshot: JSON.stringify(data),
              serverChanged: false,
              serverDeleted: false,
              serverSnapshot: undefined,
            }
          : entry,
      ),
    );
  }
  async function saveDraft() {
    if (!draft || busy) {
      return;
    }
    let destination: ReturnType<typeof target>;
    try {
      destination = target(draft);
    } catch {
      setError('Open a project and create a collection before saving.');
      return;
    }
    const { owner, collection } = destination;
    if (
      (draft.serverChanged &&
        !(await ask(
          `“${draft.name}” was changed elsewhere while you were editing it. Overwrite the saved version with yours?`,
        ))) ||
      (draft.serverDeleted &&
        !(await ask(`“${draft.name}” was deleted on the server. Save it again to recreate it?`)))
    ) {
      return;
    }
    const item = draft;
    void perform(async () => {
      try {
        await writeDraft(item, owner, collection);
      } catch (reason: unknown) {
        if (owner.serverSync && isServerConflict(reason)) {
          setConflict({ message: reason, draftId: item.id });
        }
        throw reason;
      }
    });
  }
  /**
   * Resolves a rejected save: either discard the edits in favour of the server version, or
   * load the server version underneath the edits and save them on top of it.
   */
  function resolveConflict(choice: 'server' | 'mine' | 'cancel') {
    const pending = conflict;
    setConflict(null);
    const item = drafts.find((entry) => entry.id === pending?.draftId);
    if (!pending || !item?.projectHandle || choice === 'cancel') {
      return;
    }
    const handle = item.projectHandle;
    void perform(async () => {
      const view = await native.refreshServerWorkspace(handle);
      setProjects((items) => items.map((entry) => (entry.handle === view.handle ? view : entry)));
      const located = view.collections.find((entry) =>
        entry.data.requests.some((request) => request.id === item.requestId),
      );
      if (choice === 'server') {
        const request = located?.data.requests.find((entry) => entry.id === item.requestId);
        setDrafts((items) =>
          request && located
            ? items.map((entry) =>
                entry.id === item.id
                  ? fromRequest(item.id, request, view.handle, located.data.id)
                  : entry,
              )
            : items.filter((entry) => entry.id !== item.id),
        );
        if (!request) {
          setActiveId((id) => (id === item.id ? null : id));
        }
        return;
      }
      const collection =
        located ?? view.collections.find((entry) => entry.data.id === item.collectionId);
      if (!collection) {
        throw new Error(
          'The collection of this request was deleted on the server. Move the request to another collection to keep it.',
        );
      }
      // Keep the server's placement; a request whose folder is gone lands in the collection root.
      const placed = located?.data.requests.find((entry) => entry.id === item.requestId);
      const folderId =
        placed?.folderId ??
        (collection.data.folders.some((folder) => folder.id === item.folderId)
          ? item.folderId
          : null);
      await writeDraft({ ...item, collectionId: collection.data.id, folderId }, view, collection);
    });
  }

  async function saveAllDrafts() {
    const groups = new Map<
      string,
      { handle: string; collection: CollectionView; drafts: Draft[] }
    >();
    // Resolve every destination before writing anything. Session drafts need an open collection.
    for (const item of drafts.filter(isDirty)) {
      const { owner, collection } = target(item);
      const key = JSON.stringify([owner.handle, collection.data.id]);
      const group = groups.get(key) ?? { handle: owner.handle, collection, drafts: [] };
      group.drafts.push(item);
      groups.set(key, group);
    }
    for (const { handle, collection, drafts: changed } of groups.values()) {
      const requests = new Map(collection.data.requests.map((request) => [request.id, request]));
      for (const item of changed) {
        requests.set(item.requestId, requestData(item));
      }
      const view = await native.saveCollection(
        handle,
        { ...collection.data, requests: [...requests.values()] },
        collection.revision,
      );
      updateCollection(handle, view);
      setDrafts((items) =>
        items.map((item) => {
          const saved = changed.find((entry) => entry.id === item.id);
          return saved
            ? {
                ...item,
                projectHandle: handle,
                collectionId: collection.data.id,
                savedSnapshot: JSON.stringify(requestData(saved)),
              }
            : item;
        }),
      );
    }
  }

  /**
   * Quitting: `save` saves every edit first, `keep` keeps unsaved edits for the next start and
   * `discard` reopens only the saved versions next time.
   */
  async function finishQuit(choice: 'cancel' | 'discard' | 'save' | 'keep') {
    if (quitInFlight.current) {
      return;
    }
    if (choice === 'cancel') {
      setQuitOpen(false);
      setQuitError(null);
      return;
    }
    quitInFlight.current = true;
    setQuitSaving(true);
    setQuitError(null);
    quitCollections.current = new Map();
    try {
      if (choice === 'save') {
        await saveAllDrafts();
        if (additionalChanges && (!saveAdditionalChanges || !(await saveAdditionalChanges()))) {
          throw new Error(
            'Could not save all changes. Check the open editor and complete any missing fields before quitting. Successfully saved changes are kept.',
          );
        }
      }
      await rememberQueue.current;
      const tabs =
        choice === 'save' ? asSaved(drafts) : choice === 'discard' ? withoutEdits(drafts) : drafts;
      const final = native.saveSession(
        sessionInput(projects, activeHandle, tabs, activeId, requestPaneRatio, treeExpanded),
      );
      sessionClosed.current = true;
      if (choice === 'keep') {
        // Quitting would lose the edits if they could not be written.
        await final.catch((reason: unknown) => {
          sessionClosed.current = false;
          throw reason;
        });
      } else {
        await final.catch(() => {});
      }
      await getCurrentWindow().destroy();
    } catch (reason: unknown) {
      setQuitOpen(true);
      setQuitError(
        typeof reason === 'string'
          ? reason
          : reason instanceof Error
            ? reason.message
            : 'Could not save all changes or quit Fluxkit. Your remaining edits are still open.',
      );
    } finally {
      quitCollections.current = null;
      quitInFlight.current = false;
      setQuitSaving(false);
    }
  }
  function attach(accept: (file: Attachment) => void) {
    if (!draft) {
      return;
    }
    let destination: ReturnType<typeof target>;
    try {
      destination = target(draft);
    } catch {
      setError('Open a project and create a collection before attaching files.');
      return;
    }
    const { owner, collection } = destination;
    void perform(async () => {
      const file = await native.chooseAttachment(owner.handle);
      if (file) {
        setDrafts((items) =>
          items.map((item) =>
            item.id === draft.id
              ? { ...item, projectHandle: owner.handle, collectionId: collection.data.id }
              : item,
          ),
        );
        accept(file);
      }
    });
  }
  return {
    ask,
    restoredRatio,
    restoredTree,
    conflict,
    resolveConflict,
    deleteServerProject,
    acceptImportedProject: (view: ProjectView) => {
      setProjects((items) => items.map((item) => (item.handle === view.handle ? view : item)));
    },
    drafts,
    activeId,
    setActiveId: (id: number | null) => {
      setActiveId(id);
      const selected = drafts.find((item) => item.id === id);
      if (selected?.projectHandle) {
        setActiveHandle(selected.projectHandle);
      }
    },
    draft,
    project,
    projects,
    activeHandle,
    setActiveHandle: (handle: string) => {
      setActiveHandle(handle);
      setActiveId(drafts.filter((item) => item.projectHandle === handle).at(-1)?.id ?? null);
    },
    busy,
    quitOpen,
    quitSaving,
    quitError,
    finishQuit,
    error,
    setError,
    confirmation,
    setConfirmation,
    nameDialog,
    setNameDialog,
    desktop,
    createProject,
    openProject,
    openServerProject,
    refreshServerWorkspace: async (handle: string) => {
      if (busyRef.current) {
        throw 'Wait for the current operation to finish, then refresh again.';
      }
      await refreshServerWorkspace(handle);
    },
    renameProject,
    reloadProject,
    closeProject,
    createCollection,
    saveAuthentication,
    createFolder,
    renameEntity,
    removeEntity,
    addDraft,
    openRequest,
    updateDraft,
    closeDraft,
    saveDraft,
    assignDraft,
    moveDraft,
    attach,
    select: (collectionId: string, folderId: string | null) => {
      if (project) {
        setSelection({ handle: project.handle, collectionId, folderId });
      }
    },
  };
}
