import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import { AuthTest } from './AuthTest';
import { AuthDialog } from './AuthDialog';
import { HeaderInspector } from '../http/HeaderInspector';
import type { ExecutionInput } from '../http/types';
const bridge = vi.hoisted(() => ({
  beginHttpRequest: vi.fn(),
  cancelHttpRequest: vi.fn(),
  testAuthentication: vi.fn(),
  previewHttpHeaders: vi.fn(),
}));
vi.mock('../native', () => bridge);
const input: ExecutionInput = {
  request: {
    id: 'synthetic',
    name: 'Draft',
    folderId: null,
    method: 'POST',
    url: 'https://example.invalid/danger',
    headers: [{ id: 1, key: 'X-Original', value: 'literal', enabled: true }],
    params: [],
    body: { kind: 'json', text: '{"synthetic":true}', fields: [], file: null },
  },
  scope: null,
  timeoutMs: 60000,
  saveResponse: false,
};
beforeEach(() => {
  vi.resetAllMocks();
  bridge.beginHttpRequest.mockResolvedValue('reservation');
  bridge.cancelHttpRequest.mockResolvedValue(undefined);
  bridge.testAuthentication.mockResolvedValue({
    success: true,
    status: 200,
    message: 'Test endpoint returned 2xx.',
  });
});
it('tests only an explicitly chosen GET endpoint and reports HTTP rejection', async () => {
  const user = userEvent.setup();
  render(<AuthTest input={input} tokenOnly={false} />);
  expect(bridge.beginHttpRequest).not.toHaveBeenCalled();
  expect(screen.getByRole('button', { name: 'Test authentication' })).toBeDisabled();
  await user.type(screen.getByLabelText('Authentication test URL'), 'https://example.invalid/me');
  bridge.testAuthentication.mockResolvedValue({
    success: false,
    status: 401,
    message: 'Check credentials.',
  });
  await user.click(screen.getByRole('button', { name: 'Test authentication' }));
  expect(bridge.testAuthentication).toHaveBeenCalledWith(
    'reservation',
    expect.objectContaining({
      request: expect.objectContaining({
        method: 'GET',
        url: 'https://example.invalid/me',
        headers: [],
        params: [],
        body: { kind: 'none', text: '', fields: [], file: null },
      }),
    }),
    false,
  );
  expect(await screen.findByRole('status')).toHaveTextContent('Test failed · HTTP 401');
  expect(input.request.method).toBe('POST');
});
it('tests unsaved dialog authentication without saving settings', async () => {
  const user = userEvent.setup();
  const save = vi.fn();
  render(
    <AuthDialog
      title="Project auth"
      initial={null}
      root
      inherited="None"
      variables={[]}
      busy={false}
      error={null}
      onSave={save}
      onCancel={vi.fn()}
      onDirty={vi.fn()}
      testInput={input}
    />,
  );
  await user.type(screen.getByLabelText('Authentication test URL'), 'https://example.invalid/me');
  await user.click(screen.getByRole('button', { name: 'Test authentication' }));
  expect(await screen.findByRole('status')).toHaveTextContent('Test succeeded');
  expect(save).not.toHaveBeenCalled();
});
it('uses the token endpoint test, displays safe failures and never requests an API URL', async () => {
  const user = userEvent.setup();
  render(<AuthTest input={input} tokenOnly />);
  expect(screen.queryByLabelText('Authentication test URL')).not.toBeInTheDocument();
  bridge.testAuthentication.mockRejectedValue('Token extraction failed.');
  await user.click(screen.getByRole('button', { name: 'Test authentication' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Token extraction failed');
  expect(bridge.testAuthentication).toHaveBeenCalledWith('reservation', expect.any(Object), true);
});
it('cancels while registering without dispatching a late authentication test', async () => {
  const user = userEvent.setup();
  let finish: (id: string) => void = () => {};
  bridge.beginHttpRequest.mockImplementation(
    () =>
      new Promise<string>((resolve) => {
        finish = resolve;
      }),
  );
  render(<AuthTest input={input} tokenOnly />);
  await user.click(screen.getByRole('button', { name: 'Test authentication' }));
  await user.click(screen.getByRole('button', { name: 'Cancel authentication test' }));
  await act(async () => finish('late-reservation'));
  expect(bridge.testAuthentication).not.toHaveBeenCalled();
  expect(bridge.cancelHttpRequest).toHaveBeenCalledWith('late-reservation');
  expect(await screen.findByRole('alert')).toHaveTextContent('cancelled');
});
it('discards late results and cancels native work when its context closes', async () => {
  const user = userEvent.setup();
  let finish: (value: unknown) => void = () => {};
  bridge.testAuthentication.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const view = render(<AuthTest input={input} tokenOnly />);
  await user.click(screen.getByRole('button', { name: 'Test authentication' }));
  view.unmount();
  expect(bridge.cancelHttpRequest).toHaveBeenCalledWith('reservation');
  await act(async () => finish({ success: true, status: null, message: 'Late' }));
  expect(screen.queryByText('Late')).not.toBeInTheDocument();
});
it('inspects redacted headers explicitly and distinguishes last dispatch from the current draft', async () => {
  const user = userEvent.setup();
  bridge.previewHttpHeaders.mockResolvedValue([
    { name: 'authorization', value: '••••••••', source: 'authentication' },
    { name: 'user-agent', value: 'Fluxkit/0.1.0', source: 'automatic' },
  ]);
  render(
    <HeaderInspector
      input={input}
      disabled={false}
      sent={[{ name: 'accept', value: '*/*', source: 'automatic' }]}
    />,
  );
  await user.click(screen.getByText('Automatic and sent headers'));
  expect(bridge.previewHttpHeaders).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Inspect headers' }));
  const current = within(await screen.findByRole('region', { name: 'Inspected request headers' }));
  expect(current.getByRole('table')).toHaveTextContent('authorization');
  expect(current.getByRole('table')).toHaveTextContent('••••••••');
  expect(
    within(screen.getByRole('region', { name: 'Last dispatched request headers' })).getByRole(
      'table',
    ),
  ).toHaveTextContent('accept');
  expect(bridge.testAuthentication).not.toHaveBeenCalled();
});
