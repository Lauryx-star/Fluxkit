import { useEffect, useMemo, useState } from 'react';
import {
  approveServerUser,
  deleteServerUser,
  getServerAccount,
  listServerAdminUsers,
  setServerUserStatus,
  type ServerAccount,
  type ServerAccountAdminView,
} from '../native';
import { Modal } from '../projects/Dialogs';
import { Icon } from '../components/Icon';

type Filter = 'all' | ServerAccountAdminView['status'];

const STATUS_LABEL: Record<ServerAccountAdminView['status'], string> = {
  active: 'Active',
  pending: 'Awaiting approval',
  disabled: 'Disabled',
};

/** Server administrators: every registered account, with approve, disable and delete. */
export function ServerUsers({ onClose }: { onClose: () => void }) {
  const [me, setMe] = useState<ServerAccount | null>(null);
  const [users, setUsers] = useState<ServerAccountAdminView[] | null>(null);
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<Filter>('all');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<ServerAccountAdminView | null>(null);
  const [confirmation, setConfirmation] = useState('');

  useEffect(() => {
    let active = true;
    void Promise.all([getServerAccount(), listServerAdminUsers()])
      .then(([account, list]) => {
        if (active) {
          setMe(account);
          setUsers(list);
        }
      })
      .catch((reason: unknown) => {
        if (active) {
          setError(typeof reason === 'string' ? reason : 'Could not load the server accounts.');
          setUsers([]);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return (users ?? []).filter(
      (user) =>
        (filter === 'all' || user.status === filter) &&
        (!needle || user.username.toLowerCase().includes(needle)),
    );
  }, [users, query, filter]);
  const counts = useMemo(() => {
    const result = { all: 0, active: 0, pending: 0, disabled: 0 };
    for (const user of users ?? []) {
      result.all += 1;
      result[user.status] += 1;
    }
    return result;
  }, [users]);

  async function change(operation: () => Promise<void>, done: string) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await operation();
      setUsers(await listServerAdminUsers());
      setNotice(done);
      return true;
    } catch (reason) {
      setError(typeof reason === 'string' ? reason : 'Could not update the account.');
      return false;
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal label="Server users" variant="blur" onDismiss={busy ? undefined : onClose}>
      <div className="server-users">
        <div className="settings-dialog-head">
          <h2>Server users</h2>
          <button
            className="icon-button"
            aria-label="Close server users"
            disabled={busy}
            onClick={onClose}
          >
            <Icon name="close" />
          </button>
        </div>
        <div className="server-users-filters">
          <input
            type="search"
            aria-label="Search users"
            placeholder="Search by username"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          <div className="server-users-tabs" role="group" aria-label="Filter by status">
            {(['all', 'active', 'pending', 'disabled'] as const).map((item) => (
              <button
                key={item}
                type="button"
                className={`server-users-tab${filter === item ? ' active' : ''}`}
                aria-pressed={filter === item}
                onClick={() => setFilter(item)}
              >
                {item === 'all' ? 'All' : STATUS_LABEL[item]}{' '}
                <span className="server-users-count">{counts[item]}</span>
              </button>
            ))}
          </div>
        </div>
        {error && <p role="alert">{error}</p>}
        {notice && <p role="status">{notice}</p>}
        {users === null ? (
          <p className="settings-empty">Loading accounts…</p>
        ) : visible.length === 0 ? (
          <p className="settings-empty">No accounts match.</p>
        ) : (
          <ul className="server-users-list" aria-label="Registered accounts">
            {visible.map((user) => {
              const self = user.id === me?.id;
              return (
                <li key={user.id} className="server-user">
                  <div className="server-user-main">
                    <span className="server-user-name">
                      {user.username}
                      {self && <span className="server-user-badge">you</span>}
                      {user.isServerAdmin && <span className="server-user-badge">admin</span>}
                    </span>
                    <span className="server-user-meta">
                      <span className={`server-user-status ${user.status}`}>
                        {STATUS_LABEL[user.status]}
                      </span>
                      {registered(user.createdAt)}
                    </span>
                  </div>
                  <div className="server-user-actions">
                    {user.status === 'pending' ? (
                      <button
                        type="button"
                        className="secondary-button"
                        disabled={busy}
                        onClick={() =>
                          void change(
                            () => approveServerUser(user.id),
                            `Approved “${user.username}”.`,
                          )
                        }
                      >
                        Approve
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="secondary-button"
                        disabled={busy || self}
                        title={self ? 'You cannot disable your own account.' : undefined}
                        aria-label={`${user.status === 'active' ? 'Disable' : 'Reactivate'} ${user.username}`}
                        onClick={() =>
                          void change(
                            () =>
                              setServerUserStatus(
                                user.id,
                                user.status === 'active' ? 'disabled' : 'active',
                              ),
                            user.status === 'active'
                              ? `Disabled “${user.username}”. The account can no longer sign in.`
                              : `Reactivated “${user.username}”.`,
                          )
                        }
                      >
                        {user.status === 'active' ? 'Disable' : 'Reactivate'}
                      </button>
                    )}
                    <button
                      type="button"
                      className="secondary-button danger-button"
                      disabled={busy || self}
                      title={self ? 'You cannot delete your own account.' : undefined}
                      aria-label={`Delete ${user.username}`}
                      onClick={() => {
                        setError(null);
                        setNotice(null);
                        setConfirmation('');
                        setDeleting(user);
                      }}
                    >
                      Delete
                    </button>
                  </div>
                  {deleting?.id === user.id && (
                    <section className="server-user-delete" aria-label="Confirm account deletion">
                      <p>
                        Deleting <strong>{user.username}</strong> permanently removes the account,
                        its personal project
                        {typeof user.personalCollections === 'number'
                          ? ` (${plural(user.personalCollections, 'collection')})`
                          : ''}
                        , its secret vault and its encrypted values, and its membership in{' '}
                        {typeof user.teamProjects === 'number'
                          ? plural(user.teamProjects, 'team project')
                          : 'team projects'}
                        . Team projects and their requests stay. This cannot be undone; disabling
                        keeps everything instead.
                      </p>
                      <label className="dialog-label">
                        Type the username to confirm
                        <input
                          aria-label="Username to confirm deletion"
                          value={confirmation}
                          disabled={busy}
                          autoComplete="off"
                          onChange={(event) => setConfirmation(event.target.value)}
                        />
                      </label>
                      <div className="server-user-delete-actions">
                        <button
                          type="button"
                          className="secondary-button"
                          disabled={busy}
                          onClick={() => setDeleting(null)}
                        >
                          Keep account
                        </button>
                        <button
                          type="button"
                          className="secondary-button danger-button"
                          disabled={busy || confirmation !== user.username}
                          onClick={() =>
                            void change(
                              () => deleteServerUser(user.id),
                              `Deleted “${user.username}” and its private data.`,
                            ).then((deleted) => {
                              if (deleted) {
                                setDeleting(null);
                              }
                            })
                          }
                        >
                          Delete account
                        </button>
                      </div>
                    </section>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </Modal>
  );
}

function plural(count: number, noun: string) {
  return `${count} ${noun}${count === 1 ? '' : 's'}`;
}

function registered(createdAt: string | null | undefined) {
  if (!createdAt) {
    return null;
  }
  const date = new Date(createdAt);
  return Number.isNaN(date.getTime()) ? null : <span>registered {date.toLocaleDateString()}</span>;
}
