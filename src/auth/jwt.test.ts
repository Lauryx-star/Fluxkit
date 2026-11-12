import { describe, expect, it } from 'vitest';
import { decodeToken, isExpired, summaryClaims } from './jwt';

const encode = (value: unknown) =>
  btoa(String.fromCharCode(...new TextEncoder().encode(JSON.stringify(value))))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');

const header = { alg: 'RS256', typ: 'JWT', kid: 'synthetic-key' };
const payload = {
  iss: 'https://issuer.example.test/realms/synthetic',
  sub: 'synthetic-user',
  aud: ['api', 'account'],
  scope: 'openid read write',
  iat: 1_000,
  exp: 4_600,
  name: 'Jürgen Synthetic',
};

describe('JWT inspection', () => {
  it('decodes header and UTF-8 payload of a signed token without verifying it', () => {
    const decoded = decodeToken(`${encode(header)}.${encode(payload)}.c2lnbmF0dXJl`);
    expect(decoded).toEqual({ kind: 'jws', header, payload, hasSignature: true });
  });

  it('reports unsigned tokens and encrypted tokens', () => {
    expect(decodeToken(`${encode(header)}.${encode(payload)}.`)).toMatchObject({
      kind: 'jws',
      hasSignature: false,
    });
    expect(decodeToken(`${encode({ alg: 'RSA-OAEP', enc: 'A256GCM' })}.a.b.c.d`)).toEqual({
      kind: 'jwe',
      header: { alg: 'RSA-OAEP', enc: 'A256GCM' },
    });
  });

  it('treats opaque or malformed values as non-JWT tokens', () => {
    for (const token of [
      'synthetic-opaque-session',
      'a.b.c',
      `${encode(header)}.${encode('text')}.sig`,
      `${encode(header)}.not+base64url.sig`,
      `${encode(header)}.${encode(payload)}`,
    ]) {
      expect(decodeToken(token)).toEqual({ kind: 'opaque' });
    }
  });

  it('summarizes well-known claims with UTC and relative times', () => {
    const rows = summaryClaims(payload, 1_600);
    expect(rows).toEqual([
      { label: 'Issuer', value: 'https://issuer.example.test/realms/synthetic' },
      { label: 'Subject', value: 'synthetic-user' },
      { label: 'Audience', value: 'api, account' },
      { label: 'Scopes', value: 'openid read write' },
      { label: 'Issued at', value: '1970-01-01T00:16:40.000Z (10m ago)' },
      { label: 'Expires', value: '1970-01-01T01:16:40.000Z (in 50m)' },
    ]);
    expect(isExpired(payload, 1_600)).toBe(false);
    expect(isExpired(payload, 4_600)).toBe(true);
    expect(isExpired({}, 4_600)).toBe(false);
  });
});
