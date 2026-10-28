import { selectOption } from '../test/selectOption';
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import { EnvironmentControls } from './EnvironmentControls';
import { environmentAction } from '../native';
import type { EnvironmentView, TableCell } from './types';

vi.mock('../native', () => ({ environmentAction: vi.fn() }));
let view: EnvironmentView;
let values: Map<string, string>;
let usages: { requests: string[]; authentication: string[] };
const ask = vi.fn<() => Promise<boolean>>();
const onDirty = vi.fn();
const onOpen = vi.fn();
const slot = (variable: string, environment: string | null) =>
  `${variable}/${environment ?? 'default'}`;

beforeEach(() => {
  vi.clearAllMocks();
  ask.mockResolvedValue(true);
  values = new Map();
  usages = { requests: [], authentication: [] };
  view = {
    revision: '1',
    data: {
      schemaVersion: 1,
      environments: [
        { id: 'test', name: 'Testserver' },
        { id: 'prod', name: 'Prodserver' },
      ],
      variables: [
        { id: 'url', name: 'baseUrl', secret: false },
        { id: 'token', name: 'apiToken', secret: true },
        { id: 'legacy', name: 'legacyValue' },
      ],
    },
    selectedEnvironment: 'test',
    selectionMissing: false,
  };
  vi.mocked(environmentAction).mockImplementation(async (_handle, action) => {
    let value: string | null = null;
    if (action.kind === 'save') {
      view = { ...view, data: action.data, revision: String(Number(view.revision) + 1) };
    }
    if (action.kind === 'select') {
      view = { ...view, selectedEnvironment: action.environment };
    }
    if (action.kind === 'setValue') {
      const key = slot(action.variable, action.environment);
      if (action.value === null) {
        values.delete(key);
      } else {
        values.set(key, action.value);
      }
    }
    if (action.kind === 'usages') {
      return { view: structuredClone(view), value: null, usages: structuredClone(usages) };
    }
    if (action.kind === 'reveal') {
      value = values.get(slot(action.variable, action.environment)) ?? null;
    }
    if (action.kind === 'table') {
      const cell = (variable: { id: string; secret?: boolean }, environment: string | null) => {
        const stored = values.get(slot(variable.id, environment));
        const result: TableCell = { set: stored !== undefined };
        if (stored !== undefined && variable.secret === false) {
          result.value = stored;
        }
        return result;
      };
      return {
        view: structuredClone(view),
        value: null,
        table: view.data.variables.map((variable) => ({
          variable: variable.id,
          default: cell(variable, null),
          ...(action.environment ? { environment: cell(variable, action.environment) } : {}),
        })),
      };
    }
    return { view: structuredClone(view), value };
  });
});

async function open(serverOwned = false) {
  const user = userEvent.setup();
  render(
    <EnvironmentControls
      handle="project"
      serverOwned={serverOwned}
      ask={ask}
      onDirty={onDirty}
      onOpen={onOpen}
    />,
  );
  await waitFor(() => expect(screen.getByLabelText('Project environment')).toBeEnabled());
  await user.click(screen.getByRole('button', { name: 'Variables' }));
  await screen.findByRole('table');
  return user;
}
function row(name: string) {
  return screen.getByRole('rowheader', { name: new RegExp(name) }).closest('tr')!;
}
async function edit(
  user: ReturnType<typeof userEvent.setup>,
  button: string,
  field: string,
  text: string,
) {
  await user.click(screen.getByRole('button', { name: button }));
  const input = screen.getByLabelText(field);
  await user.clear(input);
  await user.type(input, text);
  return input;
}

