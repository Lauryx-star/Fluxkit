import { useMemo } from 'react';
import { Modal } from '../projects/Dialogs';
import { decodeToken, isExpired, summaryClaims } from './jwt';

/**
 * Shows one explicitly inspected token. The token lives only in the caller's state while this
 * dialog is open; it is never persisted, logged or sent anywhere.
 */
export function TokenInspector({ token, onClose }: { token: string; onClose: () => void }) {
  const decoded = useMemo(() => decodeToken(token), [token]);
  return (
    <Modal label="Token details" onDismiss={onClose}>
      <div className="token-inspector">
        <h2>Token details</h2>
        {decoded.kind === 'jws' && (
          <>
            {isExpired(decoded.payload) && (
              <p role="alert">The token’s exp claim is in the past.</p>
            )}
            <dl className="token-claims">
              {summaryClaims(decoded.payload).map((row) => (
                <div key={row.label}>
                  <dt>{row.label}</dt>
                  <dd>{row.value}</dd>
                </div>
              ))}
            </dl>
            <p className="auth-help">
              {decoded.hasSignature
                ? 'Decoded locally. The signature is present but not verified.'
                : 'Decoded locally. This token has no signature.'}
            </p>
            <h3>Header</h3>
            <pre className="token-json" aria-label="Token header">
              {JSON.stringify(decoded.header, null, 2)}
            </pre>
            <h3>Payload</h3>
            <pre className="token-json" aria-label="Token payload">
              {JSON.stringify(decoded.payload, null, 2)}
            </pre>
          </>
        )}
        {decoded.kind === 'jwe' && (
          <>
            <p>This is an encrypted JWT (JWE). Only its header can be shown.</p>
            <h3>Header</h3>
            <pre className="token-json" aria-label="Token header">
              {JSON.stringify(decoded.header, null, 2)}
            </pre>
          </>
        )}
        {decoded.kind === 'opaque' && (
          <p>This token is not a JWT. It has no readable content; only its value is shown.</p>
        )}
        <h3>Raw token</h3>
        <textarea
          className="token-raw"
          aria-label="Raw token"
          readOnly
          rows={4}
          value={token}
          spellCheck={false}
        />
        <div className="dialog-actions">
          <button type="button" className="primary-button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </Modal>
  );
}
