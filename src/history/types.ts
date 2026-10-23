/** The only visible per-entry metadata. Native UTC retention timestamps stay outside IPC. */
export interface HistoryEntry {
  url: string;
  method: string;
  status: number | null;
  elapsedMs: number | null;
}
export interface HistoryView {
  recording: boolean;
  entries: HistoryEntry[];
  warning: string | null;
}
export type HistoryAction =
  { kind: 'load' } | { kind: 'setRecording'; enabled: boolean } | { kind: 'clear' };
