import { useState } from 'react';
import { Icon } from '../components/Icon';
import type { CollectionView, Folder, SavedRequest } from './types';

interface Props {
  collections: CollectionView[];
  busy: boolean;
  projectHandle?: string;
  onDropRequest?: (
    handle: string,
    sourceId: string,
    requestId: string,
    collectionId: string,
    folderId: string | null,
  ) => void;
  onDropDraft?: (draftId: number, collectionId: string, folderId: string | null) => void;
  onAuth: (id: string, folder?: Folder) => void;
  onSelect: (id: string, folderId: string | null) => void;
  onNew: (id: string, folderId: string | null) => void;
  onFolder: (id: string, parentId: string | null) => void;
  onRename: (id: string, folder?: Folder) => void;
  onRemove: (id: string, folder?: Folder, request?: SavedRequest) => void;
  onOpen: (id: string, request: SavedRequest) => void;
}

export function ProjectTree(props: Props) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  function toggle(id: string) {
    setCollapsed((old) => {
      const next = new Set(old);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }
  function dropTarget(collectionId: string, folderId: string | null) {
    return {
      onDragOver: (event: React.DragEvent) => {
        if (
          !props.busy &&
          event.dataTransfer.types.some((type) =>
            ['application/x-fluxkit-draft', 'application/x-fluxkit-request'].includes(type),
          )
        ) {
          event.preventDefault();
          event.dataTransfer.dropEffect = 'move';
        }
      },
      onDrop: (event: React.DragEvent) => {
        event.preventDefault();
        event.stopPropagation();
        if (props.busy) {
          return;
        }
        if (event.dataTransfer.types.includes('application/x-fluxkit-request')) {
          try {
            const source: unknown = JSON.parse(
              event.dataTransfer.getData('application/x-fluxkit-request'),
            );
            if (
              source &&
              typeof source === 'object' &&
              'handle' in source &&
              'collectionId' in source &&
              'requestId' in source &&
              typeof source.handle === 'string' &&
              typeof source.collectionId === 'string' &&
              typeof source.requestId === 'string'
            ) {
              props.onDropRequest?.(
                source.handle,
                source.collectionId,
                source.requestId,
                collectionId,
                folderId,
              );
            }
          } catch {
            /* Ignore foreign or malformed drag data. */
          }
          return;
        }
        const id = Number(event.dataTransfer.getData('application/x-fluxkit-draft'));
        if (!props.busy && Number.isSafeInteger(id) && id > 0) {
          props.onDropDraft?.(id, collectionId, folderId);
        }
      },
    };
  }
  function children(collection: CollectionView, parent: string | null, depth: number) {
    if (depth > 32) {
      return null;
    }
    return (
      <>
        {collection.data.folders
          .filter((folder) => folder.parentId === parent)
          .map((folder) => (
            <div key={folder.id} className="tree-branch">
              <div className="tree-row" {...dropTarget(collection.data.id, folder.id)}>
                <button
                  className="tree-label"
                  aria-expanded={!collapsed.has(folder.id)}
                  onClick={() => {
                    toggle(folder.id);
                    props.onSelect(collection.data.id, folder.id);
                  }}
                >
                  <Icon name="folder" size={13} />
                  <span>{folder.name}</span>
                </button>
                <div className="tree-actions">
                  <button
                    disabled={props.busy}
                    className="auth-tree-button"
                    aria-label={`Authentication for folder ${folder.name}`}
                    onClick={() => props.onAuth(collection.data.id, folder)}
                  >
                    Auth
                  </button>
                  <button
                    disabled={props.busy}
                    aria-label={`New request in ${folder.name}`}
                    onClick={() => props.onNew(collection.data.id, folder.id)}
                  >
                    +
                  </button>
                  <button
                    disabled={props.busy}
                    aria-label={`Actions for ${folder.name}`}
                    onClick={() => props.onRename(collection.data.id, folder)}
                  >
                    ✎
                  </button>
                  <button
                    disabled={props.busy}
                    aria-label={`New subfolder in ${folder.name}`}
                    onClick={() => props.onFolder(collection.data.id, folder.id)}
                  >
                    <Icon name="folder" size={11} />
                  </button>
                  <button
                    disabled={props.busy}
                    aria-label={`Remove folder ${folder.name}`}
                    onClick={() => props.onRemove(collection.data.id, folder)}
                  >
                    ×
                  </button>
                </div>
              </div>
              {!collapsed.has(folder.id) && children(collection, folder.id, depth + 1)}
            </div>
          ))}
        {collection.data.requests
          .filter((request) => request.folderId === parent)
          .map((request) => (
            <div className="tree-row tree-request" key={request.id}>
              <button
                className="tree-label"
                aria-label={`${request.method} ${request.name}`}
                draggable={!props.busy && !!props.projectHandle}
                onDragStart={(event) => {
                  if (props.busy || !props.projectHandle) {
                    event.preventDefault();
                    return;
                  }
                  event.dataTransfer.setData(
                    'application/x-fluxkit-request',
                    JSON.stringify({
                      handle: props.projectHandle,
                      collectionId: collection.data.id,
                      requestId: request.id,
                    }),
                  );
                  event.dataTransfer.effectAllowed = 'move';
                }}
                onClick={() => props.onOpen(collection.data.id, request)}
              >
                <span className={`method-label method-${request.method.toLowerCase()}`}>
                  {request.method}
                </span>
                <span>{request.name}</span>
              </button>
              <div className="tree-actions">
                <button
                  disabled={props.busy}
                  aria-label={`Remove request ${request.name}`}
                  onClick={() => props.onRemove(collection.data.id, undefined, request)}
                >
                  ×
                </button>
              </div>
            </div>
          ))}
      </>
    );
  }
  return (
    <div className="project-tree" role="region" aria-label="Project collections">
      {props.collections.map((collection) => (
        <section className="collection-tree" key={collection.data.id}>
          <div className="tree-row collection-row" {...dropTarget(collection.data.id, null)}>
            <button
              className="tree-label"
              aria-expanded={!collapsed.has(collection.data.id)}
              onClick={() => {
                toggle(collection.data.id);
                props.onSelect(collection.data.id, null);
              }}
            >
              <Icon name="code" size={14} />
              <span>{collection.data.name}</span>
            </button>
            <div className="tree-actions">
              <button
                disabled={props.busy}
                className="auth-tree-button"
                aria-label={`Authentication for collection ${collection.data.name}`}
                onClick={() => props.onAuth(collection.data.id)}
              >
                Auth
              </button>
              <button
                disabled={props.busy}
                aria-label={`New request in ${collection.data.name}`}
                onClick={() => props.onNew(collection.data.id, null)}
              >
                +
              </button>
              <button
                disabled={props.busy}
                aria-label={`Rename collection ${collection.data.name}`}
                onClick={() => props.onRename(collection.data.id)}
              >
                ✎
              </button>
              <button
                disabled={props.busy}
                aria-label={`New folder in ${collection.data.name}`}
                onClick={() => props.onFolder(collection.data.id, null)}
              >
                <Icon name="folder" size={11} />
              </button>
              <button
                disabled={props.busy}
                aria-label={`Remove collection ${collection.data.name}`}
                onClick={() => props.onRemove(collection.data.id)}
              >
                ×
              </button>
            </div>
          </div>
          {!collapsed.has(collection.data.id) && children(collection, null, 0)}
        </section>
      ))}
    </div>
  );
}
