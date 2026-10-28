import { Select } from '../ui/Select';
import { useEffect, useRef, useState } from 'react';
import { environmentAction } from '../native';
import { Modal } from '../projects/Dialogs';
import { Icon } from '../components/Icon';
import type {
  Definition,
  Definitions,
  EnvironmentAction,
  EnvironmentView,
  TableCell,
  TableRow,
} from './types';

interface Props {
  handle: string;
  serverOwned?: boolean;
  ask: (message: string) => Promise<boolean>;
  onDirty: (dirty: boolean) => void;
  onOpen: (open: boolean) => void;
  onView?: (handle: string, view: EnvironmentView) => void;
  onRegisterSave?: (save: (() => Promise<boolean>) | null) => void;
}
export function EnvironmentControls({
  handle,
  serverOwned = false,
  ask,
  onDirty,
  onOpen,
  onView,
  onRegisterSave,
}: Props) {
  const [view, setView] = useState<EnvironmentView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    let disposed = false;
    void environmentAction(handle, { kind: 'load' }).then(
      (reply) => {
        if (!disposed) {
          setView(reply.view);
          onView?.(handle, reply.view);
        }
      },
      () => {
        if (!disposed) {
          setError('Could not load environments. Open variables and retry.');
        }
      },
    );
    return () => {
      disposed = true;
    };
  }, [handle, onView]);
  async function act(action: EnvironmentAction) {
    setBusy(true);
    setError(null);
    try {
      const reply = await environmentAction(handle, action);
      setView(reply.view);
      onView?.(handle, reply.view);
      return reply;
    } catch (error) {
      setError(typeof error === 'string' ? error : 'Environment operation failed. Retry.');
      return null;
    } finally {
      setBusy(false);
    }
  }
  function show(open: boolean) {
    setOpen(open);
    onOpen(open);
  }
  return (
    <div className="environment-controls">
      <Select
        aria-label="Project environment"
        value={view?.selectionMissing ? '__missing' : (view?.selectedEnvironment ?? '')}
        disabled={busy || !view}
        onValueChange={(selectedValue) =>
          void act({ kind: 'select', environment: selectedValue || null })
        }
      >
        {view?.selectionMissing && (
          <option value="__missing" disabled>
            Choose environment
          </option>
        )}
        <option value="">Defaults</option>
        {view?.data.environments.map((item) => (
          <option key={item.id} value={item.id}>
            {item.name}
          </option>
        ))}
      </Select>
      <button className="secondary-button" onClick={() => show(true)}>
        Variables
      </button>
      {!open && error && (
        <span role="alert" className="environment-warning">
          {error}
        </span>
      )}
      {!open && view?.selectionMissing && (
        <span role="alert">Select an environment: the previous selection was removed.</span>
      )}
      {open && (
        <EnvironmentManager
          serverOwned={serverOwned}
          view={view}
          busy={busy}
          error={error}
          act={act}
          ask={ask}
          onDirty={onDirty}
          onRegisterSave={onRegisterSave}
          onClose={() => {
            onDirty(false);
            show(false);
          }}
        />
      )}
    </div>
  );
}
interface ManagerProps {
  serverOwned: boolean;
  view: EnvironmentView | null;
  busy: boolean;
  error: string | null;
  act: (
    action: EnvironmentAction,
  ) =>
    | ReturnType<typeof environmentAction>
    | Promise<null | Awaited<ReturnType<typeof environmentAction>>>;
  ask: Props['ask'];
  onDirty: Props['onDirty'];
  onRegisterSave: Props['onRegisterSave'];
  onClose: () => void;
}

/** What the small edit popup is changing. */
type Editing =
  | { kind: 'value'; variable: string; environment: string | null; initial: string }
  | { kind: 'name'; group: 'variables' | 'environments'; id: string; initial: string }
  | { kind: 'newEnvironment'; initial: '' };

const slotKey = (variable: string, environment: string | null) =>
  `${variable}/${environment ?? 'default'}`;

