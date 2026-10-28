import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import * as native from '../native';
import type { ProjectView } from '../projects/types';
import { ServerConnection } from './ServerConnection';

vi.mock('../native');
const onOpenProject = vi.fn(async () => {});
const onRefreshProject = vi.fn<(handle: string) => Promise<void>>(async () => {});
const onClose = vi.fn();
const workspace: ProjectView = {
  handle: 'workspace-handle',
  directory: '/synthetic/workspace',
  revision: '1',
  serverOwned: true,
  serverSync: true,
  data: { schemaVersion: 1, id: 'project-id', name: 'Synthetic workspace' },
  collections: [],
};
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(native.serverVaultAction).mockResolvedValue({ configured: false, unlocked: false });
  vi.mocked(native.getServerConnection).mockResolvedValue({
    authenticated: true,
    serverUrl: 'https://synthetic.example.test',
    accountId: 'account-id',
  });
  vi.mocked(native.getServerAccount).mockResolvedValue({
    id: 'account-id',
    username: 'Synthetic user',
    isServerAdmin: false,
  });
  vi.mocked(native.listServerProjects).mockResolvedValue([
    {
      id: 'project-id',
      name: 'Shared project',
      kind: 'team',
      revision: 1,
      canManageMembers: true,
      canEdit: true,
    },
  ]);
  vi.mocked(native.listServerProjectMembers).mockResolvedValue([]);
});
function show(projects: ProjectView[] = []) {
  return render(
    <ServerConnection
      localProjects={projects}
      onOpenProject={onOpenProject}
      onRefreshProject={onRefreshProject}
      onClose={onClose}
    />,
  );
}

it('adds a member through the server dialog', async () => {
  const user = userEvent.setup();
  show();
  await user.click(await screen.findByRole('tab', { name: 'Projects' }));
  await user.click(await screen.findByRole('button', { name: 'Manage members' }));
  await user.type(screen.getByLabelText('Add approved account by username'), 'new-user');
  await user.click(screen.getByRole('button', { name: 'Add member' }));
  await waitFor(() =>
    expect(native.addServerProjectMember).toHaveBeenCalledWith('project-id', 'new-user', 'member'),
  );
});

it('requires closing server projects before sign-out without discarding work', async () => {
  const user = userEvent.setup();
  show([workspace]);
  await user.click(await screen.findByRole('button', { name: 'Sign out' }));
  expect(screen.getByRole('alert')).toHaveTextContent('Close all server projects');
  expect(native.logoutFromServer).not.toHaveBeenCalled();
  expect(native.closeProject).not.toHaveBeenCalled();
});

it('allows signing out while unrelated local projects stay open', async () => {
  const user = userEvent.setup();
  vi.mocked(native.logoutFromServer).mockResolvedValue({
    authenticated: false,
    serverUrl: 'https://synthetic.example.test',
  });
  show([{ ...workspace, serverOwned: false, serverSync: false }]);
  await user.click(await screen.findByRole('button', { name: 'Sign out' }));
  expect(await screen.findByText('Signed out from the server.')).toBeVisible();
  expect(native.closeProject).not.toHaveBeenCalled();
});

