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
  | { kind: 'preview'; variable: string; environment: string | null; expected: string }
  | { kind: 'check'; text: string };
export interface EnvironmentReply {
  view: EnvironmentView;
  value: string | null;
}
