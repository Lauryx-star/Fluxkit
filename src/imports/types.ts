export interface ImportPreview {
  id: string;
  source: string;
  environmentName: string | null;
  collectionName: string;
  version: string;
  requestCount: number;
  folderCount: number;
  reviewCount?: number;
  variables: {
    sourceName?: string | null;
    name: string;
    hasDefault: boolean;
    /** Postman: binds to an existing project variable whose values stay unchanged. */
    existing?: boolean;
    /** Server workspaces: secret values go to the personal vault, others are shared. */
    secret?: boolean;
  }[];
  requests: { name: string; method: string; needsReview?: boolean; url: string }[];
  warnings: string[];
  blockers: string[];
}
export interface ImportResult {
  project: import('../projects/types').ProjectView;
  error: string | null;
}
