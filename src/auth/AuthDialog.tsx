import { useEffect, useState } from 'react';
import { Modal } from '../projects/Dialogs';
import type { Auth } from './types';
import type { Definition } from '../environments/types';
import { AuthEditor } from './AuthEditor';
import { validAuth } from './rules';
export function AuthDialog({
  title,
  initial,
  root,
  inherited,
  variables,
  busy,
  error,
  onSave,
  onCancel,
  onDirty,
}: {
  title: string;
  initial: Auth | null;
  root: boolean;
  inherited: string;
  variables: Definition[];
  busy: boolean;
  error: string | null;
  onSave: (auth: Auth | null) => void;
  onCancel: (dirty: boolean) => void;
  onDirty: (dirty: boolean) => void;
}) {
  const [value, setValue] = useState(initial);
  const dirty = JSON.stringify(value) !== JSON.stringify(initial);
  useEffect(() => {
    onDirty(dirty);
    return () => onDirty(false);
  }, [dirty, onDirty]);
  return (
    <Modal label={title}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (validAuth(value, variables)) {
            onSave(value);
          }
        }}
        onKeyDown={(event) => {
          if (event.key === 'Escape' && !busy) {
            onCancel(dirty);
          }
        }}
      >
        <h2>{title}</h2>
        <p>Save applies authentication settings to this scope. Unsaved request edits stay open.</p>
        {!root && <p className="auth-effective">Parent authentication: {inherited}</p>}
        <AuthEditor
          value={value}
          onChange={setValue}
          variables={variables}
          root={root}
          disabled={busy}
        />
        {error && <p role="alert">{error}</p>}
        <div className="dialog-actions">
          <button
            type="button"
            className="secondary-button"
            disabled={busy}
            onClick={() => onCancel(dirty)}
          >
            Cancel
          </button>
          <button
            type="submit"
            className="primary-button"
            disabled={busy || !dirty || !validAuth(value, variables)}
          >
            Save authentication
          </button>
        </div>
      </form>
    </Modal>
  );
}
