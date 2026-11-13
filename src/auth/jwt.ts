/**
 * Local, display-only JWT decoding for the token inspector. Signatures are never verified:
 * the decoded content shows what the token claims, not whether the claims are trustworthy.
 */

export type JsonObject = Record<string, unknown>;

export type DecodedToken =
  | { kind: 'jws'; header: JsonObject; payload: JsonObject; hasSignature: boolean }
  | { kind: 'jwe'; header: JsonObject }
  | { kind: 'opaque' };

export interface ClaimRow {
  label: string;
  value: string;
}

const segment = /^[A-Za-z0-9_-]*$/;

function decodeSegment(value: string | undefined): JsonObject | null {
  if (!value || !segment.test(value) || value.length % 4 === 1) {
    return null;
  }
  try {
    const base64 = value.replace(/-/g, '+').replace(/_/g, '/');
    const binary = atob(base64.padEnd(base64.length + ((4 - (base64.length % 4)) % 4), '='));
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
    const parsed: unknown = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
    return parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed)
      ? (parsed as JsonObject)
      : null;
  } catch {
    return null;
  }
}

/** Decode a compact JWS (3 parts) or the protected header of a JWE (5 parts). */
export function decodeToken(token: string): DecodedToken {
  const parts = token.trim().split('.');
  const [first, second, signature] = parts;
  if (parts.length === 3) {
    const header = decodeSegment(first);
    const payload = decodeSegment(second);
    if (header && payload) {
      return { kind: 'jws', header, payload, hasSignature: Boolean(signature) };
    }
  } else if (parts.length === 5) {
    const header = decodeSegment(first);
    if (header && typeof header.enc === 'string') {
      return { kind: 'jwe', header };
    }
  }
  return { kind: 'opaque' };
}

const timeClaims = new Set(['exp', 'nbf', 'iat', 'auth_time']);

function formatTime(seconds: number, now: number): string {
  const iso = new Date(seconds * 1000).toISOString();
  const delta = Math.round(seconds - now);
  const magnitude = Math.abs(delta);
  const relative =
    magnitude < 60
      ? `${magnitude}s`
      : magnitude < 3600
        ? `${Math.floor(magnitude / 60)}m`
        : magnitude < 86400
          ? `${Math.floor(magnitude / 3600)}h ${Math.floor((magnitude % 3600) / 60)}m`
          : `${Math.floor(magnitude / 86400)}d ${Math.floor((magnitude % 86400) / 3600)}h`;
  return `${iso} (${delta >= 0 ? 'in ' : ''}${relative}${delta < 0 ? ' ago' : ''})`;
}

function formatValue(value: unknown): string {
  if (typeof value === 'string') {
    return value;
  }
  if (Array.isArray(value) && value.every((item) => typeof item === 'string')) {
    return value.join(', ');
  }
  return JSON.stringify(value);
}

/** Well-known claims in a fixed, readable order. Unknown claims stay in the raw payload view. */
export function summaryClaims(payload: JsonObject, now = Date.now() / 1000): ClaimRow[] {
  const rows: [string, string][] = [
    ['iss', 'Issuer'],
    ['sub', 'Subject'],
    ['aud', 'Audience'],
    ['azp', 'Authorized party'],
    ['client_id', 'Client ID'],
    ['scope', 'Scopes'],
    ['scp', 'Scopes'],
    ['roles', 'Roles'],
    ['iat', 'Issued at'],
    ['nbf', 'Not before'],
    ['exp', 'Expires'],
    ['auth_time', 'Authenticated at'],
    ['jti', 'Token ID'],
  ];
  return rows.flatMap(([claim, label]) => {
    const value = payload[claim];
    if (value === undefined || value === null) {
      return [];
    }
    const text =
      timeClaims.has(claim) && typeof value === 'number' && Number.isFinite(value)
        ? formatTime(value, now)
        : formatValue(value);
    return [{ label, value: text }];
  });
}

/** True when `exp` is present and in the past at `now`. */
export function isExpired(payload: JsonObject, now = Date.now() / 1000): boolean {
  return typeof payload.exp === 'number' && payload.exp <= now;
}
