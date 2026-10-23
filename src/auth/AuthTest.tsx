import { useState } from 'react';
import { testAuthentication, type AuthenticationTestResult } from '../native';
import type { ExecutionInput } from '../http/types';
import { useNativeCheck } from '../http/useNativeCheck';

export function AuthTest({
  input,
  tokenOnly,
  disabled = false,
}: {
  input: ExecutionInput | null;
  tokenOnly: boolean;
  disabled?: boolean;
}) {
  const [url, setUrl] = useState('');
  const check = useNativeCheck<AuthenticationTestResult>();
  return (
    <section className="auth-test" aria-label="Test authentication">
      <h3>Test authentication</h3>
      <p>
        {tokenOnly
          ? 'Sends the configured token/login request using these settings and the selected environment. The test token is discarded; settings and stored tokens stay unchanged.'
          : 'Sends GET to your test URL with this authentication and the selected environment. Choose an endpoint that requires authentication, such as /me. A 2xx response confirms the call, not necessarily authentication enforcement.'}
      </p>
      {!tokenOnly && (
        <label className="dialog-label">
          Test URL
          <input
            aria-label="Authentication test URL"
            value={url}
            disabled={check.running || disabled}
            placeholder="https://api.example.com/me"
            onChange={(event) => setUrl(event.target.value)}
          />
        </label>
      )}
      <button
        type="button"
        className="secondary-button"
        disabled={disabled || check.running || !input || (!tokenOnly && !url.trim())}
        onClick={() => {
          if (!input) {
            return;
          }
          void check.run((id) =>
            testAuthentication(
              id,
              {
                ...input,
                saveResponse: false,
                request: {
                  ...input.request,
                  url: tokenOnly ? input.request.url : url,
                  method: 'GET',
                  headers: [],
                  params: [],
                  body: { kind: 'none', text: '', fields: [], file: null },
                },
              },
              tokenOnly,
            ),
          );
        }}
      >
        {check.running ? 'Testing authentication…' : 'Test authentication'}
      </button>
      {check.running && (
        <button type="button" className="secondary-button" onClick={check.cancel}>
          Cancel authentication test
        </button>
      )}
      {check.result && (
        <p role="status" className={check.result.success ? 'check-success' : 'check-failure'}>
          <strong>
            {check.result.success ? 'Test succeeded' : 'Test failed'}
            {check.result.status !== null ? ` · HTTP ${check.result.status}` : ''}
          </strong>{' '}
          {check.result.message}
        </p>
      )}
      {check.error && <p role="alert">{check.error}</p>}
    </section>
  );
}