function EnvironmentManager({
  serverOwned,
  view,
  busy,
  error,
  act,
  ask,
  onDirty,
  onClose,
  onRegisterSave,
}: ManagerProps) {
  const environments = view?.data.environments ?? [];
  const [column, setColumn] = useState<string>(
    () => view?.selectedEnvironment ?? view?.data.environments[0]?.id ?? '',
  );
  // A removed or not yet chosen environment falls back to the first one.
  const shown = environments.some((item) => item.id === column)
    ? column
    : (environments[0]?.id ?? '');
  const [rows, setRows] = useState<Map<string, TableRow> | null>(null);
  const [revealed, setRevealed] = useState<Record<string, string | null>>({});
  const [editing, setEditing] = useState<Editing | null>(null);
  const [draft, setDraft] = useState('');
  const [draftVisible, setDraftVisible] = useState(false);
  const [newName, setNewName] = useState('');
  const [newSecret, setNewSecret] = useState(false);
  const [filter, setFilter] = useState('');
  const [status, setStatus] = useState<string | null>(null);
  const dirty = (editing !== null && draft !== editing.initial) || newName.trim().length > 0;
  const editor = useRef<HTMLFormElement>(null);
  // Keep a popup opened near the bottom of the scrolling table fully visible.
  useEffect(() => {
    editor.current?.scrollIntoView?.({ block: 'nearest' });
  }, [editing]);
  useEffect(() => {
    onDirty(dirty);
  }, [dirty, onDirty]);

  async function loadTable(environment = shown) {
    const reply = await act({ kind: 'table', environment: environment || null });
    if (reply?.table) {
      setRows(new Map(reply.table.map((row) => [row.variable, row])));
    }
    return reply;
  }
  // The table follows the chosen environment and every definition change.
  const revision = view?.revision;
  useEffect(() => {
    let current = true;
    void act({ kind: 'table', environment: shown || null }).then((reply) => {
      if (current && reply?.table) {
        setRows(new Map(reply.table.map((row) => [row.variable, row])));
      }
    });
    return () => {
      current = false;
    };
    // `act` changes identity on every render of the parent; reload only for real changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shown, revision]);

  function startEditing(next: Editing, visible = true) {
    setEditing(next);
    setDraft(next.initial);
    setDraftVisible(visible);
    setStatus(null);
  }
  async function saveDefinitions(data: Definitions) {
    if (!view) {
      return null;
    }
    return act({ kind: 'save', data, expected: view.revision });
  }
  async function saveValue(
    variable: string,
    environment: string | null,
    value: string | null,
    expected = view?.revision,
  ) {
    if (!expected) {
      return false;
    }
    const reply = await act({
      kind: 'setValue',
      variable,
      environment,
      value,
      expected,
      ...(view?.serverValues
        ? { expectedValueRevision: slotRevision(view.serverValues, variable, environment) }
        : {}),
    });
    if (!reply) {
      return false;
    }
    setRevealed((current) => {
      const next = { ...current };
      delete next[slotKey(variable, environment)];
      return next;
    });
    await loadTable();
    return true;
  }
  /** Saves whatever the popup holds; returns false and keeps the popup open on failure. */
  async function commitEditing(expected = view?.revision) {
    if (!editing || !view) {
      return true;
    }
    if (editing.kind === 'value') {
      if (!(await saveValue(editing.variable, editing.environment, draft, expected))) {
        return false;
      }
      setStatus(
        serverOwned
          ? 'Saved on the server. Secrets are encrypted for your account; ordinary values are shared.'
          : 'Saved in the OS credential store.',
      );
    } else {
      const name = draft.trim();
      if (!name) {
        setStatus('Enter a name.');
        return false;
      }
      const group = editing.kind === 'name' ? editing.group : 'environments';
      const entries =
        editing.kind === 'name'
          ? view.data[group].map((item) => (item.id === editing.id ? { ...item, name } : item))
          : [...view.data.environments, { id: crypto.randomUUID(), name }];
      if (!(await saveDefinitions({ ...view.data, [group]: entries }))) {
        return false;
      }
      setStatus(null);
    }
    setEditing(null);
    setDraft('');
    return true;
  }
  async function addVariable(expected = view?.revision) {
    const name = newName.trim();
    if (!view || !name || !expected) {
      return null;
    }
    const reply = await act({
      kind: 'save',
      data: {
        ...view.data,
        variables: [...view.data.variables, { id: crypto.randomUUID(), name, secret: newSecret }],
      },
      expected,
    });
    if (reply) {
      setNewName('');
      setNewSecret(false);
    }
    return reply;
  }
  // Save & quit: commit an open popup and a typed new variable name.
  useEffect(() => {
    onRegisterSave?.(async () => {
      if (!dirty) {
        return true;
      }
      if (busy || !view) {
        return false;
      }
      let expected = view.revision;
      if (newName.trim()) {
        const reply = await addVariable(expected);
        if (!reply) {
          return false;
        }
        expected = reply.view.revision;
      }
      return commitEditing(expected);
    });
    return () => onRegisterSave?.(null);
  });

  async function classify(item: Definition) {
    if (!view) {
      return;
    }
    const secret = item.secret ?? true;
    if (
      !(await ask(
        secret
          ? `Mark ${item.name} as ordinary? ${serverOwned ? 'Its encrypted values are deleted for every member; enter the value again afterwards. Ordinary values are visible to every project member.' : 'Its values stay in your OS credential store and are shown in this table.'}`
          : `Mark ${item.name} as secret? ${serverOwned ? 'Its shared values are deleted; each member then stores an encrypted value of their own.' : 'Its values stay in your OS credential store and are masked in this table.'}`,
      ))
    ) {
      return;
    }
    await saveDefinitions({
      ...view.data,
      variables: view.data.variables.map((entry) =>
        entry.id === item.id ? { ...entry, secret: !secret } : entry,
      ),
    });
  }
  async function remove(item: Definition, group: 'variables' | 'environments') {
    if (!view) {
      return;
    }
    let usage = '';
    if (group === 'variables') {
      const reply = await act({ kind: 'usages', variable: item.id });
      if (!reply?.usages) {
        return;
      }
      usage = usageWarning(item.name, reply.usages);
    }
    if (
      !(await ask(
        usage +
          (serverOwned
            ? `Remove ${item.name}? Its values are deleted on the server for every member.`
            : `Remove ${item.name} and delete its associated values from this machine's OS credential store? This cannot be undone. If deletion fails, some values may already be removed; retry the removal.`),
      ))
    ) {
      return;
    }
    if (
      editing &&
      ((editing.kind === 'name' && editing.id === item.id) ||
        (editing.kind === 'value' &&
          (editing.variable === item.id || editing.environment === item.id)))
    ) {
      setEditing(null);
    }
    await saveDefinitions({
      ...view.data,
      [group]: view.data[group].filter((entry) => entry.id !== item.id),
    });
  }
  async function toggleReveal(variable: Definition, environment: string | null) {
    const key = slotKey(variable.id, environment);
    if (key in revealed) {
      setRevealed((current) => {
        const next = { ...current };
        delete next[key];
        return next;
      });
      return;
    }
    const reply = await act({ kind: 'reveal', variable: variable.id, environment });
    if (reply) {
      setRevealed((current) => ({ ...current, [key]: reply.value }));
    }
  }
  async function clearValue(variable: Definition, environment: string | null, label: string) {
    if (
      !(await ask(
        environment
          ? `Remove the ${label} value of ${variable.name}? The default value applies again.`
          : serverOwned
            ? `Remove the default value of ${variable.name}? Shared values are removed for every project member; secret values only for your account.`
            : `Remove the default value of ${variable.name} from the OS credential store?`,
      ))
    ) {
      return;
    }
    await saveValue(variable.id, environment, null);
  }

  const environmentName = environments.find((item) => item.id === shown)?.name ?? '';
  const needle = filter.trim().toLowerCase();
  const variables = (view?.data.variables ?? []).filter(
    (item) => !needle || item.name.toLowerCase().includes(needle),
  );

  function popup(label: string, secret: boolean) {
    return (
      <form
        ref={editor}
        className="cell-editor"
        aria-label={`Edit ${label}`}
        onSubmit={(event) => {
          event.preventDefault();
          void commitEditing();
        }}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            setEditing(null);
          }
        }}
      >
        <div className="cell-editor-field">
          <input
            aria-label={label}
            autoFocus
            type={secret && !draftVisible ? 'password' : 'text'}
            autoComplete="off"
            spellCheck={false}
            maxLength={editing?.kind === 'value' ? undefined : 200}
            placeholder={secret ? 'New secret value' : undefined}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
          />
          {secret && (
            <button
              type="button"
              className="cell-icon"
              aria-label={draftVisible ? 'Hide typed value' : 'Show typed value'}
              onClick={() => setDraftVisible((current) => !current)}
            >
              <Icon name={draftVisible ? 'eyeOff' : 'eye'} size={14} />
            </button>
          )}
        </div>
        <div className="cell-editor-actions">
          <button type="button" className="secondary-button" onClick={() => setEditing(null)}>
            Cancel
          </button>
          <button type="submit" className="secondary-button" disabled={busy}>
            Save
          </button>
        </div>
      </form>
    );
  }

  function valueCell(item: Definition, environment: string | null, cell: TableCell | undefined) {
    const secret = item.secret ?? true;
    const key = slotKey(item.id, environment);
    const label = environment ? environmentName : 'Default';
    const isRevealed = key in revealed;
    const isEditing =
      editing?.kind === 'value' &&
      editing.variable === item.id &&
      editing.environment === environment;
    let content;
    if (!cell) {
      content = <span className="cell-muted">…</span>;
    } else if (!cell.set) {
      content = environment ? (
        <span className="cell-muted" title="No own value: the default value applies.">
          ↳ Default
        </span>
      ) : (
        <span className="cell-muted">not set</span>
      );
    } else if (secret && !isRevealed) {
      content = <span className="cell-mask">••••••••</span>;
    } else {
      const text = secret ? (revealed[key] ?? '') : (cell.value ?? '');
      content = text ? (
        <span className="cell-value" title={text}>
          {text}
        </span>
      ) : (
        <span className="cell-muted">empty</span>
      );
    }
    return (
      <td className="value-cell">
        <div className="cell-line">
          {content}
          <span className="cell-tools">
            {secret && cell?.set && (
              <button
                type="button"
                className="cell-icon"
                aria-label={`${isRevealed ? 'Hide' : 'Show'} ${label} value of ${item.name}`}
                aria-pressed={isRevealed}
                disabled={busy}
                onClick={() => void toggleReveal(item, environment)}
              >
                <Icon name={isRevealed ? 'eyeOff' : 'eye'} size={14} />
              </button>
            )}
            <button
              type="button"
              className="cell-icon"
              aria-label={`Edit ${label} value of ${item.name}`}
              disabled={busy}
              onClick={() =>
                startEditing(
                  {
                    kind: 'value',
                    variable: item.id,
                    environment,
                    initial: secret ? (revealed[key] ?? '') : (cell?.value ?? ''),
                  },
                  !secret,
                )
              }
            >
              <Icon name="pencil" size={14} />
            </button>
            {cell?.set && (
              <button
                type="button"
                className="cell-icon"
                aria-label={`Remove ${label} value of ${item.name}`}
                disabled={busy}
                onClick={() => void clearValue(item, environment, label)}
              >
                <Icon name="close" size={14} />
              </button>
            )}
          </span>
        </div>
        {isEditing && popup(`${label} value of ${item.name}`, secret)}
      </td>
    );
  }

  return (
    <Modal label="Project variables and environments">
      <div className="environment-manager">
        <h2>Variables & environments</h2>
        <p>
          Use <code>{'{{variableName}}'}</code> in URLs, parameters, headers and text bodies. An
          environment value overrides the default.{' '}
          {serverOwned
            ? 'Ordinary values are shared with project members; secrets are encrypted for your account and need the unlocked vault.'
            : 'Values stay in your OS credential store.'}
        </p>
        {error && (
          <p role="alert" className="error-banner">
            {error}
          </p>
        )}
        {view?.selectionMissing && (
          <p role="alert">
            The previous environment was removed. Select Defaults or another environment in the
            project header.
          </p>
        )}
        {status && <p role="status">{status}</p>}
        <section className="environment-chips" aria-label="Environments">
          <h3>Environments</h3>
          <ul>
            {environments.map((item) => (
              <li key={item.id} className="environment-chip">
                <span>{item.name}</span>
                <button
                  type="button"
                  className="cell-icon"
                  aria-label={`Rename ${item.name}`}
                  disabled={busy}
                  onClick={() =>
                    startEditing({
                      kind: 'name',
                      group: 'environments',
                      id: item.id,
                      initial: item.name,
                    })
                  }
                >
                  <Icon name="pencil" size={13} />
                </button>
                <button
                  type="button"
                  className="cell-icon"
                  aria-label={`Remove ${item.name}`}
                  disabled={busy}
                  onClick={() => void remove(item, 'environments')}
                >
                  <Icon name="close" size={13} />
                </button>
                {editing?.kind === 'name' &&
                  editing.id === item.id &&
                  popup(`Name of environment ${item.name}`, false)}
              </li>
            ))}
            <li className="environment-chip add">
              <button
                type="button"
                className="secondary-button"
                disabled={busy || !view}
                onClick={() => startEditing({ kind: 'newEnvironment', initial: '' })}
              >
                <Icon name="plus" size={13} /> Add environment
              </button>
              {editing?.kind === 'newEnvironment' && popup('New environment name', false)}
            </li>
          </ul>
        </section>
        <section className="variables-section" aria-label="Variables">
          <div className="variables-toolbar">
            <h3>Variables</h3>
            <input
              type="search"
              aria-label="Filter variables"
              placeholder="Filter"
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
            />
            {environments.length > 0 && (
              <Select
                aria-label="Environment column"
                value={shown}
                disabled={busy}
                onValueChange={(selected) => setColumn(selected)}
              >
                {environments.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name}
                  </option>
                ))}
              </Select>
            )}
          </div>
          <div className="variables-table-scroll">
            <table className="variables-table">
              <thead>
                <tr>
                  <th scope="col">Variable</th>
                  <th scope="col">Default</th>
                  {shown && <th scope="col">{environmentName}</th>}
                </tr>
              </thead>
              <tbody>
                {variables.map((item) => {
                  const secret = item.secret ?? true;
                  const row = rows?.get(item.id);
                  return (
                    <tr key={item.id}>
                      <th scope="row" className="variable-cell">
                        <div className="cell-line">
                          <button
                            type="button"
                            className={`cell-icon secret-toggle${secret ? ' on' : ''}`}
                            aria-label={`Mark ${item.name} as ${secret ? 'ordinary' : 'secret'}`}
                            title={
                              secret
                                ? 'Secret – click to make ordinary'
                                : 'Ordinary – click to make secret'
                            }
                            disabled={busy}
                            onClick={() => void classify(item)}
                          >
                            <Icon name={secret ? 'lock' : 'unlock'} size={14} />
                          </button>
                          <span className="cell-value" title={item.name}>
                            {item.name}
                          </span>
                          <span className="cell-tools">
                            <button
                              type="button"
                              className="cell-icon"
                              aria-label={`Rename ${item.name}`}
                              disabled={busy}
                              onClick={() =>
                                startEditing({
                                  kind: 'name',
                                  group: 'variables',
                                  id: item.id,
                                  initial: item.name,
                                })
                              }
                            >
                              <Icon name="pencil" size={14} />
                            </button>
                            <button
                              type="button"
                              className="cell-icon row-only"
                              aria-label={`Remove ${item.name}`}
                              disabled={busy}
                              onClick={() => void remove(item, 'variables')}
                            >
                              <Icon name="trash" size={14} />
                            </button>
                          </span>
                        </div>
                        {editing?.kind === 'name' &&
                          editing.id === item.id &&
                          popup(`Name of ${item.name}`, false)}
                      </th>
                      {valueCell(item, null, row?.default)}
                      {shown && valueCell(item, shown, row?.environment)}
                    </tr>
                  );
                })}
                {view && variables.length === 0 && (
                  <tr>
                    <td colSpan={shown ? 3 : 2} className="cell-muted">
                      {needle ? 'No variables match the filter.' : 'No variables yet.'}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
          <form
            className="add-variable"
            onSubmit={(event) => {
              event.preventDefault();
              void addVariable();
            }}
          >
            <input
              aria-label="New variable name"
              placeholder="New variable, e.g. baseUrl"
              maxLength={200}
              value={newName}
              disabled={busy || !view}
              onChange={(event) => setNewName(event.target.value)}
            />
            <label>
              <input
                type="checkbox"
                checked={newSecret}
                disabled={busy || !view}
                onChange={(event) => setNewSecret(event.target.checked)}
              />
              Secret
            </label>
            <button
              type="submit"
              className="secondary-button"
              disabled={busy || !view || !newName.trim()}
            >
              <Icon name="plus" size={13} /> Add variable
            </button>
          </form>
        </section>
        <div className="dialog-actions">
          {!view && (
            <button
              className="secondary-button"
              disabled={busy}
              onClick={() => void act({ kind: 'select', environment: null })}
            >
              Use Defaults
            </button>
          )}
          <button
            className="secondary-button"
            disabled={busy}
            onClick={() => {
              void (async () => {
                if (dirty && !(await ask('Discard unsaved variable edits and reload?'))) {
                  return;
                }
                setEditing(null);
                setNewName('');
                setRevealed({});
                await act({ kind: 'load' });
                await loadTable();
              })();
            }}
          >
            Reload
          </button>
          <button
            className="primary-button"
            disabled={busy}
            onClick={() => {
              void (async () => {
                if (!dirty || (await ask('Discard unsaved variable edits and close?'))) {
                  onClose();
                }
              })();
            }}
          >
            Done
          </button>
        </div>
      </div>
    </Modal>
  );
}

