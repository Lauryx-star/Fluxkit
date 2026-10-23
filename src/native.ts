import { invoke, isTauri } from '@tauri-apps/api/core';

export interface AppInfo {
  name: string;
  version: string;
}

export function isDesktop(): boolean {
  return isTauri();
}

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>('app_info');
}

export async function chooseProject(name: string | null = null) {
  return invoke<import('./projects/types').ProjectView | null>('choose_project', { name });
}
export function reloadProject(handle: string) {
  return invoke<import('./projects/types').ProjectView>('reload_project', { handle });
}
export function renameProject(handle: string, name: string, expected: string) {
  return invoke<import('./projects/types').ProjectView>('rename_project', {
    handle,
    name,
    expected,
  });
}
export function saveCollection(
  handle: string,
  collection: import('./projects/types').Collection,
  expected: string | null,
) {
  return invoke<import('./projects/types').CollectionView>('save_collection', {
    handle,
    collection,
    expected,
  });
}
export function removeCollection(handle: string, id: string, expected: string) {
  return invoke<void>('remove_collection', { handle, id, expected });
}
export function chooseAttachment(handle: string) {
  return invoke<import('./projects/types').Attachment | null>('choose_attachment', { handle });
}
export function closeProject(handle: string) {
  return invoke<void>('close_project', { handle });
}

export function environmentAction(
  handle: string,
  action: import('./environments/types').EnvironmentAction,
) {
  return invoke<import('./environments/types').EnvironmentReply>('environment_action', {
    handle,
    action,
  });
}

export function beginHttpRequest() {
  return invoke<string>('begin_http_request');
}
export function executeHttpRequest(id: string, input: import('./http/types').ExecutionInput) {
  return invoke<import('./http/types').ExecutionResult>('execute_http_request', { id, input });
}
export function cancelHttpRequest(id: string) {
  return invoke<void>('cancel_http_request', { id });
}

export function saveProjectAuth(
  handle: string,
  auth: import('./auth/types').Auth | null,
  expected: string,
) {
  return invoke<import('./projects/types').ProjectView>('save_project_auth', {
    handle,
    auth,
    expected,
  });
}
