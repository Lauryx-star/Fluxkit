import { describe, expect, it } from 'vitest';
import { effectiveAuth, manualAuthHint, validAuth } from './rules';
import type { ProjectView } from '../projects/types';

describe('authentication presentation', () => {
  it('reports nearest explicit source and stops at None across nested folders', () => {
    const project: ProjectView = {
      handle: 'project',
      directory: '/fixture',
      revision: '1',
      data: {
        schemaVersion: 1,
        id: 'project',
        name: 'Project',
        auth: { kind: 'bearer', token: 'token' },
      },
      collections: [
        {
          revision: '1',
          data: {
            schemaVersion: 1,
            id: 'api',
            name: 'API',
            auth: { kind: 'basic', username: 'user', password: 'password' },
            requests: [],
            folders: [
              { id: 'parent', name: 'Public', parentId: null, auth: { kind: 'none' } },
              { id: 'child', name: 'Nested', parentId: 'parent' },
            ],
          },
        },
      ],
    };
    expect(effectiveAuth(project, 'api', 'child')).toEqual({
      auth: { kind: 'none' },
      source: 'Folder: Public',
    });
    expect(effectiveAuth(project, 'api', null).source).toBe('Collection: API');
    expect(effectiveAuth(project, null, null).source).toBe('Project: Project');
    expect(effectiveAuth(project, 'api', 'child', { kind: 'bearer', token: 'other' }).source).toBe(
      'Request',
    );
    expect(effectiveAuth(project, 'api', 'missing').source).toContain('Missing folder');
  });
  it('requires existing variable identities without accepting credential text', () => {
    const variables = [{ id: 'token-id', name: 'token' }];
    expect(validAuth({ kind: 'bearer', token: 'literal-token' }, variables)).toBe(false);
    expect(validAuth({ kind: 'bearer', token: 'token-id' }, variables)).toBe(true);
    expect(
      validAuth({ kind: 'apiKey', name: '', value: 'token-id', placement: 'query' }, variables),
    ).toBe(false);
  });
});

it('marks manual header/query priority without resolving values', () => {
  const request = {
    headers: [{ id: 1, enabled: true, key: 'aUtHoRiZaTiOn', value: '{{secret}}' }],
    params: [],
    url: 'https://example.test/?access_key=',
  };
  expect(manualAuthHint({ kind: 'bearer', token: 'id' }, request)).toContain(
    'Manual Authorization',
  );
  expect(
    manualAuthHint(
      { kind: 'apiKey', name: 'access_key', value: 'id', placement: 'query' },
      request,
    ),
  ).toContain('query parameter takes priority');
  request.headers[0]!.enabled = false;
  expect(manualAuthHint({ kind: 'bearer', token: 'id' }, request)).toBeNull();
});
