import { useMemo, useState } from 'react';
import { Modal } from './Dialogs';
import { Select } from '../ui/Select';
import { effectiveAuth, authLabel } from '../auth/rules';
import { folderPath, requestLocation } from './location';
import { isDirty, type Draft, type ProjectView } from './types';

export function MoveRequestDialog({
  draft,
  project,
  initialCollectionId,
  initialFolderId,
  busy,
  error,
  onMove,
  onCancel,
}: {
  draft: Draft;
  project: ProjectView;
  initialCollectionId?: string;
  initialFolderId?: string | null;
  busy: boolean;
  error: string | null;
  onMove: (collectionId: string, folderId: string | null, allowMetadataLoss: boolean) => void;
  onCancel: () => void;
}) {
  const [collectionId, setCollectionId] = useState(
    initialCollectionId ?? draft.collectionId ?? project.collections[0]?.data.id ?? '',
  );
  const [folderId, setFolderId] = useState<string | null>(
    initialCollectionId ? (initialFolderId ?? null) : draft.folderId,
  );
  const [allowMetadataLoss, setAllowMetadataLoss] = useState(false);
  const destination = project.collections.find(
    (collection) => collection.data.id === collectionId,
  )?.data;
  const source = project.collections.find(
    (collection) => collection.data.id === draft.collectionId,
  )?.data;
  const metadataConflict = useMemo(
    () =>
      source?.id !== destination?.id &&
      !!source?.openApi?.operations?.[draft.requestId] &&
      !!destination?.openApi &&
      (source.openApi.version !== destination.openApi.version ||
        JSON.stringify(source.openApi.document) !== JSON.stringify(destination.openApi.document)),
    [source, destination, draft.requestId],
  );
  const auth = effectiveAuth(project, collectionId, folderId, draft.auth);
  const changed = collectionId !== draft.collectionId || folderId !== draft.folderId;
  const canMove =
    !busy &&
    !!destination &&
    (changed || isDirty(draft)) &&
    (!metadataConflict || allowMetadataLoss);
  return (
    <Modal label="Move request">
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (canMove) {
            onMove(collectionId, folderId, allowMetadataLoss);
          }
        }}
        onKeyDown={(event) => {
          if (event.key === 'Escape' && !busy) {
            onCancel();
          }
        }}
      >
        <h2>Move request</h2>
        <p>
          <strong>{draft.name}</strong>
          <br />
          Current location: {requestLocation(project, draft.collectionId, draft.folderId)}
        </p>
        <label className="dialog-label">
          Collection
          <Select
            aria-label="Destination collection"
            value={collectionId}
            disabled={busy}
            onValueChange={(id) => {
              setCollectionId(id);
              setFolderId(null);
              setAllowMetadataLoss(false);
            }}
          >
            {project.collections.map((collection) => (
              <option key={collection.data.id} value={collection.data.id}>
                {collection.data.name}
              </option>
            ))}
          </Select>
        </label>
        <label className="dialog-label">
          Folder
          <Select
            aria-label="Destination folder"
            value={folderId ?? ''}
            disabled={busy || !destination}
            onValueChange={(id) => setFolderId(id || null)}
          >
            <option value="">Collection root</option>
            {destination?.folders.map((folder) => (
              <option key={folder.id} value={folder.id}>
                {folderPath(destination, folder.id)}
              </option>
            ))}
          </Select>
        </label>
        <p>
          Destination: <strong>{requestLocation(project, collectionId, folderId)}</strong>
        </p>
        <p>
          Authentication after move: <strong>{authLabel(auth.auth)}</strong> · {auth.source}.
          Inherited authentication follows the destination. Explicit request authentication stays
          attached.
        </p>
        <p>
          Save &amp; move saves this request’s open changes immediately. Project variables, selected
          environment and attachments stay available in this project.
        </p>
        {metadataConflict && (
          <label className="token-checkbox">
            <input
              type="checkbox"
              checked={allowMetadataLoss}
              disabled={busy}
              onChange={(event) => setAllowMetadataLoss(event.target.checked)}
            />
            These collections use different OpenAPI source documents. Allow moving without
            transferring the original schema association. Executable request fields remain intact;
            the original source metadata stays in the source collection.
          </label>
        )}
        {error && <p role="alert">{error}</p>}
        <div className="dialog-actions">
          <button type="button" className="secondary-button" disabled={busy} onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" className="primary-button" disabled={!canMove}>
            {busy ? 'Saving & moving…' : 'Save & move'}
          </button>
        </div>
      </form>
    </Modal>
  );
}
