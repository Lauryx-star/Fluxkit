import type { Definition } from '../environments/types';
import type { Collection, ProjectView } from '../projects/types';

import type { Auth, Composition, LoginRequest, TokenStatus } from './types';

export const tokenStatusLabel = (status: TokenStatus): string =>
  ({
    missing: 'No local token',
    valid: 'Token available',
    unknownExpiry: 'Expiry unknown',
    renewalDue: 'Renewal due before sending',
    expired: 'Token expired',
  })[status.state];

export function authLabel(auth: Auth | null | undefined): string {
  return auth
    ? {
        none: 'None',
        basic: 'Basic',
        bearer: 'Bearer',
        apiKey: 'API Key',
        oauth2: 'OAuth2 Client Credentials',
        customLogin: 'Custom Login',
      }[auth.kind]
    : 'None';
}
/** Presentation only. Native execution reloads and resolves the authoritative ancestry. */
export function effectiveAuth(
  project: ProjectView | undefined,
  collectionId: string | null,
  folderId: string | null,
  own?: Auth | null,
) {
  if (own) {
    return { auth: own, source: 'Request' };
  }
  const collection: Collection | undefined = project?.collections.find(
    (c) => c.data.id === collectionId,
  )?.data;
  let parent = folderId;
  for (let level = 0; parent && level < 32; level++) {
    const folder = collection?.folders.find((f) => f.id === parent);
    if (!folder) {
      return { auth: null, source: 'Missing folder — reload project' };
    }
    if (folder.auth) {
      return { auth: folder.auth, source: `Folder: ${folder.name}` };
    }
    parent = folder.parentId;
  }
  if (collection?.auth) {
    return { auth: collection.auth, source: `Collection: ${collection.name}` };
  }
  return {
    auth: project?.data.auth ?? null,
    source: project ? `Project: ${project.data.name}` : 'Session',
  };
}

export function validAuth(value: Auth | null, variables: Definition[]): boolean {
  const exists = (id: string) => variables.some((v) => v.id === id);
  if (!value || value.kind === 'none') {
    return true;
  }
  if (value.kind === 'basic') {
    return exists(value.username) && exists(value.password);
  }
  if (value.kind === 'bearer') {
    return exists(value.token);
  }
  const composition = (value: Composition, renewal = false) =>
    value.parts.every((part) =>
      part.kind === 'variable' ? exists(part.id) : part.kind === 'literal' || renewal,
    );
  if (value.kind === 'oauth2') {
    return (
      exists(value.config.clientId) &&
      exists(value.config.clientSecret) &&
      composition(value.config.tokenUrl) &&
      composition(value.config.scope)
    );
  }
  if (value.kind === 'customLogin') {
    const request = (r: LoginRequest, renewal: boolean) => {
      const expressions = [
        r.url,
        ...r.headers.map((v) => v.value),
        ...r.params.map((v) => v.value),
        ...(r.body.kind === 'urlEncoded'
          ? r.body.fields.map((v) => v.value)
          : r.body.kind === 'none'
            ? []
            : [r.body.value]),
      ];
      return (
        expressions.every((v) => composition(v, renewal)) &&
        r.successStatuses.length > 0 &&
        r.successStatuses.every((s) => s >= 200 && s <= 599) &&
        (r.lifetimeSeconds === null || (r.lifetimeSeconds >= 1 && r.lifetimeSeconds <= 315360000))
      );
    };
    return (
      !!value.config.placement.name.trim() &&
      request(value.config.login, false) &&
      (!value.config.renewal || request(value.config.renewal, true))
    );
  }
  return !!value.name.trim() && exists(value.value);
}

/** Template-only hint; never resolve/reveal stored values for presentation. */
export function manualAuthHint(
  auth: Auth | null | undefined,
  request: Pick<import('../projects/types').SavedRequest, 'headers' | 'params' | 'url'>,
): string | null {
  const placement =
    auth?.kind === 'customLogin'
      ? auth.config.placement
      : auth?.kind === 'apiKey'
        ? auth
        : { name: 'Authorization', placement: 'header' };
  if (placement.placement === 'query') {
    const query = request.url.split('?')[1]?.split('#')[0] ?? '';
    if (
      new URLSearchParams(query).has(placement.name) ||
      request.params.some((row) => row.enabled && row.key === placement.name)
    ) {
      return 'Manual query parameter takes priority';
    }
    if (
      query.includes('{{') ||
      request.params.some((row) => row.enabled && row.key.includes('{{'))
    ) {
      return 'Variable query names are checked before sending';
    }
    return null;
  }
  const name = placement.name;
  if (request.headers.some((row) => row.enabled && row.key.toLowerCase() === name.toLowerCase())) {
    return `Manual ${name} header takes priority`;
  }
  return request.headers.some((row) => row.enabled && row.key.includes('{{'))
    ? 'Variable header names are checked before sending'
    : null;
}
