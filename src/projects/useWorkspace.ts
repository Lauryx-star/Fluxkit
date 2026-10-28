import { useCallback, useEffect, useEffectEvent, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
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

export function useWorkspace(
  additionalChanges = false,
  saveAdditionalChanges?: () => Promise<boolean>,
  additionalBusy = false,
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
  const startup = useRef<Promise<ProjectView | null> | null>(null);
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
    startup.current ??= native.restoreLastProject().then(async (view) => {
      if (!view?.readOnly || view.serverRecovery) {
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
    });
    void startup.current
      .then(
        (view) => {
          if (!disposed) {
            remembered.current = view?.handle ?? null;
            if (view) {
              setProjects([view]);
              setActiveHandle(view.handle);
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

  // Server workspaces publish each save automatically. A failed publish keeps the local save;
  // show why the server copy is behind.
  useEffect(() => {
    if (!desktop) {
      return;
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<{ handle: string; message: string }>('server-sync-failed', (event) => {
      setError(event.payload.message);
    })
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [desktop]);

  async function perform(operation: () => Promise<void>) {
    if (busy) {
      return;
    }
    setError(null);
    setBusy(true);
    try {
      await operation();
    } catch (reason: unknown) {
      setError(
        typeof reason === 'string' ? reason : 'The operation failed. Your edits are still open.',
      );
    } finally {
      setBusy(false);
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
  async function refreshServerProject(handle: string) {
    if (
      busy ||
      additionalBusy ||
      additionalChanges ||
      drafts.some((item) => item.projectHandle === handle && isDirty(item))
    ) {
      throw new Error(
        'Save or close unsaved editors before loading the server version. Your changes remain open.',
      );
    }
    setBusy(true);
    try {
      const result = await native.refreshServerProject(handle);
      setProjects((items) => [
        ...items.filter((item) => item.handle !== result.recovery.handle),
        result.recovery,
        result.workspace,
      ]);
      setActiveHandle(result.workspace.handle);
      setActiveId(null);
    } finally {
      setBusy(false);
    }
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
  function saveDraft() {
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
    const data = requestData(draft);
    void perform(async () => {
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
        items.map((item) =>
          item.id === draft.id
            ? {
                ...item,
                projectHandle: owner.handle,
                collectionId: collection.data.id,
                savedSnapshot: JSON.stringify(data),
              }
            : item,
        ),
      );
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

  async function finishQuit(choice: 'cancel' | 'discard' | 'save') {
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
    refreshServerProject,
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
