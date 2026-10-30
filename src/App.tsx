import { MoveRequestDialog } from './projects/MoveRequestDialog';
import { requestLocation } from './projects/location';
import { Select } from './ui/Select';
import { ExportDialog } from './exports/ExportDialog';
import type { ExportPreview, ExportTarget } from './exports/types';
import { previewOpenApiExport, saveOpenApiExport, discardOpenApiExport } from './native';
import { PostmanImportPicker } from './imports/PostmanImportPicker';
import { ImportDialog } from './imports/ImportDialog';
import type { ImportPreview } from './imports/types';
import {
  chooseOpenApiImport,
  renameImportVariable,
  choosePostmanImport,
  commitOpenApiImport,
  discardOpenApiImport,
} from './native';
import { AuthEditor } from './auth/AuthEditor';
import { AuthTest } from './auth/AuthTest';
import { HeaderInspector } from './http/HeaderInspector';
import { splitUrl, urlFieldText } from './http/query';
import { emptyDraft } from './projects/types';
import { AuthDialog } from './auth/AuthDialog';
import { TokenControls } from './auth/TokenControls';
import { HistoryPanel } from './history/HistoryPanel';
import type { Auth, AuthTarget } from './auth/types';
import { authLabel, effectiveAuth, manualAuthHint, validAuth } from './auth/rules';
import brandMark from '../assets/fluxkit.svg';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Icon } from './components/Icon';
import { KeyValueEditor } from './components/KeyValueEditor';
import {
  environmentAction,
  getAppInfo,
  getServerAccount,
  getServerConnection,
  isDesktop,
} from './native';
import { useWorkspace } from './projects/useWorkspace';
import { isDirty, projectKindLabel, requestData } from './projects/types';
import { useHttp } from './http/useHttp';
import { ResponseView } from './http/ResponseView';
import { ProjectTree } from './projects/ProjectTree';
import { BodyEditor } from './projects/BodyEditor';
import { NameDialog, ConfirmDialog, ConflictDialog, QuitDialog } from './projects/Dialogs';
import { EnvironmentControls } from './environments/EnvironmentControls';
import { ServerConnection } from './server/ServerConnection';
import { VaultLock } from './server/VaultLock';
import { SettingsDialog } from './projects/SettingsDialog';
import { DEFAULT_REQUEST_PANE_RATIO } from './projects/session';
import { ServerUsers } from './server/ServerUsers';
import { VariableContext } from './environments/VariableContext';
import { VariableInput } from './environments/VariableInput';
import type { EnvironmentView } from './environments/types';

