import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import * as native from '../native';
import { ServerUsers } from './ServerUsers';

vi.mock('../native');
const onClose = vi.fn();
const accounts: native.ServerAccountAdminView[] = [
  {
    id: 'admin-id',
    username: 'synthetic-admin',
    status: 'active',
    isServerAdmin: true,
    createdAt: '2026-10-01T08:00:00Z',
    personalCollections: 0,
    teamProjects: 2,
  },
  {
    id: 'anna-id',
    username: 'anna',
    status: 'active',
    isServerAdmin: false,
    createdAt: '2026-10-02T08:00:00Z',
    personalCollections: 3,
    teamProjects: 1,
  },
  { id: 'ben-id', username: 'ben', status: 'pending', isServerAdmin: false },
  { id: 'cleo-id', username: 'cleo', status: 'disabled', isServerAdmin: false },
];

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(native.getServerAccount).mockResolvedValue({
    id: 'admin-id',
    username: 'synthetic-admin',
    isServerAdmin: true,
  });
  vi.mocked(native.listServerAdminUsers).mockResolvedValue(structuredClone(accounts));
  vi.mocked(native.approveServerUser).mockResolvedValue();
  vi.mocked(native.setServerUserStatus).mockResolvedValue();
  vi.mocked(native.deleteServerUser).mockResolvedValue();
});

function row(name: string) {
  return within(screen.getByRole('list', { name: 'Registered accounts' }))
    .getByText(name)
    .closest('li')!;
}

it('lists every registered account with status, registration date and filters', async () => {
  const user = userEvent.setup();
  render(<ServerUsers onClose={onClose} />);
  const list = await screen.findByRole('list', { name: 'Registered accounts' });
  expect(within(list).getAllByRole('listitem')).toHaveLength(4);
  expect(row('anna')).toHaveTextContent('Active');
  expect(row('anna')).toHaveTextContent(new Date('2026-10-02T08:00:00Z').toLocaleDateString());
  expect(row('ben')).toHaveTextContent('Awaiting approval');
  expect(row('synthetic-admin')).toHaveTextContent('you');
  await user.click(screen.getByRole('button', { name: /Awaiting approval/ }));
  expect(within(list).getAllByRole('listitem')).toHaveLength(1);
  await user.click(screen.getByRole('button', { name: /^All/ }));
  await user.type(screen.getByLabelText('Search users'), 'cle');
  expect(within(list).getAllByRole('listitem')).toHaveLength(1);
  expect(row('cleo')).toHaveTextContent('Disabled');
});

it('approves, disables and reactivates accounts but never the own account', async () => {
  const user = userEvent.setup();
  render(<ServerUsers onClose={onClose} />);
  await screen.findByRole('list', { name: 'Registered accounts' });
  expect(within(row('synthetic-admin')).getByRole('button', { name: /Disable/ })).toBeDisabled();
  expect(within(row('synthetic-admin')).getByRole('button', { name: /Delete/ })).toBeDisabled();
  await user.click(within(row('ben')).getByRole('button', { name: 'Approve' }));
  await waitFor(() => expect(native.approveServerUser).toHaveBeenCalledWith('ben-id'));
  await user.click(within(row('anna')).getByRole('button', { name: 'Disable anna' }));
  await waitFor(() =>
    expect(native.setServerUserStatus).toHaveBeenCalledWith('anna-id', 'disabled'),
  );
  expect(await screen.findByRole('status')).toHaveTextContent('Disabled “anna”');
  await user.click(within(row('cleo')).getByRole('button', { name: 'Reactivate cleo' }));
  await waitFor(() => expect(native.setServerUserStatus).toHaveBeenCalledWith('cleo-id', 'active'));
});

it('deletes an account only after typing its username and says what is removed', async () => {
  const user = userEvent.setup();
  render(<ServerUsers onClose={onClose} />);
  await screen.findByRole('list', { name: 'Registered accounts' });
  await user.click(screen.getByRole('button', { name: 'Delete anna' }));
  const confirm = screen.getByRole('region', { name: 'Confirm account deletion' });
  expect(confirm).toHaveTextContent('personal project (3 collections)');
  expect(confirm).toHaveTextContent('membership in 1 team project');
  const remove = within(confirm).getByRole('button', { name: 'Delete account' });
  expect(remove).toBeDisabled();
  await user.type(within(confirm).getByLabelText('Username to confirm deletion'), 'ann');
  expect(remove).toBeDisabled();
  await user.click(within(confirm).getByRole('button', { name: 'Keep account' }));
  expect(native.deleteServerUser).not.toHaveBeenCalled();

  vi.mocked(native.listServerAdminUsers).mockResolvedValue(
    structuredClone(accounts.filter((item) => item.id !== 'anna-id')),
  );
  await user.click(screen.getByRole('button', { name: 'Delete anna' }));
  const again = screen.getByRole('region', { name: 'Confirm account deletion' });
  await user.type(within(again).getByLabelText('Username to confirm deletion'), 'anna');
  await user.click(within(again).getByRole('button', { name: 'Delete account' }));
  await waitFor(() => expect(native.deleteServerUser).toHaveBeenCalledWith('anna-id'));
  expect(await screen.findByRole('status')).toHaveTextContent('Deleted “anna”');
  expect(
    screen.queryByRole('region', { name: 'Confirm account deletion' }),
  ).not.toBeInTheDocument();
  expect(
    within(screen.getByRole('list', { name: 'Registered accounts' })).queryByText('anna'),
  ).not.toBeInTheDocument();
});

it('keeps the confirmation open and shows why the server refused a deletion', async () => {
  const user = userEvent.setup();
  vi.mocked(native.deleteServerUser).mockRejectedValue(
    'At least one active server administrator must remain.',
  );
  render(<ServerUsers onClose={onClose} />);
  await screen.findByRole('list', { name: 'Registered accounts' });
  await user.click(screen.getByRole('button', { name: 'Delete anna' }));
  await user.type(screen.getByLabelText('Username to confirm deletion'), 'anna');
  await user.click(screen.getByRole('button', { name: 'Delete account' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('At least one active');
  expect(screen.getByRole('region', { name: 'Confirm account deletion' })).toBeVisible();
});