it('shows ordinary values for the default and the chosen environment and masks secrets', async () => {
  values.set('url/default', 'https://synthetic.invalid');
  values.set('url/test', 'https://test.synthetic.invalid');
  values.set('token/default', 'synthetic-token');
  values.set('legacy/default', 'synthetic-legacy');
  await open();
  const headers = screen.getAllByRole('columnheader').map((cell) => cell.textContent);
  expect(headers).toEqual(['Variable', 'Default', 'Testserver']);
  await waitFor(() => expect(row('baseUrl')).toHaveTextContent('https://synthetic.invalid'));
  expect(row('baseUrl')).toHaveTextContent('https://test.synthetic.invalid');
  expect(row('apiToken')).toHaveTextContent('••••••••');
  expect(row('apiToken')).toHaveTextContent('↳ Default');
  expect(row('legacyValue')).toHaveTextContent('••••••••');
  expect(screen.getByRole('table')).not.toHaveTextContent('synthetic-token');
  expect(environmentAction).not.toHaveBeenCalledWith(
    'project',
    expect.objectContaining({ kind: 'reveal' }),
  );
});

it('reveals a secret on request and masks it again without reading it twice', async () => {
  values.set('token/default', 'synthetic-token');
  const user = await open();
  await user.click(await screen.findByRole('button', { name: 'Show Default value of apiToken' }));
  expect(await screen.findByText('synthetic-token')).toBeVisible();
  expect(environmentAction).toHaveBeenCalledWith('project', {
    kind: 'reveal',
    variable: 'token',
    environment: null,
  });
  await user.click(screen.getByRole('button', { name: 'Hide Default value of apiToken' }));
  expect(screen.queryByText('synthetic-token')).not.toBeInTheDocument();
  expect(
    vi.mocked(environmentAction).mock.calls.filter(([, action]) => action.kind === 'reveal'),
  ).toHaveLength(1);
});

it('saves a value from the pencil popup immediately and refreshes the table', async () => {
  const user = await open();
  await edit(
    user,
    'Edit Default value of baseUrl',
    'Default value of baseUrl',
    'https://new.invalid',
  );
  await user.keyboard('{Enter}');
  await waitFor(() => expect(values.get('url/default')).toBe('https://new.invalid'));
  expect(await screen.findByRole('status')).toHaveTextContent('Saved in the OS credential store');
  await waitFor(() => expect(row('baseUrl')).toHaveTextContent('https://new.invalid'));
  expect(screen.queryByLabelText('Default value of baseUrl')).not.toBeInTheDocument();
  await edit(user, 'Edit Testserver value of baseUrl', 'Testserver value of baseUrl', 'override');
  await user.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(values.get('url/test')).toBe('override'));
});

it('falls back to the default in the environment column and removes an override after asking', async () => {
  values.set('url/default', 'https://synthetic.invalid');
  values.set('url/test', 'https://test.synthetic.invalid');
  const user = await open();
  await waitFor(() => expect(row('baseUrl')).toHaveTextContent('https://test.synthetic.invalid'));
  ask.mockResolvedValueOnce(false);
  await user.click(screen.getByRole('button', { name: 'Remove Testserver value of baseUrl' }));
  expect(values.has('url/test')).toBe(true);
  await user.click(screen.getByRole('button', { name: 'Remove Testserver value of baseUrl' }));
  await waitFor(() => expect(values.has('url/test')).toBe(false));
  await waitFor(() => expect(row('baseUrl')).toHaveTextContent('↳ Default'));
  expect(values.get('url/default')).toBe('https://synthetic.invalid');
});

it('switches the environment column without changing the active project environment', async () => {
  values.set('url/prod', 'https://prod.synthetic.invalid');
  const user = await open();
  await selectOption(user, screen.getByLabelText('Environment column'), 'prod');
  await waitFor(() =>
    expect(screen.getAllByRole('columnheader')[2]).toHaveTextContent('Prodserver'),
  );
  await waitFor(() => expect(row('baseUrl')).toHaveTextContent('https://prod.synthetic.invalid'));
  expect(environmentAction).toHaveBeenCalledWith('project', { kind: 'table', environment: 'prod' });
  expect(environmentAction).not.toHaveBeenCalledWith(
    'project',
    expect.objectContaining({ kind: 'select' }),
  );
});

