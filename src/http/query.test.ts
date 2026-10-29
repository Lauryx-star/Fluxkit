import { describe, expect, it } from 'vitest';
import type { Entry } from '../components/KeyValueEditor';
import { composeUrl, splitUrl, urlFieldText } from './query';

const row = (id: number, key: string, value: string, enabled = true): Entry => ({
  id,
  enabled,
  key,
  value,
});

describe('URL query parameters', () => {
  it('moves a pasted query into parameters with decoded values and keeps the fragment', () => {
    const split = splitUrl('https://api.example.test/report?id=42&name=a+b%26c&flag#top', [
      row(1, '', ''),
    ]);
    expect(split.url).toBe('https://api.example.test/report#top');
    expect(split.params).toEqual([
      row(2, 'id', '42'),
      row(3, 'name', 'a b&c'),
      row(4, 'flag', ''),
      row(1, '', ''),
    ]);
    expect(composeUrl(split.url, split.params)).toBe(
      'https://api.example.test/report?id=42&name=a b%26c&flag#top',
    );
  });

  it('replaces enabled rows in order and keeps ids, disabled rows and variables', () => {
    const params = [row(1, 'a', '1'), row(2, 'off', 'x', false), row(3, 'b', '2')];
    expect(composeUrl('{{baseUrl}}/items', params)).toBe('{{baseUrl}}/items?a=1&b=2');
    const split = splitUrl('{{baseUrl}}/items?a=1&b={{page}}', params);
    expect(split).toEqual({
      url: '{{baseUrl}}/items',
      params: [row(1, 'a', '1'), row(2, 'off', 'x', false), row(3, 'b', '{{page}}')],
    });
    expect(splitUrl('{{baseUrl}}/items?a=1', params).params).toEqual([
      row(1, 'a', '1'),
      row(2, 'off', 'x', false),
    ]);
    expect(splitUrl('{{baseUrl}}/items', params).params).toEqual([row(2, 'off', 'x', false)]);
  });

  it('round-trips characters that would change the query and leaves literal percent signs', () => {
    const params = [row(1, 'k=1', 'a+b&c#d %41'), row(2, 'p', '100%')];
    const text = composeUrl('https://x.test', params);
    expect(text).toBe('https://x.test?k%3D1=a%2Bb%26c%23d %2541&p=100%');
    expect(splitUrl(text, params).params).toEqual(params);
  });

  it('keeps raw typing while it describes the draft and shows parameter edits otherwise', () => {
    const typed = 'https://x.test/a?';
    const draft = { id: 7, ...splitUrl(typed, []) };
    expect(urlFieldText(draft, { draft: 7, text: typed })).toBe(typed);
    const pending = { id: 7, ...splitUrl('https://x.test/a?q=1&', []) };
    expect(urlFieldText(pending, { draft: 7, text: 'https://x.test/a?q=1&' })).toBe(
      'https://x.test/a?q=1&',
    );
    const edited = { ...pending, params: [row(1, 'q', '2')] };
    expect(urlFieldText(edited, { draft: 7, text: 'https://x.test/a?q=1&' })).toBe(
      'https://x.test/a?q=2',
    );
    expect(urlFieldText(edited, { draft: 8, text: 'other' })).toBe('https://x.test/a?q=2');
  });

  it('shows legacy URLs that already contain a query together with the parameters', () => {
    expect(composeUrl('https://x.test?a=1', [row(1, 'b', '2')])).toBe('https://x.test?a=1&b=2');
  });
});
