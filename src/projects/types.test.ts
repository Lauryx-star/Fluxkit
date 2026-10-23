import { expect, it } from 'vitest';
import { fromRequest, isDirty, type SavedRequest } from './types';

it('opens native-ordered authenticated requests cleanly and detects subsequent edits', () => {
  const request: SavedRequest = {
    id: 'request',
    name: 'Synthetic',
    auth: { kind: 'bearer', token: 'variable-id' },
    folderId: null,
    method: 'GET',
    url: 'https://example.test',
    params: [],
    headers: [],
    body: { kind: 'none', text: '', fields: [], file: null },
  };
  const draft = fromRequest(1, request, 'project', 'collection');
  expect(isDirty(draft)).toBe(false);
  expect(isDirty({ ...draft, name: 'Edited' })).toBe(true);
  expect(request.name).toBe('Synthetic');
});

it('normalizes null or omitted inherited authentication without dirtying a loaded request', () => {
  const request: SavedRequest = {
    id: 'request',
    name: 'Synthetic',
    folderId: null,
    method: 'GET',
    url: '',
    params: [],
    headers: [],
    body: { kind: 'none', text: '', fields: [], file: null },
  };
  expect(isDirty(fromRequest(1, request, 'project', 'collection'))).toBe(false);
  expect(isDirty(fromRequest(1, { ...request, auth: null }, 'project', 'collection'))).toBe(false);
});
