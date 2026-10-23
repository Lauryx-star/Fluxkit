import { Select } from '../ui/Select';
import { useState } from 'react';
import { Modal } from '../projects/Dialogs';
import type { ProjectView } from '../projects/types';
import type { ExportPreview, ExportTarget } from './types';

export function ExportDialog({
  project,
  dirty,
  preview,
  busy,
  error,
  onPreview,
  onSave,
  onClose,
}: {
  project: ProjectView;
  dirty: boolean;
  preview: ExportPreview | null;
  busy: boolean;
  error: string | null;
  onPreview: (target: ExportTarget) => void;
  onSave: () => void;
  onClose: () => void;
}) {
  const [scope, setScope] = useState<ExportTarget['kind']>('project');
  const [collectionId, setCollectionId] = useState(project.collections[0]?.data.id ?? '');
  const collection = project.collections.find((c) => c.data.id === collectionId)?.data;
  const [entity, setEntity] = useState('');
  const entities = scope === 'folder' ? collection?.folders : collection?.requests;
  const selectedEntity = entities?.some((e) => e.id === entity)
    ? entity
    : (entities?.[0]?.id ?? '');
  const canPreview =
    scope === 'project' || (!!collection && (scope === 'collection' || !!selectedEntity));
  function target(): ExportTarget | null {
    if (scope === 'project') {
      return { kind: 'project' };
    }
    if (!collection) {
      return null;
    }
    if (scope === 'collection') {
      return { kind: 'collection', collection: collection.id };
    }
    if (!selectedEntity) {
      return null;
    }
    return scope === 'folder'
      ? { kind: 'folder', collection: collection.id, folder: selectedEntity }
      : { kind: 'request', collection: collection.id, request: selectedEntity };
  }
  return (
    <Modal label="Export OpenAPI">
      <div
        className="import-preview"
        onKeyDown={(e) => {
          if (e.key === 'Escape' && !busy) {
            onClose();
          }
        }}
      >
        <span className="eyebrow">OPENAPI 3.1.0 · YAML</span>
        <h2>{preview ? 'Review your export' : 'Export API description'}</h2>
        {dirty && (
          <p role="status">
            There are unsaved request changes in this project. This export uses the saved versions
            only. Close this dialog and save first to include your changes.
          </p>
        )}
        {!preview ? (
          <>
            <label className="dialog-label">
              Export scope
              <Select
                aria-label="Export scope"
                value={scope}
                disabled={busy}
                onValueChange={(selectedValue) => setScope(selectedValue as ExportTarget['kind'])}
              >
                <option value="project">Entire project — one YAML file per collection</option>
                <option value="collection">Collection</option>
                <option value="folder">Folder subtree</option>
                <option value="request">Request</option>
              </Select>
            </label>
            {scope !== 'project' && (
              <label className="dialog-label">
                Collection
                <Select
                  aria-label="Export collection"
                  value={collectionId}
                  disabled={busy}
                  onValueChange={(selectedValue) => {
                    setCollectionId(selectedValue);
                    setEntity('');
                  }}
                >
                  {project.collections.map(({ data: c }) => (
                    <option key={c.id} value={c.id}>
                      {c.name}
                    </option>
                  ))}
                </Select>
              </label>
            )}
            {(scope === 'folder' || scope === 'request') && (
              <label className="dialog-label">
                {scope === 'folder' ? 'Folder subtree' : 'Request'}
                <Select
                  aria-label="Export entity"
                  value={selectedEntity}
                  disabled={busy}
                  onValueChange={(selectedValue) => setEntity(selectedValue)}
                >
                  {!entities?.length && (
                    <option value="">No saved {scope === 'folder' ? 'folders' : 'requests'}</option>
                  )}
                  {entities?.map((e) => (
                    <option key={e.id} value={e.id}>
                      {e.name}
                    </option>
                  ))}
                </Select>
              </label>
            )}
            <p>
              Variable values and credentials are never read from the OS store. Raw body examples,
              header values, attachments and history are excluded. Review mappings and conflicts
              before saving.
            </p>
          </>
        ) : (
          <>
            <p>
              {preview.directory ? (
                <>
                  A new folder <code>{preview.folderName}</code> will contain one YAML document per
                  non-empty collection.
                </>
              ) : (
                'One YAML document will contain the selected saved operations.'
              )}{' '}
              Existing files are never overwritten. No project files are changed.
            </p>
            <section aria-label="Export files">
              <h3>Files</h3>
              <ul>
                {preview.files.slice(0, 200).map((file) => (
                  <li key={file.name}>
                    <code>{file.name}</code> · {file.operations} operations ·{' '}
                    {file.bytes.toLocaleString('en')} bytes
                  </li>
                ))}
              </ul>
              {preview.files.length > 200 && <p>Showing 200 of {preview.files.length} files.</p>}
            </section>
            <section aria-label="Export compatibility">
              <h3>Compatibility and privacy</h3>
              <ul>
                {preview.warnings.map((s, i) => (
                  <li key={i}>{s}</li>
                ))}
              </ul>
            </section>
            {!!preview.blockers.length && (
              <section role="alert" aria-label="Export blockers">
                <h3>Changes needed before export</h3>
                <ul>
                  {preview.blockers.map((s, i) => (
                    <li key={i}>{s}</li>
                  ))}
                </ul>
              </section>
            )}
          </>
        )}
        {error && (
          <p role="alert" className="import-error">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={busy} onClick={onClose}>
            Close
          </button>
          <button
            className="primary-button"
            disabled={busy || !!preview?.blockers.length || (!preview && !canPreview)}
            onClick={() => {
              if (preview) {
                onSave();
              } else {
                const selected = target();
                if (selected) {
                  onPreview(selected);
                }
              }
            }}
          >
            {busy
              ? 'Preparing…'
              : preview
                ? preview.directory
                  ? 'Choose export folder…'
                  : 'Save YAML…'
                : 'Preview export'}
          </button>
        </div>
      </div>
    </Modal>
  );
}
