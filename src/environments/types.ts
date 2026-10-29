export interface Definition {
  id: string;
  name: string;
  /** Missing in legacy files; legacy variables are treated as secrets. */
  secret?: boolean;
}
export interface Definitions {
  schemaVersion: number;
  environments: Definition[];
  variables: Definition[];
}
export interface EnvironmentView {
  revision: string;
  data: Definitions;
  selectedEnvironment: string | null;
  selectionMissing: boolean;
  /** Server revision of each stored value, keyed by `variableId/environmentId` (`default` for none). */
  serverValues?: { slots: Record<string, number> };
}
export type EnvironmentAction =
  | { kind: 'load' }
  | { kind: 'select'; environment: string | null }
  | { kind: 'save'; data: Definitions; expected: string }
  | {
      kind: 'setValue';
      variable: string;
      environment: string | null;
      value: string | null;
      expected: string;
      expectedValueRevision?: number;
    }
  | { kind: 'reveal'; variable: string; environment: string | null }
  | { kind: 'table'; environment: string | null }
  | { kind: 'usages'; variable: string }
  | { kind: 'preview'; variable: string; environment: string | null; expected: string }
  | { kind: 'check'; text: string };
export interface TableCell {
  set: boolean;
  /** Only ordinary values are listed; a secret needs an explicit reveal. */
  value?: string;
}
export interface TableRow {
  variable: string;
  default: TableCell;
  environment?: TableCell;
}
export interface EnvironmentReply {
  view: EnvironmentView;
  value: string | null;
  /** Present in the reply to a `table` action. */
  table?: TableRow[];
  /** Present in the reply to a `usages` action: saved places that use the variable. */
  usages?: { requests: string[]; authentication: string[] };
}
