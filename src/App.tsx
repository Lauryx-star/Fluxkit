import { AuthEditor } from './auth/AuthEditor';
import { AuthDialog } from './auth/AuthDialog';
import { TokenControls } from './auth/TokenControls';
import { HistoryPanel } from './history/HistoryPanel';
import type { Auth, AuthTarget } from './auth/types';
import { authLabel, effectiveAuth, manualAuthHint } from './auth/rules';
import brandMark from '../assets/fluxkit.svg';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { Icon } from './components/Icon';
import { KeyValueEditor } from './components/KeyValueEditor';
import { environmentAction, getAppInfo, isDesktop } from './native';
import { useWorkspace } from './projects/useWorkspace';
import { isDirty, requestData } from './projects/types';
import { useHttp } from './http/useHttp';
import { ResponseView } from './http/ResponseView';
import { ProjectTree } from './projects/ProjectTree';
import { BodyEditor } from './projects/BodyEditor';
import { NameDialog, ConfirmDialog } from './projects/Dialogs';
import { EnvironmentControls } from './environments/EnvironmentControls';
import { VariableContext } from './environments/VariableContext';
import { VariableInput } from './environments/VariableInput';
import type { EnvironmentView } from './environments/types';

type EditorTab = 'Params' | 'Headers' | 'Body' | 'Auth';
const methods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];
const editorTabs: EditorTab[] = ['Params', 'Headers', 'Body', 'Auth'];
export default function App() {
  const [variableChanges, setVariableChanges] = useState(false);
  const [environmentState, setEnvironmentState] = useState<Record<string, EnvironmentView>>({});
  const onEnvironmentView = useCallback((handle: string, view: EnvironmentView) => {
    setEnvironmentState((items) => ({ ...items, [handle]: view }));
  }, []);
  const [variablesOpen, setVariablesOpen] = useState(false);
  const [checkingVariables, setCheckingVariables] = useState(false);
  const [variableCheck, setVariableCheck] = useState<{
    requestId: string;
    handle: string;
    state: 'checking' | 'success' | 'error';
    message: string;
  } | null>(null);
  const [authChanges, setAuthChanges] = useState(false);
  const [historyHandle, setHistoryHandle] = useState<string | null>(null);
  const [authDialog, setAuthDialog] = useState<{
    handle: string;
    target: AuthTarget;
    title: string;
    initial: Auth | null;
  } | null>(null);
  const workspace = useWorkspace(variableChanges || authChanges);
  const historyProject = workspace.projects.find((item) => item.handle === historyHandle);
  const { drafts, activeId, setActiveId, draft, project, busy, updateDraft, closeDraft } =
    workspace;
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
  const canSend =
    workspace.desktop &&
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
          !variablesOpen &&
          !authDialog &&
          !historyProject
        ) {
          workspace.saveDraft();
        }
      }
    }
    window.addEventListener('keydown', onSave);
    return () => window.removeEventListener('keydown', onSave);
  }, [workspace, variablesOpen, authDialog, historyProject]);

  return (
    <VariableContext.Provider value={variableContext}>
      <div className="app-shell">
        <aside className="sidebar" aria-label="Workspace navigation">
          <div className="brand">
            <img className="brand-mark" src={brandMark} alt="" />
            <span>
              fluxkit<span className="brand-dot">.</span>
            </span>
            <span className="local-badge">LOCAL</span>
          </div>
          <div className="workspace-label">YOUR WORKSPACE</div>
          <div className="project-controls">
            <button
              className="secondary-button"
              disabled={!workspace.desktop || busy}
              onClick={workspace.createProject}
            >
              New project
            </button>
            <button
              className="secondary-button"
              disabled={!workspace.desktop || busy}
              onClick={workspace.openProject}
            >
              Open project
            </button>
          </div>
          {!workspace.desktop && (
            <p className="sidebar-hint">Open the desktop app to work with local projects.</p>
          )}
          {project ? (
            <>
              <div className="project-picker">
                <select
                  aria-label="Active project"
                  value={workspace.activeHandle ?? ''}
                  disabled={busy}
                  onChange={(event) => workspace.setActiveHandle(event.target.value)}
                >
                  {workspace.projects.map((item) => (
                    <option key={item.handle} value={item.handle}>
                      {item.data.name}
                    </option>
                  ))}
                </select>
              </div>
              <div className="project-actions">
                <button disabled={busy} onClick={workspace.renameProject}>
                  Rename
                </button>
                <button disabled={busy} onClick={() => void workspace.reloadProject()}>
                  Reload
                </button>
                <button disabled={busy} onClick={() => void workspace.closeProject()}>
                  Close project
                </button>
              </div>
              <button
                className="secondary-button project-auth-button"
                disabled={busy || !activeEnvironmentView}
                onClick={() => editAuthentication()}
              >
                Project authentication
              </button>
              <button
                className="secondary-button project-history-button"
                disabled={busy}
                onClick={() => setHistoryHandle(project.handle)}
              >
                Request history
              </button>
              <div className="sidebar-section collections-heading">
                <span>COLLECTIONS</span>
                <button
                  className="icon-button"
                  aria-label="Create collection"
                  disabled={busy}
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
                onAuth={(id, folder) => editAuthentication(id, folder?.id ?? null)}
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
          <div className="sidebar-note">
            <span className="tiny-orbit" />
            <div>
              <strong>Local by design.</strong>
              <p>Your workspace. Your machine.</p>
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
              <span>{draft ? draft.name : 'Overview'}</span>
            </div>
            {project ? (
              <EnvironmentControls
                key={project.handle}
                handle={project.handle}
                ask={workspace.ask}
                onDirty={setVariableChanges}
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
                <button
                  className="icon-button"
                  aria-label="Dismiss error"
                  onClick={() => workspace.setError(null)}
                >
                  <Icon name="close" />
                </button>
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
              <span className="session-label">{project ? 'LOCAL PROJECT' : 'SESSION ONLY'}</span>
            </div>

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
                  </div>
                  <div className="save-controls">
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
                      disabled={!workspace.desktop || busy || !isDirty(draft)}
                      onClick={workspace.saveDraft}
                    >
                      Save
                    </button>
                  </div>
                </div>
                <div className="url-bar">
                  <select
                    aria-label="HTTP method"
                    value={draft.method}
                    className={`method-${draft.method.toLowerCase()}`}
                    onChange={(event) => updateDraft({ method: event.target.value })}
                  >
                    {methods.map((method) => (
                      <option key={method}>{method}</option>
                    ))}
                  </select>
                  <VariableInput
                    label="Request URL"
                    variant="url"
                    placeholder="https://api.example.com/v1/resource"
                    value={draft.url}
                    onChange={(url) => updateDraft({ url })}
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
                  Authentication: <strong>{authLabel(effective.auth)}</strong> · {effective.source}
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
                          target = editorTabs[(index + editorTabs.length - 1) % editorTabs.length];
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
                    <KeyValueEditor
                      label="Header"
                      entries={draft.headers}
                      onChange={(headers) => updateDraft({ headers })}
                    />
                  )}
                  {editorTab === 'Body' && (
                    <BodyEditor draft={draft} onChange={updateDraft} onFile={workspace.attach} />
                  )}
                  {editorTab === 'Auth' && (
                    <>
                      {(effective.auth?.kind === 'oauth2' ||
                        effective.auth?.kind === 'customLogin') && (
                        <TokenControls
                          key={JSON.stringify([draft.id, effective, executionScope])}
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
                        variables={authProject ? (executionEnvironment?.data.variables ?? []) : []}
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
            <ResponseView
              key={draft?.id ?? 'empty'}
              environmentLabel={execution?.environmentLabel}
              result={execution?.result ?? null}
              running={execution?.running ?? false}
            />
          </main>
          <footer className="statusbar">
            <span>
              <span className="status-dot" />
              {connection}
            </span>
            <span>
              Local workspace <span className="status-divider">/</span> No cloud required
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
        {authDialog && (
          <AuthDialog
            key={`${authDialog.handle}-${authDialog.target.collectionId}-${authDialog.target.folderId}`}
            title={authDialog.title}
            initial={authDialog.initial}
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
            onSave={(auth) => {
              void workspace.saveAuthentication(authDialog.target, auth).then((saved) => {
                if (saved) {
                  setAuthDialog(null);
                }
              });
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
    </VariableContext.Provider>
  );
}
