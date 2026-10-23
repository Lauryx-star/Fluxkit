import { useEffect, useRef, useState } from 'react';
import { historyAction } from '../native';
import { Modal } from '../projects/Dialogs';
import type { HistoryAction, HistoryView } from './types';

export function HistoryPanel({
  handle,
  name,
  refresh,
  ask,
  onClose,
}: {
  handle: string;
  name: string;
  refresh: unknown;
  ask: (message: string) => Promise<boolean>;
  onClose: () => void;
}) {
  const [view, setView] = useState<HistoryView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const mounted = useRef(true);
  const mutating = useRef(false);
  const sequence = useRef(0);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    let active = true;
    async function load() {
      if (mutating.current) {
        return;
      }
      const request = ++sequence.current;
      try {
        const next = await historyAction(handle, { kind: 'load' });
        if (active && sequence.current === request) {
          setView(next);
          setError(null);
        }
      } catch (error) {
        if (active && sequence.current === request) {
          setView(null);
          setError(typeof error === 'string' ? error : 'Could not load local request history.');
        }
      }
    }
    void load();
    const timer = window.setInterval(() => void load(), 60_000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [handle, refresh]);
  async function act(action: HistoryAction) {
    if (mutating.current) {
      return;
    }
    mutating.current = true;
    setBusy(true);
    try {
      if (
        action.kind === 'clear' &&
        !(await ask(
          'Clear this project’s local request history? This cannot be undone. If stored history is unreadable, recording will reset to off.',
        ))
      ) {
        return;
      }
      const request = ++sequence.current;
      const next = await historyAction(handle, action);
      if (mounted.current && sequence.current === request) {
        setView(next);
        setError(null);
      }
    } catch (error) {
      if (mounted.current) {
        setError(
          typeof error === 'string' ? error : 'History could not be changed. Retry explicitly.',
        );
      }
    } finally {
      mutating.current = false;
      if (mounted.current) {
        setBusy(false);
      }
    }
  }
  return (
    <Modal label={`Request history: ${name}`}>
      <section
        className="history-panel"
        onKeyDown={(event) => {
          if (event.key === 'Escape' && !busy) {
            onClose();
          }
        }}
      >
        <h2>Request history</h2>
        <p>{name} · At most 100 entries, retained for seven days.</p>
        <p className="history-help">
          Literal hosts are stored locally. Credentials, fragments, and literal path/query contents
          are removed; variables remain placeholders. Tokens, headers, bodies, and response payloads
          are never recorded.
        </p>
        <label className="history-recording">
          <input
            type="checkbox"
            checked={view?.recording ?? false}
            disabled={!view || busy}
            onChange={(event) => void act({ kind: 'setRecording', enabled: event.target.checked })}
          />
          Record request history for this project
        </label>
        {view && !view.recording && (
          <p className="history-help">
            Recording is off. Existing entries still expire and can be cleared. Session requests and
            background token calls are excluded.
          </p>
        )}
        {!view && !error && <p role="status">Loading local history…</p>}
        {error && <p role="alert">{error}</p>}
        {view?.warning && <p role="alert">{view.warning}</p>}
        {view && view.entries.length === 0 && <p>No recorded requests.</p>}
        {view && view.entries.length > 0 && (
          <div className="history-table-scroll">
            <table className="history-table" aria-label="Recorded requests">
              <thead>
                <tr>
                  <th>Method</th>
                  <th>URL</th>
                  <th>Status</th>
                  <th>Time</th>
                </tr>
              </thead>
              <tbody>
                {view.entries.map((entry, index) => (
                  <tr key={index}>
                    <td>{entry.method}</td>
                    <td>{entry.url}</td>
                    <td>{entry.status ?? <span aria-label="No HTTP status">—</span>}</td>
                    <td>
                      {entry.elapsedMs !== null ? (
                        `${entry.elapsedMs} ms`
                      ) : (
                        <span aria-label="No response time">—</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <div className="dialog-actions">
          <button
            type="button"
            className="secondary-button"
            disabled={busy || (!error && !view?.entries.length)}
            onClick={() => void act({ kind: 'clear' })}
          >
            Clear history
          </button>
          <button type="button" className="primary-button" disabled={busy} onClick={onClose}>
            Close history
          </button>
        </div>
      </section>
    </Modal>
  );
}