type EditorTab = 'Params' | 'Headers' | 'Body' | 'Auth';
const methods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS', 'TRACE'];
const editorTabs: EditorTab[] = ['Params', 'Headers', 'Body', 'Auth'];
const SPLIT_RATIO_MIN = 0.2;
const SPLIT_RATIO_MAX = 0.8;
function clampSplitRatio(value: number) {
  return Math.min(SPLIT_RATIO_MAX, Math.max(SPLIT_RATIO_MIN, value));
}
export default function App() {
  const variableSave = useRef<(() => Promise<boolean>) | null>(null);
  const authSave = useRef<(() => Promise<boolean>) | null>(null);
  const registerVariableSave = useCallback((save: (() => Promise<boolean>) | null) => {
    variableSave.current = save;
  }, []);
  const registerAuthSave = useCallback((save: (() => Promise<boolean>) | null) => {
    authSave.current = save;
  }, []);
  const [variableChanges, setVariableChanges] = useState(false);
  const [environmentState, setEnvironmentState] = useState<Record<string, EnvironmentView>>({});
  const onEnvironmentView = useCallback((handle: string, view: EnvironmentView) => {
    setEnvironmentState((items) => ({ ...items, [handle]: view }));
  }, []);
  const [variablesOpen, setVariablesOpen] = useState(false);
  const [serverConnectionOpen, setServerConnectionOpen] = useState(false);
  /** Raw URL field text while it still describes the draft (keeps a trailing `?` or `&`). */
  const [urlText, setUrlText] = useState<{ draft: number; text: string } | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [serverUsersOpen, setServerUsersOpen] = useState(false);
  // Reload the status-bar vault state when the dialogs that can change it close.
  const vaultDialogsOpen = settingsOpen || serverConnectionOpen;
  const [vaultRefresh, setVaultRefresh] = useState(0);
  const [vaultDialogsWereOpen, setVaultDialogsWereOpen] = useState(vaultDialogsOpen);
  if (vaultDialogsWereOpen !== vaultDialogsOpen) {
    setVaultDialogsWereOpen(vaultDialogsOpen);
    if (!vaultDialogsOpen) {
      setVaultRefresh((value) => value + 1);
    }
  }
  const [checkingVariables, setCheckingVariables] = useState(false);
  const [variableCheck, setVariableCheck] = useState<{
    requestId: string;
    handle: string;
    state: 'checking' | 'success' | 'error';
    message: string;
  } | null>(null);
  const [authChanges, setAuthChanges] = useState(false);
  const [historyHandle, setHistoryHandle] = useState<string | null>(null);
  const splitRef = useRef<HTMLDivElement>(null);
  // The divider position is part of the saved session; without one the response takes the
  // bottom quarter.
  const [requestPaneRatio, setRequestPaneRatio] = useState(DEFAULT_REQUEST_PANE_RATIO);
  const [resizingSplit, setResizingSplit] = useState(false);
  useEffect(() => {
    if (!resizingSplit) {
      return;
    }
    const previousCursor = document.body.style.cursor;
    const previousUserSelect = document.body.style.userSelect;
    document.body.style.cursor = 'row-resize';
    document.body.style.userSelect = 'none';
    return () => {
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousUserSelect;
    };
  }, [resizingSplit]);
  function resizeSplitFromPointer(clientY: number) {
    const rect = splitRef.current?.getBoundingClientRect();
    if (!rect || rect.height === 0) {
      return;
    }
    setRequestPaneRatio(clampSplitRatio((clientY - rect.top) / rect.height));
  }
  const [authDialog, setAuthDialog] = useState<{
    handle: string;
    target: AuthTarget;
    title: string;
    initial: Auth | null;
  } | null>(null);
  const [exportHandle, setExportHandle] = useState<string | null>(null);
  const [exportPreview, setExportPreview] = useState<ExportPreview | null>(null);
  const [exportBusy, setExportBusy] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  const [exportNotice, setExportNotice] = useState<string | null>(null);
  const [postmanPicker, setPostmanPicker] = useState(false);
  const [choosingImport, setChoosingImport] = useState(false);
  const [importBusy, setImportBusy] = useState(false);
  const [importStarted, setImportStarted] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  const [importDialog, setImportDialog] = useState<{
    handle: string;
    projectName: string;
    preview: ImportPreview;
  } | null>(null);
  const [importRefresh, setImportRefresh] = useState<Record<string, number>>({});
  const saveOtherEditors = useCallback(async () => {
    if (variableChanges && (!variableSave.current || !(await variableSave.current()))) {
      return false;
    }
    if (authChanges && (!authSave.current || !(await authSave.current()))) {
      return false;
    }
    return true;
  }, [variableChanges, authChanges]);
  const workspace = useWorkspace(
    variableChanges || authChanges,
    saveOtherEditors,
    choosingImport || importBusy || exportBusy,
    requestPaneRatio,
  );
  // Apply the restored divider position once, when the session has been read.
  const [appliedRatio, setAppliedRatio] = useState<number | null>(null);
  if (workspace.restoredRatio !== null && workspace.restoredRatio !== appliedRatio) {
    setAppliedRatio(workspace.restoredRatio);
    setRequestPaneRatio(clampSplitRatio(workspace.restoredRatio));
  }
  const [moveTarget, setMoveTarget] = useState<{
    draftId: number;
    collectionId?: string;
    folderId?: string | null;
  } | null>(null);
  const movingDraft = workspace.drafts.find((item) => item.id === moveTarget?.draftId);
  const movingProject = workspace.projects.find(
    (item) => item.handle === (movingDraft?.projectHandle ?? workspace.activeHandle),
  );
  function chooseMove(draftId: number, collectionId?: string, folderId?: string | null) {
    const item = workspace.drafts.find((item) => item.id === draftId);
    if (!item || busy || !project) {
      return;
    }
    if (item.projectHandle && item.projectHandle !== project.handle) {
      workspace.setError(
        'Choose a destination in this request’s project. Moving between projects is not supported.',
      );
      return;
    }
    workspace.setError(null);
    setMoveTarget({ draftId, collectionId, folderId });
  }
  const historyProject = workspace.projects.find((item) => item.handle === historyHandle);
  const {
    drafts,
    activeId,
    setActiveId,
    draft,
    project,
    busy: workspaceBusy,
    updateDraft,
    closeDraft,
  } = workspace;
  const busy = workspaceBusy || choosingImport || importBusy || exportBusy;
  const exportProject = workspace.projects.find((p) => p.handle === exportHandle);
  async function prepareExport(target: ExportTarget) {
    if (!exportHandle || exportBusy) {
      return;
    }
    setExportBusy(true);
    setExportError(null);
    try {
      setExportPreview(await previewOpenApiExport(exportHandle, target));
    } catch (error) {
      setExportError(typeof error === 'string' ? error : 'Could not prepare the export.');
    } finally {
      setExportBusy(false);
    }
  }
  async function saveExport() {
    if (!exportHandle || !exportPreview || exportBusy) {
      return;
    }
    setExportBusy(true);
    setExportError(null);
    try {
      if (await saveOpenApiExport(exportHandle, exportPreview.id)) {
        setExportHandle(null);
        setExportPreview(null);
        setExportNotice('OpenAPI YAML export saved.');
      }
    } catch (error) {
      setExportError(typeof error === 'string' ? error : 'Could not save the export.');
    } finally {
      setExportBusy(false);
    }
  }
  async function closeExport() {
    if (exportBusy) {
      return;
    }
    setExportBusy(true);
    try {
      if (exportPreview) {
        await discardOpenApiExport(exportPreview.id);
      }
      setExportHandle(null);
      setExportPreview(null);
      setExportError(null);
    } catch {
      setExportError('Could not close this export preview. Retry closing it.');
    } finally {
      setExportBusy(false);
    }
  }
  async function chooseImport(
    source: 'OpenAPI' | 'Postman' = 'OpenAPI',
    collectionId: string | null = null,
  ) {
    if (!project || busy) {
      return;
    }
    setPostmanPicker(false);
    setChoosingImport(true);
    workspace.setError(null);
    setImportError(null);
    setImportStarted(false);
    try {
      const preview =
        source === 'Postman'
          ? await choosePostmanImport(project.handle, collectionId)
          : await chooseOpenApiImport(project.handle);
      if (preview) {
        setImportDialog({ handle: project.handle, projectName: project.data.name, preview });
      }
    } catch (error) {
      workspace.setError(typeof error === 'string' ? error : 'Could not prepare the import.');
    } finally {
      setChoosingImport(false);
    }
  }
  async function renameImportedVariable(oldName: string, name: string) {
    if (!importDialog || importBusy || importStarted) {
      return false;
    }
    setImportBusy(true);
    setImportError(null);
    try {
      const preview = await renameImportVariable(
        importDialog.handle,
        importDialog.preview.id,
        oldName,
        name,
      );
      setImportDialog((current) => (current ? { ...current, preview } : null));
      return true;
    } catch (error) {
      setImportError(typeof error === 'string' ? error : 'Could not rename the import variable.');
      return false;
    } finally {
      setImportBusy(false);
    }
  }
  async function commitImport(storeValues: boolean) {
    if (!importDialog || importBusy) {
      return;
    }
    setImportBusy(true);
    setImportStarted(true);
    setImportError(null);
    try {
      const result = await commitOpenApiImport(
        importDialog.handle,
        importDialog.preview.id,
        storeValues,
      );
      workspace.acceptImportedProject(result.project);
      setEnvironmentState((items) => {
        const next = { ...items };
        delete next[importDialog.handle];
        return next;
      });
      setImportRefresh((items) => ({
        ...items,
        [importDialog.handle]: (items[importDialog.handle] ?? 0) + 1,
      }));
      if (result.error) {
        setImportError(result.error);
      } else {
        setImportDialog(null);
      }
    } catch (error) {
      setImportError(
        typeof error === 'string'
          ? error
          : 'Import could not finish. Inspect Project variables before retrying.',
      );
    } finally {
      setImportBusy(false);
    }
  }
  async function cancelImport() {
    if (!importDialog || importBusy) {
      return;
    }
    setImportBusy(true);
    try {
      await discardOpenApiImport(importDialog.preview.id);
      setImportDialog(null);
    } catch {
      setImportError('Could not release this preview. Retry closing it.');
    } finally {
      setImportBusy(false);
    }
  }
  const checkResult =
    variableCheck?.requestId === draft?.requestId && variableCheck?.handle === project?.handle
      ? variableCheck
      : null;
  const activeEnvironmentView = project ? (environmentState[project.handle] ?? null) : null;
  const http = useHttp(drafts);
  const [timeoutSeconds, setTimeoutSeconds] = useState(60);
  const execution = draft ? http.states[draft.id] : undefined;
  const authProject = draft?.projectHandle
    ? workspace.projects.find((item) => item.handle === draft.projectHandle)
    : undefined;
  const executionEnvironment = authProject ? (environmentState[authProject.handle] ?? null) : null;
  const effective = effectiveAuth(
    authProject,
    draft?.collectionId ?? null,
    draft?.folderId ?? null,
    draft?.auth,
  );
  function editAuthentication(collectionId: string | null = null, folderId: string | null = null) {
    if (!project || !activeEnvironmentView) {
      return;
    }
    const collection = project.collections.find((item) => item.data.id === collectionId)?.data;
    const folder = collection?.folders.find((item) => item.id === folderId);
    workspace.setError(null);
    setAuthDialog({
      handle: project.handle,
      target: { collectionId, folderId },
      title: `Authentication: ${folder?.name ?? collection?.name ?? project.data.name}`,
      initial: folderId
        ? (folder?.auth ?? null)
        : collectionId
          ? (collection?.auth ?? null)
          : (project.data.auth ?? null),
    });
  }
  const savedImportIssues = draft?.collectionId
    ? (authProject?.collections
        .find((c) => c.data.id === draft.collectionId)
        ?.data.requests.find((r) => r.id === draft.requestId)?.importIssues ?? [])
    : [];
  const draftImportIssues = draft?.importIssues ?? [];
  const needsImportReview = draftImportIssues.length > 0 || savedImportIssues.length > 0;
  const canSend =
    workspace.desktop &&
    !needsImportReview &&
    !busy &&
    (!authProject || executionEnvironment !== null) &&
    timeoutSeconds >= 1 &&
    timeoutSeconds <= 300;
  const executionScope =
    authProject && executionEnvironment && draft
      ? {
          handle: authProject.handle,
          projectRevision: authProject.revision,
          collectionId: draft.collectionId,
          collectionRevision:
            authProject.collections.find((item) => item.data.id === draft.collectionId)?.revision ??
            null,
          environment: executionEnvironment.selectedEnvironment,
          revision: executionEnvironment.revision,
        }
      : null;
  function send(saveResponse = false) {
    if (!draft || !canSend) {
      return;
    }
    const scope = executionScope;
    const environmentLabel =
      executionEnvironment?.data.environments.find((item) => item.id === scope?.environment)
        ?.name ?? (authProject ? 'Defaults' : 'Session');
    void http.send(draft, scope, timeoutSeconds, saveResponse, environmentLabel);
  }
  const variableContext = useMemo(
    () => ({ handle: project?.handle ?? null, view: activeEnvironmentView }),
    [project?.handle, activeEnvironmentView],
  );
  const [editorTab, setEditorTab] = useState<EditorTab>('Params');
  const [connection, setConnection] = useState(() =>
    isDesktop() ? 'Connecting to desktop…' : 'Browser preview',
  );

  useEffect(() => {
    let cancelled = false;
    if (!isDesktop()) {
      return;
    } else {
      void getAppInfo().then(
        (info) => {
          if (!cancelled) {
            setConnection(`Desktop connected · v${info.version}`);
          }
        },
        () => {
          if (!cancelled) {
            setConnection('Desktop connection unavailable');
          }
        },
      );
    }
    return () => {
      cancelled = true;
    };
  }, []);

  const [serverStatus, setServerStatus] = useState<'local' | 'connected' | 'unreachable'>('local');
  const [serverStatusUrl, setServerStatusUrl] = useState<string | null>(null);
  const [serverAdmin, setServerAdmin] = useState(false);
  const checkServerStatus = useCallback(async () => {
    if (!workspace.desktop) {
      return;
    }
    try {
      const current = await getServerConnection();
      if (!current.authenticated) {
        setServerStatus('local');
        setServerStatusUrl(null);
        setServerAdmin(false);
        return;
      }
      setServerStatusUrl(current.serverUrl);
      try {
        const account = await getServerAccount();
        setServerStatus('connected');
        setServerAdmin(account.isServerAdmin);
      } catch {
        setServerStatus('unreachable');
      }
    } catch {
      setServerStatus('local');
      setServerStatusUrl(null);
      setServerAdmin(false);
    }
  }, [workspace.desktop]);

  useEffect(() => {
    const immediate = setTimeout(() => void checkServerStatus(), 0);
    const timer = setInterval(() => void checkServerStatus(), 30000);
    return () => {
      clearTimeout(immediate);
      clearInterval(timer);
    };
  }, [checkServerStatus]);

  function addDraft(collectionId?: string, folderId: string | null = null) {
    workspace.addDraft(collectionId, folderId);
    setEditorTab('Params');
  }
  useEffect(() => {
    function onSave(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        if (
          !workspace.nameDialog &&
          !workspace.confirmation &&
          !workspace.conflict &&
          !workspace.quitOpen &&
          !serverUsersOpen &&
          !variablesOpen &&
          !authDialog &&
          !moveTarget &&
          !importDialog &&
          !postmanPicker &&
          !exportHandle &&
          !historyProject
        ) {
          void workspace.saveDraft();
        }
      }
    }
    window.addEventListener('keydown', onSave);
    return () => window.removeEventListener('keydown', onSave);
  }, [
    workspace,
    serverUsersOpen,
    variablesOpen,
    authDialog,
    moveTarget,
    historyProject,
    importDialog,
    postmanPicker,
    exportHandle,
  ]);

  return (
    <VariableContext.Provider value={variableContext}>
      <div className="app-shell" inert={workspace.quitOpen || !!moveTarget}>
        <aside className="sidebar" aria-label="Workspace navigation">
          <div className="brand">
            <img className="brand-mark" src={brandMark} alt="" />
            <span>
              fluxkit<span className="brand-dot">.</span>
            </span>
            <span className="local-badge">LOCAL</span>
          </div>
          <div className="workspace-label">YOUR WORKSPACE</div>
          <div className="sidebar-scroll">
            {project ? (
              <>
                <div className="project-picker">
                  <Select
                    aria-label="Active project"
                    value={workspace.activeHandle ?? ''}
                    disabled={busy}
                    onValueChange={(selectedValue) => workspace.setActiveHandle(selectedValue)}
                  >
                    {workspace.projects.map((item) => (
                      <option key={item.handle} value={item.handle}>
                        {item.data.name}
                        {projectKindLabel(item)}
                      </option>
                    ))}
                  </Select>
                </div>
                <div className="project-actions">
                  <button disabled={busy} onClick={() => void workspace.reloadProject()}>
                    Reload
                  </button>
                  <button disabled={busy} onClick={() => void workspace.closeProject()}>
                    Close project
                  </button>
                </div>
                <div className="sidebar-section collections-heading">
                  <span>COLLECTIONS</span>
                  <button
                    className="icon-button"
                    aria-label="Create collection"
                    disabled={busy || project.readOnly}
                    onClick={workspace.createCollection}
                  >
                    <Icon name="plus" />
                  </button>
                </div>
                {project.collections.length === 0 && (
                  <p className="sidebar-hint">Create a collection to save your requests.</p>
                )}
                <ProjectTree
                  collections={project.collections}
                  busy={busy}
                  readOnly={project.readOnly}
                  onAuth={(id, folder) => editAuthentication(id, folder?.id ?? null)}
                  projectHandle={project.handle}
                  onDropDraft={chooseMove}
                  onDropRequest={(handle, sourceId, requestId, collectionId, folderId) => {
                    if (handle !== project.handle || busy) {
                      return;
                    }
                    const request = project.collections
                      .find((item) => item.data.id === sourceId)
                      ?.data.requests.find((item) => item.id === requestId);
                    if (!request) {
                      return;
                    }
                    const id = workspace.openRequest(sourceId, request);
                    if (id !== undefined) {
                      workspace.setError(null);
                      setMoveTarget({ draftId: id, collectionId, folderId });
                    }
                  }}
                  onSelect={workspace.select}
                  onNew={addDraft}
                  onFolder={workspace.createFolder}
                  onRename={workspace.renameEntity}
                  onRemove={(...args) => void workspace.removeEntity(...args)}
                  onOpen={workspace.openRequest}
                />
              </>
            ) : (
              <div className="project-placeholder">
                <Icon name="folder" />
                <div>
                  <strong>No project open</strong>
                  <small>A little space for your APIs.</small>
                </div>
              </div>
            )}
            <div className="sidebar-section">
              <span>OPEN REQUESTS</span>
              <button className="icon-button" aria-label="New request" onClick={() => addDraft()}>
                <Icon name="plus" />
              </button>
            </div>
            <div className="draft-list">
              {drafts.length === 0 ? (
                <p className="sidebar-hint">Your requests will appear here.</p>
              ) : (
                drafts.map((item) => (
                  <button
                    key={item.id}
                    className={`draft-link ${item.id === activeId ? 'selected' : ''}`}
                    onClick={() => setActiveId(item.id)}
                  >
                    <span className={`method-label method-${item.method.toLowerCase()}`}>
                      {item.method}
                    </span>
                    <span>{item.name}</span>
                  </button>
                ))
              )}
            </div>
          </div>
          <div className="sidebar-note">
            <span className="tiny-orbit" />
            <div>
              <strong>
                {project?.serverRecovery
                  ? 'Recovery copy.'
                  : project?.readOnly
                    ? 'Server snapshot.'
                    : project?.serverSync
                      ? 'Server workspace.'
                      : 'Local by design.'}
              </strong>
              <p>
                {project?.readOnly
                  ? 'Read-only copy cached on this machine.'
                  : project?.serverSync
                    ? 'Changes stay here until you sync from Server connection.'
                    : 'Your workspace. Your machine.'}
              </p>
            </div>
          </div>
          <div className="sidebar-footer">
            <Icon name="bolt" size={14} />
            <span>Small footprint. Big plans.</span>
          </div>
        </aside>

        <div className="main-shell">
          <header className="workspace-header">
            <div className="breadcrumb">
              Workspace <Icon name="chevron" size={12} />
              {draft && (
                <button
                  className="breadcrumb-location"
                  disabled={!project || busy}
                  onClick={() => chooseMove(draft.id)}
                  aria-label="Change request location"
                >
                  {requestLocation(authProject ?? project, draft.collectionId, draft.folderId)}
                </button>
              )}
              <span>{draft ? draft.name : 'Overview'}</span>
            </div>
            {project ? (
              <EnvironmentControls
                serverOwned={project?.serverOwned}
                key={`${project.handle}-${importRefresh[project.handle] ?? 0}`}
                handle={project.handle}
                ask={workspace.ask}
                onDirty={setVariableChanges}
                onRegisterSave={registerVariableSave}
                onOpen={setVariablesOpen}
                onView={onEnvironmentView}
              />
            ) : (
              <span className="environment-pill">
                <span className="status-dot" />
                No environment
              </span>
            )}
          </header>
          <main>
            {workspace.error && (
              <div role="alert" className="error-banner">
                <span>{workspace.error}</span>
                {workspace.error.startsWith('Your server sign-in has ended') && (
                  <button
                    className="secondary-button"
                    onClick={() => {
                      workspace.setError(null);
                      setServerConnectionOpen(true);
                    }}
                  >
                    Sign in again
                  </button>
                )}
                <button
                  className="icon-button"
                  aria-label="Dismiss error"
                  onClick={() => workspace.setError(null)}
                >
                  <Icon name="close" />
                </button>
              </div>
            )}
            {project?.readOnly && (
              <div className="operation-status" role="status">
                {project.serverRecovery
                  ? 'Read-only recovery copy. Your saved edits are preserved here. Copy desired changes into the current server workspace before syncing.'
                  : 'Read-only server snapshot. Changes are disabled in this workspace.'}
              </div>
            )}
            {busy && (
              <div className="operation-status" role="status">
                Working with local files…
              </div>
            )}
            <div className="request-tabbar" aria-label="Request tabs">
              <div className="request-tabs">
                {drafts.map((item) => (
                  <div
                    className={`request-tab ${item.id === activeId ? 'active' : ''}`}
                    key={item.id}
                    draggable={!busy}
                    onDragStart={(event) => {
                      if (busy) {
                        event.preventDefault();
                        return;
                      }
                      event.dataTransfer.setData('application/x-fluxkit-draft', String(item.id));
                      event.dataTransfer.effectAllowed = 'move';
                    }}
                  >
                    <button
                      className="tab-select"
                      aria-label={`${item.method} ${item.name}`}
                      aria-pressed={item.id === activeId}
                      onClick={() => setActiveId(item.id)}
                    >
                      <span className={`method-label method-${item.method.toLowerCase()}`}>
                        {item.method}
                      </span>
                      <span>{item.name}</span>
                      {isDirty(item) && <span className="draft-dot" title="Unsaved changes" />}
                    </button>
                    <button
                      className="icon-button"
                      aria-label={`Close ${item.name}`}
                      onClick={() => closeDraft(item.id)}
                    >
                      <Icon name="close" size={12} />
                    </button>
                  </div>
                ))}
              </div>
              <button
                className="icon-button new-tab"
                aria-label="New request tab"
                onClick={() => addDraft()}
              >
                <Icon name="plus" />
              </button>
              <span className="session-label">
                {project?.readOnly
                  ? 'SERVER SNAPSHOT · READ ONLY'
                  : project
                    ? project.serverSync
                      ? 'SERVER WORKSPACE · SAVED ON SERVER'
                      : 'LOCAL PROJECT'
                    : 'SESSION ONLY'}
              </span>
            </div>

            <div className="request-response-split" ref={splitRef}>
              <div className="split-top" style={{ flexBasis: `${requestPaneRatio * 100}%` }}>
                {!draft ? (
                  <section className="welcome">
                    <span className="eyebrow">
                      <span className="status-dot" /> A FRESH START
                    </span>
                    <div className="welcome-symbol">
                      <Icon name="code" size={38} />
                    </div>
                    <h1>
                      Less friction.
                      <br />
                      <span>More flow.</span>
                    </h1>
                    <p>
                      A focused home for your APIs.
                      <br />
                      Start with a request and make yourself at home.
                    </p>
                    <button className="primary-button" onClick={() => addDraft()}>
                      <Icon name="plus" size={16} /> New request <Icon name="arrow" size={16} />
                    </button>
                    <div className="welcome-footnote">
                      Open a project to organize and save your requests locally.
                    </div>
                    <div className="welcome-grid">
                      <div>
                        <span className="feature-number">01</span>
                        <strong>Built for focus</strong>
                        <p>A clear view of the request and response.</p>
                      </div>
                      <div>
                        <span className="feature-number">02</span>
                        <strong>Yours to keep</strong>
                        <p>Local files. No account required.</p>
                      </div>
                      <div>
                        <span className="feature-number">03</span>
                        <strong>Ready to grow</strong>
                        <p>A small foundation for a capable toolkit.</p>
                      </div>
                    </div>
                  </section>
                ) : (
                  <section className="request-editor" aria-label="Request editor">
                    {needsImportReview && (
                      <section className="import-review" aria-label="Import review required">
                        <h3>Imported request needs review</h3>
                        <p>
                          Sending and token acquisition are locked. Review the imported
                          configuration, choose local attachments and configure
                          authentication/variables as needed.
                        </p>
                        <ul>
                          {(draftImportIssues.length ? draftImportIssues : savedImportIssues).map(
                            (issue, index) => (
                              <li key={index}>{issue}</li>
                            ),
                          )}
                        </ul>
                        <label>
                          <input
                            type="checkbox"
                            checked={!draftImportIssues.length}
                            disabled={busy}
                            onChange={(e) =>
                              updateDraft({
                                importIssues: e.target.checked ? [] : savedImportIssues,
                              })
                            }
                          />{' '}
                          I reviewed and configured this request
                        </label>
                        {!draftImportIssues.length && (
                          <p>Save this request to record the review and enable sending.</p>
                        )}
                      </section>
                    )}
                    <div className="editor-heading">
                      <div>
                        <span className="eyebrow">
                          {isDirty(draft) ? 'UNSAVED CHANGES' : 'SAVED REQUEST'}
                        </span>
                        <input
                          className="request-name"
                          aria-label="Request name"
                          value={draft.name}
                          onChange={(event) => updateDraft({ name: event.target.value })}
                        />
                        <p className="request-location">
                          {requestLocation(
                            authProject ?? project,
                            draft.collectionId,
                            draft.folderId,
                          )}
                        </p>
                      </div>
                      <div className="save-controls">
                        <button
                          className="secondary-button"
                          disabled={!project || project.readOnly || busy}
                          title={requestLocation(
                            authProject ?? project,
                            draft.collectionId,
                            draft.folderId,
                          )}
                          onClick={() => chooseMove(draft.id)}
                        >
                          Move…
                        </button>
                        <span className="subtle-tag">{isDirty(draft) ? 'Unsaved' : 'Saved'}</span>
                        <button
                          className="secondary-button"
                          disabled={!project || checkingVariables || busy}
                          aria-label="Check variables"
                          title="Check whether this request's placeholders have values. Does not send a request."
                          onClick={() => {
                            if (!project) {
                              return;
                            }
                            const text = [
                              draft.url,
                              ...[...draft.params, ...draft.headers]
                                .filter((entry) => entry.enabled && entry.key !== '')
                                .flatMap((entry) => [entry.key, entry.value]),
                              ...(['text', 'json'].includes(draft.bodyFormat) ? [draft.body] : []),
                              ...(['urlEncoded', 'multipart'].includes(draft.bodyFormat)
                                ? draft.fields
                                    .filter((field) => field.enabled && field.key !== '')
                                    .flatMap((field) => [
                                      field.key,
                                      ...(draft.bodyFormat === 'multipart' && field.file
                                        ? []
                                        : [field.value]),
                                    ])
                                : []),
                            ].join('\n');
                            const context = { requestId: draft.requestId, handle: project.handle };
                            setCheckingVariables(true);
                            setVariableCheck({
                              ...context,
                              state: 'checking',
                              message: 'Checking placeholders in the selected project environment…',
                            });
                            void environmentAction(project.handle, { kind: 'check', text })
                              .then(
                                (reply) => {
                                  const environment =
                                    reply.view.data.environments.find(
                                      (item) => item.id === reply.view.selectedEnvironment,
                                    )?.name ?? 'Defaults';
                                  setVariableCheck({
                                    ...context,
                                    state: 'success',
                                    message: text.includes('{{')
                                      ? `All request-field placeholders have values in ${environment} or shared defaults. Authentication references are validated when sending.`
                                      : 'This request contains no variable placeholders to resolve. Authentication references are validated when sending.',
                                  });
                                },
                                (reason: unknown) =>
                                  setVariableCheck({
                                    ...context,
                                    state: 'error',
                                    message:
                                      typeof reason === 'string'
                                        ? reason
                                        : 'Could not check variables.',
                                  }),
                              )
                              .finally(() => setCheckingVariables(false));
                          }}
                        >
                          {checkingVariables ? 'Checking…' : 'Check variables'}
                        </button>
                        <button
                          className="secondary-button"
                          disabled={
                            !workspace.desktop || project?.readOnly || busy || !isDirty(draft)
                          }
                          onClick={() => void workspace.saveDraft()}
                        >
                          Save
                        </button>
                      </div>
                    </div>
                    <div className="url-bar">
                      <Select
                        aria-label="HTTP method"
                        value={draft.method}
                        className={`method-${draft.method.toLowerCase()}`}
                        onValueChange={(selectedValue) => updateDraft({ method: selectedValue })}
                      >
                        {methods.map((method) => (
                          <option key={method}>{method}</option>
                        ))}
                      </Select>
                      <VariableInput
                        label="Request URL"
                        variant="url"
                        placeholder="https://api.example.com/v1/resource"
                        value={urlFieldText(draft, urlText)}
                        onChange={(text) => {
                          setUrlText({ draft: draft.id, text });
                          updateDraft(splitUrl(text, draft.params));
                        }}
                      />
                      <button
                        className="send-button"
                        disabled={execution?.running ? false : !canSend}
                        onClick={() => (execution?.running ? void http.cancel(draft.id) : send())}
                        title={
                          execution?.running
                            ? 'Cancel this request'
                            : 'Send the current draft without saving it'
                        }
                      >
                        {execution?.running ? 'Cancel' : 'Send'} <Icon name="arrow" size={15} />
                      </button>
                    </div>
                    <div className="send-options">
                      <label>
                        Timeout (seconds)
                        <input
                          aria-label="Request timeout in seconds"
                          type="number"
                          min={1}
                          max={300}
                          value={timeoutSeconds}
                          onChange={(event) => setTimeoutSeconds(Number(event.target.value))}
                        />
                      </label>
                      <button
                        className="secondary-button"
                        disabled={!canSend || execution?.running}
                        onClick={() => send(true)}
                      >
                        Send & save response…
                      </button>
                      <span>Direct connection · redirects shown · no cookie session</span>
                    </div>
                    <p className="auth-effective" aria-label="Effective authentication">
                      Authentication: <strong>{authLabel(effective.auth)}</strong> ·{' '}
                      {effective.source}
                      {draft.auth ? '' : ' · inherited'}
                      {manualAuthHint(effective.auth, draft) && (
                        <span className="auth-override">
                          {' '}
                          · {manualAuthHint(effective.auth, draft)}
                        </span>
                      )}
                    </p>
                    {checkResult && (
                      <div
                        className={`variable-check variable-check-${checkResult.state}`}
                        role={checkResult.state === 'error' ? 'alert' : 'status'}
                        aria-label="Variable check result"
                        aria-busy={checkResult.state === 'checking'}
                      >
                        <strong>
                          {checkResult.state === 'checking'
                            ? 'Checking variables…'
                            : checkResult.state === 'error'
                              ? 'Variable check failed'
                              : 'Variable check complete'}
                        </strong>
                        <span>{checkResult.message}</span>
                        <small>
                          No HTTP request is sent. Run the check again after editing or changing
                          environments.
                        </small>
                      </div>
                    )}
                    <div className="editor-tabs" role="tablist" aria-label="Request configuration">
                      {editorTabs.map((tab) => (
                        <button
                          key={tab}
                          role="tab"
                          id={`tab-${tab}`}
                          aria-controls={`panel-${tab}`}
                          tabIndex={editorTab === tab ? 0 : -1}
                          onKeyDown={(event) => {
                            const index = editorTabs.indexOf(tab);
                            let target: EditorTab | undefined;
                            if (event.key === 'ArrowRight') {
                              target = editorTabs[(index + 1) % editorTabs.length];
                            } else if (event.key === 'ArrowLeft') {
                              target =
                                editorTabs[(index + editorTabs.length - 1) % editorTabs.length];
                            } else if (event.key === 'Home') {
                              target = editorTabs[0];
                            } else if (event.key === 'End') {
                              target = editorTabs.at(-1);
                            }
                            if (target) {
                              event.preventDefault();
                              setEditorTab(target);
                              document.getElementById(`tab-${target}`)?.focus();
                            }
                          }}
                          aria-selected={editorTab === tab}
                          className={editorTab === tab ? 'active' : ''}
                          onClick={() => setEditorTab(tab)}
                        >
                          {tab}
                          {tab === 'Headers' &&
                          draft.headers.some((entry) => entry.key && entry.enabled) ? (
                            <span className="count-badge">
                              {draft.headers.filter((entry) => entry.key && entry.enabled).length}
                            </span>
                          ) : null}
                        </button>
                      ))}
                    </div>
                    <div
                      className="editor-panel"
                      role="tabpanel"
                      id={`panel-${editorTab}`}
                      aria-labelledby={`tab-${editorTab}`}
                    >
                      {editorTab === 'Params' && (
                        <KeyValueEditor
                          label="Parameter"
                          entries={draft.params}
                          onChange={(params) => updateDraft({ params })}
                        />
                      )}
                      {editorTab === 'Headers' && (
                        <>
                          <HeaderInspector
                            input={{
                              request: requestData(draft),
                              scope: executionScope,
                              timeoutMs: timeoutSeconds * 1000,
                              saveResponse: false,
                            }}
                            disabled={!canSend}
                            sent={execution?.result?.requestHeaders}
                          />
                          <KeyValueEditor
                            label="Header"
                            entries={draft.headers}
                            onChange={(headers) => updateDraft({ headers })}
                          />
                        </>
                      )}
                      {editorTab === 'Body' && (
                        <BodyEditor
                          draft={draft}
                          onChange={updateDraft}
                          onFile={workspace.attach}
                        />
                      )}
                      {editorTab === 'Auth' && (
                        <>
                          <AuthTest
                            key={`test:${JSON.stringify([draft.id, effective, executionScope])}`}
                            input={{
                              request: requestData(draft),
                              scope: executionScope,
                              timeoutMs: timeoutSeconds * 1000,
                              saveResponse: false,
                            }}
                            tokenOnly={['oauth2', 'customLogin'].includes(
                              effective.auth?.kind ?? '',
                            )}
                            disabled={
                              !canSend ||
                              !validAuth(
                                draft.auth ?? null,
                                executionEnvironment?.data.variables ?? [],
                              )
                            }
                          />
                          {(effective.auth?.kind === 'oauth2' ||
                            effective.auth?.kind === 'customLogin') && (
                            <TokenControls
                              key={`token:${JSON.stringify([draft.id, effective, executionScope])}`}
                              input={{
                                request: requestData(draft),
                                scope: executionScope,
                                timeoutMs: timeoutSeconds * 1000,
                                saveResponse: false,
                              }}
                              disabled={!canSend || !executionScope}
                            />
                          )}
                          <AuthEditor
                            value={draft.auth ?? null}
                            onChange={(auth) => updateDraft({ auth })}
                            variables={
                              authProject ? (executionEnvironment?.data.variables ?? []) : []
                            }
                          />
                        </>
                      )}
                    </div>
                    <p className="draft-notice">
                      {draft.savedSnapshot
                        ? 'Saved locally. Use Save or Cmd/Ctrl+S to keep your changes.'
                        : 'Unsaved request. Open a project and collection, then use Save or Cmd/Ctrl+S.'}{' '}
                      Sending uses this draft without saving changes.
                    </p>
                  </section>
                )}
              </div>
              <div
                className={`pane-resizer${resizingSplit ? ' is-dragging' : ''}`}
                role="separator"
                aria-orientation="horizontal"
                aria-label="Resize request and response panes"
                aria-valuenow={Math.round(requestPaneRatio * 100)}
                aria-valuemin={Math.round(SPLIT_RATIO_MIN * 100)}
                aria-valuemax={Math.round(SPLIT_RATIO_MAX * 100)}
                tabIndex={0}
                onPointerDown={(event) => {
                  event.currentTarget.setPointerCapture(event.pointerId);
                  setResizingSplit(true);
                  resizeSplitFromPointer(event.clientY);
                }}
                onPointerMove={(event) => {
                  if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                    resizeSplitFromPointer(event.clientY);
                  }
                }}
                onPointerUp={(event) => {
                  event.currentTarget.releasePointerCapture(event.pointerId);
                  setResizingSplit(false);
                }}
                onKeyDown={(event) => {
                  if (event.key === 'ArrowUp') {
                    event.preventDefault();
                    setRequestPaneRatio((value) => clampSplitRatio(value - 0.02));
                  } else if (event.key === 'ArrowDown') {
                    event.preventDefault();
                    setRequestPaneRatio((value) => clampSplitRatio(value + 0.02));
                  } else if (event.key === 'Home') {
                    event.preventDefault();
                    setRequestPaneRatio(SPLIT_RATIO_MIN);
                  } else if (event.key === 'End') {
                    event.preventDefault();
                    setRequestPaneRatio(SPLIT_RATIO_MAX);
                  }
                }}
              />
              <div className="split-bottom">
                <ResponseView
                  key={draft?.id ?? 'empty'}
                  environmentLabel={execution?.environmentLabel}
                  result={execution?.result ?? null}
                  running={execution?.running ?? false}
                />
              </div>
            </div>
          </main>
          <footer className="statusbar">
            <span>
              <span className="status-dot" />
              {connection}
            </span>
            <span className="statusbar-right">
              {serverStatus === 'connected' ? (
                <span className="statusbar-connection" title={serverStatusUrl ?? undefined}>
                  <span className="status-dot" /> Connected to server
                </span>
              ) : serverStatus === 'unreachable' ? (
                <span className="statusbar-connection" title={serverStatusUrl ?? undefined}>
                  <span className="status-dot status-dot-warning" /> Server unreachable
                </span>
              ) : (
                <>
                  Local workspace <span className="status-divider">/</span> No cloud required
                </>
              )}
              {serverStatus !== 'local' && (
                <VaultLock
                  refreshKey={vaultRefresh}
                  onSetup={() => setServerConnectionOpen(true)}
                />
              )}
              <button
                className="icon-button settings-trigger"
                aria-label="Settings"
                onClick={() => setSettingsOpen(true)}
              >
                <Icon name="settings" size={13} />
              </button>
            </span>
          </footer>
        </div>
        {historyProject && (
          <HistoryPanel
            key={historyProject.handle}
            handle={historyProject.handle}
            name={historyProject.data.name}
            refresh={http.states}
            ask={workspace.ask}
            onClose={() => setHistoryHandle(null)}
          />
        )}
        {exportNotice && (
          <p role="status" className="export-notice">
            {exportNotice}
          </p>
        )}
        {exportProject && (
          <ExportDialog
            project={exportProject}
            dirty={drafts.some((d) => d.projectHandle === exportProject.handle && isDirty(d))}
            preview={exportPreview}
            busy={exportBusy}
            error={exportError}
            onPreview={(t) => void prepareExport(t)}
            onSave={() => void saveExport()}
            onClose={() => void closeExport()}
          />
        )}
        {postmanPicker && project && (
          <PostmanImportPicker
            collections={project.collections}
            onChoose={(id) => void chooseImport('Postman', id)}
            onCancel={() => setPostmanPicker(false)}
          />
        )}
        {importDialog && (
          <ImportDialog
            serverOwned={
              workspace.projects.find((project) => project.handle === importDialog.handle)
                ?.serverOwned
            }
            preview={importDialog.preview}
            projectName={importDialog.projectName}
            busy={importBusy}
            started={importStarted}
            error={importError}
            onImport={(values) => void commitImport(values)}
            onRename={renameImportedVariable}
            onCancel={() => void cancelImport()}
          />
        )}
        {authDialog && (
          <AuthDialog
            key={`${authDialog.handle}-${authDialog.target.collectionId}-${authDialog.target.folderId}`}
            title={authDialog.title}
            initial={authDialog.initial}
            testInput={(() => {
              const owner = workspace.projects.find((item) => item.handle === authDialog.handle);
              const environment = environmentState[authDialog.handle];
              if (!owner || !environment) {
                return null;
              }
              const collection = owner.collections.find(
                (item) => item.data.id === authDialog.target.collectionId,
              );
              const parentFolder = authDialog.target.folderId
                ? (collection?.data.folders.find(
                    (folder) => folder.id === authDialog.target.folderId,
                  )?.parentId ?? null)
                : null;
              const parentCollection = authDialog.target.folderId ? collection : undefined;
              return {
                request: requestData(
                  emptyDraft(0, owner.handle, parentCollection?.data.id ?? null, parentFolder),
                ),
                scope: {
                  handle: owner.handle,
                  projectRevision: owner.revision,
                  collectionId: parentCollection?.data.id ?? null,
                  collectionRevision: parentCollection?.revision ?? null,
                  environment: environment.selectedEnvironment,
                  revision: environment.revision,
                },
                timeoutMs: 60000,
                saveResponse: false,
              };
            })()}
            inheritedAuth={(() => {
              const owner = workspace.projects.find((item) => item.handle === authDialog.handle);
              const collection = owner?.collections.find(
                (item) => item.data.id === authDialog.target.collectionId,
              )?.data;
              const parent = authDialog.target.folderId
                ? (collection?.folders.find((item) => item.id === authDialog.target.folderId)
                    ?.parentId ?? null)
                : null;
              return effectiveAuth(
                owner,
                authDialog.target.folderId ? authDialog.target.collectionId : null,
                parent,
              ).auth;
            })()}

            inherited={(() => {
              const owner = workspace.projects.find((item) => item.handle === authDialog.handle);
              const collection = owner?.collections.find(
                (item) => item.data.id === authDialog.target.collectionId,
              )?.data;
              const parent = authDialog.target.folderId
                ? (collection?.folders.find((item) => item.id === authDialog.target.folderId)
                    ?.parentId ?? null)
                : null;
              const inherited = effectiveAuth(
                owner,
                authDialog.target.folderId ? authDialog.target.collectionId : null,
                parent,
              );
              return `${authLabel(inherited.auth)} · ${inherited.source}`;
            })()}
            root={!authDialog.target.collectionId}
            variables={environmentState[authDialog.handle]?.data.variables ?? []}
            busy={busy}
            error={workspace.error}
            onDirty={setAuthChanges}
            onRegisterSave={registerAuthSave}
            onSave={async (auth) => {
              const saved = await workspace.saveAuthentication(authDialog.target, auth);
              if (saved) {
                setAuthDialog(null);
              }
              return saved;
            }}
            onCancel={(dirty) => {
              if (!dirty) {
                setAuthDialog(null);
              } else {
                void workspace.ask('Discard unsaved authentication changes?').then((yes) => {
                  if (yes) {
                    setAuthDialog(null);
                  }
                });
              }
            }}
          />
        )}
        {workspace.nameDialog && (
          <NameDialog
            key={`${workspace.nameDialog.title}-${workspace.nameDialog.initial}`}
            title={workspace.nameDialog.title}
            initial={workspace.nameDialog.initial}
            onCancel={() => workspace.setNameDialog(null)}
            onSubmit={(name) => {
              workspace.nameDialog?.submit(name);
              workspace.setNameDialog(null);
            }}
          />
        )}
        {workspace.conflict && !workspace.confirmation && (
          <ConflictDialog
            message={workspace.conflict.message}
            onChoose={workspace.resolveConflict}
          />
        )}
        {workspace.confirmation && (
          <ConfirmDialog
            message={workspace.confirmation.message}
            onAnswer={(yes) => {
              workspace.confirmation?.resolve(yes);
              workspace.setConfirmation(null);
            }}
          />
        )}
      </div>
      {moveTarget && movingDraft && movingProject && (
        <div inert={workspace.quitOpen}>
          <MoveRequestDialog
            key={`${moveTarget.draftId}-${moveTarget.collectionId}-${moveTarget.folderId}`}
            draft={movingDraft}
            project={movingProject}
            initialCollectionId={moveTarget.collectionId}
            initialFolderId={moveTarget.folderId}
            busy={busy}
            error={workspace.error}
            onCancel={() => setMoveTarget(null)}
            onMove={(collectionId, folderId, allowMetadataLoss) => {
              void workspace
                .moveDraft(movingDraft.id, collectionId, folderId, allowMetadataLoss)
                .then((saved) => {
                  if (saved) {
                    setMoveTarget(null);
                  }
                });
            }}
          />
        </div>
      )}
      {workspace.quitOpen && (
        <QuitDialog
          saving={workspace.quitSaving}
          error={workspace.quitError}
          onChoose={(choice) => void workspace.finishQuit(choice)}
        />
      )}
      {serverConnectionOpen && (
        <ServerConnection
          localProjects={workspace.projects}
          onOpenProject={workspace.openServerProject}
          onRefreshProject={workspace.refreshServerWorkspace}
          onDeleteProject={workspace.deleteServerProject}
          onManageUsers={() => {
            setServerConnectionOpen(false);
            void checkServerStatus();
            setServerUsersOpen(true);
          }}
          onClose={() => {
            setServerConnectionOpen(false);
            void checkServerStatus();
          }}
        />
      )}
      {serverUsersOpen && <ServerUsers onClose={() => setServerUsersOpen(false)} />}
      {settingsOpen && (
        <SettingsDialog
          onClose={() => setSettingsOpen(false)}
          desktop={workspace.desktop}
          onServerConnection={() => {
            setSettingsOpen(false);
            setServerConnectionOpen(true);
          }}
          serverConnectionDisabled={busy}
          onServerUsers={
            serverAdmin && serverStatus === 'connected'
              ? () => {
                  setSettingsOpen(false);
                  setServerUsersOpen(true);
                }
              : undefined
          }
          onNewLocalProject={() => {
            setSettingsOpen(false);
            workspace.createProject();
          }}
          onOpenLocalProject={() => {
            setSettingsOpen(false);
            workspace.openProject();
          }}
          localProjectDisabled={busy}
          project={project ? { name: project.data.name } : null}
          onRename={() => {
            setSettingsOpen(false);
            workspace.renameProject();
          }}
          renameDisabled={busy || !!project?.readOnly}
          onHistory={() => {
            if (project) {
              setSettingsOpen(false);
              setHistoryHandle(project.handle);
            }
          }}
          historyDisabled={busy}
          onAuth={() => {
            setSettingsOpen(false);
            editAuthentication();
          }}
          authDisabled={busy || !!project?.readOnly || !activeEnvironmentView}
          onImportOpenApi={() => {
            setSettingsOpen(false);
            void chooseImport();
          }}
          importOpenApiLabel={choosingImport ? 'Reading file…' : 'Import OpenAPI…'}
          onImportPostman={() => {
            setSettingsOpen(false);
            setPostmanPicker(true);
          }}
          importDisabled={busy || !!project?.readOnly || variablesOpen || !!authDialog}
          onExport={() => {
            if (project) {
              setSettingsOpen(false);
              setExportHandle(project.handle);
              setExportPreview(null);
              setExportError(null);
              setExportNotice(null);
            }
          }}
          exportDisabled={busy || variablesOpen || !!authDialog}
        />
      )}
    </VariableContext.Provider>
  );
}
