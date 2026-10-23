import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { AuthEditor } from './AuthEditor';
import { TokenControls } from './TokenControls';
import { literal, loginRequest } from './defaults';
import { validAuth, manualAuthHint } from './rules';
import type { Auth, TokenStatus } from './types';
import type { ExecutionInput } from '../http/types';

const bridge = vi.hoisted(() => ({
  beginHttpRequest: vi.fn(),
  authenticationAction: vi.fn(),
  cancelHttpRequest: vi.fn(),
}));
vi.mock('../native', () => bridge);
beforeEach(() => {
  vi.resetAllMocks();
  bridge.beginHttpRequest.mockResolvedValue('reservation');
  bridge.cancelHttpRequest.mockResolvedValue(undefined);
  bridge.authenticationAction.mockResolvedValue({ state: 'unknownExpiry', expiresAt: null });
});
const variables = [
  { id: 'user-id', name: 'client' },
  { id: 'secret-id', name: 'secret' },
];
const oauth: Auth = {
  kind: 'oauth2',
  config: {
    tokenUrl: literal('https://example.test/token'),
    clientId: 'user-id',
    clientSecret: 'secret-id',
    scope: literal('read'),
    clientAuthentication: 'basic',
  },
};
const input: ExecutionInput = {
  request: {
    id: 'request',
    name: 'Synthetic',
    folderId: null,
    method: 'GET',
    url: 'https://example.test/api',
    auth: oauth,
    params: [],
    headers: [],
    body: { kind: 'none', text: '', fields: [], file: null },
  },
  scope: {
    handle: 'project-handle',
    environment: 'environment-id',
    revision: 'env-revision',
    projectRevision: 'project-revision',
    collectionId: null,
    collectionRevision: null,
  },
  timeoutMs: 60000,
  saveResponse: false,
};

describe('token configuration', () => {
  it('edits OAuth2 and stable variable composition without accessing stored values', async () => {
    const user = userEvent.setup();
    const changed = vi.fn();
    function Editor() {
      const [value, setValue] = useState<Auth | null>(null);
      return (
        <AuthEditor
          value={value}
          variables={variables}
          onChange={(v) => {
            changed(v);
            setValue(v);
          }}
        />
      );
    }
    render(<Editor />);
    await user.selectOptions(screen.getByLabelText('Authentication method'), 'oauth2');
    await user.selectOptions(screen.getByLabelText('Client ID variable'), 'user-id');
    await user.selectOptions(screen.getByLabelText('Client secret variable'), 'secret-id');
    await user.selectOptions(screen.getByLabelText('Token endpoint input 1 source'), 'variable');
    await user.selectOptions(screen.getByLabelText('Token endpoint input 1 variable'), 'user-id');
    const result = changed.mock.lastCall?.[0] as Auth;
    expect(result).toMatchObject({
      kind: 'oauth2',
      config: {
        clientId: 'user-id',
        clientSecret: 'secret-id',
        tokenUrl: { parts: [{ kind: 'variable', id: 'user-id' }] },
      },
    });
    expect(validAuth(result, variables)).toBe(true);
    expect(bridge.authenticationAction).not.toHaveBeenCalled();
  });
  it('configures separate renewal, response extraction and unknown expiry', async () => {
    const user = userEvent.setup();
    const changed = vi.fn();
    function Editor() {
      const [value, setValue] = useState<Auth | null>(null);
      return (
        <AuthEditor
          value={value}
          variables={variables}
          onChange={(v) => {
            changed(v);
            setValue(v);
          }}
        />
      );
    }
    render(<Editor />);
    await user.selectOptions(screen.getByLabelText('Authentication method'), 'customLogin');
    expect(screen.getByText(/Expiry unknown/)).toBeVisible();
    expect(screen.queryByRole('option', { name: 'Stored renewal value' })).not.toBeInTheDocument();
    await user.click(screen.getByLabelText('Use a separate renewal request'));
    await user.selectOptions(
      screen.getByLabelText('Renewal endpoint input 1 source'),
      'renewalToken',
    );
    await user.selectOptions(screen.getByLabelText('Login token extraction source'), 'header');
    fireEvent.change(screen.getByLabelText('Login token extraction header name'), {
      target: { value: 'X-Session' },
    });
    fireEvent.change(screen.getByLabelText('Login validity seconds'), { target: { value: '120' } });
    expect(changed.mock.lastCall?.[0]).toMatchObject({
      kind: 'customLogin',
      config: {
        login: { token: { kind: 'header', name: 'X-Session' }, lifetimeSeconds: 120 },
        renewal: { url: { parts: [{ kind: 'renewalToken' }] } },
      },
    });
    expect(bridge.authenticationAction).not.toHaveBeenCalled();
  });
  it('checks custom references and manual placement priority without resolving values', () => {
    const custom: Auth = {
      kind: 'customLogin',
      config: {
        login: {
          ...loginRequest(),
          url: { ...literal(), parts: [{ kind: 'variable', id: 'missing' }] },
        },
        renewal: null,
        placement: { name: 'session', placement: 'query', prefix: '' },
      },
    };
    expect(validAuth(custom, variables)).toBe(false);
    expect(
      manualAuthHint(custom, { url: 'https://example.test/?session=', headers: [], params: [] }),
    ).toContain('Manual query');
    expect(
      manualAuthHint(oauth, {
        url: '',
        headers: [{ id: 1, enabled: true, key: 'AUTHORIZATION', value: 'manual' }],
        params: [],
      }),
    ).toContain('Manual Authorization');
  });
});

