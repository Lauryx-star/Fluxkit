import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
import * as native from '../native';
import { TransferPreview } from './TransferPreview';
vi.mock('../native');
const preview: native.TransferPreview = {
  id: 'opaque-plan',
  name: 'Synthetic local project',
  collections: 1,
  requests: 2,
  ready: false,
  selected: 0,
  available: 0,
  missing: [],
  definitions: {
    schemaVersion: 1,
    environments: [],
    variables: [
      { id: 'ordinary-id', name: 'baseUrl', secret: false },
      { id: 'secret-id', name: 'token' },
    ],
  },
};
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(native.previewServerTransfer).mockResolvedValue(structuredClone(preview));
  vi.mocked(native.discardServerTransfer).mockResolvedValue();
});
it('selects no values automatically, requires preparation, and uploads only the reviewed plan', async () => {
  const user = userEvent.setup();
  const done = vi.fn(async () => {});
  vi.mocked(native.prepareServerTransfer).mockResolvedValue({
    ...preview,
    ready: true,
    selected: 1,
    available: 1,
  });
  vi.mocked(native.commitServerTransfer).mockResolvedValue({
    id: 'project',
    name: preview.name,
    kind: 'personal',
    revision: 1,
    snapshot: '{}',
  });
  render(
    <TransferPreview
      handle="local-handle"
      busy={false}
      onBusy={vi.fn()}
      onCancel={vi.fn()}
      onDone={done}
    />,
  );
  const shared = await screen.findByRole('checkbox', { name: /baseUrl/ });
  expect(shared).not.toBeChecked();
  expect(screen.getByRole('checkbox', { name: /token/ })).not.toBeChecked();
  expect(native.prepareServerTransfer).not.toHaveBeenCalled();
  expect(screen.getByRole('button', { name: 'Upload reviewed copy' })).toBeDisabled();
  await user.click(shared);
  await user.click(screen.getByRole('button', { name: 'Check selected values and prepare' }));
  expect(native.prepareServerTransfer).toHaveBeenCalledWith('local-handle', preview.id, [
    { variableId: 'ordinary-id', environmentId: 'default' },
  ]);
  await user.click(await screen.findByRole('button', { name: 'Upload reviewed copy' }));
  await waitFor(() => expect(done).toHaveBeenCalledWith(preview.name));
  expect(native.commitServerTransfer).toHaveBeenCalledWith('local-handle', preview.id);
});
it('allows definitions-only preparation without values and shows missing slots without their contents', async () => {
  const user = userEvent.setup();
  vi.mocked(native.prepareServerTransfer)
    .mockResolvedValueOnce({ ...preview, ready: true })
    .mockResolvedValueOnce({
      ...preview,
      ready: true,
      selected: 1,
      available: 0,
      missing: [{ variableId: 'secret-id', environmentId: 'default' }],
    });
  render(
    <TransferPreview
      handle="local-handle"
      busy={false}
      onBusy={vi.fn()}
      onCancel={vi.fn()}
      onDone={vi.fn()}
    />,
  );
  await user.click(
    await screen.findByRole('button', { name: 'Check selected values and prepare' }),
  );
  expect(native.prepareServerTransfer).toHaveBeenCalledWith('local-handle', preview.id, []);
  await user.click(screen.getByRole('checkbox', { name: /token/ }));
  expect(screen.getByRole('button', { name: 'Upload reviewed copy' })).toBeDisabled();
  await user.click(screen.getByRole('button', { name: 'Check selected values and prepare' }));
  expect(await screen.findByLabelText('Missing selected values')).toHaveTextContent(
    'token · Default: missing',
  );
});
it('cancels and discards the plan without uploading or reading selected OS entries', async () => {
  const user = userEvent.setup();
  const cancel = vi.fn();
  const view = render(
    <TransferPreview
      handle="local-handle"
      busy={false}
      onBusy={vi.fn()}
      onCancel={cancel}
      onDone={vi.fn()}
    />,
  );
  await screen.findByRole('checkbox', { name: /token/ });
  await user.click(screen.getByRole('button', { name: 'Cancel transfer' }));
  expect(cancel).toHaveBeenCalled();
  view.unmount();
  expect(native.discardServerTransfer).toHaveBeenCalledWith(preview.id);
  expect(native.prepareServerTransfer).not.toHaveBeenCalled();
  expect(native.commitServerTransfer).not.toHaveBeenCalled();
});
