import { selectOption } from '../test/selectOption';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ExportDialog } from './ExportDialog';
import type { ProjectView } from '../projects/types';
import type { ExportPreview } from './types';
const project: ProjectView = {
  handle: 'project',
  directory: '/synthetic',
  revision: '1',
  data: { id: 'project-id', name: 'Synthetic', schemaVersion: 1 },
  collections: [
    {
      revision: '1',
      data: {
        id: 'collection',
        name: 'API',
        schemaVersion: 1,
        folders: [{ id: 'folder', name: 'Group', parentId: null }],
        requests: [
          {
            id: 'request',
            name: 'Get item',
            method: 'GET',
            url: 'https://synthetic.invalid/item',
            folderId: 'folder',
            params: [],
            headers: [],
            body: { kind: 'none', text: '', fields: [], file: null },
          },
        ],
      },
    },
  ],
};
const preview: ExportPreview = {
  id: 'preview',
  title: 'Synthetic',
  directory: false,
  folderName: 'unused',
  files: [{ name: 'api.yml', operations: 1, bytes: 512 }],
  warnings: ['No local values are exported.'],
  blockers: [],
};
describe('OpenAPI export', () => {
  it('selects each saved scope by identity and warns about unsaved changes', async () => {
    const user = userEvent.setup();
    const prepare = vi.fn();
    render(
      <ExportDialog
        project={project}
        dirty
        preview={null}
        busy={false}
        error={null}
        onPreview={prepare}
        onSave={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent('saved versions only');
    await user.click(screen.getByRole('button', { name: 'Preview export' }));
    expect(prepare).toHaveBeenLastCalledWith({ kind: 'project' });
    await selectOption(user, screen.getByLabelText('Export scope'), 'collection');
    await user.click(screen.getByRole('button', { name: 'Preview export' }));
    expect(prepare).toHaveBeenLastCalledWith({ kind: 'collection', collection: 'collection' });
    await selectOption(user, screen.getByLabelText('Export scope'), 'folder');
    await user.click(screen.getByRole('button', { name: 'Preview export' }));
    expect(prepare).toHaveBeenLastCalledWith({
      kind: 'folder',
      collection: 'collection',
      folder: 'folder',
    });
    await selectOption(user, screen.getByLabelText('Export scope'), 'request');
    await user.click(screen.getByRole('button', { name: 'Preview export' }));
    expect(prepare).toHaveBeenLastCalledWith({
      kind: 'request',
      collection: 'collection',
      request: 'request',
    });
  });
  it('blocks conflicts and shows retryable save errors without discarding the preview', async () => {
    const save = vi.fn();
    const props = {
      project,
      dirty: false,
      preview: { ...preview, blockers: ['Duplicate method and path.'] },
      busy: false,
      error: null,
      onPreview: vi.fn(),
      onSave: save,
      onClose: vi.fn(),
    };
    const { rerender } = render(<ExportDialog {...props} />);
    expect(screen.getByRole('alert', { name: 'Export blockers' })).toHaveTextContent('Duplicate');
    expect(screen.getByRole('button', { name: 'Save YAML…' })).toBeDisabled();
    rerender(
      <ExportDialog
        {...props}
        preview={preview}
        error="File already exists. Nothing overwritten."
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('Nothing overwritten');
    await userEvent.click(screen.getByRole('button', { name: 'Save YAML…' }));
    expect(save).toHaveBeenCalled();
  });
});
