import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { ProjectTree } from './ProjectTree';
it('accepts internal unsaved-draft drops on collections and folders', () => {
  const drop = vi.fn();
  render(
    <ProjectTree
      collections={[
        {
          revision: 'r',
          data: {
            schemaVersion: 1,
            id: 'collection',
            name: 'API',
            requests: [],
            folders: [{ id: 'folder', name: 'Routes', parentId: null }],
          },
        },
      ]}
      busy={false}
      onDropDraft={drop}
      onAuth={vi.fn()}
      onSelect={vi.fn()}
      onNew={vi.fn()}
      onFolder={vi.fn()}
      onRename={vi.fn()}
      onRemove={vi.fn()}
      onOpen={vi.fn()}
    />,
  );
  const dataTransfer = {
    types: ['application/x-fluxkit-draft'],
    getData: vi.fn(() => '3'),
    dropEffect: '',
  };
  fireEvent.dragOver(screen.getByRole('button', { name: 'API' }), { dataTransfer });
  expect(dataTransfer.dropEffect).toBe('move');
  fireEvent.drop(screen.getByRole('button', { name: 'API' }), { dataTransfer });
  expect(drop).toHaveBeenLastCalledWith(3, 'collection', null);
  fireEvent.drop(screen.getByRole('button', { name: 'Routes' }), { dataTransfer });
  expect(drop).toHaveBeenLastCalledWith(3, 'collection', 'folder');
});
