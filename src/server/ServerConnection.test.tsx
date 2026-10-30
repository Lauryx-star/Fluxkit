import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import * as native from '../native';
import type { ProjectView } from '../projects/types';
import { ServerConnection } from './ServerConnection';

vi.mock('../native');
const onOpenProject = vi.fn(async () => {});
const onRefreshProject = vi.fn<(handle: string) => Promise<void>>(async () => {});
const onDeleteProject = vi.fn<(projectId: string, revision: number) => Promise<boolean>>(
  async () => true,
);
const onClose = vi.fn();
const onManageUsers = vi.fn();
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
  onOpenProject.mockResolvedValue(undefined);
  onRefreshProject.mockResolvedValue(undefined);
  onDeleteProject.mockResolvedValue(true);
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
      onDeleteProject={onDeleteProject}
      onManageUsers={onManageUsers}
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

it('refreshes a server workspace on request without a manual sync step', async () => {
  const user = userEvent.setup();
  show([workspace]);
  await user.click(await screen.findByRole('tab', { name: 'Workspaces' }));
  expect(screen.queryByRole('button', { name: 'Sync saved changes' })).not.toBeInTheDocument();
  await user.click(await screen.findByRole('button', { name: 'Refresh from server' }));
  await waitFor(() => expect(onRefreshProject).toHaveBeenCalledWith(workspace.handle));
  expect(await screen.findByRole('status')).toHaveTextContent(
    'Loaded the latest server version of “Synthetic workspace”',
  );
  expect(onClose).not.toHaveBeenCalled();
});

it('keeps the workspace list usable when a refresh fails', async () => {
  const user = userEvent.setup();
  onRefreshProject.mockRejectedValueOnce('Could not reach the Fluxkit server.');
  show([workspace]);
  await user.click(await screen.findByRole('tab', { name: 'Workspaces' }));
  await user.click(await screen.findByRole('button', { name: 'Refresh from server' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Could not reach the Fluxkit server');
  expect(screen.getByRole('button', { name: 'Refresh from server' })).toBeEnabled();
});

it('shows when each project was created and how many collections it has', async () => {
  const user = userEvent.setup();
  vi.mocked(native.listServerProjects).mockResolvedValue([
    {
      id: 'project-id',
      name: 'Shared project',
      kind: 'team',
      revision: 1,
      canEdit: true,
      createdAt: '2026-10-01T08:00:00Z',
      collectionCount: 3,
    },
  ]);
  show();
  await user.click(await screen.findByRole('tab', { name: 'Projects' }));
  const item = (await screen.findByText(/Shared project/)).closest('li')!;
  expect(item).toHaveTextContent(new Date('2026-10-01T08:00:00Z').toLocaleDateString());
  expect(item).toHaveTextContent('3 collections');
  expect(screen.queryByRole('button', { name: 'Delete project' })).not.toBeInTheDocument();
});

it('lets server administrators delete only empty team projects after confirming', async () => {
  const user = userEvent.setup();
  vi.mocked(native.getServerAccount).mockResolvedValue({
    id: 'account-id',
    username: 'Synthetic admin',
    isServerAdmin: true,
  });
  vi.mocked(native.listServerAdminUsers).mockResolvedValue([]);
  vi.mocked(native.listServerProjects)
    .mockResolvedValueOnce([
      { id: 'empty-id', name: 'Empty team', kind: 'team', revision: 4, collectionCount: 0 },
      { id: 'full-id', name: 'Busy team', kind: 'team', revision: 2, collectionCount: 2 },
      { id: 'mine-id', name: 'Personal', kind: 'personal', revision: 1, collectionCount: 0 },
    ])
    .mockResolvedValueOnce([
      { id: 'full-id', name: 'Busy team', kind: 'team', revision: 2, collectionCount: 2 },
    ]);
  show();
  await user.click(await screen.findByRole('tab', { name: 'Projects' }));
  const buttons = await screen.findAllByRole('button', { name: 'Delete project' });
  expect(buttons).toHaveLength(1);
  await user.click(buttons[0]!);
  const confirm = screen.getByRole('region', { name: 'Confirm project deletion' });
  expect(confirm).toHaveTextContent('Delete “Empty team”?');
  await user.click(screen.getByRole('button', { name: 'Keep project' }));
  expect(onDeleteProject).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Delete project' }));
  const region = screen.getByRole('region', { name: 'Confirm project deletion' });
  await user.click(within(region).getByRole('button', { name: 'Delete project' }));
  await waitFor(() => expect(onDeleteProject).toHaveBeenCalledWith('empty-id', 4));
  expect(await screen.findByRole('status')).toHaveTextContent('Deleted “Empty team”');
  const list = screen.getByRole('region', { name: 'Server projects' });
  await waitFor(() => expect(list).not.toHaveTextContent('Empty team'));
  expect(list).toHaveTextContent('Busy team');
});

it('shows why the server refused to delete a project', async () => {
  const user = userEvent.setup();
  vi.mocked(native.getServerAccount).mockResolvedValue({
    id: 'account-id',
    username: 'Synthetic admin',
    isServerAdmin: true,
  });
  vi.mocked(native.listServerAdminUsers).mockResolvedValue([]);
  vi.mocked(native.listServerProjects).mockResolvedValue([
    { id: 'empty-id', name: 'Empty team', kind: 'team', revision: 4, collectionCount: 0 },
  ]);
  onDeleteProject.mockRejectedValueOnce(
    'Only empty projects can be deleted. Remove its collections, environments and variables first.',
  );
  show();
  await user.click(await screen.findByRole('tab', { name: 'Projects' }));
  await user.click(await screen.findByRole('button', { name: 'Delete project' }));
  const region = screen.getByRole('region', { name: 'Confirm project deletion' });
  await user.click(within(region).getByRole('button', { name: 'Delete project' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Only empty projects');
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

it('points administrators from the Administration step to the server user view', async () => {
  const user = userEvent.setup();
  vi.mocked(native.getServerAccount).mockResolvedValue({
    id: 'account-id',
    username: 'Synthetic admin',
    isServerAdmin: true,
  });
  vi.mocked(native.listServerAdminUsers).mockResolvedValue([
    { id: 'account-id', username: 'Synthetic admin', status: 'active', isServerAdmin: true },
    { id: 'pending-id', username: 'newcomer', status: 'pending', isServerAdmin: false },
  ]);
  show();
  await user.click(await screen.findByRole('tab', { name: 'Administration' }));
  const section = screen.getByRole('region', { name: 'Server account administration' });
  expect(section).toHaveTextContent('2 registered accounts, 1 awaiting approval.');
  await user.click(screen.getByRole('button', { name: 'Manage server users' }));
  expect(onManageUsers).toHaveBeenCalled();
});

it('offers to sign in again after an ended sign-in without closing open server projects', async () => {
  const user = userEvent.setup();
  vi.mocked(native.getServerConnection).mockResolvedValue({
    authenticated: false,
    serverUrl: 'https://synthetic.example.test',
    sessionEnded: true,
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
  show([workspace]);
  expect(await screen.findByText(/Your sign-in to this server has ended/)).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Sign in again' }));
  await waitFor(() => expect(native.loginWithOidc).toHaveBeenCalledOnce());
  expect(screen.queryByText(/Close all server projects/)).not.toBeInTheDocument();
  expect(await screen.findByRole('button', { name: 'Sign out' })).toBeVisible();
});
