import { useState } from 'react';
import type { Definition } from '../environments/types';
import type { Auth, Composition, Extraction, LoginEntry, LoginRequest, Part } from './types';
import { literal, loginRequest } from './defaults';

function Variable({
  label,
  value,
  onChange,
  variables,
}: {
  label: string;
  value: string;
  onChange: (id: string) => void;
  variables: Definition[];
}) {
  return (
    <label className="dialog-label">
      {label}
      <select aria-label={label} required value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">Choose a project variable…</option>
        {value && !variables.some((v) => v.id === value) && (
          <option value={value}>Missing variable — choose a replacement</option>
        )}
        {variables.map((v) => (
          <option key={v.id} value={v.id}>
            {v.name}
          </option>
        ))}
      </select>
    </label>
  );
}

export function CompositionEditor({
  label,
  value,
  onChange,
  variables,
  renewal = false,
}: {
  label: string;
  value: Composition;
  onChange: (v: Composition) => void;
  variables: Definition[];
  renewal?: boolean;
}) {
  const change = (index: number, part: Part) =>
    onChange({ ...value, parts: value.parts.map((v, i) => (i === index ? part : v)) });
  return (
    <fieldset className="token-composition">
      <legend>{label}</legend>
      {value.parts.map((part, index) => (
        <div className="token-part" key={index}>
          <label className="dialog-label">
            Input {index + 1} source
            <select
              aria-label={`${label} input ${index + 1} source`}
              value={part.kind}
              onChange={(e) => {
                const kind = e.target.value as Part['kind'];
                change(
                  index,
                  kind === 'literal'
                    ? { kind, text: '' }
                    : kind === 'variable'
                      ? { kind, id: '' }
                      : { kind },
                );
              }}
            >
              <option value="literal">Literal text</option>
              <option value="variable">Project variable</option>
              {renewal && (
                <>
                  <option value="accessToken">Stored access token</option>
                  <option value="renewalToken">Stored renewal value</option>
                </>
              )}
            </select>
          </label>
          {part.kind === 'literal' && (
            <label className="dialog-label">
              Text
              <textarea
                aria-label={`${label} input ${index + 1} text`}
                value={part.text}
                maxLength={65536}
                onChange={(e) => change(index, { ...part, text: e.target.value })}
              />
            </label>
          )}
          {part.kind === 'variable' && (
            <Variable
              label={`${label} input ${index + 1} variable`}
              value={part.id}
              onChange={(id) => change(index, { ...part, id })}
              variables={variables}
            />
          )}
          <button
            type="button"
            className="secondary-button"
            aria-label={`Remove ${label} input ${index + 1}`}
            onClick={() => onChange({ ...value, parts: value.parts.filter((_, i) => i !== index) })}
          >
            Remove input
          </button>
        </div>
      ))}
      <button
        type="button"
        className="secondary-button"
        disabled={value.parts.length >= 100}
        onClick={() =>
          onChange({ ...value, parts: [...value.parts, { kind: 'literal', text: '' }] })
        }
      >
        Add {label} input
      </button>
      <details>
        <summary>Composition and Base64</summary>
        {(['separator', 'prefix', 'suffix'] as const).map((key) => (
          <label className="dialog-label" key={key}>
            {key}
            <input
              aria-label={`${label} ${key}`}
              value={value[key]}
              maxLength={4096}
              onChange={(e) => onChange({ ...value, [key]: e.target.value })}
            />
          </label>
        ))}
        <label className="token-checkbox">
          <input
            type="checkbox"
            checked={value.base64}
            onChange={(e) => onChange({ ...value, base64: e.target.checked })}
          />
          Base64-encode joined inputs before adding prefix and suffix
        </label>
      </details>
    </fieldset>
  );
}

