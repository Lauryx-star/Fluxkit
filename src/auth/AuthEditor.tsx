import { Select } from '../ui/Select';
import type { Auth } from './types';
import type { Definition } from '../environments/types';
import { TokenAuthEditor } from './TokenAuthEditor';
import { literal, loginRequest } from './defaults';

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
        <Select
          aria-label={label}
          value={id}
          disabled={disabled}
          required
          onValueChange={(selectedValue) => change(selectedValue)}
        >
          <option value="">Choose a project variable…</option>
          {id && !variables.some((item) => item.id === id) && (
            <option value={id}>Deleted variable — choose a replacement</option>
          )}
          {variables.map((item) => (
            <option key={item.id} value={item.id}>
              {item.name}
            </option>
          ))}
        </Select>
      </label>
    );
  }
  return (
    <div className="auth-editor">
      <label className="dialog-label">
        Authentication method
        <Select
          aria-label="Authentication method"
          value={value?.kind ?? (root ? 'none' : 'inherit')}
          disabled={disabled}
          onValueChange={(selectedValue) => {
            const kind = selectedValue;
            onChange(
              kind === 'oauth2'
                ? {
                    kind,
                    config: {
                      tokenUrl: literal(),
                      clientId: '',
                      clientSecret: '',
                      scope: literal(),
                      clientAuthentication: 'basic',
                    },
                  }
                : kind === 'customLogin'
                  ? {
                      kind,
                      config: {
                        login: loginRequest(),
                        renewal: null,
                        placement: {
                          name: 'Authorization',
                          placement: 'header',
                          prefix: 'Bearer ',
                        },
                      },
                    }
                  : kind === 'inherit'
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
          <option value="oauth2">OAuth2 Client Credentials</option>
          <option value="customLogin">Custom Login</option>
        </Select>
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
          <div className="field-row">
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
              <Select
                aria-label="API key placement"
                value={value.placement}
                disabled={disabled}
                onValueChange={(selectedValue) =>
                  onChange({ ...value, placement: selectedValue as 'header' | 'query' })
                }
              >
                <option value="header">Header</option>
                <option value="query">Query parameter</option>
              </Select>
            </label>
          </div>
          {variable('API key variable', value.value, (id) => onChange({ ...value, value: id }))}
        </>
      )}
      {(value?.kind === 'oauth2' || value?.kind === 'customLogin') && (
        <TokenAuthEditor
          value={value}
          onChange={onChange}
          variables={variables}
          disabled={disabled}
        />
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