describe('local token controls', () => {
  it('checks metadata explicitly and keeps usable status after renewal failure', async () => {
    const user = userEvent.setup();
    render(<TokenControls input={input} />);
    expect(bridge.authenticationAction).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Check token status' }));
    expect(await screen.findByText('Expiry unknown')).toBeVisible();
    expect(bridge.authenticationAction).toHaveBeenCalledWith('reservation', input, 'status');
    bridge.authenticationAction.mockRejectedValueOnce(
      'Required extraction failed. Existing token was kept.',
    );
    await user.click(screen.getByRole('button', { name: 'Acquire / renew token' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Existing token was kept');
    expect(screen.getByText('Expiry unknown')).toBeVisible();
    expect(bridge.authenticationAction).toHaveBeenCalledTimes(2);
  });
  it('requires confirmation before forgetting only the selected token', async () => {
    const user = userEvent.setup();
    render(<TokenControls input={input} />);
    await user.click(screen.getByRole('button', { name: 'Forget local token' }));
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(bridge.authenticationAction).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Forget local token' }));
    bridge.authenticationAction.mockResolvedValueOnce({ state: 'missing', expiresAt: null });
    await user.click(screen.getByRole('button', { name: 'Continue' }));
    expect(await screen.findByText('No local token')).toBeVisible();
    expect(bridge.authenticationAction).toHaveBeenCalledWith('reservation', input, 'forget');
  });
  it('cancels pending registration on context change and discards late results', async () => {
    let register!: (id: string) => void;
    bridge.beginHttpRequest.mockReturnValueOnce(
      new Promise((resolve) => {
        register = resolve;
      }),
    );
    const user = userEvent.setup();
    const { rerender } = render(<TokenControls key="old" input={input} />);
    await user.click(screen.getByRole('button', { name: 'Acquire / renew token' }));
    rerender(
      <TokenControls
        key="new"
        input={{ ...input, scope: { ...input.scope!, environment: 'other' } }}
      />,
    );
    await act(async () => register('late-reservation'));
    await waitFor(() => expect(bridge.cancelHttpRequest).toHaveBeenCalledWith('late-reservation'));
    expect(bridge.authenticationAction).not.toHaveBeenCalled();
    let complete!: (result: TokenStatus) => void;
    bridge.authenticationAction.mockReturnValueOnce(
      new Promise((resolve) => {
        complete = resolve;
      }),
    );
    await user.click(screen.getByRole('button', { name: 'Check token status' }));
    await waitFor(() => expect(bridge.authenticationAction).toHaveBeenCalledTimes(1));
    rerender(<TokenControls key="third" input={input} />);
    await act(async () => complete({ state: 'unknownExpiry', expiresAt: null }));
    expect(screen.queryByText('Expiry unknown')).not.toBeInTheDocument();
    expect(screen.getByText('Local token status not checked')).toBeVisible();
  });
});
