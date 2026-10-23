export type ExportTarget =
  | { kind: 'project' }
  | { kind: 'collection'; collection: string }
  | { kind: 'folder'; collection: string; folder: string }
  | { kind: 'request'; collection: string; request: string };
export interface ExportPreview {
  id: string;
  title: string;
  directory: boolean;
  folderName: string;
  files: { name: string; operations: number; bytes: number }[];
  warnings: string[];
  blockers: string[];
}
