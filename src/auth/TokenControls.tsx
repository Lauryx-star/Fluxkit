import { useEffect, useRef, useState } from 'react';
import * as native from '../native';
import type { ExecutionInput } from '../http/types';
import type { TokenStatus } from './types';
import { ConfirmDialog } from '../projects/Dialogs';
import { tokenStatusLabel } from './rules';

interface Pending {
  id: string | null;
  cancelled: boolean;
}

/** Key this component by draft/authentication/environment so late replies cannot cross contexts. */
export function TokenControls({
  input,
  disabled = false,
}: {
  input: ExecutionInput;
  disabled?: boolean;
}) {
  const [status, setStatus] = useState<TokenStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [confirmForget, setConfirmForget] = useState(false);
  const pending = useRef<Pending | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      const operation = pending.current;
      if (operation) {
        operation.cancelled = true;
        if (operation.id) {
          void native.cancelHttpRequest(operation.id).catch(() => {});
        }
      }
    };
  }, []);
  async function run(action: 'status' | 'renew' | 'forget') {
    if (pending.current || disabled) {
      return;
    }
    const operation: Pending = { id: null, cancelled: false };
    pending.current = operation;
    setRunning(true);
    setError(null);
    try {
      operation.id = await native.beginHttpRequest();
      if (operation.cancelled) {
        throw 'Token operation cancelled.';
      }
      const result = await native.authenticationAction(operation.id, input, action);
      if (mounted.current && !operation.cancelled) {
        setStatus(result);
      }
    } catch (error) {
      if (mounted.current) {
        setError(
          operation.cancelled
            ? 'Token operation cancelled.'
            : typeof error === 'string'
              ? error
              : 'Could not complete the token operation. Retry explicitly.',
        );
      }
    } finally {
      if (operation.id) {
        void native.cancelHttpRequest(operation.id).catch(() => {});
      }
      if (pending.current === operation) {
        pending.current = null;
      }
      if (mounted.current) {
        setRunning(false);
      }
    }
  }
  return (
    <section className="token-controls" aria-label="Local authentication token">
      <p role="status">
        {running
          ? 'Working with local token…'
          : status
            ? tokenStatusLabel(status)
            : 'Local token status not checked'}
      </p>
      {status?.expiresAt !== null && status?.expiresAt !== undefined && (
        <p>Expires: {new Date(status.expiresAt * 1000).toISOString()}</p>
      )}
      <p className="auth-help">
        Status never reveals token values. These actions use the effective authentication shown
        above and the selected project environment. Renew uses saved ancestor settings and this
        request’s unsaved authentication.
      </p>
      <div className="token-actions">
        <button
          type="button"
          className="secondary-button"
          disabled={disabled || running}
          onClick={() => void run('status')}
        >
          Check token status
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={disabled || running}
          onClick={() => void run('renew')}
        >
          Acquire / renew token
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={disabled || running}
          onClick={() => setConfirmForget(true)}
        >
          Forget local token
        </button>
        {running && (
          <button
            type="button"
            className="secondary-button"
            onClick={() => {
              const operation = pending.current;
              if (operation) {
                operation.cancelled = true;
                if (operation.id) {
                  void native.cancelHttpRequest(operation.id).catch(() => {
                    setError('Could not confirm cancellation. Do not retry automatically.');
                  });
                }
              }
            }}
          >
            Cancel token operation
          </button>
        )}
      </div>
      {error && <p role="alert">{error}</p>}
      {confirmForget && (
        <ConfirmDialog
          message="Forget this configuration’s local token and renewal value in the selected environment? Project variables and other tokens stay available."
          onAnswer={(yes) => {
            setConfirmForget(false);
            if (yes) {
              void run('forget');
            }
          }}
        />
      )}
    </section>
  );
}
