import { beforeEach, describe, expect, it } from 'vitest';
import {
  GROUP_MS,
  historyKey,
  keepHistoryScopes,
  recordEdit,
  redoEdit,
  undoEdit,
} from './textHistory';

const key = historyKey('draft:1', 'body');

beforeEach(() => keepHistoryScopes([]));

describe('text history', () => {
  it('groups fast typing into one step and separates pauses, newlines and deletes', () => {
    recordEdit(key, '', 'a', 0, 1000);
    recordEdit(key, 'a', 'ab', 1, 1100);
    recordEdit(key, 'ab', 'abc', 2, 1200);
    recordEdit(key, 'abc', 'abc\n', 3, 1300);
    recordEdit(key, 'abc\n', 'abc\nd', 4, 1400 + GROUP_MS);
    recordEdit(key, 'abc\nd', 'abc\n', 5, 1500 + GROUP_MS);
    expect(undoEdit(key, 'abc\n', 4)).toEqual({ text: 'abc\nd', caret: 5 });
    expect(undoEdit(key, 'abc\nd', 5)).toEqual({ text: 'abc\n', caret: 4 });
    expect(undoEdit(key, 'abc\n', 4)).toEqual({ text: 'abc', caret: 3 });
    expect(undoEdit(key, 'abc', 3)).toEqual({ text: '', caret: 0 });
    expect(undoEdit(key, '', 0)).toBeNull();
    expect(redoEdit(key, '', 0)).toEqual({ text: 'abc', caret: 3 });
    expect(redoEdit(key, 'abc', 3)).toEqual({ text: 'abc\n', caret: 4 });
  });

  it('keeps separate histories per field and tab and drops whole tabs when they close', () => {
    recordEdit(historyKey('draft:1', 'body'), '', 'first', 0, 0);
    recordEdit(historyKey('draft:2', 'body'), '', 'second', 0, 0);
    recordEdit(historyKey('draft:2', 'url'), '', 'https://x.test', 0, 0);
    recordEdit(historyKey('draft:2', 'header:1:value'), '', 'json', 0, 0);
    expect(undoEdit(historyKey('draft:2', 'body'), 'second', 6)?.text).toBe('');
    expect(undoEdit(historyKey('draft:1', 'body'), 'first', 5)?.text).toBe('');
    expect(undoEdit(historyKey('draft:2', 'url'), 'https://x.test', 14)?.text).toBe('');
    recordEdit(historyKey('draft:3', 'url'), '', 'third', 0, 0);
    recordEdit(historyKey('draft:3', 'header:1:key'), '', 'Accept', 0, 0);
    keepHistoryScopes(['draft:1', 'draft:2']);
    expect(undoEdit(historyKey('draft:3', 'url'), 'third', 5)).toBeNull();
    expect(undoEdit(historyKey('draft:3', 'header:1:key'), 'Accept', 6)).toBeNull();
    expect(undoEdit(historyKey('draft:2', 'header:1:value'), 'json', 4)?.text).toBe('');
  });

  it('starts over after an outside change and clears redo after a new edit', () => {
    recordEdit(key, '', 'local', 0, 0);
    expect(undoEdit(key, 'reloaded from server', 0)).toBeNull();
    recordEdit(key, 'reloaded', 'reloaded!', 8, 5000);
    expect(undoEdit(key, 'reloaded!', 9)).toEqual({ text: 'reloaded', caret: 8 });
    expect(undoEdit(key, 'reloaded', 8)).toBeNull();
    recordEdit(key, 'reloaded', 'reloaded?', 8, 9000);
    expect(redoEdit(key, 'reloaded?', 9)).toBeNull();
  });
});