function ExtractionEditor({
  label,
  value,
  onChange,
  optional = false,
}: {
  label: string;
  value: Extraction | null;
  onChange: (v: Extraction | null) => void;
  optional?: boolean;
}) {
  return (
    <fieldset className="token-composition">
      <legend>{label}</legend>
      <select
        aria-label={`${label} source`}
        value={value?.kind ?? 'none'}
        onChange={(e) => {
          const kind = e.target.value;
          onChange(
            kind === 'none'
              ? null
              : kind === 'json'
                ? { kind, pointer: '/access_token' }
                : kind === 'header'
                  ? { kind, name: 'X-Access-Token' }
                  : { kind: 'text' },
          );
        }}
      >
        {optional && <option value="none">Not configured</option>}
        <option value="text">Complete response text (trimmed)</option>
        <option value="json">JSON field (JSON Pointer)</option>
        <option value="header">Response header</option>
      </select>
      {value?.kind === 'json' && (
        <label className="dialog-label">
          JSON Pointer
          <input
            aria-label={`${label} JSON Pointer`}
            value={value.pointer}
            placeholder="/access_token"
            maxLength={1024}
            onChange={(e) => onChange({ ...value, pointer: e.target.value })}
          />
        </label>
      )}
      {value?.kind === 'header' && (
        <label className="dialog-label">
          Header name
          <input
            aria-label={`${label} header name`}
            value={value.name}
            required
            onChange={(e) => onChange({ ...value, name: e.target.value })}
          />
        </label>
      )}
    </fieldset>
  );
}

function Entries({
  label,
  entries,
  onChange,
  variables,
  renewal,
}: {
  label: string;
  entries: LoginEntry[];
  onChange: (v: LoginEntry[]) => void;
  variables: Definition[];
  renewal: boolean;
}) {
  return (
    <details className="token-entries">
      <summary>
        {label} ({entries.length})
      </summary>
      {entries.map((entry, index) => (
        <fieldset className="token-composition" key={index}>
          <legend>
            {label} {index + 1}
          </legend>
          <label className="dialog-label">
            Name
            <input
              aria-label={`${label} ${index + 1} name`}
              required
              value={entry.name}
              maxLength={200}
              onChange={(e) =>
                onChange(entries.map((v, i) => (i === index ? { ...v, name: e.target.value } : v)))
              }
            />
          </label>
          <CompositionEditor
            label={`${label} ${index + 1} value`}
            value={entry.value}
            onChange={(value) =>
              onChange(entries.map((v, i) => (i === index ? { ...v, value } : v)))
            }
            variables={variables}
            renewal={renewal}
          />
          <button
            type="button"
            className="secondary-button"
            onClick={() => onChange(entries.filter((_, i) => i !== index))}
          >
            Remove {label} {index + 1}
          </button>
        </fieldset>
      ))}
      <button
        type="button"
        className="secondary-button"
        disabled={entries.length >= 200}
        onClick={() => onChange([...entries, { name: '', value: literal() }])}
      >
        Add {label}
      </button>
    </details>
  );
}

function StatusCodes({
  value,
  onChange,
  label,
}: {
  value: number[];
  onChange: (v: number[]) => void;
  label: string;
}) {
  const [code, setCode] = useState(200);
  return (
    <fieldset className="token-composition">
      <legend>Accepted success statuses</legend>
      <p>{value.length ? value.join(', ') : 'Choose at least one status.'}</p>
      <label className="dialog-label">
        HTTP status
        <input
          aria-label={`${label} success status`}
          type="number"
          min={200}
          max={599}
          value={code}
          onChange={(e) => setCode(Number(e.target.value))}
        />
      </label>
      <button
        type="button"
        className="secondary-button"
        disabled={code < 200 || code > 599 || !Number.isInteger(code)}
        onClick={() => onChange([...new Set([...value, code])].sort())}
      >
        Accept status
      </button>
      {value.map((status) => (
        <button
          type="button"
          className="secondary-button"
          key={status}
          onClick={() => onChange(value.filter((v) => v !== status))}
        >
          Remove {status}
        </button>
      ))}
    </fieldset>
  );
}

