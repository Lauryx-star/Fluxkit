import { act, render, screen, within, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App';
import * as native from '../native';
import type { Collection, ProjectView } from './types';
import type { EnvironmentReply } from '../environments/types';
import type { ExecutionResult } from '../http/types';

vi.mock('../native', () => ({
  isDesktop: vi.fn(() => true),
  getAppInfo: vi.fn(),
  chooseProject: vi.fn(),
  saveCollection: vi.fn(),
  reloadProject: vi.fn(),
  renameProject: vi.fn(),
  saveProjectAuth: vi.fn(),
  removeCollection: vi.fn(),
  chooseAttachment: vi.fn(),
  closeProject: vi.fn(),
  environmentAction: vi.fn(),
  beginHttpRequest: vi.fn(),
  executeHttpRequest: vi.fn(),
  cancelHttpRequest: vi.fn(),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ onCloseRequested: async () => () => {}, destroy: async () => {} }),
}));

let stored: ProjectView;
let revision = 0;
function fixture(name = 'Shop'): ProjectView {
  return {
    handle: crypto.randomUUID(),
    directory: '/synthetic/project',
    revision: 'project-1',
    data: { schemaVersion: 1, id: crypto.randomUUID(), name },
    collections: [],
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  revision = 0;
  stored = fixture();
  vi.mocked(native.isDesktop).mockReturnValue(true);
  vi.mocked(native.getAppInfo).mockResolvedValue({ name: 'Fluxkit', version: '0.1.0' });
  vi.mocked(native.chooseProject).mockImplementation(async (name) => {
    if (name) {
      stored.data.name = name;
    }
    return structuredClone(stored);
  });
  vi.mocked(native.saveCollection).mockImplementation(async (_handle, data: Collection) => {
    const view = { data: structuredClone(data), revision: `collection-${++revision}` };
    stored.collections = stored.collections.some((item) => item.data.id === data.id)
      ? stored.collections.map((item) => (item.data.id === data.id ? view : item))
      : [...stored.collections, view];
    return structuredClone(view);
  });
  vi.mocked(native.saveProjectAuth).mockImplementation(async (_handle, auth) => {
    stored.data.auth = auth;
    stored.revision = `project-${++revision}`;
    return structuredClone(stored);
  });
  vi.mocked(native.reloadProject).mockImplementation(async () => structuredClone(stored));
  vi.mocked(native.environmentAction).mockResolvedValue({
    view: {
      revision: 'missing',
      data: { schemaVersion: 1, variables: [], environments: [] },
      selectedEnvironment: null,
      selectionMissing: false,
    },
    value: null,
  });
  vi.mocked(native.closeProject).mockResolvedValue();
  vi.mocked(native.chooseAttachment).mockResolvedValue(null);
  vi.mocked(native.beginHttpRequest).mockResolvedValue(crypto.randomUUID());
  vi.mocked(native.cancelHttpRequest).mockResolvedValue();
  vi.mocked(native.executeHttpRequest).mockResolvedValue({
    outcome: 'response',
    status: 200,
    elapsedMs: 42,
    byteCount: 11,
    headers: [{ name: 'content-type', value: 'application/json' }],
    text: '{"ok":true}',
    jsonText: '{\n  "ok": true\n}',
    contentKind: 'json',
    truncated: false,
    saved: false,
    message: null,
  });
});

async function setupCollection() {
  const user = userEvent.setup();
  render(<App />);
  await user.click(screen.getByRole('button', { name: 'New project' }));
  await user.type(screen.getByRole('textbox', { name: 'Name' }), 'Shop');
  await user.click(screen.getByRole('button', { name: 'Create' }));
  await user.click(await screen.findByRole('button', { name: 'Create collection' }));
  await user.type(screen.getByRole('textbox', { name: 'Name' }), 'Shop API');
  await user.click(screen.getByRole('button', { name: 'Create' }));
  await screen.findByRole('button', { name: 'New request in Shop API' });
  return user;
}

