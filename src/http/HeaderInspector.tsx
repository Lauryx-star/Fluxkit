import * as native from '../native';
import { useNativeCheck } from './useNativeCheck';
import type { ExecutionInput, RequestHeader } from './types';
export function HeaderTable({ headers }: { headers: RequestHeader[] }) {
  return (
    <table className="request-header-table">
      <thead>
        <tr>
          <th>Header</th>
          <th>Value</th>
          <th>Source</th>
        </tr>
      </thead>
      <tbody>
        {headers.map((row, index) => (
          <tr key={index}>
            <td>
              <code>{row.name}</code>
            </td>
            <td>
              <code>{row.value}</code>
            </td>
            <td>{row.source}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
export function HeaderInspector({
  input,
  disabled,
  sent,
}: {
  input: ExecutionInput;
  disabled: boolean;
  sent?: RequestHeader[];
}) {
  const check = useNativeCheck<RequestHeader[]>();
  return (
    <details className="header-inspector">
      <summary>Automatic and sent headers</summary>
      <p>
        Inspect the current draft locally. This resolves active variables but sends no request and
        acquires no token. Manual and authentication values stay masked. Host and transfer framing
        are managed by the HTTP transport and are not part of this list.
      </p>
      <button
        type="button"
        className="secondary-button"
        disabled={disabled || check.running}
        onClick={() => void check.run((id) => native.previewHttpHeaders(id, input))}
      >
        {check.running ? 'Inspecting…' : 'Inspect headers'}
      </button>
      {check.running && (
        <button type="button" className="secondary-button" onClick={check.cancel}>
          Cancel inspection
        </button>
      )}
      {check.error && <p role="alert">{check.error}</p>}
      {check.result && (
        <section aria-label="Inspected request headers">
          <h3>Draft at inspection</h3>
          <HeaderTable headers={check.result} />
          <p>Inspect again after changes. Multipart boundaries are generated for each send.</p>
        </section>
      )}
      {sent && (
        <section aria-label="Last dispatched request headers">
          <h3>Last dispatch</h3>
          <HeaderTable headers={sent} />
        </section>
      )}
    </details>
  );
}
