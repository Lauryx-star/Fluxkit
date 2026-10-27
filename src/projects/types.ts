import type { Auth } from '../auth/types';
import type { Entry } from '../components/KeyValueEditor';

export type BodyFormat = 'none' | 'json' | 'text' | 'urlEncoded' | 'multipart' | 'file';
export interface Attachment {
  path: string;
  name: string;
}
export interface FormField extends Entry {
  file: Attachment | null;
}
export interface RequestBody {
  kind: BodyFormat;
  text: string;
  fields: FormField[];
  file: Attachment | null;
}
export interface SavedRequest {
  importIssues?: string[];
  auth?: Auth | null;
  id: string;
  name: string;
  folderId: string | null;
  method: string;
  url: string;
  params: Entry[];
  headers: Entry[];
  body: RequestBody;
}
export interface Folder {
  auth?: Auth | null;
  id: string;
  name: string;
  parentId: string | null;
}
export interface Collection {
  postman?: { version: string; variables: Record<string, string> } | null;
  openApi?: {
    version: string;
    document: Record<string, unknown>;
    operations?: Record<string, string>;
  } | null;
  auth?: Auth | null;
  schemaVersion: number;
  id: string;
  name: string;
  folders: Folder[];
  requests: SavedRequest[];
}
export interface CollectionView {
  revision: string;
  data: Collection;
}
export interface ProjectView {
  handle: string;
  directory: string;
  revision: string;
  readOnly?: boolean;
  serverSync?: boolean;
  serverOwned?: boolean;
  serverRecovery?: boolean;
  data: { schemaVersion: number; id: string; name: string; auth?: Auth | null };
  collections: CollectionView[];
}
export interface Draft {
  importIssues?: string[];
  auth?: Auth | null;
  id: number;
  requestId: string;
  name: string;
  method: string;
  url: string;
  params: Entry[];
  headers: Entry[];
  body: string;
  bodyFormat: BodyFormat;
  fields: FormField[];
  file: Attachment | null;
  projectHandle: string | null;
  collectionId: string | null;
  folderId: string | null;
  savedSnapshot: string | null;
}
export function requestData(draft: Draft): SavedRequest {
  return {
    ...(draft.importIssues?.length ? { importIssues: draft.importIssues } : {}),
    ...(draft.auth ? { auth: draft.auth } : {}),
    id: draft.requestId,
    name: draft.name,
    folderId: draft.folderId,
    method: draft.method,
    url: draft.url,
    params: draft.params,
    headers: draft.headers,
    body: { kind: draft.bodyFormat, text: draft.body, fields: draft.fields, file: draft.file },
  };
}
export function isDirty(draft: Draft): boolean {
  return draft.savedSnapshot !== JSON.stringify(requestData(draft));
}
export function fromRequest(
  id: number,
  request: SavedRequest,
  projectHandle: string,
  collectionId: string,
): Draft {
  const draft: Draft = {
    id,
    requestId: request.id,
    auth: request.auth,
    importIssues: request.importIssues,
    name: request.name,
    method: request.method,
    url: request.url,
    params: request.params,
    headers: request.headers,
    body: request.body.text,
    bodyFormat: request.body.kind,
    fields: request.body.fields,
    file: request.body.file,
    projectHandle,
    collectionId,
    folderId: request.folderId,
    savedSnapshot: null,
  };
  // Native serialization and hand-authored JSON may place auth after other fields.
  // Compare snapshots in the editor's field order so opening a saved request stays clean.
  draft.savedSnapshot = JSON.stringify(requestData(draft));
  return draft;
}
export function emptyDraft(
  id: number,
  projectHandle: string | null,
  collectionId: string | null,
  folderId: string | null,
): Draft {
  return {
    id,
    requestId: crypto.randomUUID(),
    name: `Request ${id}`,
    method: 'GET',
    url: '',
    params: [{ id: 1, enabled: true, key: '', value: '' }],
    headers: [{ id: 1, enabled: true, key: '', value: '' }],
    body: '',
    bodyFormat: 'none',
    fields: [],
    file: null,
    projectHandle,
    collectionId,
    folderId,
    savedSnapshot: null,
  };
}
export function folderDescendants(collection: Collection, folderId: string): Set<string> {
  const ids = new Set([folderId]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const folder of collection.folders) {
      if (folder.parentId && ids.has(folder.parentId) && !ids.has(folder.id)) {
        ids.add(folder.id);
        changed = true;
      }
    }
  }
  return ids;
}
