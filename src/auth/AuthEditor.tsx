import type { Auth } from './types';
import type { Definition } from '../environments/types';

export function AuthEditor({
  value,
  onChange,
  variables,
  root = false,
  disabled = false,
}: {
  value: Auth | null;
  onChange: (value: Auth | null) => void;
  variables: Definition[];
  root?: boolean;
  disabled?: boolean;
}) {
  function variable(label: string, id: string, change: (id: string) => void) {
    return (
      <label className="dialog-label">
        {label}
        <select
          aria-label={label}
          value={id}
          disabled={disabled}
          required
          onChange={(event) => change(event.target.value)}
        >
          <option value="">Choose a project variable…</option>
          {id && !variables.some((item) => item.id === id) && (
            <option value={id}>Missing variable — choose a replacement</option>
          )}
          {variables.map((item) => (
            <option key={item.id} value={item.id}>
              {item.name}
            </option>
          ))}
        </select>
      </label>
    );
  }
  return (
    <div className="auth-editor">
      <label className="dialog-label">
        Authentication method
        <select
          aria-label="Authentication method"
          value={value?.kind ?? (root ? 'none' : 'inherit')}
          disabled={disabled}
          onChange={(event) => {
            const kind = event.target.value;
            onChange(
              kind === 'inherit'
                ? null
                : kind === 'basic'
                  ? { kind, username: '', password: '' }
                  : kind === 'bearer'
                    ? { kind, token: '' }
                    : kind === 'apiKey'
                      ? { kind, name: 'X-API-Key', value: '', placement: 'header' }
                      : { kind: 'none' },
            );
          }}
        >
          {!root && <option value="inherit">Inherit from parent</option>}
          <option value="none">None</option>
          <option value="basic">Basic</option>
          <option value="bearer">Bearer / JWT</option>
          <option value="apiKey">API Key</option>
        </select>
      </label>
      {value?.kind === 'basic' && (
        <div className="auth-fields">
          {variable('Username variable', value.username, (username) =>
            onChange({ ...value, username }),
          )}
          {variable('Password variable', value.password, (password) =>
            onChange({ ...value, password }),
          )}
        </div>
      )}
      {value?.kind === 'bearer' &&
        variable('Token variable', value.token, (token) => onChange({ ...value, token }))}
      {value?.kind === 'apiKey' && (
        <>
          <label className="dialog-label">
            API key name
            <input
              aria-label="API key name"
              value={value.name}
              maxLength={200}
              required
              disabled={disabled}
              onChange={(event) => onChange({ ...value, name: event.target.value })}
            />
          </label>
          <label className="dialog-label">
            API key placement
            <select
              aria-label="API key placement"
              value={value.placement}
              disabled={disabled}
              onChange={(event) =>
                onChange({ ...value, placement: event.target.value as 'header' | 'query' })
              }
            >
              <option value="header">Header</option>
              <option value="query">Query parameter</option>
            </select>
          </label>
          {variable('API key variable', value.value, (id) => onChange({ ...value, value: id }))}
        </>
      )}
      <p className="auth-help">
        Credentials use project variables and the selected environment, with shared defaults as
        fallback. Manage values in Project variables. Manual headers and query parameters take
        priority over configured authentication.
      </p>
      {variables.length === 0 && value && value.kind !== 'none' && (
        <p role="status">Create project variables before selecting credentials.</p>
      )}
    </div>
  );
}
