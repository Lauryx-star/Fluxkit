import { describe, expect, it } from 'vitest';
import {
  asSaved,
  restoredDrafts,
  sessionInput,
  toggleExpanded,
  treeIds,
  withExpanded,
  withoutEdits,
} from './session';
import { emptyDraft, fromRequest, isDirty, type ProjectView, type SavedRequest } from './types';

const saved: SavedRequest = {
  id: 'request',
  name: 'List items',
  folderId: null,
  method: 'GET',
  url: '{{baseUrl}}/items',
  params: [],
  headers: [],
  body: { kind: 'none', text: '', fields: [], file: null },
};
const project = { handle: 'one' } as ProjectView;

describe('session tabs', () => {
  it('lists projects and tabs in order with the active tab index and layout', () => {
    const clean = fromRequest(3, saved, 'one', 'collection');
    const loose = emptyDraft(7, null, null, null);
    expect(sessionInput([project], 'one', [clean, loose], 7, 0.6)).toEqual({
      projects: ['one'],
      activeProject: 'one',
      tabs: [
        { project: 'one', draft: clean },
        { project: null, draft: loose },
      ],
      activeTab: 1,
      requestPaneRatio: 0.6,
      treeExpanded: null,
    });
  });

  it('expands created branches last and leaves an unremembered state fully expanded', () => {
    expect(withExpanded(['a', 'b'], ['b', 'c'])).toEqual(['a', 'b', 'c']);
    expect(withExpanded(null, ['c'])).toBeNull();
  });

  it('starts remembering tree expansion from all open branches at the first toggle', () => {
    const view = {
      handle: 'one',
      collections: [
        { revision: '1', data: { id: 'c1', folders: [{ id: 'f1' }, { id: 'f2' }] } },
        { revision: '1', data: { id: 'c2', folders: [] } },
      ],
    } as unknown as ProjectView;
    expect(treeIds([view])).toEqual(['c1', 'f1', 'f2', 'c2']);
    const first = toggleExpanded(null, 'f1', [view]);
    expect(first).toEqual(['c1', 'f2', 'c2']);
    // Re-expanding moves the id to the end; ids of closed projects are kept.
    expect(toggleExpanded(['closed', ...first], 'f1', [view])).toEqual([
      'closed',
      'c1',
      'f2',
      'c2',
      'f1',
    ]);
  });

  it('discarding drops new tabs and returns edited tabs to their saved version', () => {
    const clean = fromRequest(1, saved, 'one', 'collection');
    const edited = { ...fromRequest(2, saved, 'one', 'collection'), url: 'https://edited.invalid' };
    const fresh = emptyDraft(3, 'one', 'collection', null);
    const result = withoutEdits([clean, edited, fresh]);
    expect(result).toHaveLength(2);
    expect(result[0]).toBe(clean);
    expect(result[1]).toMatchObject({ id: 2, url: '{{baseUrl}}/items', projectHandle: 'one' });
    expect(isDirty(result[1]!)).toBe(false);
  });

  it('marks every tab as saved after Save & quit', () => {
    const edited = { ...fromRequest(2, saved, 'one', 'collection'), url: 'https://edited.invalid' };
    const [result] = asSaved([edited]);
    expect(isDirty(result!)).toBe(false);
    expect(result!.url).toBe('https://edited.invalid');
  });

  it('restores tabs with new ids, new handles, and keeps tabs of missing projects loose', () => {
    const edited = {
      ...fromRequest(9, saved, 'old-one', 'collection'),
      url: 'https://edited.invalid',
    };
    const orphan = fromRequest(4, saved, 'old-two', 'collection');
    const { drafts, ids } = restoredDrafts(
      [
        { project: 'old-one', draft: edited },
        { project: null, draft: { name: 'broken' } },
        { project: 'old-two', draft: orphan },
      ],
      new Map([['old-one', 'new-one']]),
    );
    expect(ids).toEqual([1, null, 2]);
    expect(drafts[0]).toMatchObject({
      id: 1,
      projectHandle: 'new-one',
      collectionId: 'collection',
      url: 'https://edited.invalid',
    });
    expect(isDirty(drafts[0]!)).toBe(true);
    expect(drafts[1]).toMatchObject({ id: 2, projectHandle: null, collectionId: null });
  });
});
