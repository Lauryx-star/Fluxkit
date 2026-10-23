import { useCallback, useEffect, useState } from 'react';
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

export function useWorkspace(additionalChanges = false) {
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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
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
    if (!desktop || !hasChanges) {
      return;
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        event.preventDefault();
        if (await ask('Close Fluxkit and discard unsaved changes?')) {
          await getCurrentWindow().destroy();
        }
      })
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
  }, [desktop, hasChanges, ask]);

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
    return project?.collections.find((item) => item.data.id === id);
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
      return;
    }
    setDrafts((items) => [...items, fromRequest(nextId, request, project.handle, collectionId)]);
    setActiveId(nextId);
    setNextId(nextId + 1);
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
    error,
    setError,
    confirmation,
    setConfirmation,
    nameDialog,
    setNameDialog,
    desktop,
    createProject,
    openProject,
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
    attach,
    select: (collectionId: string, folderId: string | null) => {
      if (project) {
        setSelection({ handle: project.handle, collectionId, folderId });
      }
    },
  };
}