describe('local project workflows', () => {
  it('sends the unsaved draft with a 60-second default and displays response views', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'http://127.0.0.1:12345/synthetic',
    );
    await user.click(screen.getByRole('button', { name: 'Send' }));
    const response = within(screen.getByRole('region', { name: 'Response' }));
    expect(await response.findByText(/HTTP 200/)).toHaveTextContent('42 ms');
    expect(response.getByRole('tabpanel')).toHaveTextContent('{"ok":true}');
    await user.click(response.getByRole('tab', { name: 'JSON' }));
    expect(response.getByRole('tabpanel')).toHaveTextContent('"ok": true');
    await user.click(response.getByRole('tab', { name: 'Headers' }));
    expect(response.getByRole('table')).toHaveTextContent('application/json');
    expect(native.executeHttpRequest).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({
        timeoutMs: 60000,
        saveResponse: false,
        scope: {
          handle: stored.handle,
          environment: null,
          revision: 'missing',
          projectRevision: stored.revision,
          collectionId: stored.collections[0]?.data.id,
          collectionRevision: stored.collections[0]?.revision,
        },
      }),
    );
    expect(stored.collections[0]?.data.requests).toHaveLength(0);
  });
  it('cancels before native registration completes without ever dispatching', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    let ready: ((id: string) => void) | undefined;
    vi.mocked(native.beginHttpRequest).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          ready = resolve;
        }),
    );
    await user.click(screen.getByRole('button', { name: 'Send' }));
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    const id = crypto.randomUUID();
    await act(async () => {
      ready?.(id);
    });
    expect(await screen.findByRole('alert')).toHaveTextContent('Cancelled before dispatch');
    expect(native.executeHttpRequest).not.toHaveBeenCalled();
    expect(native.cancelHttpRequest).toHaveBeenCalledWith(id);
  });
  it('keeps responses with their draft and exposes explicit send-and-save without retrying', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    let done: ((result: ExecutionResult) => void) | undefined;
    vi.mocked(native.executeHttpRequest).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          done = resolve;
        }),
    );
    await user.click(screen.getByRole('button', { name: 'Send & save response…' }));
    await waitFor(() =>
      expect(native.executeHttpRequest).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({ saveResponse: true }),
      ),
    );
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    await act(async () => {
      done?.({
        outcome: 'response',
        status: 201,
        elapsedMs: 10,
        byteCount: 1,
        headers: [],
        text: 'synthetic first response',
        jsonText: null,
        contentKind: 'text',
        truncated: false,
        saved: true,
        message: null,
      });
    });
    expect(screen.queryByText('synthetic first response')).not.toBeInTheDocument();
    await user.click(
      within(screen.getByLabelText('Request tabs')).getByRole('button', { name: 'GET Request 1' }),
    );
    expect(screen.getByRole('tabpanel', { name: 'Raw' })).toHaveTextContent(
      'synthetic first response',
    );
    expect(native.executeHttpRequest).toHaveBeenCalledTimes(1);
  });

  it('shows pending, successful, and failed variable checks in a persistent result panel', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    await user.click(screen.getByRole('textbox', { name: 'Request URL' }));
    await user.paste('{{baseUrl}}/health');
    let finish: ((reply: EnvironmentReply) => void) | undefined;
    vi.mocked(native.environmentAction).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await user.click(screen.getByRole('button', { name: 'Check variables' }));
    expect(screen.getByLabelText('Variable check result')).toHaveTextContent('Checking variables…');
    expect(screen.getByRole('button', { name: 'Check variables' })).toBeDisabled();
    await act(async () => {
      finish?.({
        view: {
          revision: '1',
          data: { schemaVersion: 1, environments: [], variables: [] },
          selectedEnvironment: null,
          selectionMissing: false,
        },
        value: null,
      });
    });
    expect(screen.getByLabelText('Variable check result')).toHaveTextContent(
      'All request-field placeholders have values in Defaults or shared defaults. Authentication references are validated when sending.',
    );
    expect(native.environmentAction).toHaveBeenLastCalledWith(stored.handle, {
      kind: 'check',
      text: expect.stringContaining('{{baseUrl}}/health'),
    });
    vi.mocked(native.environmentAction).mockRejectedValueOnce(
      'A referenced variable has no value.',
    );
    await user.click(screen.getByRole('button', { name: 'Check variables' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Variable check failed');
    expect(screen.getByRole('alert')).toHaveTextContent('A referenced variable has no value.');
    expect(screen.getByRole('alert')).toHaveTextContent('No HTTP request is sent.');
  });

  it('explains when there are no placeholders and keeps results scoped to the checked request', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    await user.click(screen.getByRole('button', { name: 'Check variables' }));
    expect(await screen.findByLabelText('Variable check result')).toHaveTextContent(
      'This request contains no variable placeholders to resolve.',
    );
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    expect(screen.queryByLabelText('Variable check result')).not.toBeInTheDocument();
  });

  it('creates project/collection and explicitly saves a request with Cmd+S', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    await user.clear(screen.getByRole('textbox', { name: 'Request name' }));
    await user.type(screen.getByRole('textbox', { name: 'Request name' }), 'Create product');
    await user.selectOptions(screen.getByRole('combobox', { name: 'HTTP method' }), 'POST');
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'https://example.test/products',
    );
    await user.click(screen.getByRole('tab', { name: 'Body' }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Body type' }), 'json');
    await user.type(screen.getByRole('textbox', { name: 'Request body' }), 'hello');
    expect(stored.collections[0]?.data.requests).toHaveLength(0);
    await user.keyboard('{Meta>}s{/Meta}');
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled());
    expect(stored.collections[0]?.data.requests[0]?.name).toBe('Create product');
    expect(stored.collections[0]?.data.requests[0]?.body.text).toBe('hello');
    expect(stored.collections[0]?.data.requests[0]?.method).toBe('POST');
    const id = stored.collections[0]?.data.requests[0]?.id;
    await user.click(screen.getByRole('button', { name: 'Close Create product' }));
    const tree = screen.getByLabelText('Workspace navigation');
    await user.click(within(tree).getByRole('button', { name: 'POST Create product' }));
    expect(screen.getByRole('textbox', { name: 'Request URL' })).toHaveValue(
      'https://example.test/products',
    );
    expect(screen.getByRole('textbox', { name: 'Request name' })).toHaveValue('Create product');
    expect(stored.collections[0]?.data.requests[0]?.id).toBe(id);
  });

  it('keeps unsaved input when a save detects an external conflict', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'https://example.test/keep',
    );
    vi.mocked(native.saveCollection).mockRejectedValueOnce(
      'Files changed outside Fluxkit. Reload before saving.',
    );
    await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Files changed outside Fluxkit');
    expect(screen.getByRole('textbox', { name: 'Request URL' })).toHaveValue(
      'https://example.test/keep',
    );
    expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
    expect(stored.collections[0]?.data.requests).toHaveLength(0);
  });

  it('supports nested folders and removing a folder subtree', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New folder in Shop API' }));
    await user.type(screen.getByRole('textbox', { name: 'Name' }), 'Products');
    await user.click(screen.getByRole('button', { name: 'Create' }));
    await user.click(await screen.findByRole('button', { name: 'New subfolder in Products' }));
    await user.type(screen.getByRole('textbox', { name: 'Name' }), 'Details');
    await user.click(screen.getByRole('button', { name: 'Create' }));
    await user.click(await screen.findByRole('button', { name: 'New request in Details' }));
    await user.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(stored.collections[0]?.data.requests).toHaveLength(1));
    expect(stored.collections[0]?.data.requests[0]?.folderId).toBe(
      stored.collections[0]?.data.folders[1]?.id,
    );
    await user.click(screen.getByRole('button', { name: 'Remove folder Products' }));
    await user.click(screen.getByRole('button', { name: 'Continue' }));
    await waitFor(() => expect(stored.collections[0]?.data.folders).toHaveLength(0));
    expect(stored.collections[0]?.data.requests).toHaveLength(0);
    expect(screen.queryByRole('textbox', { name: 'Request URL' })).not.toBeInTheDocument();
  });

  it('cancels discarding a draft and preserves form values and attached references', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    await user.click(screen.getByRole('tab', { name: 'Body' }));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Body type' }), 'multipart');
    await user.click(screen.getByRole('button', { name: 'Add field' }));
    await user.type(screen.getByRole('textbox', { name: 'Field key 1' }), 'document');
    const file = { path: `attachments/${crypto.randomUUID()}.bin`, name: 'test.txt' };
    vi.mocked(native.chooseAttachment).mockResolvedValueOnce(file);
    await user.click(screen.getByRole('button', { name: 'Attach file to field 1' }));
    await screen.findByText('Field 1: test.txt');
    await user.click(screen.getByRole('button', { name: 'Close Request 1' }));
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.getByRole('textbox', { name: 'Field key 1' })).toHaveValue('document');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(stored.collections[0]?.data.requests[0]?.body.fields[0]?.file).toEqual(file),
    );
  });

  it('does not redirect an existing draft to a different active project', async () => {
    const user = await setupCollection();
    const firstHandle = stored.handle;
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    const other = fixture('Other');
    vi.mocked(native.chooseProject).mockResolvedValueOnce(other);
    await user.click(screen.getByRole('button', { name: 'Open project' }));
    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'Active project' })).toHaveValue(other.handle),
    );
    await user.click(
      within(screen.getByLabelText('Request tabs')).getByRole('button', { name: 'GET Request 1' }),
    );
    await user.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(native.saveCollection).toHaveBeenLastCalledWith(
        firstHandle,
        expect.anything(),
        expect.any(String),
      ),
    );
  });
});

