import type { SavedRequest } from '../projects/types';
export interface ExecutionScope {
  handle: string;
  environment: string | null;
  revision: string;
  projectRevision: string;
  collectionId: string | null;
  collectionRevision: string | null;
}
export interface ExecutionInput {
  request: SavedRequest;
  scope: ExecutionScope | null;
  timeoutMs: number;
  saveResponse: boolean;
}
export interface ExecutionResult {
  outcome: 'response' | 'cancelled' | 'timeout' | 'networkError' | 'invalidRequest' | 'saveError';
  status: number | null;
  elapsedMs: number | null;
  headers: { name: string; value: string }[];
  byteCount: number;
  text: string | null;
  jsonText: string | null;
  contentKind: 'empty' | 'text' | 'json' | 'binary';
  truncated: boolean;
  saved: boolean;
  message: string | null;
  tokenStatus?: import('../auth/types').TokenStatus;
  historyWarning?: string;
}
