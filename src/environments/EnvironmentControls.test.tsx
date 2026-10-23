import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import { EnvironmentControls } from './EnvironmentControls';
import { environmentAction } from '../native';
import type { EnvironmentView } from './types';

vi.mock('../native', () => ({ environmentAction: vi.fn() }));
let view: EnvironmentView;
let values: Map<string, string>;
const ask = vi.fn<() => Promise<boolean>>();
const onDirty = vi.fn();
const onOpen = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  ask.mockResolvedValue(true);
  values = new Map();
  view = {
    revision: '1',
    data: {
      schemaVersion: 1,
      environments: [{ id: 'env', name: 'Testserver' }],
      variables: [{ id: 'var', name: 'baseUrl' }],
    },
    selectedEnvironment: null,
    selectionMissing: false,
  };
  vi.mocked(environmentAction).mockImplementation(async (_handle, action) => {
    let value: string | null = null;
    if (action.kind === 'save') {
      view = { ...view, data: action.data, revision: '2' };
    }
    if (action.kind === 'select') {
      view = { ...view, selectedEnvironment: action.environment };
    }
    if (action.kind === 'setValue') {
      const key = `${action.variable}/${action.environment}`;
      if (action.value === null) {
        values.delete(key);
      } else {
        values.set(key, action.value);
      }
    }
    if (action.kind === 'reveal') {
      value = values.get(`${action.variable}/${action.environment}`) ?? null;
    }
    return { view: structuredClone(view), value };
  });
});
async function open() {
  const user = userEvent.setup();
  render(<EnvironmentControls handle="project" ask={ask} onDirty={onDirty} onOpen={onOpen} />);
  await screen.findByRole('option', { name: 'Testserver' });
  await user.click(screen.getByRole('button', { name: 'Variables' }));
  return user;
}
it('shows new input and lets users mask it without loading or changing stored values', async () => {
  const user = await open();
  await user.selectOptions(screen.getByLabelText('Value variable'), 'var');
  const input = screen.getByLabelText('Variable value');
  expect(input).toHaveAttribute('type', 'text');
  await user.type(input, 'https://example.test');
  await user.click(screen.getByRole('button', { name: 'Hide value' }));
  expect(input).toHaveAttribute('type', 'password');
  expect(input).toHaveValue('https://example.test');
  await user.click(screen.getByRole('button', { name: 'Show value' }));
  expect(input).toHaveAttribute('type', 'text');
  expect(environmentAction).toHaveBeenCalledTimes(1);
  expect(values.size).toBe(0);
  await user.click(screen.getByRole('button', { name: 'Hide value' }));
  await user.selectOptions(screen.getByLabelText('Value environment'), 'env');
  expect(input).toHaveValue('');
  expect(input).toHaveAttribute('type', 'text');
});
it('loads only definitions, explicitly saves a value and reveals it on demand', async () => {
  values.set('var/null', 'synthetic-private');
  const user = await open();
  expect(environmentAction).toHaveBeenCalledTimes(1);
  await user.selectOptions(screen.getByLabelText('Value variable'), 'var');
  expect(screen.getByLabelText('Variable value')).toHaveValue('');
  await user.click(screen.getByRole('button', { name: 'Load & reveal' }));
  expect(await screen.findByLabelText('Variable value')).toHaveValue('synthetic-private');
  await user.click(screen.getByRole('button', { name: 'Hide value' }));
  expect(screen.getByLabelText('Variable value')).toHaveAttribute('type', 'password');
  await user.click(screen.getByRole('button', { name: 'Show value' }));
  await user.clear(screen.getByLabelText('Variable value'));
  await user.type(screen.getByLabelText('Variable value'), 'replacement');
  await user.click(screen.getByRole('button', { name: 'Save value' }));
  await screen.findByText('Saved in OS credential store.');
  expect(screen.getByLabelText('Variable value')).toHaveValue('');
  expect(screen.getByLabelText('Variable value')).toHaveAttribute('type', 'text');
  expect(values.get('var/null')).toBe('replacement');
  expect(environmentAction).not.toHaveBeenCalledWith(
    'project',
    expect.objectContaining({ kind: 'save' }),
  );
});
it('confirms removal of definitions and OS values and preserves edits when cancelled', async () => {
  const user = await open();
  ask.mockResolvedValueOnce(false);
  await user.click(screen.getByRole('button', { name: 'Remove baseUrl' }));
  expect(ask).toHaveBeenCalledWith(expect.stringContaining("this machine's OS credential store"));
  expect(view.data.variables).toHaveLength(1);
  await user.selectOptions(screen.getByLabelText('Value variable'), 'var');
  await user.type(screen.getByLabelText('Variable value'), 'unsaved');
  ask.mockResolvedValueOnce(false);
  await user.click(screen.getByRole('button', { name: 'Done' }));
  expect(screen.getByLabelText('Variable value')).toHaveValue('unsaved');
  await user.click(screen.getByRole('button', { name: 'Remove baseUrl' }));
  await waitFor(() => expect(view.data.variables).toHaveLength(0));
});
it('keeps input after credential-store failure and changes the project-wide selection', async () => {
  const user = await open();
  await user.selectOptions(screen.getByLabelText('Value variable'), 'var');
  await user.type(screen.getByLabelText('Variable value'), 'keep-me');
  vi.mocked(environmentAction).mockRejectedValueOnce('Unlock the OS credential store and retry.');
  await user.click(screen.getByRole('button', { name: 'Save value' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Unlock the OS credential store');
  expect(screen.getByLabelText('Variable value')).toHaveValue('keep-me');
  await user.click(screen.getByRole('button', { name: 'Done' }));
  await user.selectOptions(screen.getByLabelText('Project environment'), 'env');
  expect(environmentAction).toHaveBeenLastCalledWith('project', {
    kind: 'select',
    environment: 'env',
  });
});

it('lets users explicitly reset an environment removed outside the app', async () => {
  view.selectionMissing = true;
  const user = await open();
  expect(screen.getByLabelText('Project environment')).toHaveValue('__missing');
  await user.click(screen.getByRole('button', { name: 'Done' }));
  await user.selectOptions(screen.getByLabelText('Project environment'), '');
  expect(environmentAction).toHaveBeenLastCalledWith('project', {
    kind: 'select',
    environment: null,
  });
});