it('adds variables as ordinary unless Secret is checked', async () => {
  const user = await open();
  expect(screen.getByRole('checkbox', { name: 'Secret' })).not.toBeChecked();
  await user.type(screen.getByLabelText('New variable name'), 'region');
  await user.click(screen.getByRole('button', { name: 'Add variable' }));
  await waitFor(() =>
    expect(view.data.variables.find((item) => item.name === 'region')?.secret).toBe(false),
  );
  expect(screen.getByLabelText('New variable name')).toHaveValue('');
  await user.type(screen.getByLabelText('New variable name'), 'password');
  await user.click(screen.getByRole('checkbox', { name: 'Secret' }));
  await user.click(screen.getByRole('button', { name: 'Add variable' }));
  await waitFor(() =>
    expect(view.data.variables.find((item) => item.name === 'password')?.secret).toBe(true),
  );
  expect(screen.getByRole('checkbox', { name: 'Secret' })).not.toBeChecked();
});

it('renames a variable in place, keeping its identity, and Escape cancels the popup', async () => {
  const user = await open();
  await edit(user, 'Rename baseUrl', 'Name of baseUrl', 'apiBase');
  await user.keyboard('{Escape}');
  expect(view.data.variables[0]!.name).toBe('baseUrl');
  await edit(user, 'Rename baseUrl', 'Name of baseUrl', 'apiBase');
  await user.keyboard('{Enter}');
  await waitFor(() =>
    expect(view.data.variables[0]).toEqual({ id: 'url', name: 'apiBase', secret: false }),
  );
});

it('changes the classification and removes variables only after confirmation', async () => {
  values.set('legacy/default', 'Synthetic local secret');
  const user = await open();
  ask.mockResolvedValueOnce(false);
  await user.click(screen.getByRole('button', { name: 'Mark legacyValue as ordinary' }));
  expect(view.data.variables[2]!.secret).toBeUndefined();
  await user.click(screen.getByRole('button', { name: 'Mark legacyValue as ordinary' }));
  await waitFor(() => expect(view.data.variables[2]!.secret).toBe(false));
  expect(values.get('legacy/default')).toBe('Synthetic local secret');
  ask.mockResolvedValueOnce(false);
  await user.click(screen.getByRole('button', { name: 'Remove baseUrl' }));
  expect(ask).toHaveBeenLastCalledWith(
    expect.stringContaining("this machine's OS credential store"),
  );
  expect(view.data.variables).toHaveLength(3);
  await user.click(screen.getByRole('button', { name: 'Remove baseUrl' }));
  await waitFor(() => expect(view.data.variables).toHaveLength(2));
});

it('adds, renames and removes environments from the compact list', async () => {
  const user = await open();
  const list = within(screen.getByRole('region', { name: 'Environments' }));
  await user.click(list.getByRole('button', { name: 'Add environment' }));
  await user.type(screen.getByLabelText('New environment name'), 'Local');
  await user.keyboard('{Enter}');
  await waitFor(() => expect(view.data.environments.map((item) => item.name)).toContain('Local'));
  await edit(user, 'Rename Prodserver', 'Name of environment Prodserver', 'Production');
  await user.keyboard('{Enter}');
  await waitFor(() =>
    expect(view.data.environments[1]).toEqual({ id: 'prod', name: 'Production' }),
  );
  await user.click(list.getByRole('button', { name: 'Remove Production' }));
  await waitFor(() => expect(view.data.environments.map((item) => item.id)).not.toContain('prod'));
});

it('keeps the popup input after a credential-store failure', async () => {
  const user = await open();
  const implementation = vi.mocked(environmentAction).getMockImplementation()!;
  vi.mocked(environmentAction).mockImplementation(async (handle, action) => {
    if (action.kind === 'setValue') {
      throw 'Unlock the OS credential store and retry.';
    }
    return implementation(handle, action);
  });
  await edit(user, 'Edit Default value of baseUrl', 'Default value of baseUrl', 'keep-me');
  await user.keyboard('{Enter}');
  expect(await screen.findByRole('alert')).toHaveTextContent('Unlock the OS credential store');
  expect(screen.getByLabelText('Default value of baseUrl')).toHaveValue('keep-me');
  expect(onDirty).toHaveBeenLastCalledWith(true);
});

