import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import { SettingsDialog } from './SettingsDialog';

function show(onServerUsers?: () => void) {
  const noop = vi.fn();
  return render(
    <SettingsDialog
      onClose={noop}
      desktop
      onServerConnection={noop}
      serverConnectionDisabled={false}
      onServerUsers={onServerUsers}
      onNewLocalProject={noop}
      onOpenLocalProject={noop}
      localProjectDisabled={false}
      project={null}
      onRename={noop}
      renameDisabled
      onHistory={noop}
      historyDisabled
      onAuth={noop}
      authDisabled
      onImportOpenApi={noop}
      importOpenApiLabel="Import OpenAPI…"
      onImportPostman={noop}
      importDisabled
      onExport={noop}
      exportDisabled
    />,
  );
}

it('offers server user administration only to signed-in server administrators', async () => {
  const { unmount } = show();
  expect(screen.queryByRole('button', { name: 'Server users' })).not.toBeInTheDocument();
  unmount();
  const open = vi.fn();
  show(open);
  await userEvent.setup().click(screen.getByRole('button', { name: 'Server users' }));
  expect(open).toHaveBeenCalled();
});
