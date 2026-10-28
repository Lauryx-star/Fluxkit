import { Select } from '../ui/Select';
import { useEffect, useState } from 'react';
import { environmentAction } from '../native';
import { Modal } from '../projects/Dialogs';
import type { Definition, Definitions, EnvironmentAction, EnvironmentView } from './types';

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
  const [kind, setKind] = useState<'variables' | 'environments'>('variables');
  const [name, setName] = useState('');
  const [secret, setSecret] = useState(true);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [variable, setVariable] = useState('');
  const [environment, setEnvironment] = useState(view?.selectedEnvironment ?? '');
  const [value, setValue] = useState('');
  const [valueDirty, setValueDirty] = useState(false);
  const [visible, setVisible] = useState(true);
  const [status, setStatus] = useState('Value not loaded.');
  const dirty = valueDirty || name.length > 0;
  useEffect(() => {
    onDirty(dirty);
  }, [dirty, onDirty]);
  async function discardValue() {
    if (valueDirty && !(await ask('Discard the unsaved variable value?'))) {
      return false;
    }
    setValue('');
    setValueDirty(false);
    setVisible(true);
    setStatus('Value not loaded.');
    return true;
  }
  async function saveDefinitions(data: Definitions) {
    if (!view) {
      return false;
    }
    return Boolean(await act({ kind: 'save', data, expected: view.revision }));
  }
  async function saveName() {
    if (!view || !name.trim()) {
      setStatus('Enter a non-empty definition name before saving.');
      return null;
    }
    const entries = renaming
      ? view.data[kind].map((item) =>
          item.id === renaming ? { ...item, name: name.trim() } : item,
        )
      : [
          ...view.data[kind],
          {
            id: crypto.randomUUID(),
            name: name.trim(),
            ...(kind === 'variables' ? { secret } : {}),
          },
        ];
    const reply = await act({
      kind: 'save',
      data: { ...view.data, [kind]: entries },
      expected: view.revision,
    });
    if (reply) {
      setName('');
      setSecret(true);
      setRenaming(null);
    }
    return reply;
  }
  async function classify(item: Definition) {
    if (!view) {
      return;
    }
    const ordinary = item.secret ?? true;
    if (
      !(await ask(
        ordinary
          ? `Mark ${item.name} as ordinary? Values explicitly transferred or saved in server mode will be visible to every project member. Existing local values are not uploaded by this change.`
          : `Mark ${item.name} as secret? Remove synchronized values first. Future secret values will be encrypted for your account; previously shared values cannot be made private retrospectively.`,
      ))
    ) {
      return;
    }
    await saveDefinitions({
      ...view.data,
      variables: view.data.variables.map((entry) =>
        entry.id === item.id ? { ...entry, secret: !ordinary } : entry,
      ),
    });
  }
  async function saveValue(expected = view?.revision) {
    if (!variable || !expected) {
      return false;
    }
    const reply = await act({
      kind: 'setValue',
      variable,
      environment: environment || null,
      value,
      expected,
      ...(view?.serverValues
        ? { expectedValueRevision: slotRevision(view.serverValues, variable, environment) }
        : {}),
    });
    if (reply) {
      setValue('');
      setVisible(true);
      setValueDirty(false);
      setStatus(
        serverOwned
          ? 'Saved on the server. Secrets are encrypted for your account; ordinary values are shared.'
          : 'Saved in OS credential store.',
      );
    }
    return !!reply;
  }
  // Register the current editor snapshot; nothing is saved or revealed until explicitly requested.
  useEffect(() => {
    onRegisterSave?.(async () => {
      if (!dirty) {
        return true;
      }
      if (busy || !view) {
        return false;
      }
      let revision = view.revision;
      if (name.length) {
        const reply = await saveName();
        if (!reply) {
          return false;
        }
        revision = reply.view.revision;
      }
      return !valueDirty || (await saveValue(revision));
    });
    return () => onRegisterSave?.(null);
  });
  async function remove(item: Definition, group: typeof kind) {
    if (
      !view ||
      !(await ask(
        serverOwned
          ? `Remove ${item.name}? Remove its synchronized values explicitly first. Definitions are published on the next project sync; other users’ encrypted values remain retained.`
          : `Remove ${item.name} and delete its associated values from this machine's OS credential store? This cannot be undone. Recovery copies contain definitions only. If deletion fails, some values may already be removed; retry the removal.`,
      ))
    ) {
      return;
    }
    if (!(await discardValue())) {
      return;
    }
    if (
      await saveDefinitions({
        ...view.data,
        [group]: view.data[group].filter((entry) => entry.id !== item.id),
      })
    ) {
      if (group === 'variables' && variable === item.id) {
        setVariable('');
      }
      if (group === 'environments' && environment === item.id) {
        setEnvironment('');
      }
      if (renaming === item.id) {
        setName('');
        setRenaming(null);
      }
    }
  }
  return (
    <Modal label="Project variables and environments">
      <div className="environment-manager">
        <h2>Project variables & environments</h2>
        <p>
          {serverOwned
            ? 'Sync new definitions before saving server values. Ordinary values are shared with members; secrets need the unlocked account vault in Server connection. Use Reload to resolve value conflicts.'
            : 'Shared by every collection in this project. Values stay in your OS credential store.'}
        </p>
        <p>
          Use <code>{'{{variableName}}'}</code> in URLs, parameters, headers, and text bodies.
          Environment values override defaults. Empty values stay empty.
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
        <fieldset disabled={busy}>
          <legend>Definitions</legend>
          <div className="definition-columns">
            {(['environments', 'variables'] as const).map((group) => (
              <section key={group}>
                <h3>{group === 'variables' ? 'Variables' : 'Environments'}</h3>
                <ul className="definition-list">
                  {view?.data[group].map((item) => (
                    <li key={item.id}>
                      <span>
                        {item.name}
                        {group === 'variables' && (
                          <small> · {(item.secret ?? true) ? 'Secret' : 'Ordinary value'}</small>
                        )}
                      </span>
                      {group === 'variables' && (
                        <button
                          aria-label={`Mark ${item.name} as ${(item.secret ?? true) ? 'ordinary' : 'secret'}`}
                          onClick={() => void classify(item)}
                        >
                          {(item.secret ?? true) ? 'Make ordinary' : 'Make secret'}
                        </button>
                      )}
                      <button
                        aria-label={`Rename ${item.name}`}
                        onClick={() => {
                          setKind(group);
                          setName(item.name);
                          setRenaming(item.id);
                        }}
                      >
                        Rename
                      </button>
                      <button
                        aria-label={`Remove ${item.name}`}
                        onClick={() => void remove(item, group)}
                      >
                        Remove
                      </button>
                    </li>
                  ))}
                </ul>
                {view?.data[group].length === 0 && <p>No {group} yet.</p>}
              </section>
            ))}
          </div>
          <form
            className="definition-form"
            onSubmit={(event) => {
              event.preventDefault();
              void saveName();
            }}
          >
            <Select
              aria-label="Definition type"
              value={kind}
              disabled={renaming !== null}
              onValueChange={(selectedValue) => setKind(selectedValue as typeof kind)}
            >
              <option value="variables">Variable</option>
              <option value="environments">Environment</option>
            </Select>
            <input
              aria-label="Definition name"
              placeholder={kind === 'variables' ? 'baseUrl' : 'Testserver'}
              maxLength={200}
              required
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
            {kind === 'variables' && !renaming && (
              <label>
                <input
                  type="checkbox"
                  checked={secret}
                  onChange={(event) => setSecret(event.target.checked)}
                />
                {serverOwned
                  ? 'Secret (encrypted for your account)'
                  : 'Secret (stored only in this user’s OS credential store)'}
              </label>
            )}
            <button type="submit" className="secondary-button" disabled={!view || !name.trim()}>
              {renaming ? 'Save name' : 'Add definition'}
            </button>
            {renaming && (
              <button
                type="button"
                onClick={() => {
                  setRenaming(null);
                  setName('');
                }}
              >
                Cancel rename
              </button>
            )}
          </form>
        </fieldset>
        <fieldset disabled={busy || !view}>
          <legend>Local value</legend>
          <div className="definition-form">
            <Select
              aria-label="Value variable"
              value={variable}
              onValueChange={(selectedValue) => {
                const next = selectedValue;
                void discardValue().then((yes) => {
                  if (yes) {
                    setVariable(next);
                  }
                });
              }}
            >
              <option value="">Choose a variable</option>
              {view?.data.variables.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
            </Select>
            <Select
              aria-label="Value environment"
              value={environment}
              onValueChange={(selectedValue) => {
                const next = selectedValue;
                void discardValue().then((yes) => {
                  if (yes) {
                    setEnvironment(next);
                  }
                });
              }}
            >
              <option value="">Shared default</option>
              {view?.data.environments.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
            </Select>
          </div>
          <label className="dialog-label">
            Value
            <input
              aria-label="Variable value"
              type={visible ? 'text' : 'password'}
              autoComplete="off"
              spellCheck={false}
              disabled={!variable}
              value={value}
              onChange={(event) => {
                setValue(event.target.value);
                setValueDirty(true);
                setStatus('Unsaved value.');
              }}
            />
          </label>
          <p role="status">{status}</p>
          <div className="value-actions">
            <button
              disabled={!variable}
              aria-pressed={!visible}
              onClick={() => setVisible((current) => !current)}
            >
              {visible ? 'Hide value' : 'Show value'}
            </button>
            <button
              disabled={!variable}
              onClick={() => {
                void (async () => {
                  if (!(await discardValue())) {
                    return;
                  }
                  const reply = await act({
                    kind: 'reveal',
                    variable,
                    environment: environment || null,
                  });
                  if (reply) {
                    setValue(reply.value ?? '');
                    setVisible(true);
                    setStatus(
                      reply.value === null
                        ? 'No value for this slot. Execution falls back to the shared default.'
                        : 'Stored value revealed.',
                    );
                  }
                })();
              }}
            >
              Load & reveal
            </button>
            <button disabled={!variable || !valueDirty} onClick={() => void saveValue()}>
              Save value
            </button>
            <button
              disabled={!variable}
              onClick={() => {
                void (async () => {
                  if (
                    !view ||
                    !(await ask(
                      serverOwned
                        ? 'Delete this synchronized slot value? Shared values are removed for every project member; secret values only for your account. An environment slot will fall back to its default.'
                        : 'Delete this slot value from the OS credential store? An environment slot will fall back to its shared default.',
                    ))
                  ) {
                    return;
                  }
                  const reply = await act({
                    kind: 'setValue',
                    variable,
                    environment: environment || null,
                    value: null,
                    ...(view?.serverValues
                      ? {
                          expectedValueRevision: slotRevision(
                            view.serverValues,
                            variable,
                            environment,
                          ),
                        }
                      : {}),
                    expected: view.revision,
                  });
                  if (reply) {
                    setValue('');
                    setValueDirty(false);
                    setVisible(true);
                    setStatus('Slot value removed.');
                  }
                })();
              }}
            >
              Remove value
            </button>
            <button disabled={!value && !valueDirty} onClick={() => void discardValue()}>
              Clear editor
            </button>
          </div>
        </fieldset>
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
                if (
                  dirty &&
                  !(await ask(
                    'Discard unsaved variable or definition edits and reload environments?',
                  ))
                ) {
                  return;
                }
                if (await act({ kind: 'load' })) {
                  setValue('');
                  setValueDirty(false);
                  setVisible(true);
                  setName('');
                  setRenaming(null);
                  setVariable('');
                  setEnvironment('');
                  setStatus('Value not loaded.');
                }
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
                if (
                  !dirty ||
                  (await ask('Discard unsaved variable or definition edits and close?'))
                ) {
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

/** The server revision of one stored value; 0 means the value does not exist there yet. */
function slotRevision(
  values: { slots: Record<string, number> },
  variable: string,
  environment: string | null | undefined,
) {
  return values.slots[`${variable}/${environment || 'default'}`] ?? 0;
}