it('keeps sync conflicts visible and requires an explicit recovery choice', async () => {
  const user = userEvent.setup();
  vi.mocked(native.syncServerProject).mockRejectedValue(
    'The server project changed. Local edits remain saved.',
  );
  show([workspace]);
  await user.click(await screen.findByRole('tab', { name: 'Workspaces' }));
  await user.click(await screen.findByRole('button', { name: 'Sync saved changes' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Local edits remain saved');
  expect(onRefreshProject).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Load server version' }));
  expect(screen.getByRole('region', { name: 'Confirm workspace recovery' })).toHaveTextContent(
    'read-only recovery copy',
  );
  await user.click(screen.getByRole('button', { name: 'Cancel recovery' }));
  expect(onRefreshProject).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Load server version' }));
  await user.click(screen.getByRole('button', { name: 'Keep recovery copy and load' }));
  await waitFor(() => expect(onRefreshProject).toHaveBeenCalledWith(workspace.handle));
  expect(onClose).toHaveBeenCalled();
});

it('retains the recovery dialog and error when unsaved editors or network failure block refresh', async () => {
  const user = userEvent.setup();
  onRefreshProject.mockRejectedValueOnce(
    new Error('Save or close unsaved editors before loading the server version.'),
  );
  show([workspace]);
  await user.click(await screen.findByRole('tab', { name: 'Workspaces' }));
  await user.click(await screen.findByRole('button', { name: 'Load server version' }));
  await user.click(screen.getByRole('button', { name: 'Keep recovery copy and load' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Save or close unsaved editors');
  expect(onClose).not.toHaveBeenCalled();
  expect(screen.getByRole('button', { name: 'Keep recovery copy and load' })).toBeEnabled();
});

it('discovers OIDC before showing password fields and signs in through the browser', async () => {
  const user = userEvent.setup();
  vi.mocked(native.getServerConnection).mockResolvedValue({
    authenticated: false,
    serverUrl: 'https://synthetic.example.test',
  });
  vi.mocked(native.setServerConnection).mockResolvedValue({
    authenticated: false,
    serverUrl: 'https://synthetic.example.test',
  });
  vi.mocked(native.getServerAuthMode).mockResolvedValue({ mode: 'oidc' });
  vi.mocked(native.loginWithOidc).mockResolvedValue({
    authenticated: true,
    serverUrl: 'https://synthetic.example.test',
    accountId: 'account-id',
  });
  show();
  await waitFor(() =>
    expect(screen.getByLabelText('Server URL')).toHaveValue('https://synthetic.example.test'),
  );
  expect(screen.queryByLabelText('Server password')).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Continue' }));
  await waitFor(() => expect(native.loginWithOidc).toHaveBeenCalledOnce());
  expect(await screen.findByRole('button', { name: 'Sign out' })).toBeVisible();
  expect(native.loginToServer).not.toHaveBeenCalled();
  expect(native.registerServerAccount).not.toHaveBeenCalled();
});

it('keeps pending approval actionable and permits cancelling browser sign-in', async () => {
  const user = userEvent.setup();
  vi.mocked(native.getServerConnection).mockResolvedValue({
    authenticated: false,
    serverUrl: 'https://synthetic.example.test',
  });
  vi.mocked(native.setServerConnection).mockResolvedValue({
    authenticated: false,
    serverUrl: 'https://synthetic.example.test',
  });
  vi.mocked(native.getServerAuthMode).mockResolvedValue({ mode: 'oidc' });
  let reject!: (reason: string) => void;
  vi.mocked(native.loginWithOidc).mockImplementation(
    () =>
      new Promise((_resolve, fail) => {
        reject = fail;
      }),
  );
  vi.mocked(native.cancelOidcLogin).mockImplementation(async () => {
    reject('Browser sign-in cancelled.');
  });
  show();
  await waitFor(() =>
    expect(screen.getByLabelText('Server URL')).toHaveValue('https://synthetic.example.test'),
  );
  await user.click(screen.getByRole('button', { name: 'Continue' }));
  await user.click(await screen.findByRole('button', { name: 'Cancel browser sign-in' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('cancelled');
  vi.mocked(native.loginWithOidc).mockRejectedValue(
    'Your OIDC account is awaiting administrator approval. Sign in again after approval.',
  );
  await user.click(screen.getByRole('button', { name: 'Sign in with OIDC' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('awaiting administrator approval');
  expect(screen.queryByLabelText('Server password')).not.toBeInTheDocument();
});

it('keeps local credentials available only after discovering local account mode', async () => {
  const user = userEvent.setup();
  vi.mocked(native.getServerConnection).mockResolvedValue({
    authenticated: false,
    serverUrl: 'https://synthetic.example.test',
  });
  vi.mocked(native.setServerConnection).mockResolvedValue({
    authenticated: false,
    serverUrl: 'https://synthetic.example.test',
  });
  vi.mocked(native.getServerAuthMode).mockResolvedValue({ mode: 'local' });
  vi.mocked(native.loginToServer).mockResolvedValue({
    authenticated: true,
    serverUrl: 'https://synthetic.example.test',
    accountId: 'account-id',
  });
  show();
  await waitFor(() =>
    expect(screen.getByLabelText('Server URL')).toHaveValue('https://synthetic.example.test'),
  );
  await user.click(screen.getByRole('button', { name: 'Continue' }));
  await user.type(await screen.findByLabelText('Server username'), 'synthetic');
  await user.type(screen.getByLabelText('Server password'), 'Synthetic password');
  await user.click(screen.getByRole('button', { name: 'Connect' }));
  await waitFor(() =>
    expect(native.loginToServer).toHaveBeenCalledWith('synthetic', 'Synthetic password'),
  );
  expect(native.loginWithOidc).not.toHaveBeenCalled();
});
