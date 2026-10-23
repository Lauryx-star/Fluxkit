/** Every credential field is a stable project-variable ID, never a resolved value. */
export type Auth =
  | { kind: 'none' }
  | { kind: 'basic'; username: string; password: string }
  | { kind: 'bearer'; token: string }
  | { kind: 'apiKey'; name: string; value: string; placement: 'header' | 'query' }
  | { kind: 'oauth2'; config: OAuth2 }
  | { kind: 'customLogin'; config: CustomLogin };
export type AuthTarget = { collectionId: string | null; folderId: string | null };

export type Part =
  | { kind: 'literal'; text: string }
  | { kind: 'variable'; id: string }
  | { kind: 'accessToken' }
  | { kind: 'renewalToken' };
export interface Composition {
  parts: Part[];
  separator: string;
  prefix: string;
  suffix: string;
  base64: boolean;
}
export interface OAuth2 {
  tokenUrl: Composition;
  clientId: string;
  clientSecret: string;
  scope: Composition;
  clientAuthentication: 'basic' | 'body';
}
export interface LoginEntry {
  name: string;
  value: Composition;
}
export type LoginBody =
  | { kind: 'none' }
  | { kind: 'text' | 'json'; value: Composition }
  | { kind: 'urlEncoded'; fields: LoginEntry[] };
export type Extraction =
  { kind: 'text' } | { kind: 'json'; pointer: string } | { kind: 'header'; name: string };
export interface LoginRequest {
  url: Composition;
  method: string;
  params: LoginEntry[];
  headers: LoginEntry[];
  body: LoginBody;
  successStatuses: number[];
  token: Extraction;
  renewalToken: Extraction | null;
  expiresIn: Extraction | null;
  lifetimeSeconds: number | null;
}
export interface TokenPlacement {
  name: string;
  placement: 'header' | 'query';
  prefix: string;
}
export interface CustomLogin {
  login: LoginRequest;
  renewal: LoginRequest | null;
  placement: TokenPlacement;
}
export interface TokenStatus {
  state: 'missing' | 'valid' | 'unknownExpiry' | 'renewalDue' | 'expired';
  expiresAt: number | null;
}
