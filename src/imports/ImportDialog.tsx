import { useRef, useState } from 'react';
import { Modal } from '../projects/Dialogs';
import type { ImportPreview } from './types';

export function ImportDialog({
  preview,
  projectName,
  busy,
  started,
  error,
  onImport,
  onCancel,
  onRename,
}: {
  preview: ImportPreview;
  projectName: string;
  busy: boolean;
  started: boolean;
  error: string | null;
  onImport: (storeValues: boolean) => void;
  onCancel: () => void;
  onRename?: (oldName: string, name: string) => Promise<boolean>;
}) {
  const blockersRef = useRef<HTMLElement>(null);
  const [storeValues, setStoreValues] = useState(true);
  const [renaming, setRenaming] = useState<{ original: string; name: string } | null>(null);
  const [filter, setFilter] = useState('');
  const matchingVariables = preview.variables.filter((item) =>
    `${item.name} ${item.sourceName ?? ''}`.toLowerCase().includes(filter.trim().toLowerCase()),
  );
  const defaults = preview.variables.filter((v) => v.hasDefault).length;
  const missing = preview.variables.length - defaults;
  return (
    <Modal label={`Import ${preview.source}`}>
      <div
        className="import-preview"
        onKeyDown={(event) => {
          if (event.key === 'Escape' && !busy) {
            onCancel();
          }
        }}
      >
        <span className="eyebrow">
          {preview.source} {preview.environmentName ? 'environment' : preview.version}
        </span>
        <h2>Review your import</h2>
        <p>
          {preview.environmentName ? (
            <>
              A new project environment <strong>{preview.environmentName}</strong> will be added to{' '}
              <strong>{projectName}</strong>, using the variable mapping from{' '}
              <strong>{preview.collectionName}</strong>. The selected environment stays unchanged.
            </>
          ) : (
            <>
              A new collection <strong>{preview.collectionName}</strong> will be added to{' '}
              <strong>{projectName}</strong>. Existing collections and values stay in place.
            </>
          )}{' '}
          No HTTP requests are sent.
        </p>
        <div className="import-summary">
          <span>
            <strong>{preview.requestCount}</strong>{' '}
            {preview.requestCount === 1 ? 'request' : 'requests'}
          </span>
          <span>
            <strong>{preview.folderCount}</strong>{' '}
            {preview.folderCount === 1 ? 'folder' : 'folders'}
          </span>
          <span>
            <strong>{preview.variables.length}</strong> {preview.environmentName ? 'mapped' : 'new'}{' '}
            {preview.variables.length === 1 ? 'variable' : 'variables'}
          </span>
        </div>
        {!!preview.reviewCount && (
          <p role="status">
            {preview.reviewCount} imported{' '}
            {preview.reviewCount === 1 ? 'request needs' : 'requests need'} manual review and will
            be locked against sending until the review is confirmed and saved.
          </p>
        )}
        {preview.blockers.length > 0 && (
          <section
            ref={blockersRef}
            tabIndex={-1}
            aria-label="Blocking compatibility issues"
            role="alert"
          >
            <h3>Changes needed before import</h3>
            <ul>
              {preview.blockers.map((item, index) => (
                <li key={index}>{item}</li>
              ))}
            </ul>
          </section>
        )}
        {error && (
          <p className="import-error" role="alert">
            {error}
          </p>
        )}
        <section aria-label="Compatibility report">
          <h3>Compatibility report</h3>
          <ul>
            {preview.warnings.map((item, index) => (
              <li key={index}>{item}</li>
            ))}
          </ul>
        </section>
        <section aria-label="Imported variables">
          <h3>Variables and local values</h3>
          {preview.source === 'OpenAPI' && (
            <p>
              Only server URLs and authentication create project variables. Configure the server
              value once for every request using it. Query, header, path and form values are edited
              directly in each request. Missing required values are marked for review.
            </p>
          )}
          <p>
            {defaults} {defaults === 1 ? 'default/example is' : 'defaults/examples are'} available.{' '}
            {missing} {missing === 1 ? 'variable needs' : 'variables need'} a local value. Variable
            values stay outside project JSON.
          </p>
          <label className="import-values">
            <input
              type="checkbox"
              checked={storeValues}
              disabled={busy || started}
              onChange={(event) => setStoreValues(event.target.checked)}
            />
            Store available variable defaults/examples in the OS credential store
          </label>
          <p>
            {storeValues
              ? 'Only new import-owned values will be written.'
              : 'Definitions only: configure all variable values after importing.'}{' '}
            {!preview.environmentName &&
              (preview.source === 'OpenAPI'
                ? 'Parameter values and body examples are editable request content stored in the collection file. Review header/query values before sharing it.'
                : 'Raw body content/examples are editable request content stored in the collection file.')}
          </p>
          <details open={renaming !== null || undefined}>
            <summary>Show variable definitions</summary>
            <label className="import-variable-filter">
              Filter variables
              <input
                aria-label="Filter imported variables"
                value={filter}
                disabled={busy || renaming !== null}
                onChange={(event) => setFilter(event.target.value)}
                placeholder="Name or origin…"
              />
            </label>
            <ul>
              {matchingVariables.slice(0, 100).map((item) => (
                <li key={item.name}>
                  {item.sourceName && (
                    <>
                      <code>{item.sourceName}</code> →{' '}
                    </>
                  )}
                  <code>{item.name}</code> ·{' '}
                  {item.hasDefault ? 'default/example available' : 'value required'}
                  {preview.source === 'OpenAPI' && onRename && !started && (
                    <button
                      className="secondary-button"
                      disabled={busy || renaming !== null}
                      onClick={() => setRenaming({ original: item.name, name: item.name })}
                      aria-label={`Rename ${item.name} before import`}
                    >
                      Rename
                    </button>
                  )}
                  {renaming?.original === item.name && (
                    <form
                      className="import-variable-rename"
                      onSubmit={(event) => {
                        event.preventDefault();
                        if (onRename) {
                          void onRename(renaming.original, renaming.name).then((saved) => {
                            if (saved) {
                              setRenaming(null);
                            }
                          });
                        }
                      }}
                    >
                      <label>
                        Variable name
                        <input
                          aria-label="Imported variable name"
                          value={renaming.name}
                          disabled={busy}
                          required
                          maxLength={200}
                          pattern="[A-Za-z_][A-Za-z0-9_.-]*"
                          onChange={(event) =>
                            setRenaming({ ...renaming, name: event.target.value })
                          }
                        />
                      </label>
                      <p>
                        Updates every reference in this import. Existing project variables and
                        values stay unchanged.
                      </p>
                      <button
                        type="button"
                        className="secondary-button"
                        disabled={busy}
                        onClick={() => setRenaming(null)}
                      >
                        Cancel rename
                      </button>
                      <button
                        type="submit"
                        className="primary-button"
                        disabled={busy || !renaming.name}
                      >
                        Apply name
                      </button>
                    </form>
                  )}
                </li>
              ))}
            </ul>
            {!matchingVariables.length && <p>No matching variables.</p>}
            {matchingVariables.length > 100 && (
              <p>
                Showing 100 of {matchingVariables.length} matching variables. Filter by name or
                origin to find others.
              </p>
            )}
          </details>
        </section>
        <details open>
          <summary>Requests</summary>
          <div className="import-requests">
            {preview.requests.map((request, index) => (
              <div key={index}>
                <span className={`method-label method-${request.method.toLowerCase()}`}>
                  {request.method}
                </span>
                <span>
                  {request.name}
                  {request.needsReview ? ' · Review required' : ''}
                </span>
                <code>{request.url}</code>
              </div>
            ))}
          </div>
          {preview.requestCount > preview.requests.length && (
            <p>
              Showing the first {preview.requests.length} of {preview.requestCount} requests. All
              requests will be imported.
            </p>
          )}
        </details>
        <p>
          Keep this preview open to retry a partial import. If saving fails, any added
          variable/environment definitions and saved OS values remain visible in Project variables.
        </p>
        {preview.blockers.length > 0 && (
          <p id="import-blocked-reason" role="status">
            Import is blocked by {preview.blockers.length} compatibility{' '}
            {preview.blockers.length === 1 ? 'issue' : 'issues'}.{' '}
            <button
              className="secondary-button"
              disabled={busy}
              onClick={() => {
                blockersRef.current?.focus();
                blockersRef.current?.scrollIntoView?.({ block: 'nearest' });
              }}
            >
              Review import blockers
            </button>
          </p>
        )}
        {renaming && (
          <p id="import-rename-pending" role="status">
            Apply or cancel the variable rename before importing.
          </p>
        )}
        <div className="dialog-actions">
          <button className="secondary-button" disabled={busy} onClick={onCancel}>
            {started ? 'Close preview' : 'Cancel'}
          </button>
          <button
            className="primary-button"
            disabled={busy || preview.blockers.length > 0 || renaming !== null}
            aria-describedby={
              [
                preview.blockers.length ? 'import-blocked-reason' : '',
                renaming ? 'import-rename-pending' : '',
              ]
                .filter(Boolean)
                .join(' ') || undefined
            }
            onClick={() => onImport(storeValues)}
          >
            {busy
              ? 'Importing…'
              : started
                ? 'Retry import'
                : preview.environmentName
                  ? 'Import environment'
                  : 'Import collection'}
          </button>
        </div>
      </div>
    </Modal>
  );
}
