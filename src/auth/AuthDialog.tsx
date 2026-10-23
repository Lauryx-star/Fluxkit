import { useEffect, useState } from 'react';
import { Modal } from '../projects/Dialogs';
import type { Auth } from './types';
import type { Definition } from '../environments/types';
import { AuthEditor } from './AuthEditor';
import { AuthTest } from './AuthTest';
import type { ExecutionInput } from '../http/types';
import { validAuth } from './rules';
export function AuthDialog({
  title,
  testInput,
  inheritedAuth,
  initial,
  root,
  inherited,
  variables,
  busy,
  error,
  onSave,
  onCancel,
  onDirty,
  onRegisterSave,
}: {
  title: string;
  testInput?: ExecutionInput | null;
  inheritedAuth?: Auth | null;
  initial: Auth | null;
  root: boolean;
  inherited: string;
  variables: Definition[];
  busy: boolean;
  error: string | null;
  onSave: (auth: Auth | null) => Promise<boolean>;
  onCancel: (dirty: boolean) => void;
  onDirty: (dirty: boolean) => void;
  onRegisterSave?: (save: (() => Promise<boolean>) | null) => void;
}) {
  const [value, setValue] = useState(initial);
  const dirty = JSON.stringify(value) !== JSON.stringify(initial);
  useEffect(() => {
    onDirty(dirty);
    return () => onDirty(false);
  }, [dirty, onDirty]);
  useEffect(() => {
    onRegisterSave?.(
      async () => !dirty || (!busy && validAuth(value, variables) && (await onSave(value))),
    );
    return () => onRegisterSave?.(null);
  }, [onRegisterSave, dirty, busy, value, variables, onSave]);
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
        {testInput && (
          <AuthTest
            key={JSON.stringify([value, testInput.scope])}
            input={{ ...testInput, request: { ...testInput.request, auth: value } }}
            tokenOnly={['oauth2', 'customLogin'].includes((value ?? inheritedAuth)?.kind ?? '')}
            disabled={busy || !validAuth(value, variables)}
          />
        )}
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
