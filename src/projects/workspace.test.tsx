import { selectOption } from '../test/selectOption';
import {
  act,
  fireEvent,
  render,
  renderHook,
  screen,
  within,
  waitFor,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { StrictMode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App';
import * as native from '../native';
import type { Collection, ProjectView } from './types';
import type { EnvironmentReply } from '../environments/types';
import type { ExecutionResult } from '../http/types';
import { useWorkspace } from './useWorkspace';

const lifecycle = vi.hoisted(() => ({
  close: null as ((event: { preventDefault: () => void }) => Promise<void>) | null,
  destroy: vi.fn(async () => {}),
}));

vi.mock('../native', () => ({
  isDesktop: vi.fn(() => true),
  getAppInfo: vi.fn(),
  chooseProject: vi.fn(),
  refreshServerProject: vi.fn(),
  restoreLastProject: vi.fn(),
  rememberProject: vi.fn(),
  saveCollection: vi.fn(),
  moveRequest: vi.fn(),
  reloadProject: vi.fn(),
  renameProject: vi.fn(),
  saveProjectAuth: vi.fn(),
  removeCollection: vi.fn(),
  chooseAttachment: vi.fn(),
  closeProject: vi.fn(),
  environmentAction: vi.fn(),
  beginHttpRequest: vi.fn(),
  executeHttpRequest: vi.fn(),
  testAuthentication: vi.fn(),
  previewHttpHeaders: vi.fn(),
  cancelHttpRequest: vi.fn(),
  chooseOpenApiImport: vi.fn(),
  renameImportVariable: vi.fn(),
  choosePostmanImport: vi.fn(),
  previewOpenApiExport: vi.fn(),
  saveOpenApiExport: vi.fn(),
  discardOpenApiExport: vi.fn(),
  commitOpenApiImport: vi.fn(),
  discardOpenApiImport: vi.fn(),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    onCloseRequested: async (handler: NonNullable<typeof lifecycle.close>) => {
      lifecycle.close = handler;
      return () => {
        if (lifecycle.close === handler) {
          lifecycle.close = null;
        }
      };
    },
    destroy: lifecycle.destroy,
  }),
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
  vi.mocked(native.restoreLastProject).mockResolvedValue(null);
  vi.mocked(native.rememberProject).mockResolvedValue();
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
  vi.mocked(native.discardOpenApiImport).mockResolvedValue();
  vi.mocked(native.chooseOpenApiImport).mockResolvedValue(null);
  vi.mocked(native.choosePostmanImport).mockResolvedValue(null);
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
    expect(await response.findByText('HTTP 200')).toBeVisible();
    expect(response.getByText(/42 ms/)).toBeVisible();
    expect(response.getByRole('tab', { name: 'JSON' })).toHaveAttribute('aria-selected', 'true');
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
    await selectOption(user, screen.getByRole('combobox', { name: 'HTTP method' }), 'POST');
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'https://example.test/products',
    );
    await user.click(screen.getByRole('tab', { name: 'Body' }));
    await selectOption(user, screen.getByRole('combobox', { name: 'Body type' }), 'json');
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
    await selectOption(user, screen.getByRole('combobox', { name: 'Body type' }), 'multipart');
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
    await selectOption(
      user,
      dialog.getByRole('combobox', { name: 'Authentication method' }),
      'bearer',
    );
    await selectOption(user, dialog.getByRole('combobox', { name: 'Token variable' }), token);
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
    await selectOption(
      user,
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
    await selectOption(
      user,
      collectionDialog.getByRole('combobox', { name: 'Authentication method' }),
      'basic',
    );
    await selectOption(
      user,
      collectionDialog.getByRole('combobox', { name: 'Username variable' }),
      token,
    );
    await selectOption(
      user,
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
    await selectOption(
      user,
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
    await selectOption(
      user,
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

it('previews and imports a new collection while preserving open unsaved requests', async () => {
  const user = await setupCollection();
  await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
  await user.type(
    screen.getByRole('textbox', { name: 'Request URL' }),
    'https://example.test/unsaved',
  );
  const preview = {
    id: crypto.randomUUID(),
    source: 'OpenAPI',
    environmentName: null,
    collectionName: 'Imported API',
    version: '3.0.4',
    requestCount: 0,
    folderCount: 0,
    variables: [],
    requests: [],
    warnings: [],
    blockers: [],
  };
  vi.mocked(native.chooseOpenApiImport).mockResolvedValue(preview);
  vi.mocked(native.commitOpenApiImport).mockImplementation(async () => {
    stored.collections.push({
      revision: 'imported-1',
      data: {
        schemaVersion: 1,
        id: crypto.randomUUID(),
        name: 'Imported API',
        requests: [],
        folders: [],
        openApi: { version: '3.0.4', document: { info: { title: 'Imported API' } } },
      },
    });
    return { project: structuredClone(stored), error: null };
  });
  await user.click(screen.getByRole('button', { name: 'Import OpenAPI…' }));
  expect(await screen.findByRole('dialog', { name: 'Import OpenAPI' })).toBeVisible();
  expect(native.commitOpenApiImport).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Import collection' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(native.commitOpenApiImport).toHaveBeenCalledWith(stored.handle, preview.id, true);
  expect(
    screen.getByRole('button', { name: 'Authentication for collection Imported API' }),
  ).toBeVisible();
  expect(screen.getByRole('textbox', { name: 'Request URL' })).toHaveValue(
    'https://example.test/unsaved',
  );
  expect(stored.collections[0]?.data.requests).toHaveLength(0);
});
it('cancels previews without writing and shows recoverable native import failures', async () => {
  const user = await setupCollection();
  const preview = {
    id: crypto.randomUUID(),
    source: 'OpenAPI',
    environmentName: null,
    collectionName: 'Imported API',
    version: '3.1.2',
    requestCount: 1,
    folderCount: 0,
    variables: [],
    requests: [],
    warnings: [],
    blockers: [],
  };
  vi.mocked(native.chooseOpenApiImport).mockResolvedValue(preview);
  await user.click(screen.getByRole('button', { name: 'Import OpenAPI…' }));
  await screen.findByRole('dialog', { name: 'Import OpenAPI' });
  await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Cancel' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(native.discardOpenApiImport).toHaveBeenCalledWith(preview.id);
  expect(native.commitOpenApiImport).not.toHaveBeenCalled();
  vi.mocked(native.commitOpenApiImport).mockResolvedValue({
    project: structuredClone(stored),
    error: 'OS store locked. Imported definitions remain visible.',
  });
  await user.click(screen.getByRole('button', { name: 'Import OpenAPI…' }));
  await screen.findByRole('dialog');
  await user.click(screen.getByRole('button', { name: 'Import collection' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('definitions remain visible');
  expect(screen.getByRole('button', { name: 'Retry import' })).toBeEnabled();
});

it('routes Postman selection through native preview and preserves an unsaved draft', async () => {
  const user = await setupCollection();
  await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
  await user.type(
    screen.getByRole('textbox', { name: 'Request URL' }),
    'https://synthetic.invalid/dirty',
  );
  const preview = {
    id: crypto.randomUUID(),
    source: 'Postman',
    environmentName: null,
    collectionName: 'Migrated',
    version: '2.1',
    requestCount: 0,
    folderCount: 0,
    variables: [],
    requests: [],
    warnings: ['Scripts are omitted.'],
    blockers: [],
  };
  vi.mocked(native.choosePostmanImport).mockResolvedValue(preview);
  await user.click(screen.getByRole('button', { name: 'Import Postman…' }));
  await user.click(screen.getByRole('button', { name: 'Choose collection JSON…' }));
  expect(await screen.findByText('Scripts are omitted.')).toBeVisible();
  expect(native.choosePostmanImport).toHaveBeenCalledWith(stored.handle, null);
  expect(native.commitOpenApiImport).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Cancel' }));
  expect(native.discardOpenApiImport).toHaveBeenCalledWith(preview.id);
  expect(screen.getByRole('textbox', { name: 'Request URL' })).toHaveValue(
    'https://synthetic.invalid/dirty',
  );
});

it('exports the saved project through a preview without sending unsaved draft content', async () => {
  const user = await setupCollection();
  await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
  await user.type(
    screen.getByRole('textbox', { name: 'Request URL' }),
    'https://synthetic.invalid/unsaved',
  );
  const preview = {
    id: crypto.randomUUID(),
    title: 'Shop',
    directory: true,
    folderName: 'fluxkit-export-test',
    files: [{ name: 'shop.yml', operations: 1, bytes: 200 }],
    warnings: ['Values are excluded.'],
    blockers: [],
  };
  vi.mocked(native.previewOpenApiExport).mockResolvedValue(preview);
  vi.mocked(native.saveOpenApiExport).mockResolvedValueOnce(false).mockResolvedValueOnce(true);
  await user.click(screen.getByRole('button', { name: 'Export OpenAPI…' }));
  expect(screen.getByRole('status')).toHaveTextContent('unsaved request changes');
  await user.click(screen.getByRole('button', { name: 'Preview export' }));
  expect(await screen.findByText('Values are excluded.')).toBeVisible();
  expect(native.previewOpenApiExport).toHaveBeenCalledWith(stored.handle, { kind: 'project' });
  await user.click(screen.getByRole('button', { name: 'Choose export folder…' }));
  expect(screen.getByRole('dialog', { name: 'Export OpenAPI' })).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Choose export folder…' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(native.saveOpenApiExport).toHaveBeenCalledWith(stored.handle, preview.id);
  expect(screen.getByRole('textbox', { name: 'Request URL' })).toHaveValue(
    'https://synthetic.invalid/unsaved',
  );
  expect(screen.getByRole('status')).toHaveTextContent('export saved');
});

it('persists import review, keeps sending locked until reviewed and saved, and preserves failed saves', async () => {
  const user = await setupCollection();
  await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
  await user.type(
    screen.getByRole('textbox', { name: 'Request URL' }),
    'https://synthetic.invalid/review',
  );
  await user.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(stored.collections[0]?.data.requests).toHaveLength(1));
  const saved = stored.collections[0]!.data.requests[0]!;
  saved.importIssues = ['Configure the imported authentication and select local attachments.'];
  await user.click(screen.getByRole('button', { name: 'Reload' }));
  await user.click(screen.getByRole('button', { name: `GET ${saved.name}` }));
  expect(screen.getByRole('region', { name: 'Import review required' })).toBeVisible();
  expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
  await user.click(
    screen.getByRole('checkbox', { name: 'I reviewed and configured this request' }),
  );
  expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
  vi.mocked(native.saveCollection).mockRejectedValueOnce('Synthetic write failure.');
  await user.click(screen.getByRole('button', { name: 'Save' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Synthetic write failure');
  expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
  await user.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() =>
    expect(
      screen.queryByRole('region', { name: 'Import review required' }),
    ).not.toBeInTheDocument(),
  );
  expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled();
  expect(stored.collections[0]?.data.requests[0]?.importIssues).toBeUndefined();
});

describe('last project at startup', () => {
  it('restores once under StrictMode without opening request tabs or rewriting settings', async () => {
    vi.mocked(native.restoreLastProject).mockResolvedValue(stored);
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    expect(await screen.findByRole('combobox', { name: 'Active project' })).toHaveValue(
      stored.handle,
    );
    expect(screen.getByRole('button', { name: 'Create collection' })).toBeVisible();
    expect(screen.queryByRole('textbox', { name: 'Request URL' })).toBeNull();
    expect(native.restoreLastProject).toHaveBeenCalledTimes(1);
    expect(native.chooseProject).not.toHaveBeenCalled();
    expect(native.rememberProject).not.toHaveBeenCalled();
  });
  it('starts empty with an actionable warning when restoration fails, then permits manual opening', async () => {
    const user = userEvent.setup();
    vi.mocked(native.restoreLastProject).mockRejectedValueOnce(
      'The last project could not be reopened. Use Open project to choose it again.',
    );
    render(<App />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Use Open project');
    expect(native.rememberProject).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Open project' }));
    expect(await screen.findByRole('combobox', { name: 'Active project' })).toHaveValue(
      stored.handle,
    );
    await waitFor(() => expect(native.rememberProject).toHaveBeenCalledWith(stored.handle));
  });
  it('remembers project switching, falls back after closing, and clears after closing the last project', async () => {
    const user = userEvent.setup();
    const first = fixture('First');
    const second = fixture('Second');
    vi.mocked(native.restoreLastProject).mockResolvedValue(first);
    vi.mocked(native.chooseProject).mockResolvedValue(second);
    render(<App />);
    await screen.findByRole('combobox', { name: 'Active project' });
    await user.click(screen.getByRole('button', { name: 'Open project' }));
    await waitFor(() => expect(native.rememberProject).toHaveBeenLastCalledWith(second.handle));
    await selectOption(
      user,
      screen.getByRole('combobox', { name: 'Active project' }),
      first.handle,
    );
    await waitFor(() => expect(native.rememberProject).toHaveBeenLastCalledWith(first.handle));
    await user.click(screen.getByRole('button', { name: 'Close project' }));
    await waitFor(() => expect(native.rememberProject).toHaveBeenLastCalledWith(second.handle));
    await user.click(screen.getByRole('button', { name: 'Close project' }));
    await waitFor(() => expect(native.rememberProject).toHaveBeenLastCalledWith(null));
    expect(screen.queryByRole('combobox', { name: 'Active project' })).toBeNull();
  });
  it('keeps a manually opened project usable when remembering fails', async () => {
    const user = userEvent.setup();
    vi.mocked(native.rememberProject).mockRejectedValueOnce(
      'Could not access local workspace settings.',
    );
    render(<App />);
    await user.click(screen.getByRole('button', { name: 'Open project' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('workspace settings');
    expect(screen.getByRole('combobox', { name: 'Active project' })).toHaveValue(stored.handle);
    expect(screen.getByRole('button', { name: 'Create collection' })).toBeEnabled();
  });
  it('keeps project actions disabled until the startup restore has completed', async () => {
    let finish: ((view: ProjectView | null) => void) | undefined;
    vi.mocked(native.restoreLastProject).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    render(<App />);
    expect(screen.getByRole('button', { name: 'Open project' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'New project' })).toBeDisabled();
    await act(async () => {
      finish?.(null);
    });
    expect(screen.getByRole('button', { name: 'Open project' })).toBeEnabled();
  });
});

it('finishes remembered project changes in order before a clean window close', async () => {
  const first = fixture('First');
  const second = fixture('Second');
  vi.mocked(native.restoreLastProject).mockResolvedValue(first);
  vi.mocked(native.chooseProject).mockResolvedValue(second);
  let finish: (() => void) | undefined;
  vi.mocked(native.rememberProject).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const { result } = renderHook(() => useWorkspace());
  await waitFor(() => expect(result.current.busy).toBe(false));
  await act(async () => {
    result.current.openProject();
  });
  await waitFor(() => expect(result.current.activeHandle).toBe(second.handle));
  await waitFor(() => expect(native.rememberProject).toHaveBeenCalledWith(second.handle));
  await act(async () => {
    result.current.setActiveHandle(first.handle);
  });
  expect(native.rememberProject).toHaveBeenCalledTimes(1);
  const preventDefault = vi.fn();
  const closing = lifecycle.close?.({ preventDefault });
  expect(preventDefault).toHaveBeenCalledOnce();
  expect(lifecycle.destroy).not.toHaveBeenCalled();
  await act(async () => {
    finish?.();
    await closing;
  });
  expect(native.rememberProject).toHaveBeenNthCalledWith(2, first.handle);
  expect(lifecycle.destroy).toHaveBeenCalledOnce();
});

async function requestQuit() {
  const preventDefault = vi.fn();
  await act(async () => {
    await lifecycle.close?.({ preventDefault });
  });
  expect(preventDefault).toHaveBeenCalledOnce();
}

describe('save before quitting', () => {
  it('cancels with the button or Escape and discards only after the explicit choice', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'https://synthetic.invalid/dirty',
    );
    vi.mocked(native.saveCollection).mockClear();
    await requestQuit();
    expect(screen.getByRole('dialog', { name: 'Unsaved changes' })).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.getByRole('textbox', { name: 'Request URL' })).toHaveValue(
      'https://synthetic.invalid/dirty',
    );
    expect(lifecycle.destroy).not.toHaveBeenCalled();
    await requestQuit();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog', { name: 'Unsaved changes' })).toBeNull();
    await requestQuit();
    await user.click(screen.getByRole('button', { name: 'Discard changes' }));
    await waitFor(() => expect(lifecycle.destroy).toHaveBeenCalledOnce());
    expect(native.saveCollection).not.toHaveBeenCalled();
  });
  it('saves all dirty tabs together and waits for completion before quitting', async () => {
    const user = await setupCollection();
    for (const path of ['one', 'two']) {
      await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
      await user.type(
        screen.getByRole('textbox', { name: 'Request URL' }),
        `https://synthetic.invalid/${path}`,
      );
    }
    const save = vi.mocked(native.saveCollection).getMockImplementation()!;
    let finish: (() => void) | undefined;
    vi.mocked(native.saveCollection)
      .mockClear()
      .mockImplementationOnce(async (...args) => {
        await new Promise<void>((resolve) => {
          finish = resolve;
        });
        return save(...args);
      });
    await requestQuit();
    await user.click(screen.getByRole('button', { name: 'Save & quit' }));
    expect(screen.getByRole('status')).toHaveTextContent('Saving changes');
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled();
    expect(lifecycle.destroy).not.toHaveBeenCalled();
    await requestQuit();
    expect(native.saveCollection).toHaveBeenCalledTimes(1);
    await act(async () => {
      finish?.();
    });
    await waitFor(() => expect(lifecycle.destroy).toHaveBeenCalledOnce());
    expect(stored.collections[0]?.data.requests.map((request) => request.url)).toEqual([
      'https://synthetic.invalid/one',
      'https://synthetic.invalid/two',
    ]);
  });
  it('keeps session-only drafts open when no save destination exists', async () => {
    const user = userEvent.setup();
    render(<App />);
    await waitFor(() => expect(screen.getByRole('button', { name: 'Open project' })).toBeEnabled());
    await user.click(within(screen.getByRole('main')).getByRole('button', { name: 'New request' }));
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'https://synthetic.invalid/session',
    );
    await requestQuit();
    await user.click(screen.getByRole('button', { name: 'Save & quit' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Open a project and create a collection',
    );
    expect(lifecycle.destroy).not.toHaveBeenCalled();
    expect(native.saveCollection).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.getByRole('textbox', { name: 'Request URL' })).toHaveValue(
      'https://synthetic.invalid/session',
    );
  });
  it('preserves collection auth and the request save revision when saving an open auth dialog', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
    await user.type(
      screen.getByRole('textbox', { name: 'Request URL' }),
      'https://synthetic.invalid/auth',
    );
    await user.click(
      screen.getByRole('button', { name: 'Authentication for collection Shop API' }),
    );
    await selectOption(
      user,
      screen.getByRole('combobox', { name: 'Authentication method' }),
      'none',
    );
    vi.mocked(native.saveCollection).mockClear();
    await requestQuit();
    await user.click(screen.getByRole('button', { name: 'Save & quit' }));
    await waitFor(() => expect(lifecycle.destroy).toHaveBeenCalledOnce());
    expect(native.saveCollection).toHaveBeenCalledTimes(2);
    const first = await vi.mocked(native.saveCollection).mock.results[0]!.value;
    expect(vi.mocked(native.saveCollection).mock.calls[1]?.[2]).toBe(first.revision);
    expect(stored.collections[0]?.data.auth).toEqual({ kind: 'none' });
    expect(stored.collections[0]?.data.requests[0]?.url).toBe('https://synthetic.invalid/auth');
  });
  it('keeps invalid authentication changes open instead of quitting', async () => {
    const user = await setupCollection();
    await user.click(screen.getByRole('button', { name: 'Project authentication' }));
    await selectOption(
      user,
      screen.getByRole('combobox', { name: 'Authentication method' }),
      'basic',
    );
    await requestQuit();
    await user.click(screen.getByRole('button', { name: 'Save & quit' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not save all changes');
    expect(native.saveProjectAuth).not.toHaveBeenCalled();
    expect(lifecycle.destroy).not.toHaveBeenCalled();
    expect(
      within(screen.getByRole('dialog', { name: 'Unsaved changes' })).getByRole('button', {
        name: 'Cancel',
      }),
    ).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(screen.getByRole('combobox', { name: 'Authentication method' })).toHaveValue('basic');
  });
  it('keeps successful saves clean after a later failure and retries only the remaining collection', async () => {
    stored.collections = ['One', 'Two'].map((name) => ({
      revision: name,
      data: {
        schemaVersion: 1,
        id: crypto.randomUUID(),
        name,
        requests: [],
        folders: [],
      },
    }));
    vi.mocked(native.restoreLastProject).mockResolvedValue(structuredClone(stored));
    const { result } = renderHook(() => useWorkspace());
    await waitFor(() => expect(result.current.busy).toBe(false));
    for (const collection of stored.collections) {
      await act(async () => {
        result.current.addDraft(collection.data.id);
      });
      await act(async () => {
        result.current.updateDraft({ url: `https://synthetic.invalid/${collection.data.name}` });
      });
    }
    const save = vi.mocked(native.saveCollection).getMockImplementation()!;
    vi.mocked(native.saveCollection)
      .mockImplementationOnce(save)
      .mockRejectedValueOnce('Synthetic conflict.');
    await requestQuit();
    await act(async () => {
      await result.current.finishQuit('save');
    });
    expect(result.current.quitError).toBe('Synthetic conflict.');
    expect(lifecycle.destroy).not.toHaveBeenCalled();
    expect(result.current.drafts.map((draft) => draft.savedSnapshot !== null)).toEqual([
      true,
      false,
    ]);
    await act(async () => {
      await result.current.finishQuit('save');
    });
    expect(native.saveCollection).toHaveBeenCalledTimes(3);
    expect(vi.mocked(native.saveCollection).mock.calls[2]?.[1].name).toBe('Two');
    expect(lifecycle.destroy).toHaveBeenCalledOnce();
  });
});

it('saves dirty requests from every open project without mixing cloned collection identities', async () => {
  const sharedId = crypto.randomUUID();
  const first = fixture('First');
  const second = fixture('Second');
  for (const project of [first, second]) {
    project.collections = [
      {
        revision: project.data.name,
        data: {
          schemaVersion: 1,
          id: sharedId,
          name: 'Cloned API',
          folders: [],
          requests: [],
        },
      },
    ];
  }
  vi.mocked(native.restoreLastProject).mockResolvedValue(first);
  vi.mocked(native.chooseProject).mockResolvedValue(second);
  const { result } = renderHook(() => useWorkspace());
  await waitFor(() => expect(result.current.busy).toBe(false));
  await act(async () => {
    result.current.addDraft(sharedId);
  });
  await act(async () => {
    result.current.updateDraft({ url: 'https://synthetic.invalid/first' });
  });
  await act(async () => {
    result.current.openProject();
  });
  await waitFor(() => expect(result.current.project?.handle).toBe(second.handle));
  await act(async () => {
    result.current.addDraft(sharedId);
  });
  await act(async () => {
    result.current.updateDraft({ url: 'https://synthetic.invalid/second' });
  });
  vi.mocked(native.saveCollection).mockImplementation(async (_handle, data) => ({
    data,
    revision: 'saved',
  }));
  await requestQuit();
  await act(async () => {
    await result.current.finishQuit('save');
  });
  expect(native.saveCollection).toHaveBeenNthCalledWith(
    1,
    first.handle,
    expect.objectContaining({
      requests: [expect.objectContaining({ url: 'https://synthetic.invalid/first' })],
    }),
    'First',
  );
  expect(native.saveCollection).toHaveBeenNthCalledWith(
    2,
    second.handle,
    expect.objectContaining({
      requests: [expect.objectContaining({ url: 'https://synthetic.invalid/second' })],
    }),
    'Second',
  );
  expect(lifecycle.destroy).toHaveBeenCalledOnce();
});

it('keeps an edited variable value open after a credential-store failure during Save & quit', async () => {
  const variable = crypto.randomUUID();
  const view = {
    revision: 'env-1',
    data: { schemaVersion: 1, variables: [{ id: variable, name: 'baseUrl' }], environments: [] },
    selectedEnvironment: null,
    selectionMissing: false,
  };
  vi.mocked(native.environmentAction).mockImplementation(async (_handle, action) => {
    if (action.kind === 'setValue') {
      throw 'Synthetic credential-store failure.';
    }
    return { view, value: null };
  });
  const user = await setupCollection();
  await user.click(screen.getByRole('button', { name: 'Variables' }));
  await selectOption(user, screen.getByRole('combobox', { name: 'Value variable' }), variable);
  await user.type(screen.getByLabelText('Variable value'), 'synthetic-value');
  await requestQuit();
  await user.click(screen.getByRole('button', { name: 'Save & quit' }));
  const quit = within(screen.getByRole('dialog', { name: 'Unsaved changes' }));
  expect(await quit.findByRole('alert')).toHaveTextContent('Could not save all changes');
  expect(lifecycle.destroy).not.toHaveBeenCalled();
  expect(native.environmentAction).toHaveBeenLastCalledWith(
    stored.handle,
    expect.objectContaining({
      kind: 'setValue',
      value: 'synthetic-value',
    }),
  );
  await user.click(quit.getByRole('button', { name: 'Cancel' }));
  expect(screen.getByLabelText('Variable value')).toHaveValue('synthetic-value');
});

it('applies a native preview rename before committing the same import plan', async () => {
  const user = await setupCollection();
  const preview = {
    id: crypto.randomUUID(),
    source: 'OpenAPI',
    environmentName: null,
    collectionName: 'Imported API',
    version: '3.0.0',
    requestCount: 60,
    folderCount: 0,
    variables: [{ sourceName: 'Server URL', name: 'baseUrl', hasDefault: true }],
    requests: [{ name: 'Synthetic route', method: 'GET', url: '{{baseUrl}}/items' }],
    warnings: [],
    blockers: [],
  };
  vi.mocked(native.chooseOpenApiImport).mockResolvedValue(preview);
  vi.mocked(native.renameImportVariable).mockResolvedValue({
    ...preview,
    variables: [{ sourceName: 'Server URL', name: 'apiServer', hasDefault: true }],
    requests: [{ ...preview.requests[0]!, url: '{{apiServer}}/items' }],
  });
  vi.mocked(native.commitOpenApiImport).mockResolvedValue({ project: stored, error: null });
  await user.click(screen.getByRole('button', { name: 'Import OpenAPI…' }));
  await user.click(await screen.findByText('Show variable definitions'));
  await user.click(screen.getByRole('button', { name: 'Rename baseUrl before import' }));
  const input = screen.getByRole('textbox', { name: 'Imported variable name' });
  await user.clear(input);
  await user.type(input, 'apiServer');
  await user.click(screen.getByRole('button', { name: 'Apply name' }));
  expect(await screen.findByText('{{apiServer}}/items')).toBeVisible();
  expect(native.renameImportVariable).toHaveBeenCalledWith(
    stored.handle,
    preview.id,
    'baseUrl',
    'apiServer',
  );
  await user.click(screen.getByRole('button', { name: 'Import collection' }));
  await waitFor(() =>
    expect(native.commitOpenApiImport).toHaveBeenCalledWith(stored.handle, preview.id, true),
  );
});

it('assigns a session draft to a chosen collection/folder and saves without losing edits', async () => {
  const { result } = renderHook(() => useWorkspace());
  await waitFor(() => expect(result.current.busy).toBe(false));
  act(() => result.current.addDraft());
  act(() => result.current.updateDraft({ url: 'https://synthetic.invalid/me' }));
  const id = result.current.draft!.id;
  const requestId = result.current.draft!.requestId;
  const collection: Collection = {
    schemaVersion: 1,
    id: crypto.randomUUID(),
    name: 'Destination',
    auth: { kind: 'none' },
    folders: [{ id: crypto.randomUUID(), name: 'Folder', parentId: null }],
    requests: [],
  };
  stored.collections = [{ data: collection, revision: 'original' }];
  await act(async () => result.current.openProject());
  await waitFor(() => expect(result.current.project?.handle).toBe(stored.handle));
  act(() => result.current.assignDraft(id, collection.id, collection.folders[0]!.id));
  expect(result.current.draft).toMatchObject({
    id,
    requestId,
    url: 'https://synthetic.invalid/me',
    projectHandle: stored.handle,
    collectionId: collection.id,
    folderId: collection.folders[0]!.id,
    savedSnapshot: null,
  });
  expect(native.saveCollection).not.toHaveBeenCalled();
  await act(async () => result.current.saveDraft());
  await waitFor(() => expect(native.saveCollection).toHaveBeenCalled());
  expect(result.current.draft?.savedSnapshot).not.toBeNull();
  expect(stored.collections[0]?.data.requests[0]).toMatchObject({
    id: requestId,
    folderId: collection.folders[0]!.id,
  });
});

it('tests unsaved collection inheritance against its parent without saving the old override', async () => {
  const user = await setupCollection();
  stored.collections[0]!.data.auth = {
    kind: 'apiKey',
    name: 'X-Key',
    placement: 'header',
    value: 'missing-id',
  };
  // Reload so React receives the same saved override as the native fixture.
  await user.click(screen.getByRole('button', { name: 'Reload' }));
  await user.click(screen.getByRole('button', { name: 'Authentication for collection Shop API' }));
  await selectOption(user, screen.getByLabelText('Authentication method'), 'inherit');
  await user.type(screen.getByLabelText('Authentication test URL'), 'https://example.invalid/me');
  vi.mocked(native.testAuthentication).mockResolvedValue({
    success: true,
    status: 204,
    message: 'Synthetic success',
  });
  const writes = vi.mocked(native.saveCollection).mock.calls.length;
  await user.click(screen.getByRole('button', { name: 'Test authentication' }));
  expect(await screen.findByRole('status')).toHaveTextContent('Test succeeded');
  expect(native.testAuthentication).toHaveBeenCalledWith(
    expect.any(String),
    expect.objectContaining({
      request: expect.objectContaining({ auth: null, folderId: null, method: 'GET' }),
      scope: expect.objectContaining({ collectionId: null, collectionRevision: null }),
    }),
    false,
  );
  expect(native.saveCollection).toHaveBeenCalledTimes(writes);
});

it('replaces a token test with exactly one URL test when the request auth method changes', async () => {
  const user = await setupCollection();
  await user.click(screen.getByRole('button', { name: 'New request in Shop API' }));
  await user.click(screen.getByRole('tab', { name: 'Auth' }));
  await selectOption(user, screen.getByLabelText('Authentication method'), 'customLogin');
  expect(screen.getAllByRole('button', { name: 'Test authentication' })).toHaveLength(1);
  await selectOption(user, screen.getByLabelText('Authentication method'), 'none');
  expect(screen.getAllByRole('button', { name: 'Test authentication' })).toHaveLength(1);
  expect(screen.getByLabelText('Authentication test URL')).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Acquire / renew token' })).not.toBeInTheDocument();
});

it('rejects moving a project-scoped loose draft into another project', async () => {
  const first = fixture('First');
  const second = fixture('Second');
  const collection: Collection = {
    schemaVersion: 1,
    id: crypto.randomUUID(),
    name: 'Other',
    folders: [],
    requests: [],
  };
  second.collections = [{ data: collection, revision: 'r' }];
  vi.mocked(native.restoreLastProject).mockResolvedValue(first);
  vi.mocked(native.chooseProject).mockResolvedValue(second);
  const { result } = renderHook(() => useWorkspace());
  await waitFor(() => expect(result.current.project?.handle).toBe(first.handle));
  act(() => result.current.addDraft());
  const draft = result.current.draft!;
  await act(async () => result.current.openProject());
  await waitFor(() => expect(result.current.project?.handle).toBe(second.handle));
  act(() => result.current.assignDraft(draft.id, collection.id));
  expect(result.current.error).toContain('this request’s project');
  expect(result.current.drafts.find((item) => item.id === draft.id)).toEqual(draft);
  expect(native.saveCollection).not.toHaveBeenCalled();
});

it('dispatches the same saved OAuth request with Test, Prod and Test environment scopes', async () => {
  const user = userEvent.setup();
  const testId = crypto.randomUUID();
  const prodId = crypto.randomUUID();
  const clientId = crypto.randomUUID();
  const secret = crypto.randomUUID();
  const collectionId = crypto.randomUUID();
  const requestId = crypto.randomUUID();
  const auth = {
    kind: 'oauth2' as const,
    config: {
      tokenUrl: {
        parts: [{ kind: 'literal' as const, text: 'https://example.invalid/token' }],
        separator: '',
        prefix: '',
        suffix: '',
        base64: false,
      },
      clientId,
      clientSecret: secret,
      scope: { parts: [], separator: '', prefix: '', suffix: '', base64: false },
      clientAuthentication: 'body' as const,
    },
  };
  stored.collections = [
    {
      revision: 'saved-collection',
      data: {
        schemaVersion: 1,
        id: collectionId,
        name: 'Saved API',
        auth,
        folders: [],
        requests: [
          {
            id: requestId,
            name: 'Saved protected request',
            folderId: null,
            method: 'GET',
            url: 'https://example.invalid/me',
            params: [],
            headers: [],
            body: { kind: 'none', text: '', fields: [], file: null },
          },
        ],
      },
    },
  ];
  vi.mocked(native.restoreLastProject).mockResolvedValue(structuredClone(stored));
  let selectedEnvironment: string = testId;
  vi.mocked(native.environmentAction).mockImplementation(async (_handle, action) => {
    if (action.kind === 'select' && action.environment) {
      selectedEnvironment = action.environment;
    }
    return {
      value: null,
      view: {
        revision: 'env-definitions',
        selectedEnvironment,
        selectionMissing: false,
        data: {
          schemaVersion: 1,
          environments: [
            { id: testId, name: 'Test' },
            { id: prodId, name: 'Prod' },
          ],
          variables: [
            { id: clientId, name: 'clientId' },
            { id: secret, name: 'secret' },
          ],
        },
      },
    };
  });
  render(<App />);
  await user.click(await screen.findByRole('button', { name: 'GET Saved protected request' }));
  for (const environment of [testId, prodId, testId]) {
    await selectOption(
      user,
      screen.getByRole('combobox', { name: 'Project environment' }),
      environment,
    );
    await user.click(screen.getByRole('button', { name: 'Send' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled());
  }
  expect(native.executeHttpRequest).toHaveBeenCalledTimes(3);
  const calls = vi.mocked(native.executeHttpRequest).mock.calls;
  expect(calls.map((call) => call[1].scope?.environment)).toEqual([testId, prodId, testId]);
  for (const [, input] of calls) {
    expect(input.scope).toMatchObject({
      handle: stored.handle,
      collectionId,
      collectionRevision: 'saved-collection',
      revision: 'env-definitions',
    });
    expect(input.request).toMatchObject({ id: requestId, headers: [] });
  }
  expect(native.saveCollection).not.toHaveBeenCalled();
});

async function setupMove() {
  const user = userEvent.setup();
  const request = {
    id: crypto.randomUUID(),
    name: 'Saved movable request',
    folderId: null,
    method: 'GET',
    url: 'https://example.invalid/original',
    headers: [],
    params: [],
    body: { kind: 'none' as const, text: '', fields: [], file: null },
  };
  const source: Collection = {
    schemaVersion: 1,
    id: crypto.randomUUID(),
    name: 'Source',
    folders: [],
    requests: [request],
  };
  const parent = crypto.randomUUID(),
    folder = crypto.randomUUID();
  const target: Collection = {
    schemaVersion: 1,
    id: crypto.randomUUID(),
    name: 'Destination',
    auth: { kind: 'none' },
    folders: [
      { id: parent, name: 'Parent', parentId: null },
      { id: folder, name: 'Child', parentId: parent },
    ],
    requests: [],
  };
  stored.collections = [
    { data: source, revision: 'source-r' },
    { data: target, revision: 'target-r' },
  ];
  vi.mocked(native.restoreLastProject).mockResolvedValue(structuredClone(stored));
  vi.mocked(native.moveRequest).mockImplementation(async (_handle, input) => {
    const source = stored.collections.find((item) => item.data.id === input.sourceCollectionId);
    const target = stored.collections.find((item) => item.data.id === input.targetCollectionId)!;
    target.data.requests = [
      ...target.data.requests.filter((request) => request.id !== input.request.id),
      input.request,
    ];
    target.revision = 'target-moved';
    if (source && source !== target) {
      source.data.requests = source.data.requests.filter(
        (request) => request.id !== input.request.id,
      );
      source.revision = 'source-moved';
    }
    return structuredClone(source && source !== target ? [target, source] : [target]);
  });
  render(<App />);
  await user.click(await screen.findByRole('button', { name: 'GET Saved movable request' }));
  return { user, request, source, target, folder };
}

it('saves and moves a dirty saved request to a nested folder with authoritative revisions', async () => {
  const { user, request, source, target, folder } = await setupMove();
  await user.clear(screen.getByRole('textbox', { name: 'Request URL' }));
  await user.type(
    screen.getByRole('textbox', { name: 'Request URL' }),
    'https://example.invalid/edited',
  );
  await user.click(screen.getByRole('button', { name: 'Move…' }));
  expect(screen.getByRole('dialog', { name: 'Move request' }).closest('[inert]')).toBeNull();
  const dialog = within(screen.getByRole('dialog', { name: 'Move request' }));
  await selectOption(
    user,
    dialog.getByRole('combobox', { name: 'Destination collection' }),
    target.id,
  );
  await selectOption(user, dialog.getByRole('combobox', { name: 'Destination folder' }), folder);
  expect(dialog.getByText('Destination / Parent / Child')).toBeVisible();
  expect(native.moveRequest).not.toHaveBeenCalled();
  await user.click(dialog.getByRole('button', { name: 'Save & move' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(native.moveRequest).toHaveBeenCalledWith(
    stored.handle,
    expect.objectContaining({
      request: expect.objectContaining({
        id: request.id,
        folderId: folder,
        url: 'https://example.invalid/edited',
      }),
      sourceCollectionId: source.id,
      sourceRevision: 'source-r',
      targetCollectionId: target.id,
      targetRevision: 'target-r',
      projectRevision: 'project-1',
    }),
  );
  expect(stored.collections.find((item) => item.data.id === source.id)?.data.requests).toHaveLength(
    0,
  );
  expect(screen.getByRole('button', { name: 'Change request location' })).toHaveTextContent(
    'Destination / Parent / Child',
  );
  expect(screen.getByLabelText('Effective authentication')).toHaveTextContent(
    'Collection: Destination',
  );
  expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
});

it('keeps a failed move and its dirty draft open for retry, and Cancel never writes', async () => {
  const { user, target } = await setupMove();
  await user.click(screen.getByRole('button', { name: 'Move…' }));
  await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Cancel' }));
  expect(native.moveRequest).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Change request location' }));
  const dialog = within(screen.getByRole('dialog'));
  await selectOption(
    user,
    dialog.getByRole('combobox', { name: 'Destination collection' }),
    target.id,
  );
  vi.mocked(native.moveRequest).mockRejectedValueOnce('Destination changed outside Fluxkit.');
  await user.click(dialog.getByRole('button', { name: 'Save & move' }));
  expect(await dialog.findByRole('alert')).toHaveTextContent('Destination changed');
  expect(screen.getByRole('dialog')).toBeVisible();
  await user.click(dialog.getByRole('button', { name: 'Save & move' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
});

it('dragging a saved tree request opens the same move confirmation without writing', async () => {
  const { user, source, request, target } = await setupMove();
  const values = new Map<string, string>();
  const dataTransfer = {
    types: [] as string[],
    effectAllowed: '',
    dropEffect: '',
    setData: (type: string, value: string) => {
      values.set(type, value);
      dataTransfer.types.push(type);
    },
    getData: (type: string) => values.get(type) ?? '',
  };
  const tree = within(screen.getByRole('region', { name: 'Project collections' }));
  fireEvent.dragStart(tree.getByRole('button', { name: 'GET Saved movable request' }), {
    dataTransfer,
  });
  expect(JSON.parse(values.get('application/x-fluxkit-request')!)).toEqual({
    handle: stored.handle,
    collectionId: source.id,
    requestId: request.id,
  });
  fireEvent.dragOver(tree.getByRole('button', { name: 'Destination' }), { dataTransfer });
  fireEvent.drop(tree.getByRole('button', { name: 'Destination' }), { dataTransfer });
  const dialog = within(await screen.findByRole('dialog', { name: 'Move request' }));
  expect(dialog.getByRole('combobox', { name: 'Destination collection' })).toHaveTextContent(
    target.name,
  );
  expect(native.moveRequest).not.toHaveBeenCalled();
  await user.click(dialog.getByRole('button', { name: 'Cancel' }));
});

it('requires explicit confirmation when moving between different imported OpenAPI documents', async () => {
  const { user, source, target, request } = await setupMove();
  const sourceView = stored.collections.find((item) => item.data.id === source.id)!;
  const targetView = stored.collections.find((item) => item.data.id === target.id)!;
  sourceView.data.openApi = {
    version: '3.1.0',
    document: { info: { title: 'Original' } },
    operations: { [request.id]: '#/paths/~1items/get' },
  };
  targetView.data.openApi = {
    version: '3.1.0',
    document: { info: { title: 'Different' } },
    operations: {},
  };
  await user.click(screen.getByRole('button', { name: 'Reload' }));
  await user.click(screen.getByRole('button', { name: 'GET Saved movable request' }));
  await user.click(screen.getByRole('button', { name: 'Move…' }));
  const dialog = within(screen.getByRole('dialog'));
  await selectOption(
    user,
    dialog.getByRole('combobox', { name: 'Destination collection' }),
    target.id,
  );
  expect(dialog.getByRole('button', { name: 'Save & move' })).toBeDisabled();
  expect(native.moveRequest).not.toHaveBeenCalled();
  await user.click(dialog.getByRole('checkbox'));
  await user.click(dialog.getByRole('button', { name: 'Save & move' }));
  await waitFor(() =>
    expect(native.moveRequest).toHaveBeenCalledWith(
      stored.handle,
      expect.objectContaining({ allowMetadataLoss: true }),
    ),
  );
});

it('loads a fresh server workspace while retaining saved edits as a read-only recovery project', async () => {
  stored.serverOwned = true;
  stored.serverSync = true;
  vi.mocked(native.restoreLastProject).mockResolvedValue(structuredClone(stored));
  const recovery = { ...structuredClone(stored), readOnly: true, serverSync: false };
  const fresh = {
    ...structuredClone(stored),
    handle: 'fresh-handle',
    directory: '/synthetic/fresh',
    data: { ...stored.data, name: 'Remote saved name' },
  };
  vi.mocked(native.refreshServerProject).mockResolvedValue({ recovery, workspace: fresh });
  const { result } = renderHook(() => useWorkspace());
  await waitFor(() => expect(result.current.busy).toBe(false));
  await act(async () => {
    await result.current.refreshServerProject(stored.handle);
  });
  expect(result.current.projects).toEqual([recovery, fresh]);
  expect(result.current.activeHandle).toBe(fresh.handle);
  expect(native.closeProject).not.toHaveBeenCalled();
});

it('does not replace server workspaces while another editor has unsaved changes', async () => {
  const { result } = renderHook(() => useWorkspace(true));
  await waitFor(() => expect(result.current.busy).toBe(false));
  await expect(result.current.refreshServerProject('workspace-handle')).rejects.toThrow(
    'Save or close unsaved editors',
  );
  expect(native.refreshServerProject).not.toHaveBeenCalled();
});

it('restores a retained recovery copy without replacing it with the current server workspace', async () => {
  stored.readOnly = true;
  stored.serverOwned = true;
  stored.serverRecovery = true;
  vi.mocked(native.restoreLastProject).mockResolvedValue(structuredClone(stored));
  const { result } = renderHook(() => useWorkspace());
  await waitFor(() => expect(result.current.busy).toBe(false));
  expect(result.current.project?.serverRecovery).toBe(true);
  expect(result.current.project?.handle).toBe(stored.handle);
  expect(native.closeProject).not.toHaveBeenCalled();
});
