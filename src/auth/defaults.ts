import type { Composition, LoginRequest } from './types';

export const literal = (text = ''): Composition => ({
  parts: [{ kind: 'literal', text }],
  separator: '',
  prefix: '',
  suffix: '',
  base64: false,
});
export const loginRequest = (): LoginRequest => ({
  url: literal(),
  method: 'POST',
  params: [],
  headers: [],
  body: { kind: 'none' },
  successStatuses: [200],
  token: { kind: 'json', pointer: '/access_token' },
  renewalToken: null,
  expiresIn: null,
  lifetimeSeconds: null,
});
