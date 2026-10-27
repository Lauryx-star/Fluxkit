import { useEffect, useState } from 'react';
import {
  previewServerTransfer,
  prepareServerTransfer,
  commitServerTransfer,
  discardServerTransfer,
  type TransferPreview as Preview,
  type TransferSlot,
} from '../native';
import { Select } from '../ui/Select';

export function TransferPreview({
  handle,
  onDone,
  onCancel,
  onBusy,
  busy,
}: {
  handle: string;
  onDone: (name: string) => Promise<void>;
  onCancel: () => void;
  onBusy: (busy: boolean) => void;
  busy: boolean;
}) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [environment, setEnvironment] = useState('default');
  const [selected, setSelected] = useState<TransferSlot[]>([]);
  const [limit, setLimit] = useState(100);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    let previewId: string | null = null;
    void previewServerTransfer(handle).then(
      (preview) => {
        previewId = preview.id;
        if (active) {
          setPreview(preview);
        } else {
          void discardServerTransfer(preview.id);
        }
      },
      (error: unknown) => {
        if (active) {
          setError(typeof error === 'string' ? error : 'Could not preview transfer.');
        }
      },
    );
    return () => {
      active = false;
      if (previewId) {
        void discardServerTransfer(previewId);
      }
    };
  }, [handle]);
  async function prepare() {
    if (!preview) {
      return;
    }
    onBusy(true);
    setError(null);
    try {
      setPreview(await prepareServerTransfer(handle, preview.id, selected));
    } catch (error) {
      setError(typeof error === 'string' ? error : 'Could not check selected values.');
    } finally {
      onBusy(false);
    }
  }
  async function commit() {
    if (!preview?.ready) {
      return;
    }
    onBusy(true);
    setError(null);
    try {
      const saved = await commitServerTransfer(handle, preview.id);
      await onDone(saved.name);
    } catch (error) {
      setPreview({ ...preview, ready: false });
      setError(
        typeof error === 'string' ? error : 'Transfer failed. Your local files remain unchanged.',
      );
    } finally {
      onBusy(false);
    }
  }
  return (
    <section aria-label="Local project transfer preview">
      <h3>Review project transfer</h3>
      <p>
        This creates a new personal server copy. Local project files and OS values remain unchanged.
        Saved literal request fields, including URLs, headers and bodies, are uploaded as project
        data. Unsaved request edits, attachments, history and cached API tokens are excluded.
      </p>
      {error && <p role="alert">{error}</p>}
      {preview && (
        <>
          <p>
            {preview.name}: {preview.collections} collections, {preview.requests} saved requests.
          </p>
          <p>
            No values are selected automatically. Select only values you want to transfer. Ordinary
            values become shared project data; secrets are encrypted for your account and require an
            unlocked vault. Existing and legacy variables remain secrets by default.
          </p>
          <Select
            aria-label="Transfer value environment"
            value={environment}
            disabled={busy}
            onValueChange={(value) => {
              setEnvironment(value);
              setLimit(100);
            }}
          >
            <option value="default">Shared default slots</option>
            {preview.definitions.environments.map((environment) => (
              <option key={environment.id} value={environment.id}>
                {environment.name}
              </option>
            ))}
          </Select>
          <ul>
            {preview.definitions.variables.slice(0, limit).map((variable) => (
              <li key={variable.id}>
                <label>
                  <input
                    type="checkbox"
                    disabled={busy}
                    checked={selected.some(
                      (slot) =>
                        slot.variableId === variable.id && slot.environmentId === environment,
                    )}
                    onChange={(event) => {
                      const rest = selected.filter(
                        (slot) =>
                          slot.variableId !== variable.id || slot.environmentId !== environment,
                      );
                      setSelected(
                        event.target.checked
                          ? [...rest, { variableId: variable.id, environmentId: environment }]
                          : rest,
                      );
                      setPreview({ ...preview, ready: false });
                    }}
                  />{' '}
                  {variable.name} ·{' '}
                  {(variable.secret ?? true)
                    ? 'Secret (encrypted, only you)'
                    : 'Ordinary (shared with members)'}
                </label>
              </li>
            ))}
          </ul>
          {preview.definitions.variables.length > limit && (
            <button type="button" disabled={busy} onClick={() => setLimit(limit + 100)}>
              Show more variables
            </button>
          )}
          <p>
            {selected.length} selected value slots across environments. Checking reads only these OS
            entries.
          </p>
          <button
            type="button"
            className="secondary-button"
            disabled={busy || selected.length > 1000}
            onClick={() => void prepare()}
          >
            Check selected values and prepare
          </button>
          {preview.ready && (
            <p role="status">
              Ready: {preview.available} values found; {preview.missing.length} selected slots are
              missing and will remain unconfigured. Values were captured at preparation time.
            </p>
          )}
          {preview.ready && preview.missing.length > 0 && (
            <ul aria-label="Missing selected values">
              {preview.missing.slice(0, 100).map((slot) => (
                <li key={`${slot.variableId}/${slot.environmentId}`}>
                  {
                    preview.definitions.variables.find((entry) => entry.id === slot.variableId)
                      ?.name
                  }{' '}
                  ·{' '}
                  {slot.environmentId === 'default'
                    ? 'Default'
                    : preview.definitions.environments.find(
                        (entry) => entry.id === slot.environmentId,
                      )?.name}
                  : missing
                </li>
              ))}
            </ul>
          )}
          <button
            type="button"
            className="primary-button"
            disabled={busy || !preview.ready}
            onClick={() => void commit()}
          >
            Upload reviewed copy
          </button>{' '}
        </>
      )}
      <button type="button" className="secondary-button" disabled={busy} onClick={onCancel}>
        Cancel transfer
      </button>
    </section>
  );
}
