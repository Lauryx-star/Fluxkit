import { useEffect, useState } from 'react';
import { serverVaultAction, type VaultStatus } from '../native';

export function VaultControls({
  busy,
  onBusy,
}: {
  busy: boolean;
  onBusy: (busy: boolean) => void;
}) {
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [changing, setChanging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    void serverVaultAction('status').then(
      (status) => {
        if (active) {
          setStatus(status);
        }
      },
      (error: unknown) => {
        if (active) {
          setError(typeof error === 'string' ? error : 'Could not load vault status.');
        }
      },
    );
    return () => {
      active = false;
    };
  }, []);
  async function perform(action: 'status' | 'create' | 'unlock' | 'lock' | 'changePassword') {
    if ((action === 'create' || action === 'changePassword') && password !== confirmation) {
      setError('The master passwords do not match.');
      return;
    }
    onBusy(true);
    setError(null);
    try {
      setStatus(
        await serverVaultAction(
          action,
          action === 'lock' || action === 'status' ? undefined : password,
        ),
      );
      setPassword('');
      setConfirmation('');
      setChanging(false);
    } catch (error) {
      setError(typeof error === 'string' ? error : 'Vault operation failed.');
    } finally {
      onBusy(false);
    }
  }
  return (
    <section
      aria-label="Account secret vault"
      onKeyDown={(event) => {
        if (event.key === 'Enter' && event.target instanceof HTMLInputElement) {
          event.preventDefault();
          if (!busy && status) {
            void perform(changing ? 'changePassword' : status.configured ? 'unlock' : 'create');
          }
        }
      }}
    >
      <h3>Account secret vault</h3>
      <p>
        One master password protects your own secrets across this server account’s projects. It is
        not your sign-in password. A forgotten master password cannot be recovered. Shared ordinary
        values are visible to project members.
      </p>
      <p role="status">
        {status
          ? status.configured
            ? status.unlocked
              ? 'Vault unlocked for this session.'
              : 'Vault locked.'
            : 'No account vault configured.'
          : 'Loading vault status…'}
      </p>
      {error && <p role="alert">{error}</p>}
      {status && (!status.unlocked || changing || !status.configured) && (
        <>
          <label className="dialog-label">
            {changing ? 'New master password' : 'Master password'}
            <input
              aria-label={changing ? 'New master password' : 'Master password'}
              type="password"
              autoComplete="off"
              value={password}
              disabled={busy}
              onChange={(event) => setPassword(event.target.value)}
            />
          </label>
          {(changing || !status.configured) && (
            <label className="dialog-label">
              Confirm master password
              <input
                aria-label="Confirm master password"
                type="password"
                autoComplete="off"
                value={confirmation}
                disabled={busy}
                onChange={(event) => setConfirmation(event.target.value)}
              />
            </label>
          )}
          <button
            type="button"
            className="secondary-button"
            disabled={busy || !password}
            onClick={() =>
              void perform(changing ? 'changePassword' : status.configured ? 'unlock' : 'create')
            }
          >
            {changing
              ? 'Save new master password'
              : status.configured
                ? 'Unlock vault'
                : 'Create vault'}
          </button>
        </>
      )}
      {status?.unlocked && (
        <>
          <button
            type="button"
            className="secondary-button"
            disabled={busy}
            onClick={() => void perform('lock')}
          >
            Lock vault
          </button>{' '}
          <button
            type="button"
            className="secondary-button"
            disabled={busy}
            onClick={() => {
              setChanging(!changing);
              setPassword('');
              setConfirmation('');
            }}
          >
            Change master password
          </button>
        </>
      )}
      <button
        type="button"
        className="secondary-button"
        disabled={busy}
        onClick={() => void perform('status')}
      >
        Refresh vault status
      </button>
      <p>
        Keys stay in native memory until you lock, close a server project, sign out or quit.
        Existing secrets are never replaced by a password reset.
      </p>
    </section>
  );
}