function LoginRequestEditor({
  label,
  value,
  onChange,
  variables,
  renewal = false,
}: {
  label: string;
  value: LoginRequest;
  onChange: (v: LoginRequest) => void;
  variables: Definition[];
  renewal?: boolean;
}) {
  return (
    <fieldset className="token-request">
      <legend>{label}</legend>
      <CompositionEditor
        label={`${label} endpoint`}
        value={value.url}
        onChange={(url) => onChange({ ...value, url })}
        variables={variables}
        renewal={renewal}
      />
      <label className="dialog-label">
        HTTP method
        <select
          aria-label={`${label} method`}
          value={value.method}
          onChange={(e) => onChange({ ...value, method: e.target.value })}
        >
          {['POST', 'GET', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'].map((m) => (
            <option key={m}>{m}</option>
          ))}
        </select>
      </label>
      <Entries
        label={`${label} query parameter`}
        entries={value.params}
        onChange={(params) => onChange({ ...value, params })}
        variables={variables}
        renewal={renewal}
      />
      <Entries
        label={`${label} header`}
        entries={value.headers}
        onChange={(headers) => onChange({ ...value, headers })}
        variables={variables}
        renewal={renewal}
      />
      <label className="dialog-label">
        Body
        <select
          aria-label={`${label} body type`}
          value={value.body.kind}
          onChange={(e) => {
            const kind = e.target.value as LoginRequest['body']['kind'];
            onChange({
              ...value,
              body:
                kind === 'none'
                  ? { kind }
                  : kind === 'urlEncoded'
                    ? { kind, fields: [] }
                    : { kind, value: literal() },
            });
          }}
        >
          <option value="none">None</option>
          <option value="text">Text</option>
          <option value="json">JSON</option>
          <option value="urlEncoded">URL-encoded form</option>
        </select>
      </label>
      {(value.body.kind === 'text' || value.body.kind === 'json') && (
        <CompositionEditor
          label={`${label} body`}
          value={value.body.value}
          onChange={(body) =>
            onChange({ ...value, body: { kind: value.body.kind as 'text' | 'json', value: body } })
          }
          variables={variables}
          renewal={renewal}
        />
      )}
      {value.body.kind === 'urlEncoded' && (
        <Entries
          label={`${label} form field`}
          entries={value.body.fields}
          onChange={(fields) => onChange({ ...value, body: { kind: 'urlEncoded', fields } })}
          variables={variables}
          renewal={renewal}
        />
      )}
      <StatusCodes
        label={label}
        value={value.successStatuses}
        onChange={(successStatuses) => onChange({ ...value, successStatuses })}
      />
      <ExtractionEditor
        label={`${label} token extraction`}
        value={value.token}
        onChange={(token) => {
          if (token) {
            onChange({ ...value, token });
          }
        }}
      />
      <ExtractionEditor
        label={`${label} renewal value extraction`}
        value={value.renewalToken}
        onChange={(renewalToken) => onChange({ ...value, renewalToken })}
        optional
      />
      <ExtractionEditor
        label={`${label} lifetime extraction (seconds)`}
        value={value.expiresIn}
        onChange={(expiresIn) => onChange({ ...value, expiresIn, lifetimeSeconds: null })}
        optional
      />
      {!value.expiresIn && (
        <label className="dialog-label">
          Configured validity (seconds, optional)
          <input
            aria-label={`${label} validity seconds`}
            type="number"
            min={1}
            max={315360000}
            value={value.lifetimeSeconds ?? ''}
            onChange={(e) =>
              onChange({
                ...value,
                lifetimeSeconds: e.target.value === '' ? null : Number(e.target.value),
              })
            }
          />
        </label>
      )}
      {!value.expiresIn && !value.lifetimeSeconds && (
        <p className="auth-help">
          Expiry unknown. This token stays available until you renew or forget it.
        </p>
      )}
    </fieldset>
  );
}