describe('authentication workflows', () => {
  it('saves project auth references, preserves dirty drafts, and saves an explicit request None', async () => {
    const token = crypto.randomUUID();
    vi.mocked(native.environmentAction).mockResolvedValue({
      view: {
        revision: 'env-1',
        data: { schemaVersion: 1, variables: [{ id: token, name: 'apiToken' }], environments: [] },
        selectedEnvironment: null,
        selectionMissing: false,
      },
      value: null,
    });
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'https://example.test/unsaved',
    );
    await user.click(screen.getByRole('button', { name: 'Project authentication' }));
    const dialog = within(screen.getByRole('dialog', { name: 'Authentication: Shop' }));
    await user.selectOptions(
      dialog.getByRole('combobox', { name: 'Authentication method' }),
      'bearer',
    );
    await user.selectOptions(dialog.getByRole('combobox', { name: 'Token variable' }), token);
    await user.click(dialog.getByRole('button', { name: 'Save authentication' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(native.saveProjectAuth).toHaveBeenCalledWith(
      stored.handle,
      { kind: 'bearer', token },
      'project-1',
    );
    expect(screen.getByLabelText('Effective authentication')).toHaveTextContent(
      'Bearer · Project: Shop',
    );
    expect(screen.getByRole('textbox', { name: 'Request URL' })).toHaveValue(
      'https://example.test/unsaved',
    );
    expect(stored.collections[0]?.data.requests).toHaveLength(0);
    await user.click(screen.getByRole('tab', { name: 'Auth' }));
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Authentication method' }),
      'none',
    );
    expect(screen.getByLabelText('Effective authentication')).toHaveTextContent('None · Request');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(stored.collections[0]?.data.requests[0]?.auth).toEqual({ kind: 'none' }),
    );
    await user.click(
      screen.getByRole('button', { name: 'Authentication for collection Shop API' }),
    );
    const collectionDialog = within(
      screen.getByRole('dialog', { name: 'Authentication: Shop API' }),
    );
    await user.selectOptions(
      collectionDialog.getByRole('combobox', { name: 'Authentication method' }),
      'basic',
    );
    await user.selectOptions(
      collectionDialog.getByRole('combobox', { name: 'Username variable' }),
      token,
    );
    await user.selectOptions(
      collectionDialog.getByRole('combobox', { name: 'Password variable' }),
      token,
    );
    await user.click(collectionDialog.getByRole('button', { name: 'Save authentication' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(stored.collections[0]?.data.auth).toEqual({
      kind: 'basic',
      username: token,
      password: token,
    });
    expect(screen.getByLabelText('Effective authentication')).toHaveTextContent('None · Request');
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Authentication method' }),
      'inherit',
    );
    expect(screen.getByLabelText('Effective authentication')).toHaveTextContent(
      'Basic · Collection: Shop API',
    );
    expect(native.environmentAction).not.toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ kind: 'reveal' }),
    );
  });
  it('keeps failed authentication edits open and confirms discarding them', async () => {
    const user = await setupCollection();
    vi.mocked(native.saveCollection).mockRejectedValueOnce('Files changed. Reload before saving.');
    await user.click(
      screen.getByRole('button', { name: 'Authentication for collection Shop API' }),
    );
    let dialog = within(screen.getByRole('dialog', { name: 'Authentication: Shop API' }));
    await user.selectOptions(
      dialog.getByRole('combobox', { name: 'Authentication method' }),
      'none',
    );
    await user.click(dialog.getByRole('button', { name: 'Save authentication' }));
    expect(await dialog.findByRole('alert')).toHaveTextContent('Files changed');
    expect(dialog.getByRole('combobox', { name: 'Authentication method' })).toHaveValue('none');
    await user.click(dialog.getByRole('button', { name: 'Cancel' }));
    const confirmation = within(screen.getByRole('dialog', { name: 'Confirm action' }));
    await user.click(confirmation.getByRole('button', { name: 'Cancel' }));
    dialog = within(screen.getByRole('dialog', { name: 'Authentication: Shop API' }));
    expect(dialog.getByRole('combobox', { name: 'Authentication method' })).toHaveValue('none');
    await user.click(dialog.getByRole('button', { name: 'Cancel' }));
    await user.click(
      within(screen.getByRole('dialog', { name: 'Confirm action' })).getByRole('button', {
        name: 'Continue',
      }),
    );
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(stored.collections[0]?.data.auth).toBeUndefined();
  });
  it('never borrows an active project authentication context for an existing session draft', async () => {
    const user = userEvent.setup();
    stored.data.auth = { kind: 'bearer', token: crypto.randomUUID() };
    render(<App />);
    await user.click(within(screen.getByRole('main')).getByRole('button', { name: 'New request' }));
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'https://example.test/session',
    );
    await user.click(screen.getByRole('button', { name: 'Open project' }));
    await screen.findByRole('button', { name: 'Project authentication' });
    expect(screen.getByLabelText('Effective authentication')).toHaveTextContent('None · Session');
    await user.click(screen.getByRole('button', { name: 'Send' }));
    await waitFor(() =>
      expect(native.executeHttpRequest).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({ scope: null }),
      ),
    );
  });
});
