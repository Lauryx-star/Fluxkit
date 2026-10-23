import { useState } from 'react';
import type { ExecutionResult } from './types';

export function ResponseView({
  result,
  running,
  environmentLabel,
}: {
  result: ExecutionResult | null;
  running: boolean;
  environmentLabel?: string;
}) {
  const [tab, setTab] = useState<'raw' | 'json' | 'headers'>('raw');
  const active = tab === 'json' && !result?.jsonText ? 'raw' : tab;
  return (
    <section className="response-section" aria-label="Response">
      <div className="response-heading">
        <span>Response</span>
        <span className="response-meta">
          {result?.status !== null && result?.status !== undefined ? `HTTP ${result.status}` : '—'}
          <span>·</span>
          {result?.elapsedMs !== null && result?.elapsedMs !== undefined
            ? `${result.elapsedMs} ms`
            : '— ms'}
          <span>·</span>
          {result ? `${result.byteCount.toLocaleString('en-US')} B` : '— B'}
        </span>
      </div>
      {environmentLabel && (
        <p className="response-notice">
          Environment at dispatch: {environmentLabel}. Results belong to the last send of this
          draft.
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
                {(['raw', 'json', 'headers'] as const).map((name) => (
                  <button
                    key={name}
                    role="tab"
                    tabIndex={active === name ? 0 : -1}
                    onKeyDown={(event) => {
                      const names: ('raw' | 'json' | 'headers')[] = result.jsonText
                        ? ['raw', 'json', 'headers']
                        : ['raw', 'headers'];
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
                    disabled={name === 'json' && !result.jsonText}
                    onClick={() => setTab(name)}
                  >
                    {name === 'raw' ? 'Raw' : name === 'json' ? 'JSON' : 'Headers'}
                  </button>
                ))}
              </div>
              <div
                className="response-content"
                role="tabpanel"
                id={`response-${active}`}
                aria-labelledby={`response-tab-${active}`}
              >
                {active === 'headers' ? (
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
