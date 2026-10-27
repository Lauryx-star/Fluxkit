import { invoke, isTauri } from '@tauri-apps/api/core';

export interface AppInfo {
  name: string;
  version: string;
}

export interface ServerConnection {
  serverUrl: string;
  authenticated: boolean;
}

export interface ServerProject {
  id: string;
  name: string;
  kind: string;
  revision: number;
  canManageMembers?: boolean;
  canEdit?: boolean;
}

export interface ServerProjectSnapshot extends ServerProject {
  snapshot: string;
}

export interface ServerProjectMember {
  id: string;
  username: string;
  role: 'member' | 'projectAdmin';
}

export interface ServerAccount {
  id: string;
  username: string;
  isServerAdmin: boolean;
}

export interface ServerAccountAdminView extends ServerAccount {
  status: 'pending' | 'active' | 'disabled';
}

export function getServerConnection() {
  return invoke<ServerConnection>('server_connection_get');
}
export function setServerConnection(serverUrl: string) {
  return invoke<ServerConnection>('server_connection_set', { serverUrl });
}
export function registerServerAccount(username: string, password: string) {
  return invoke<void>('server_register', { username, password });
}
export function loginToServer(username: string, password: string) {
  return invoke<ServerConnection>('server_login', { username, password });
}
export function logoutFromServer() {
  return invoke<ServerConnection>('server_logout');
}
export function listServerProjects() {
  return invoke<ServerProject[]>('server_projects');
}
export function getServerProjectSnapshot(projectId: string) {
  return invoke<ServerProjectSnapshot>('server_project_snapshot', { projectId });
}
export function openServerProject(projectId: string) {
  return invoke<import('./projects/types').ProjectView>('server_open_project', { projectId });
}
export function saveServerProjectSnapshot(
  projectId: string,
  revision: number,
  snapshot: string,
) {
  return invoke<ServerProjectSnapshot>('server_save_project_snapshot', {
    projectId,
    revision,
    snapshot,
  });
}
export function uploadLocalProjectToServer(handle: string) {
  return invoke<ServerProjectSnapshot>('server_upload_local_project', { handle });
}
export function syncServerProject(handle: string) {
  return invoke<ServerProjectSnapshot>('server_sync_project', { handle });
}
export function listServerProjectMembers(projectId: string) {
  return invoke<ServerProjectMember[]>('server_project_members', { projectId });
}
export function addServerProjectMember(
  projectId: string,
  username: string,
  role: ServerProjectMember['role'],
) {
  return invoke<void>('server_add_project_member', { projectId, username, role });
}
export function updateServerProjectMember(
  projectId: string,
  userId: string,
  role: ServerProjectMember['role'],
) {
  return invoke<void>('server_update_project_member', { projectId, userId, role });
}
export function removeServerProjectMember(projectId: string, userId: string) {
  return invoke<void>('server_remove_project_member', { projectId, userId });
}
export function getServerAccount() {
  return invoke<ServerAccount>('server_account');
}
export function listServerAdminUsers() {
  return invoke<ServerAccountAdminView[]>('server_admin_users');
}
export function approveServerUser(userId: string) {
  return invoke<void>('server_admin_approve_user', { userId });
}
export function setServerUserStatus(userId: string, status: 'active' | 'disabled') {
  return invoke<void>('server_admin_set_user_status', { userId, status });
}
export function createServerTeamProject(name: string) {
  return invoke<ServerProjectSnapshot>('server_create_team_project', { name });
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
export function restoreLastProject() {
  return invoke<import('./projects/types').ProjectView | null>('restore_last_project');
}
export function rememberProject(handle: string | null) {
  return invoke<void>('remember_project', { handle });
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

export function authenticationAction(
  id: string,
  input: import('./http/types').ExecutionInput,
  action: 'status' | 'renew' | 'forget',
) {
  return invoke<import('./auth/types').TokenStatus>('authentication_action', { id, input, action });
}

export function historyAction(handle: string, action: import('./history/types').HistoryAction) {
  return invoke<import('./history/types').HistoryView>('history_action', { handle, action });
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

export function chooseOpenApiImport(handle: string) {
  return invoke<import('./imports/types').ImportPreview | null>('choose_openapi_import', {
    handle,
  });
}
export function commitOpenApiImport(handle: string, id: string, storeValues: boolean) {
  return invoke<import('./imports/types').ImportResult>('commit_openapi_import', {
    handle,
    id,
    storeValues,
  });
}
export function discardOpenApiImport(id: string) {
  return invoke<void>('discard_openapi_import', { id });
}

export function choosePostmanImport(handle: string, collectionId: string | null = null) {
  return invoke<import('./imports/types').ImportPreview | null>('choose_postman_import', {
    handle,
    collectionId,
  });
}

export function previewOpenApiExport(
  handle: string,
  target: import('./exports/types').ExportTarget,
) {
  return invoke<import('./exports/types').ExportPreview>('preview_openapi_export', {
    handle,
    target,
  });
}
export function saveOpenApiExport(handle: string, id: string) {
  return invoke<boolean>('save_openapi_export', { handle, id });
}
export function discardOpenApiExport(id: string) {
  return invoke<void>('discard_openapi_export', { id });
}

export function renameImportVariable(handle: string, id: string, oldName: string, name: string) {
  return invoke<import('./imports/types').ImportPreview>('rename_import_variable', {
    handle,
    id,
    oldName,
    name,
  });
}

export function previewHttpHeaders(id: string, input: import('./http/types').ExecutionInput) {
  return invoke<import('./http/types').RequestHeader[]>('preview_http_headers', { id, input });
}
export interface AuthenticationTestResult {
  success: boolean;
  status: number | null;
  message: string;
}
export function testAuthentication(
  id: string,
  input: import('./http/types').ExecutionInput,
  tokenOnly: boolean,
) {
  return invoke<AuthenticationTestResult>('test_authentication', { id, input, tokenOnly });
}

export interface MoveRequestInput {
  request: import('./projects/types').SavedRequest;
  sourceCollectionId: string | null;
  sourceRevision: string | null;
  targetCollectionId: string;
  targetRevision: string;
  projectRevision: string;
  allowMetadataLoss: boolean;
}
export function moveRequest(handle: string, input: MoveRequestInput) {
  return invoke<import('./projects/types').CollectionView[]>('move_request', { handle, input });
}
