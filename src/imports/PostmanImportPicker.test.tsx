import { selectOption } from '../test/selectOption';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { PostmanImportPicker } from './PostmanImportPicker';
import { ImportDialog } from './ImportDialog';
import type { CollectionView } from '../projects/types';

function collection(id: string, imported: boolean): CollectionView {
  return {
    revision: '1',
    data: {
      id,
      name: id,
      schemaVersion: 1,
      requests: [],
      folders: [],
      ...(imported ? { postman: { version: '2.1', variables: {} } } : {}),
    },
  };
}
describe('Postman import selection', () => {
  it('requires an explicit imported collection for environments and passes its identity', async () => {
    const user = userEvent.setup();
    const choose = vi.fn();
    render(
      <PostmanImportPicker
        collections={[
          collection('Native collection', false),
          collection('Imported A', true),
          collection('Imported B', true),
        ]}
        onChoose={choose}
        onCancel={vi.fn()}
      />,
    );
    await user.click(screen.getByRole('combobox'));
    expect(screen.getByRole('option', { name: 'Imported A' })).toBeVisible();
    expect(screen.queryByRole('option', { name: 'Native collection' })).toBeNull();
    await selectOption(user, screen.getByRole('combobox'), 'Imported B');
    await user.click(screen.getByRole('button', { name: 'Choose environment JSON…' }));
    expect(choose).toHaveBeenCalledWith('Imported B');
    await user.click(screen.getByRole('button', { name: 'Choose collection JSON…' }));
    expect(choose).toHaveBeenLastCalledWith(null);
  });
  it('keeps environment import unavailable before a collection has been imported', () => {
    render(<PostmanImportPicker collections={[]} onChoose={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.getByRole('button', { name: 'Choose environment JSON…' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Choose collection JSON…' })).toBeEnabled();
  });
  it('previews a project-wide environment and its source-to-local variable mapping', async () => {
    const user = userEvent.setup();
    const commit = vi.fn();
    render(
      <ImportDialog
        preview={{
          id: 'preview',
          source: 'Postman',
          version: '2.1',
          environmentName: 'Production',
          collectionName: 'Imported A',
          requestCount: 0,
          folderCount: 0,
          variables: [{ sourceName: 'baseUrl', name: 'pm_abcd_1_baseUrl', hasDefault: true }],
          requests: [],
          warnings: ['The selected environment stays unchanged.'],
          blockers: [],
        }}
        projectName="Project"
        busy={false}
        started={false}
        error={null}
        onCancel={vi.fn()}
        onImport={commit}
      />,
    );
    expect(screen.getByRole('dialog', { name: 'Import Postman' })).toHaveTextContent(
      'new project environment Production',
    );
    await user.click(screen.getByText('Show variable definitions'));
    expect(screen.getByText('pm_abcd_1_baseUrl')).toBeVisible();
    expect(screen.getByText('baseUrl')).toBeVisible();
    await user.click(screen.getByRole('checkbox'));
    await user.click(screen.getByRole('button', { name: 'Import environment' }));
    expect(commit).toHaveBeenCalledWith(false);
  });
});
