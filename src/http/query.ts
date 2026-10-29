import type { Entry } from '../components/KeyValueEditor';

/**
 * The URL field shows the stored base URL plus the enabled query parameters, like Postman.
 * Requests keep storing them separately: the native side appends enabled parameters to the URL
 * with form encoding, so parameters hold decoded values and the field shows a readable form.
 */

function sendable(entry: Entry) {
  return entry.enabled && entry.key !== '';
}

function decode(text: string) {
  const spaced = text.replace(/\+/g, ' ');
  try {
    return decodeURIComponent(spaced);
  } catch {
    return spaced;
  }
}

/** Escapes only what would change the meaning when the field is parsed again. */
function encode(text: string, key: boolean) {
  const escaped = text
    .replace(/%(?=[0-9A-Fa-f]{2})/g, '%25')
    .replace(/&/g, '%26')
    .replace(/#/g, '%23')
    .replace(/\+/g, '%2B');
  return key ? escaped.replace(/=/g, '%3D') : escaped;
}

function splitFragment(text: string): [string, string] {
  const hash = text.indexOf('#');
  return hash < 0 ? [text, ''] : [text.slice(0, hash), text.slice(hash)];
}

/** The text shown in the URL field for a stored URL and its parameters. */
export function composeUrl(url: string, params: Entry[]) {
  const query = params
    .filter(sendable)
    .map((entry) =>
      entry.value === ''
        ? encode(entry.key, true)
        : `${encode(entry.key, true)}=${encode(entry.value, false)}`,
    )
    .join('&');
  if (query === '') {
    return url;
  }
  const [base, fragment] = splitFragment(url);
  return `${base}${base.includes('?') ? '&' : '?'}${query}${fragment}`;
}

/**
 * Splits edited URL text into the stored base URL and parameters. Enabled parameter rows are
 * replaced in order (keeping their ids); disabled and empty rows stay where they are.
 */
export function splitUrl(text: string, params: Entry[]): { url: string; params: Entry[] } {
  const [withQuery, fragment] = splitFragment(text);
  const mark = withQuery.indexOf('?');
  if (mark < 0) {
    // Removing the query from the field removes the enabled parameters it showed.
    return {
      url: text,
      params: params.some(sendable) ? params.filter((e) => !sendable(e)) : params,
    };
  }
  const parsed = withQuery
    .slice(mark + 1)
    .split('&')
    .filter((part) => part !== '')
    .map((part) => {
      const equals = part.indexOf('=');
      return equals < 0
        ? { key: decode(part), value: '' }
        : { key: decode(part.slice(0, equals)), value: decode(part.slice(equals + 1)) };
    })
    .filter((item) => item.key !== '');
  let next = 0;
  let id = Math.max(0, ...params.map((entry) => entry.id));
  const kept: Entry[] = [];
  for (const entry of params) {
    if (!sendable(entry)) {
      kept.push(entry);
    } else if (next < parsed.length) {
      kept.push({ ...entry, ...parsed[next++] });
    }
  }
  let trailing = kept.length;
  while (trailing > 0 && kept[trailing - 1]?.key === '' && kept[trailing - 1]?.value === '') {
    trailing -= 1;
  }
  const added = parsed.slice(next).map((item) => ({ id: ++id, enabled: true, ...item }));
  return {
    url: withQuery.slice(0, mark) + fragment,
    params: [...kept.slice(0, trailing), ...added, ...kept.slice(trailing)],
  };
}

/**
 * The URL field text: the raw text last typed into it while it still describes the draft (so a
 * trailing `?`, `&` or a pending `=` survives typing), otherwise the composed URL.
 */
export function urlFieldText(
  draft: { id: number; url: string; params: Entry[] },
  typed: { draft: number; text: string } | null,
) {
  const composed = composeUrl(draft.url, draft.params);
  if (typed?.draft !== draft.id) {
    return composed;
  }
  const split = splitUrl(typed.text, draft.params);
  return composeUrl(split.url, split.params) === composed ? typed.text : composed;
}
