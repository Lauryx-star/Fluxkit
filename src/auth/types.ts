/** Every credential field is a stable project-variable ID, never a resolved value. */
export type Auth =
  | { kind: 'none' }
  | { kind: 'basic'; username: string; password: string }
  | { kind: 'bearer'; token: string }
  | { kind: 'apiKey'; name: string; value: string; placement: 'header' | 'query' };
export type AuthTarget = { collectionId: string | null; folderId: string | null };