export function TokenAuthEditor({
  value,
  onChange,
  variables,
  disabled,
}: {
  value: Extract<Auth, { kind: 'oauth2' | 'customLogin' }>;
  onChange: (v: Auth) => void;
  variables: Definition[];
  disabled: boolean;
}) {
  return (
    <fieldset className="token-auth-fields" disabled={disabled}>
      <p className="auth-help">
        Use project variables for credentials and reusable values. Literal text is saved in project
        files. Token endpoints require HTTPS; numeric loopback HTTP is available for local testing.
      </p>
      {value.kind === 'oauth2' ? (
        <>
          <CompositionEditor
            label="Token endpoint"
            value={value.config.tokenUrl}
            onChange={(tokenUrl) => onChange({ ...value, config: { ...value.config, tokenUrl } })}
            variables={variables}
          />
          <Variable
            label="Client ID variable"
            value={value.config.clientId}
            onChange={(clientId) => onChange({ ...value, config: { ...value.config, clientId } })}
            variables={variables}
          />
          <Variable
            label="Client secret variable"
            value={value.config.clientSecret}
            onChange={(clientSecret) =>
              onChange({ ...value, config: { ...value.config, clientSecret } })
            }
            variables={variables}
          />
          <CompositionEditor
            label="OAuth2 scope"
            value={value.config.scope}
            onChange={(scope) => onChange({ ...value, config: { ...value.config, scope } })}
            variables={variables}
          />
          <label className="dialog-label">
            Client authentication
            <select
              aria-label="Client authentication"
              value={value.config.clientAuthentication}
              onChange={(e) =>
                onChange({
                  ...value,
                  config: {
                    ...value.config,
                    clientAuthentication: e.target.value as 'basic' | 'body',
                  },
                })
              }
            >
              <option value="basic">HTTP Basic</option>
              <option value="body">Client credentials in form body</option>
            </select>
          </label>
          <p className="auth-help">
            Acquires a Bearer token. Known expiry triggers reacquisition before sending. No
            refresh-token grant is used.
          </p>
        </>
      ) : (
        <>
          <LoginRequestEditor
            label="Login"
            value={value.config.login}
            onChange={(login) => onChange({ ...value, config: { ...value.config, login } })}
            variables={variables}
          />
          <label className="token-checkbox">
            <input
              type="checkbox"
              checked={value.config.renewal !== null}
              onChange={(e) =>
                onChange({
                  ...value,
                  config: { ...value.config, renewal: e.target.checked ? loginRequest() : null },
                })
              }
            />
            Use a separate renewal request
          </label>
          {value.config.renewal ? (
            <LoginRequestEditor
              label="Renewal"
              value={value.config.renewal}
              onChange={(renewal) => onChange({ ...value, config: { ...value.config, renewal } })}
              variables={variables}
              renewal
            />
          ) : (
            <p className="auth-help">Renewal repeats the login request.</p>
          )}
          <fieldset className="token-composition">
            <legend>Use token in API requests</legend>
            <label className="dialog-label">
              Token placement
              <select
                aria-label="Token placement"
                value={value.config.placement.placement}
                onChange={(e) =>
                  onChange({
                    ...value,
                    config: {
                      ...value.config,
                      placement: {
                        ...value.config.placement,
                        placement: e.target.value as 'header' | 'query',
                      },
                    },
                  })
                }
              >
                <option value="header">Header</option>
                <option value="query">Query parameter</option>
              </select>
            </label>
            <label className="dialog-label">
              Token field name
              <input
                aria-label="Token field name"
                required
                value={value.config.placement.name}
                onChange={(e) =>
                  onChange({
                    ...value,
                    config: {
                      ...value.config,
                      placement: { ...value.config.placement, name: e.target.value },
                    },
                  })
                }
              />
            </label>
            <label className="dialog-label">
              Token prefix
              <input
                aria-label="Token prefix"
                value={value.config.placement.prefix}
                onChange={(e) =>
                  onChange({
                    ...value,
                    config: {
                      ...value.config,
                      placement: { ...value.config.placement, prefix: e.target.value },
                    },
                  })
                }
              />
            </label>
          </fieldset>
        </>
      )}
      <p className="auth-help">
        Token responses stay in native memory and the OS credential store. Failed API requests are
        never replayed automatically.
      </p>
    </fieldset>
  );
}
