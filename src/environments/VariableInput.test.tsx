import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import { useState } from 'react';
import { VariableInput } from './VariableInput';
import { VariableContext } from './VariableContext';
import { environmentAction } from '../native';
import type { EnvironmentReply, EnvironmentView } from './types';

vi.mock('../native', () => ({ environmentAction: vi.fn() }));
const view: EnvironmentView = {
  revision: 'definitions-1',
  selectedEnvironment: 'test',
  selectionMissing: false,
  data: {
    schemaVersion: 1,
    environments: [{ id: 'test', name: 'Testserver' }],
    variables: [
      { id: 'url', name: 'baseUrl' },
      { id: 'token', name: 'apiToken' },
      { id: 'user', name: 'username' },
    ],
  },
};
const context = { handle: 'project', view };
function Editor({ initial = '', variant = 'url' }: { initial?: string; variant?: 'url' | 'body' }) {
  const [value, setValue] = useState(initial);
  return <VariableInput label="Template" value={value} onChange={setValue} variant={variant} />;
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(environmentAction).mockResolvedValue({ view, value: 'synthetic-preview' });
});
it('filters suggestions after opening braces and inserts by keyboard or mouse without reading values', async () => {
  const user = userEvent.setup();
  render(
    <VariableContext.Provider value={context}>
      <Editor />
    </VariableContext.Provider>,
  );
  const input = screen.getByRole('textbox', { name: 'Template' });
  await user.click(input);
  await user.paste('{{');
  expect(screen.getAllByRole('option')).toHaveLength(3);
  await user.keyboard('{ArrowDown}{Enter}');
  expect(input).toHaveValue('{{apiToken}}');
  expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  await user.clear(input);
  await user.paste('{{bas');
  expect(screen.getAllByRole('option')).toHaveLength(1);
  await user.click(screen.getByRole('option', { name: '{{baseUrl}}' }));
  expect(input).toHaveValue('{{baseUrl}}');
  expect(environmentAction).not.toHaveBeenCalled();
});
it('preserves existing closing braces, surrounding content and multiline input', async () => {
  const user = userEvent.setup();
  render(
    <VariableContext.Provider value={context}>
      <Editor initial={'prefix {{ba}}\nsuffix'} variant="body" />
    </VariableContext.Provider>,
  );
  const input = screen.getByRole('textbox', { name: 'Template' }) as HTMLTextAreaElement;
  await user.click(input);
  input.setSelectionRange(11, 11);
  fireEvent.select(input);
  await user.keyboard('{Tab}');
  expect(input).toHaveValue('prefix {{baseUrl}}\nsuffix');
});
it('closes suggestions with Escape and shows an empty result for unknown prefixes', async () => {
  const user = userEvent.setup();
  render(
    <VariableContext.Provider value={context}>
      <Editor />
    </VariableContext.Provider>,
  );
  const input = screen.getByRole('textbox', { name: 'Template' });
  await user.click(input);
  await user.paste('{{notDefined');
  expect(screen.getByText(/No matching variables/)).toBeVisible();
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
});
function pointAtToken(container: HTMLElement) {
  const token = container.querySelector<HTMLElement>('[data-variable="baseUrl"]');
  if (!token) {
    throw new Error('Expected highlighted token');
  }
  vi.spyOn(token, 'getClientRects').mockReturnValue([
    { left: 10, right: 100, top: 10, bottom: 30 },
  ] as unknown as DOMRectList);
  fireEvent.mouseMove(screen.getByRole('textbox', { name: 'Template' }), {
    clientX: 20,
    clientY: 20,
  });
}
it('highlights tokens and loads only the hovered value, then discards it on mouse leave', async () => {
  const { container } = render(
    <VariableContext.Provider value={context}>
      <Editor initial="{{baseUrl}}/{{unknown}}" />
    </VariableContext.Provider>,
  );
  expect(container.querySelector('.variable-known')).toHaveTextContent('{{baseUrl}}');
  expect(container.querySelector('.variable-unknown')).toHaveTextContent('{{unknown}}');
  expect(environmentAction).not.toHaveBeenCalled();
  pointAtToken(container);
  expect(await screen.findByRole('tooltip')).toHaveTextContent('synthetic-preview');
  expect(environmentAction).toHaveBeenCalledExactlyOnceWith('project', {
    kind: 'preview',
    variable: 'url',
    environment: 'test',
    expected: 'definitions-1',
  });
  fireEvent.mouseLeave(container.querySelector('.variable-field')!);
  expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
});
it('discards late values after changing project environment and displays store errors', async () => {
  let finish: ((reply: EnvironmentReply) => void) | undefined;
  vi.mocked(environmentAction).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const { container, rerender } = render(
    <VariableContext.Provider value={context}>
      <Editor initial="{{baseUrl}}" />
    </VariableContext.Provider>,
  );
  pointAtToken(container);
  expect(screen.getByRole('tooltip')).toHaveTextContent('Loading');
  const changed = { handle: 'project', view: { ...view, selectedEnvironment: null } };
  rerender(
    <VariableContext.Provider value={changed}>
      <Editor initial="{{baseUrl}}" />
    </VariableContext.Provider>,
  );
  await act(async () => {
    finish?.({ view, value: 'outdated-value' });
  });
  expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  vi.mocked(environmentAction).mockRejectedValueOnce('Unlock the OS credential store.');
  pointAtToken(container);
  await waitFor(() =>
    expect(screen.getByRole('tooltip')).toHaveTextContent('Unlock the OS credential store.'),
  );
  expect(screen.getByRole('tooltip')).not.toHaveTextContent('outdated-value');
});

it('bounds highlighted tokens while preserving all text in a large field', () => {
  const text = '{{baseUrl}} '.repeat(501);
  const { container } = render(
    <VariableContext.Provider value={context}>
      <Editor initial={text} variant="body" />
    </VariableContext.Provider>,
  );
  expect(container.querySelectorAll('[data-variable]')).toHaveLength(500);
  expect(screen.getByRole('textbox', { name: 'Template' })).toHaveValue(text);
  expect(screen.getByText(/limited to the first 500 placeholders/)).toBeVisible();
  expect(environmentAction).not.toHaveBeenCalled();
});
