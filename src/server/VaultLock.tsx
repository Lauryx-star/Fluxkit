import { useCallback, useEffect, useState } from 'react';
import { Icon } from '../components/Icon';
import { serverVaultAction, type VaultStatus } from '../native';
import { Modal } from '../projects/Dialogs';

/**
 * Status-bar lock for the account vault: open lock = unlocked (click locks it), closed lock =
 * locked (click asks for the master password). `refreshKey` reloads the status after other
 * parts of the app (server dialog, account switch) may have changed it.
 */
export function VaultLock({ refreshKey, onSetup }: { refreshKey: number; onSetup: () => void }) {
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [dialog, setDialog] = useState(false);
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(async () => {
    try {
      setStatus(await serverVaultAction('status'));
    } catch {
      setStatus(null);
    }
  }, []);
  useEffect(() => {
    const immediate = setTimeout(() => void load(), 0);
    window.addEventListener('focus', load);
    return () => {
      clearTimeout(immediate);
      window.removeEventListener('focus', load);
    };
  }, [load, refreshKey]);
  function close() {
    if (!busy) {
      setDialog(false);
      setPassword('');
      setError(null);
    }
  }
  async function lock() {
    setBusy(true);
    try {
      setStatus(await serverVaultAction('lock'));
    } catch (reason) {
      setError(typeof reason === 'string' ? reason : 'Could not lock the vault.');
      setDialog(true);
    } finally {
      setBusy(false);
    }
  }
  async function unlock() {
    setBusy(true);
    setError(null);
    try {
      setStatus(await serverVaultAction('unlock', password));
      setPassword('');
      setDialog(false);
    } catch (reason) {
      setError(typeof reason === 'string' ? reason : 'Could not unlock the vault.');
    } finally {
      setBusy(false);
    }
  }
  if (!status) {
    return null;
  }
  const unlocked = status.configured && status.unlocked;
  return (
    <>
      <button
        type="button"
        className={`icon-button vault-trigger${unlocked ? ' vault-unlocked' : ''}`}
        aria-label={unlocked ? 'Vault unlocked. Lock vault' : 'Vault locked. Unlock vault'}
        title={unlocked ? 'Vault unlocked – click to lock' : 'Vault locked – click to unlock'}
        disabled={busy}
        onClick={() => (unlocked ? void lock() : setDialog(true))}
      >
        <Icon name={unlocked ? 'unlock' : 'lock'} size={13} />
      </button>
      {dialog && (
        <Modal label="Unlock vault" onDismiss={close}>
          {status.configured ? (
            <form
              onSubmit={(event) => {
                event.preventDefault();
                if (password && !busy) {
                  void unlock();
                }
              }}
            >
              <h2>Unlock vault</h2>
              <p>Enter your vault master password to use your secret values in this session.</p>
              <label className="dialog-label">
                Master password
                <input
                  aria-label="Master password"
                  type="password"
                  autoComplete="off"
                  value={password}
                  disabled={busy}
                  onChange={(event) => setPassword(event.target.value)}
                />
              </label>
              {error && <p role="alert">{error}</p>}
              <div className="dialog-actions">
                <button type="button" className="secondary-button" disabled={busy} onClick={close}>
                  Cancel
                </button>
                <button className="primary-button" type="submit" disabled={busy || !password}>
                  {busy ? 'Unlocking…' : 'Unlock'}
                </button>
              </div>
            </form>
          ) : (
            <>
              <h2>No vault yet</h2>
              <p>
                Set up your account vault with a master password in the server connection settings
                before storing secret values.
              </p>
              {error && <p role="alert">{error}</p>}
              <div className="dialog-actions">
                <button type="button" className="secondary-button" onClick={close}>
                  Cancel
                </button>
                <button
                  type="button"
                  className="primary-button"
                  onClick={() => {
                    close();
                    onSetup();
                  }}
                >
                  Open settings
                </button>
              </div>
            </>
          )}
        </Modal>
      )}
    </>
  );
}
