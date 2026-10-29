import { useState } from 'react';
import type { ExecutionResult } from './types';
import { tokenStatusLabel } from '../auth/rules';
import { MediaPreview } from './MediaPreview';

export function ResponseView({
  result,
  running,
  environmentLabel,
}: {
  result: ExecutionResult | null;
  running: boolean;
  environmentLabel?: string;
}) {
  type Tab = 'preview' | 'raw' | 'json' | 'headers';
  const defaultTab: Tab = result?.media ? 'preview' : result?.jsonText ? 'json' : 'raw';
  const [selection, setSelection] = useState<{ result: ExecutionResult | null; tab: Tab }>(() => ({
    result,
    tab: defaultTab,
  }));
  if (selection.result !== result) {
    setSelection({ result, tab: defaultTab });
  }
  const tab = selection.result === result ? selection.tab : defaultTab;
  const tabs: Tab[] = [...(result?.media ? (['preview'] as const) : []), 'raw', 'json', 'headers'];
  /** Tabs that can be selected; JSON stays visible but disabled without formatted JSON. */
  const enabled = tabs.filter((name) => name !== 'json' || Boolean(result?.jsonText));
  const active = enabled.includes(tab) ? tab : 'raw';
  function setTab(tab: Tab) {
    setSelection({ result, tab });
  }
  const status = result?.status;
  const statusClass =
    status !== null && status !== undefined
      ? status >= 200 && status < 300
        ? 'success'
        : status >= 300 && status < 400
          ? 'redirect'
          : status >= 400 && status < 600
            ? 'error'
            : 'neutral'
      : 'neutral';
  return (
    <section className="response-section" aria-label="Response">
      <div className="response-heading">
        <div className="response-title">
          <span>Response</span>
          {(environmentLabel || result?.tokenStatus) && (
            <details
              className="response-details"
              onKeyDown={(event) => {
                if (event.key === 'Escape') {
                  event.currentTarget.open = false;
                  event.currentTarget.querySelector('summary')?.focus();
                }
              }}
            >
              <summary>Details</summary>
              <div className="response-dispatch-details">
                {environmentLabel && (
                  <p>
                    Environment at dispatch: {environmentLabel}. Results belong to the last send of
                    this draft.
                  </p>
                )}
                {result?.tokenStatus && (
                  <p>
                    Authentication token at dispatch: {tokenStatusLabel(result.tokenStatus)}
                    {result.tokenStatus.expiresAt !== null
                      ? ` · expires ${new Date(result.tokenStatus.expiresAt * 1000).toISOString()}`
                      : ''}
                    .
                  </p>
                )}
              </div>
            </details>
          )}
        </div>
        <span className="response-meta">
          {status !== null && status !== undefined ? (
            <span className={`response-status response-status-${statusClass}`}>HTTP {status}</span>
          ) : (
            '—'
          )}
          <span className="response-divider">·</span>
          {result?.elapsedMs !== null && result?.elapsedMs !== undefined
            ? `${result.elapsedMs} ms`
            : '— ms'}
          <span className="response-divider">·</span>
          {result ? `${result.byteCount.toLocaleString('en-US')} B` : '— B'}
        </span>
      </div>
      {result?.historyWarning && (
        <p className="response-notice" role="status">
          {result.historyWarning}
        </p>
      )}
      {running && (
        <p className="response-notice" role="status">
          Preparing or sending request… Cancel remains available. Saving a response opens a
          destination chooser before sending.
        </p>
      )}
      {!running && !result && (
        <div className="response-empty">
          <strong>No response yet</strong>
          <p>Send a request to inspect its response.</p>
        </div>
      )}
      {result && (
        <>
          {result.message && (
            <p
              className={`response-notice ${result.outcome !== 'response' ? 'response-error' : ''}`}
              role={result.outcome !== 'response' ? 'alert' : 'status'}
            >
              {result.message}
            </p>
          )}
          {result.saved && (
            <p className="response-notice" role="status">
              Full response saved to the selected file.
            </p>
          )}
          {result.truncated && (
            <p className="response-notice">
              Preview is limited to 1 MiB or the exchange was interrupted. Byte count shows received
              body bytes. The preview does not contain the full response.
            </p>
          )}
          {result.status !== null && (
            <>
              <div className="response-tabs" role="tablist" aria-label="Response views">
                {tabs.map((name) => (
                  <button
                    key={name}
                    role="tab"
                    tabIndex={active === name ? 0 : -1}
                    onKeyDown={(event) => {
                      const names = enabled;
                      const index = names.indexOf(name);
                      const target =
                        event.key === 'ArrowRight'
                          ? names[(index + 1) % names.length]
                          : event.key === 'ArrowLeft'
                            ? names[(index + names.length - 1) % names.length]
                            : event.key === 'Home'
                              ? names[0]
                              : event.key === 'End'
                                ? names.at(-1)
                                : undefined;
                      if (target) {
                        event.preventDefault();
                        setTab(target);
                        document.getElementById(`response-tab-${target}`)?.focus();
                      }
                    }}
                    aria-selected={active === name}
                    aria-controls={`response-${name}`}
                    id={`response-tab-${name}`}
                    disabled={!enabled.includes(name)}
                    onClick={() => setTab(name)}
                  >
                    {name === 'preview'
                      ? 'Preview'
                      : name === 'raw'
                        ? 'Raw'
                        : name === 'json'
                          ? 'JSON'
                          : 'Headers'}
                  </button>
                ))}
              </div>
              <div
                className="response-content"
                role="tabpanel"
                id={`response-${active}`}
                aria-labelledby={`response-tab-${active}`}
              >
                {active === 'preview' && result.media ? (
                  <MediaPreview media={result.media} />
                ) : active === 'headers' ? (
                  <table>
                    <thead>
                      <tr>
                        <th>Header</th>
                        <th>Value</th>
                      </tr>
                    </thead>
                    <tbody>
                      {result.headers.map((header, index) => (
                        <tr key={index}>
                          <td>{header.name}</td>
                          <td>{header.value}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                ) : active === 'json' ? (
                  <pre>{result.jsonText}</pre>
                ) : result.text !== null ? (
                  <pre>{result.text}</pre>
                ) : result.contentKind === 'binary' && result.media ? (
                  <p>Binary {result.media.mime} content. The Preview tab shows it.</p>
                ) : result.contentKind === 'binary' ? (
                  <p>
                    Binary, compressed, or non-UTF-8 content. Use “Send & save response…” for a new
                    request that streams its response to a file.
                  </p>
                ) : result.byteCount === 0 && result.outcome === 'response' ? (
                  <p>Empty response body.</p>
                ) : (
                  <p>No response preview is available.</p>
                )}
              </div>
              {result.contentKind === 'json' && !result.jsonText && result.text !== null && (
                <p className="response-notice">
                  Formatted JSON is unavailable for invalid, truncated, deeply nested, or oversized
                  JSON. Raw remains available.
                </p>
              )}
            </>
          )}
        </>
      )}
    </section>
  );
}
