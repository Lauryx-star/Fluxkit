import { useEffect, useState } from 'react';
import {
  addServerProjectMember,
  approveServerUser,
  createServerTeamProject,
  getServerAccount,
  getServerConnection,
  listServerProjectMembers,
  listServerAdminUsers,
  listServerProjects,
  loginToServer,
  logoutFromServer,
  registerServerAccount,
  removeServerProjectMember,
  setServerConnection,
  syncServerProject,
  setServerUserStatus,
  updateServerProjectMember,
  uploadLocalProjectToServer,
  type ServerConnection as Connection,
  type ServerAccount,
  type ServerAccountAdminView,
  type ServerProject,
  type ServerProjectMember,
} from '../native';
import { Modal } from '../projects/Dialogs';
import type { ProjectView } from '../projects/types';

export function ServerConnection({
  onClose,
  localProjects,
  onOpenProject,
}: {
  onClose: () => void;
  localProjects: ProjectView[];
  onOpenProject: (projectId: string) => Promise<void>;
}) {
  const [connection, setConnection] = useState<Connection | null>(null);
  const [projects, setProjects] = useState<ServerProject[]>([]);
  const [url, setUrl] = useState('');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [memberProject, setMemberProject] = useState<string | null>(null);
  const [members, setMembers] = useState<ServerProjectMember[]>([]);
  const [memberUsername, setMemberUsername] = useState('');
  const [memberRole, setMemberRole] = useState<ServerProjectMember['role']>('member');
  const [account, setAccount] = useState<ServerAccount | null>(null);
  const [adminUsers, setAdminUsers] = useState<ServerAccountAdminView[]>([]);
  const [teamProjectName, setTeamProjectName] = useState('');

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const value = await getServerConnection();
        if (!active) return;
        setConnection(value);
        setUrl(value.serverUrl);
        if (value.authenticated) {
          const available = await listServerProjects();
          if (active) setProjects(available);
          const currentAccount = await getServerAccount();
          if (active) {
            setAccount(currentAccount);
            setAdminUsers(
              currentAccount.isServerAdmin ? await listServerAdminUsers() : [],
            );
          }
        }
      } catch (reason) {
        if (active) {
          setError(typeof reason === 'string' ? reason : 'Could not load server settings.');
        }
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  async function perform(operation: () => Promise<Connection>) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const result = await operation();
      setConnection(result);
      setUrl(result.serverUrl);
      setPassword('');
      if (result.authenticated) {
        setProjects(await listServerProjects());
        const currentAccount = await getServerAccount();
        setAccount(currentAccount);
        setAdminUsers(
          currentAccount.isServerAdmin ? await listServerAdminUsers() : [],
        );
      } else {
        setProjects([]);
        setAccount(null);
        setAdminUsers([]);
      }
    } catch (reason) {
      setError(typeof reason === 'string' ? reason : 'The server operation failed.');
    } finally {
      setBusy(false);
    }
  }

  async function refreshMembers(projectId: string) {
    setBusy(true);
    setError(null);
    setMemberProject(projectId);
    setMembers([]);
    try {
      const result = await listServerProjectMembers(projectId);
      setMembers(result);
    } catch (reason) {
      setError(typeof reason === 'string' ? reason : 'Could not load project members.');
    } finally {
      setBusy(false);
    }
  }

  async function changeMembership(operation: () => Promise<void>) {
    if (!memberProject) return;
    setBusy(true);
    setError(null);
    try {
      await operation();
      setMembers(await listServerProjectMembers(memberProject));
      setMemberUsername('');
    } catch (reason) {
      setError(typeof reason === 'string' ? reason : 'Could not update project membership.');
    } finally {
      setBusy(false);
    }
  }

  async function changeAccount(operation: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await operation();
      setAdminUsers(await listServerAdminUsers());
    } catch (reason) {
      setError(typeof reason === 'string' ? reason : 'Could not update the account.');
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal label="Server connection">
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void perform(async () => {
            const saved = await setServerConnection(url);
            setConnection(saved);
            if (!saved.serverUrl) {
              return saved;
            }
            return loginToServer(username, password);
          });
        }}
      >
        <h2>Self-hosted server</h2>
        <p>
          Configure a Fluxkit server account. Passwords are sent only to the server and are not
          saved by Fluxkit.
        </p>
        <p>
          Projects can be edited when your account has write access and the server is online.
          Offline snapshots are read-only; use Sync saved changes to publish local workspace edits.
        </p>
        <label className="dialog-label">
          Server URL
          <input
            aria-label="Server URL"
            type="url"
            required
            value={url}
            disabled={busy}
            placeholder="https://fluxkit.example.org"
            onChange={(event) => setUrl(event.target.value)}
          />
        </label>
        <label className="dialog-label">
          Username
          <input
            aria-label="Server username"
            autoComplete="username"
            value={username}
            disabled={busy}
            onChange={(event) => setUsername(event.target.value)}
          />
        </label>
        <label className="dialog-label">
          Password
          <input
            aria-label="Server password"
            type="password"
            autoComplete="current-password"
            value={password}
            disabled={busy}
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        {error && <p role="alert">{error}</p>}
        {notice && <p role="status">{notice}</p>}
        {connection?.authenticated && (
          <>
            <section aria-label="Server projects">
              <h3>Projects available to this account</h3>
              {projects.length ? (
                <ul>
                  {projects.map((project) => (
                    <li key={project.id}>
                      {project.name} <span>({project.kind})</span>{' '}
                      <button
                        type="button"
                        className="secondary-button"
                        disabled={busy}
                        onClick={() => {
                          setBusy(true);
                          setError(null);
                          void onOpenProject(project.id)
                            .then(onClose)
                            .catch((reason: unknown) =>
                              setError(
                                typeof reason === 'string'
                                  ? reason
                                  : 'Could not open the server snapshot.',
                              ),
                            )
                            .finally(() => setBusy(false));
                        }}
                      >
                        {project.canEdit ? 'Open workspace' : 'Open read-only'}
                      </button>
                      {project.canManageMembers && (
                        <button
                          type="button"
                          className="secondary-button"
                          disabled={busy}
                          onClick={() => void refreshMembers(project.id)}
                        >
                          Manage members
                        </button>
                      )}
                    </li>
                  ))}
                </ul>
              ) : (
                <p>No projects are available to this account.</p>
              )}
            </section>
            <section aria-label="Sync server workspaces">
              <h3>Server workspaces on this device</h3>
              {localProjects.filter((project) => project.serverSync).length ? (
                <ul>
                  {localProjects
                    .filter((project) => project.serverSync)
                    .map((project) => (
                      <li key={project.handle}>
                        {project.data.name}{' '}
                        <button
                          type="button"
                          className="secondary-button"
                          disabled={busy}
                          onClick={() => {
                            setBusy(true);
                            setError(null);
                            setNotice(null);
                            void syncServerProject(project.handle)
                              .then(async (saved) => {
                                setProjects(await listServerProjects());
                                setNotice(
                                  `Saved “${saved.name}” to the server. The local workspace remains available offline.`,
                                );
                              })
                              .catch((reason: unknown) =>
                                setError(
                                  typeof reason === 'string'
                                    ? reason
                                    : 'Could not sync this server workspace.',
                                ),
                              )
                              .finally(() => setBusy(false));
                          }}
                        >
                          Sync saved changes
                        </button>
                      </li>
                    ))}
                </ul>
              ) : (
                <p>Open an editable server project to create a workspace here.</p>
              )}
            </section>
            {memberProject && (
              <section aria-label="Manage team project members">
                <h3>Team project members</h3>
                <div>
                  <label className="dialog-label">
                    Add approved account by username
                    <input
                      value={memberUsername}
                      disabled={busy}
                      onChange={(event) => setMemberUsername(event.target.value)}
                    />
                  </label>
                  <label className="dialog-label">
                    Role
                    <select
                      value={memberRole}
                      disabled={busy}
                      onChange={(event) =>
                        setMemberRole(event.target.value as ServerProjectMember['role'])
                      }
                    >
                      <option value="member">Member</option>
                      <option value="projectAdmin">Project administrator</option>
                    </select>
                  </label>
                  <button
                    type="button"
                    className="secondary-button"
                    disabled={busy || !memberUsername.trim()}
                    onClick={() =>
                      void changeMembership(() =>
                        addServerProjectMember(memberProject, memberUsername, memberRole),
                      ),
                    }
                  >
                    Add member
                  </button>
                </div>
                <ul>
                  {members.map((member) => (
                    <li key={member.id}>
                      {member.username} ({member.role}){' '}
                      <select
                        aria-label={`Role for ${member.username}`}
                        value={member.role}
                        disabled={busy}
                        onChange={(event) =>
                          void changeMembership(() =>
                            updateServerProjectMember(
                              memberProject,
                              member.id,
                              event.target.value as ServerProjectMember['role'],
                            ),
                          )
                        }
                      >
                        <option value="member">Member</option>
                        <option value="projectAdmin">Project administrator</option>
                      </select>{' '}
                      <button
                        type="button"
                        className="secondary-button"
                        disabled={busy}
                        onClick={() =>
                          void changeMembership(() =>
                            removeServerProjectMember(memberProject, member.id),
                          )
                        }
                      >
                        Remove
                      </button>
                    </li>
                  ))}
                </ul>
              </section>
            )}
            {account?.isServerAdmin && (
              <section aria-label="Server account administration">
                <h3>Server administration</h3>
                <label className="dialog-label">
                  Create team project
                  <input
                    value={teamProjectName}
                    disabled={busy}
                    onChange={(event) => setTeamProjectName(event.target.value)}
                  />
                </label>
                <button
                  type="button"
                  className="secondary-button"
                  disabled={busy || !teamProjectName.trim()}
                  onClick={() => {
                    setBusy(true);
                    setError(null);
                    void createServerTeamProject(teamProjectName)
                      .then(async (created) => {
                        setProjects(await listServerProjects());
                        setTeamProjectName('');
                        setNotice(`Created team project “${created.name}”.`);
                      })
                      .catch((reason: unknown) =>
                        setError(
                          typeof reason === 'string'
                            ? reason
                            : 'Could not create the team project.',
                        ),
                      )
                      .finally(() => setBusy(false));
                  }}
                >
                  Create project
                </button>
                <h3>Server accounts</h3>
                <ul>
                  {adminUsers.map((user) => (
                    <li key={user.id}>
                      {user.username} ({user.status}){user.isServerAdmin ? ' · admin' : ''}{' '}
                      {user.status === 'pending' ? (
                        <button
                          type="button"
                          className="secondary-button"
                          disabled={busy}
                          onClick={() =>
                            void changeAccount(() => approveServerUser(user.id))
                          }
                        >
                          Approve account
                        </button>
                      ) : (
                        <button
                          type="button"
                          className="secondary-button"
                          disabled={
                            busy || (user.id === account.id && user.status === 'active')
                          }
                          onClick={() =>
                            void changeAccount(() =>
                              setServerUserStatus(
                                user.id,
                                user.status === 'active' ? 'disabled' : 'active',
                              ),
                            )
                          }
                        >
                          {user.status === 'active' ? 'Disable' : 'Reactivate'}
                        </button>
                      )}
                    </li>
                  ))}
                </ul>
              </section>
            )}
            <section aria-label="Upload local project">
              <h3>Upload a local project copy</h3>
              <p>
                This creates a personal server copy and keeps your local files. Reusable values
                from the OS credential store and request history are not uploaded. Projects that
                reference local file attachments cannot be uploaded yet. After opening the server
                project, changes stay in its local workspace until you sync them here.
              </p>
              <ul>
                {localProjects.filter((project) => !project.readOnly).map((project) => (
                  <li key={project.handle}>
                    {project.data.name}{' '}
                    <button
                      type="button"
                      className="secondary-button"
                      disabled={busy}
                      onClick={() => {
                        setBusy(true);
                        setError(null);
                        setNotice(null);
                        void uploadLocalProjectToServer(project.handle)
                          .then(async (uploaded) => {
                            setProjects(await listServerProjects());
                            setNotice(
                              `Uploaded “${uploaded.name}” to the server. Your local project remains unchanged.`,
                            );
                          })
                          .catch((reason: unknown) =>
                            setError(
                              typeof reason === 'string' ? reason : 'Project upload failed.',
                            ),
                          )
                          .finally(() => setBusy(false));
                      }}
                    >
                      Upload copy
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          </>
        )}
        <div className="dialog-actions">
          {connection?.authenticated ? (
            <button
              type="button"
              className="secondary-button"
              disabled={busy}
              onClick={() => {
                setBusy(true);
                void logoutFromServer()
                  .then((value) => {
                    setConnection(value);
                    setProjects([]);
                    setAccount(null);
                    setAdminUsers([]);
                    setMemberProject(null);
                    setMembers([]);
                    setNotice('Signed out from the server.');
                  })
                  .catch((reason: unknown) =>
                    setError(typeof reason === 'string' ? reason : 'Could not sign out.'),
                  )
                  .finally(() => setBusy(false));
              }}
            >
              Sign out
            </button>
          ) : (
            <>
              <button
                type="button"
                className="secondary-button"
                disabled={busy || !url.trim() || !username.trim() || !password}
                onClick={() => {
                  setBusy(true);
                  setError(null);
                  setNotice(null);
                  void setServerConnection(url)
                    .then(() => registerServerAccount(username, password))
                    .then(() => {
                      setPassword('');
                      setNotice(
                        'Registration submitted. A server administrator must approve this account before sign-in.',
                      );
                    })
                    .catch((reason: unknown) =>
                      setError(typeof reason === 'string' ? reason : 'Registration failed.'),
                    )
                    .finally(() => setBusy(false));
                }}
              >
                Request account
              </button>
              <button
                type="submit"
                className="primary-button"
                disabled={busy || !url.trim() || !username.trim() || !password}
              >
                {busy ? 'Connecting…' : 'Connect'}
              </button>
            </>
          )}
          <button
            type="button"
            className="secondary-button"
            disabled={busy}
            onClick={onClose}
          >
            Close
          </button>
        </div>
      </form>
    </Modal>
  );
}