it('submits the revision of the edited server slot', async () => {
  view.serverValues = { slots: { 'url/test': 7, 'url/default': 3 } };
  const user = await open(true);
  await edit(user, 'Edit Testserver value of baseUrl', 'Testserver value of baseUrl', 'shared');
  await user.keyboard('{Enter}');
  await waitFor(() =>
    expect(environmentAction).toHaveBeenCalledWith(
      'project',
      expect.objectContaining({ kind: 'setValue', environment: 'test', expectedValueRevision: 7 }),
    ),
  );
  await edit(user, 'Edit Testserver value of apiToken', 'Testserver value of apiToken', 'mine');
  await user.keyboard('{Enter}');
  await waitFor(() =>
    expect(environmentAction).toHaveBeenCalledWith(
      'project',
      expect.objectContaining({ variable: 'token', expectedValueRevision: 0 }),
    ),
  );
});

it('saves an open popup and a typed new variable for Save & quit', async () => {
  const user = userEvent.setup();
  let save: (() => Promise<boolean>) | null = null;
  render(
    <EnvironmentControls
      handle="project"
      ask={ask}
      onDirty={onDirty}
      onOpen={onOpen}
      onRegisterSave={(callback) => {
        save = callback;
      }}
    />,
  );
  await waitFor(() => expect(screen.getByLabelText('Project environment')).toBeEnabled());
  await user.click(screen.getByRole('button', { name: 'Variables' }));
  await screen.findByRole('table');
  await edit(user, 'Edit Default value of baseUrl', 'Default value of baseUrl', 'synthetic-value');
  await user.type(screen.getByLabelText('New variable name'), 'otherVariable');
  let saved = false;
  await act(async () => {
    saved = await save!();
  });
  expect(saved).toBe(true);
  expect(values.get('url/default')).toBe('synthetic-value');
  expect(environmentAction).toHaveBeenCalledWith(
    'project',
    expect.objectContaining({ kind: 'setValue', expected: '2' }),
  );
  expect(view.data.variables.some((variable) => variable.name === 'otherVariable')).toBe(true);
});

it('changes the project-wide selection from the header', async () => {
  const user = await open();
  await user.click(screen.getByRole('button', { name: 'Done' }));
  await selectOption(user, screen.getByLabelText('Project environment'), 'prod');
  expect(environmentAction).toHaveBeenLastCalledWith('project', {
    kind: 'select',
    environment: 'prod',
  });
});

it('lets users explicitly reset an environment removed outside the app', async () => {
  view.selectionMissing = true;
  view.selectedEnvironment = 'gone';
  const user = await open();
  expect(screen.getByLabelText('Project environment')).toHaveValue('__missing');
  await user.click(screen.getByRole('button', { name: 'Done' }));
  await selectOption(user, screen.getByLabelText('Project environment'), '');
  expect(environmentAction).toHaveBeenLastCalledWith('project', {
    kind: 'select',
    environment: null,
  });
});

it('warns where a variable is still used and deletes it once confirmed', async () => {
  usages = {
    requests: ['Synthetic API › List items', 'Synthetic API › Get item'],
    authentication: ['Collection Synthetic API'],
  };
  const user = await open();
  ask.mockResolvedValueOnce(false);
  await user.click(screen.getByRole('button', { name: 'Remove baseUrl' }));
  await waitFor(() =>
    expect(ask).toHaveBeenLastCalledWith(
      expect.stringContaining(
        'baseUrl is still used in 2 saved requests (Synthetic API › List items, Synthetic API › Get item) and 1 authentication setting (Collection Synthetic API).',
      ),
    ),
  );
  expect(ask).toHaveBeenLastCalledWith(expect.stringContaining('Unknown variable {{baseUrl}}'));
  expect(view.data.variables).toHaveLength(3);
  await user.click(screen.getByRole('button', { name: 'Remove baseUrl' }));
  await waitFor(() => expect(view.data.variables).toHaveLength(2));
  expect(environmentAction).toHaveBeenCalledWith('project', { kind: 'usages', variable: 'url' });
});
