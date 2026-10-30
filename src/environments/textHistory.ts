/**
 * Undo/redo history for text fields, kept in memory per key, so switching tabs never mixes edits
 * of different requests. Keys are `<scope>|<field>`, with one scope per open tab, for example
 * `draft:3|url` or `draft:3|header:2:value`; closing a tab drops its whole scope.
 *
 * Fast consecutive typing or deleting is grouped into one step, like in common editors. A value
 * changed from outside (switching body, reloading from the server) starts a fresh history, so
 * undo never resurrects text the field no longer showed.
 */

export interface TextState {
  text: string;
  /** Caret position to restore with this text. */
  caret: number;
}

interface History {
  past: TextState[];
  future: TextState[];
  current: string;
  lastEdit: number;
  kind: 'insert' | 'delete' | 'other';
}

/** Typing pauses longer than this start a new undo step. */
export const GROUP_MS = 1000;
const LIMIT = 200;
const histories = new Map<string, History>();

function history(key: string, current: string) {
  let entry = histories.get(key);
  if (!entry || entry.current !== current) {
    entry = { past: [], future: [], current, lastEdit: 0, kind: 'other' };
    histories.set(key, entry);
  }
  return entry;
}

/** Records an edit from `previous` (caret before the edit) to `next`. */
export function recordEdit(
  key: string,
  previous: string,
  next: string,
  caretBefore: number,
  now = Date.now(),
) {
  if (previous === next) {
    return;
  }
  const entry = history(key, previous);
  const delta = next.length - previous.length;
  const kind = delta > 0 ? 'insert' : delta < 0 ? 'delete' : 'other';
  const grouped =
    entry.past.length > 0 &&
    kind !== 'other' &&
    kind === entry.kind &&
    Math.abs(delta) <= 2 &&
    !/\n/.test(next.slice(caretBefore, caretBefore + Math.max(delta, 0))) &&
    now - entry.lastEdit < GROUP_MS;
  if (!grouped) {
    entry.past.push({ text: previous, caret: caretBefore });
    if (entry.past.length > LIMIT) {
      entry.past.shift();
    }
  }
  entry.future = [];
  entry.current = next;
  entry.lastEdit = now;
  entry.kind = kind;
}

function step(key: string, current: string, caret: number, direction: 'undo' | 'redo') {
  const entry = histories.get(key);
  if (!entry || entry.current !== current) {
    return null;
  }
  const [from, to] = direction === 'undo' ? [entry.past, entry.future] : [entry.future, entry.past];
  const target = from.pop();
  if (!target) {
    return null;
  }
  to.push({ text: current, caret });
  entry.current = target.text;
  entry.kind = 'other';
  return target;
}

/** The text to show after undoing, or null when there is nothing to undo. */
export function undoEdit(key: string, current: string, caret: number) {
  return step(key, current, caret, 'undo');
}

/** The text to show after redoing, or null when there is nothing to redo. */
export function redoEdit(key: string, current: string, caret: number) {
  return step(key, current, caret, 'redo');
}

/** The history key of a field within a scope such as an open tab. */
export function historyKey(scope: string, field: string) {
  return `${scope}|${field}`;
}

/** Drops the histories of scopes that no longer exist (for example closed tabs). */
export function keepHistoryScopes(scopes: Iterable<string>) {
  const keep = new Set(scopes);
  for (const key of histories.keys()) {
    if (!keep.has(key.slice(0, key.indexOf('|')))) {
      histories.delete(key);
    }
  }
}
