import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { serverVaultAction } from '../native';
import { VaultLock } from './VaultLock';

vi.mock('../native');

const locked = { configured: true, unlocked: false };
const unlocked = { configured: true, unlocked: true };

beforeEach(() => {
  vi.mocked(serverVaultAction).mockReset();
});

describe('status-bar vault lock', () => {
  it('unlocks a locked vault with the master password from a dialog', async () => {
    const user = userEvent.setup();
    vi.mocked(serverVaultAction).mockImplementation(async (action) =>
      action === 'unlock' ? unlocked : locked,
    );
    render(<VaultLock refreshKey={0} onSetup={vi.fn()} />);
    await user.click(await screen.findByRole('button', { name: 'Vault locked. Unlock vault' }));
    const dialog = screen.getByRole('dialog', { name: 'Unlock vault' });
    expect(dialog).toBeVisible();
    await user.type(screen.getByLabelText('Master password'), 'synthetic master password');
    await user.click(screen.getByRole('button', { name: 'Unlock' }));
    expect(serverVaultAction).toHaveBeenCalledWith('unlock', 'synthetic master password');
    expect(await screen.findByRole('button', { name: 'Vault unlocked. Lock vault' })).toBeVisible();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('keeps the dialog open with the error after a wrong password', async () => {
    const user = userEvent.setup();
    vi.mocked(serverVaultAction).mockImplementation(async (action) => {
      if (action === 'unlock') {
        throw 'The master password is incorrect.';
      }
      return locked;
    });
    render(<VaultLock refreshKey={0} onSetup={vi.fn()} />);
    await user.click(await screen.findByRole('button', { name: 'Vault locked. Unlock vault' }));
    await user.type(screen.getByLabelText('Master password'), 'wrong');
    await user.click(screen.getByRole('button', { name: 'Unlock' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('incorrect');
    expect(screen.getByRole('dialog', { name: 'Unlock vault' })).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('locks an unlocked vault with one click and without a dialog', async () => {
    const user = userEvent.setup();
    vi.mocked(serverVaultAction).mockImplementation(async (action) =>
      action === 'lock' ? locked : unlocked,
    );
    render(<VaultLock refreshKey={0} onSetup={vi.fn()} />);
    await user.click(await screen.findByRole('button', { name: 'Vault unlocked. Lock vault' }));
    expect(serverVaultAction).toHaveBeenCalledWith('lock');
    expect(await screen.findByRole('button', { name: 'Vault locked. Unlock vault' })).toBeVisible();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('points to the vault setup when no vault exists yet', async () => {
    const user = userEvent.setup();
    const onSetup = vi.fn();
    vi.mocked(serverVaultAction).mockResolvedValue({ configured: false, unlocked: false });
    render(<VaultLock refreshKey={0} onSetup={onSetup} />);
    await user.click(await screen.findByRole('button', { name: 'Vault locked. Unlock vault' }));
    await user.click(screen.getByRole('button', { name: 'Open settings' }));
    expect(onSetup).toHaveBeenCalled();
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
