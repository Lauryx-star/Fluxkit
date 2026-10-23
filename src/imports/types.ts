export interface ImportPreview {
  id: string;
  source: string;
  environmentName: string | null;
  collectionName: string;
  version: string;
  requestCount: number;
  folderCount: number;
  reviewCount?: number;
  variables: { sourceName?: string | null; name: string; hasDefault: boolean }[];
  requests: { name: string; method: string; needsReview?: boolean; url: string }[];
  warnings: string[];
  blockers: string[];
}
export interface ImportResult {
  project: import('../projects/types').ProjectView;
  error: string | null;
}
