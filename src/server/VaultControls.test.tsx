import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import { serverVaultAction } from '../native';
import { VaultControls } from './VaultControls';
vi.mock('../native');
beforeEach(() => vi.resetAllMocks());
it('requires matching master passwords and clears input after explicit vault creation', async () => {
  const user = userEvent.setup();
  vi.mocked(serverVaultAction).mockImplementation(async (action) => ({
    configured: action !== 'status',
    unlocked: action === 'create',
  }));
  render(<VaultControls busy={false} onBusy={vi.fn()} />);
  await screen.findByText('No account vault configured.');
  await user.type(screen.getByLabelText('Master password'), 'Synthetic master password 42');
  await user.type(screen.getByLabelText('Confirm master password'), 'different');
  await user.click(screen.getByRole('button', { name: 'Create vault' }));
  expect(screen.getByRole('alert')).toHaveTextContent('do not match');
  expect(serverVaultAction).toHaveBeenCalledTimes(1);
  await user.clear(screen.getByLabelText('Confirm master password'));
  await user.type(screen.getByLabelText('Confirm master password'), 'Synthetic master password 42');
  await user.click(screen.getByRole('button', { name: 'Create vault' }));
  await screen.findByText('Vault unlocked for this session.');
  expect(serverVaultAction).toHaveBeenCalledWith('create', 'Synthetic master password 42');
  expect(screen.queryByLabelText('Master password')).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Lock vault' }));
  await screen.findByText('Vault locked.');
  expect(screen.getByLabelText('Master password')).toHaveValue('');
  expect(serverVaultAction).toHaveBeenCalledWith('lock', undefined);
});
it('wrong-password failures keep the vault locked and offer no destructive reset', async () => {
  const user = userEvent.setup();
  vi.mocked(serverVaultAction).mockResolvedValue({ configured: true, unlocked: false });
  render(<VaultControls busy={false} onBusy={vi.fn()} />);
  await screen.findByText('Vault locked.');
  vi.mocked(serverVaultAction).mockRejectedValueOnce(
    'The master password is incorrect. No values were changed.',
  );
  await user.type(screen.getByLabelText('Master password'), 'Synthetic wrong password');
  await user.click(screen.getByRole('button', { name: 'Unlock vault' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('No values were changed');
  expect(screen.getByText('Vault locked.')).toBeVisible();
  expect(screen.queryByRole('button', { name: /reset|create/i })).not.toBeInTheDocument();
});
it('changes a password explicitly after unlock without submitting the parent sign-in form', async () => {
  const user = userEvent.setup();
  const signIn = vi.fn((event: React.FormEvent) => event.preventDefault());
  vi.mocked(serverVaultAction).mockResolvedValue({ configured: true, unlocked: true });
  render(
    <form onSubmit={signIn}>
      <VaultControls busy={false} onBusy={vi.fn()} />
    </form>,
  );
  await user.click(await screen.findByRole('button', { name: 'Change master password' }));
  await user.type(screen.getByLabelText('New master password'), 'Synthetic new master password');
  await user.type(
    screen.getByLabelText('Confirm master password'),
    'Synthetic new master password{Enter}',
  );
  await waitFor(() =>
    expect(serverVaultAction).toHaveBeenCalledWith(
      'changePassword',
      'Synthetic new master password',
    ),
  );
  expect(signIn).not.toHaveBeenCalled();
});
