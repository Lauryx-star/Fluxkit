import { describe, expect, it } from 'vitest';
import { isServerConflict, mergeRefreshedView, refreshNotice } from './serverRefresh';
import { fromRequest, type ProjectView, type SavedRequest } from './types';

function request(id: string, url: string, folderId: string | null = null): SavedRequest {
  return {
    id,
    name: `Request ${id}`,
    folderId,
    method: 'GET',
    url,
    params: [],
    headers: [],
    body: { kind: 'none', text: '', fields: [], file: null },
  };
}

function view(requests: SavedRequest[], collectionId = 'collection'): ProjectView {
  return {
    handle: 'server',
    directory: '/synthetic/server',
    revision: 'project',
    serverSync: true,
    data: { schemaVersion: 1, id: 'project', name: 'Synthetic' },
    collections: [
      {
        revision: 'collection-revision',
        data: {
          schemaVersion: 1,
          id: collectionId,
          name: 'Synthetic API',
          auth: { kind: 'none' },
          folders: [{ id: 'folder', name: 'Folder', parentId: null }],
          requests,
        },
      },
    ],
  };
}

const original = request('a', 'https://synthetic.invalid/v0');

describe('mergeRefreshedView', () => {
  it('replaces unedited editors with the server version and keeps their editor id', () => {
    const draft = fromRequest(4, original, 'server', 'collection');
    const outcome = mergeRefreshedView(
      [draft],
      view([request('a', 'https://synthetic.invalid/v1')]),
    );
    expect(outcome.drafts).toHaveLength(1);
    expect(outcome.drafts[0]).toMatchObject({ id: 4, url: 'https://synthetic.invalid/v1' });
    expect(refreshNotice(outcome)).toBeNull();
  });

  it('keeps the same object when nothing changed', () => {
    const draft = fromRequest(4, original, 'server', 'collection');
    expect(mergeRefreshedView([draft], view([original])).drafts[0]).toBe(draft);
  });

  it('keeps edits, flags the editor once and follows a move to another collection', () => {
    const draft = { ...fromRequest(4, original, 'server', 'collection'), url: 'mine' };
    const moved = view([request('a', 'https://synthetic.invalid/v1', 'folder')], 'other');
    const first = mergeRefreshedView([draft], moved);
    expect(first.drafts[0]).toMatchObject({
      url: 'mine',
      serverChanged: true,
      collectionId: 'other',
      folderId: 'folder',
    });
    expect(first.changed).toEqual(['Request a']);
    expect(refreshNotice(first)).toContain('“Request a” was changed on the server');
    const second = mergeRefreshedView(first.drafts, moved);
    expect(second.changed).toEqual([]);
    expect(second.drafts[0]).toMatchObject({ serverChanged: true });
  });

  it('closes unedited editors of deleted requests and flags edited ones', () => {
    const clean = fromRequest(1, original, 'server', 'collection');
    const edited = { ...fromRequest(2, request('b', 'x'), 'server', 'collection'), url: 'mine' };
    const outcome = mergeRefreshedView([clean, edited], view([]));
    expect(outcome.closed).toEqual([1]);
    expect(outcome.drafts).toEqual([{ ...edited, serverDeleted: true, serverChanged: false }]);
    expect(refreshNotice(outcome)).toContain('deleted on the server. Saving recreates it.');
  });

  it('leaves editors of other projects and never-saved editors alone', () => {
    const other = fromRequest(1, original, 'local', 'collection');
    const unsaved = { ...fromRequest(2, request('new', 'x'), 'server', 'collection') };
    unsaved.savedSnapshot = null;
    const outcome = mergeRefreshedView([other, unsaved], view([]));
    expect(outcome.drafts).toEqual([other, unsaved]);
    expect(outcome.closed).toEqual([]);
  });
});

it('recognizes write-through conflicts by their message', () => {
  expect(isServerConflict('Not saved: a request was changed by anna meanwhile.')).toBe(true);
  expect(isServerConflict('Could not reach the Fluxkit server.')).toBe(false);
  expect(isServerConflict(new Error('Not saved:'))).toBe(false);
});