/**
 * Tells what keeps referring to a variable after it is deleted: saved requests by `{{name}}` and
 * authentication settings. Empty when nothing uses it.
 */
function usageWarning(name: string, usages: { requests: string[]; authentication: string[] }) {
  const list = (items: string[]) =>
    items.slice(0, 5).join(', ') + (items.length > 5 ? ` and ${items.length - 5} more` : '');
  const parts: string[] = [];
  if (usages.requests.length) {
    parts.push(
      `${usages.requests.length} saved ${usages.requests.length === 1 ? 'request' : 'requests'} (${list(usages.requests)})`,
    );
  }
  if (usages.authentication.length) {
    parts.push(
      `${usages.authentication.length} authentication ${usages.authentication.length === 1 ? 'setting' : 'settings'} (${list(usages.authentication)})`,
    );
  }
  if (!parts.length) {
    return '';
  }
  return `${name} is still used in ${parts.join(' and ')}. Those requests stop with “Unknown variable {{${name}}}” and authentication asks for another variable until you replace it. `;
}

/** The server revision of one stored value; 0 means the value does not exist there yet. */
function slotRevision(
  values: { slots: Record<string, number> },
  variable: string,
  environment: string | null | undefined,
) {
  return values.slots[`${variable}/${environment || 'default'}`] ?? 0;
}
