import { useId } from 'react';
import { Modal } from './Dialogs';
import { Icon } from '../components/Icon';

function SettingsRow({
  label,
  hint,
  disabled,
  onClick,
}: {
  label: string;
  hint: string;
  disabled: boolean;
  onClick: () => void;
}) {
  const hintId = useId();
  return (
    <button
      className="settings-row"
      disabled={disabled}
      aria-label={label}
      aria-describedby={hintId}
      onClick={onClick}
    >
      <span>{label}</span>
      <span className="settings-row-hint" id={hintId}>
        {hint}
      </span>
    </button>
  );
}

export function SettingsDialog({
  onClose,
  desktop,
  onServerConnection,
  serverConnectionDisabled,
  onServerUsers,
  onNewLocalProject,
  onOpenLocalProject,
  localProjectDisabled,
  project,
  onRename,
  renameDisabled,
  onHistory,
  historyDisabled,
  onAuth,
  authDisabled,
  onImportOpenApi,
  importOpenApiLabel,
  onImportPostman,
  importDisabled,
  onExport,
  exportDisabled,
}: {
  onClose: () => void;
  desktop: boolean;
  onServerConnection: () => void;
  serverConnectionDisabled: boolean;
  /** Present only for a signed-in server administrator. */
  onServerUsers?: () => void;
  onNewLocalProject: () => void;
  onOpenLocalProject: () => void;
  localProjectDisabled: boolean;
  project: { name: string } | null;
  onRename: () => void;
  renameDisabled: boolean;
  onHistory: () => void;
  historyDisabled: boolean;
  onAuth: () => void;
  authDisabled: boolean;
  onImportOpenApi: () => void;
  importOpenApiLabel: string;
  onImportPostman: () => void;
  importDisabled: boolean;
  onExport: () => void;
  exportDisabled: boolean;
}) {
  return (
    <Modal label="Settings" variant="blur" onDismiss={onClose}>
      <div className="settings-dialog">
        <div className="settings-dialog-head">
          <h2>Settings</h2>
          <button className="icon-button" aria-label="Close settings" onClick={onClose}>
            <Icon name="close" />
          </button>
        </div>
        <section className="settings-section">
          <h3>Global</h3>
          <SettingsRow
            label="Server connection"
            hint="Connect to a Fluxkit server and sign in"
            disabled={!desktop || serverConnectionDisabled}
            onClick={onServerConnection}
          />
          {onServerUsers && (
            <SettingsRow
              label="Server users"
              hint="See registered accounts; approve, disable or delete them"
              disabled={!desktop}
              onClick={onServerUsers}
            />
          )}
          <SettingsRow
            label="New local project"
            hint="Create a project stored only on this device"
            disabled={!desktop || localProjectDisabled}
            onClick={onNewLocalProject}
          />
          <SettingsRow
            label="Open local project"
            hint="Open a project folder already on this device"
            disabled={!desktop || localProjectDisabled}
            onClick={onOpenLocalProject}
          />
          {!desktop && (
            <p className="settings-empty">Open the desktop app to work with local projects.</p>
          )}
        </section>
        <section className="settings-section">
          <h3>Project</h3>
          {project ? (
            <>
              <SettingsRow
                label="Rename"
                hint={`Rename “${project.name}”`}
                disabled={renameDisabled}
                onClick={onRename}
              />
              <SettingsRow
                label="Request history"
                hint="Browse past requests and responses"
                disabled={historyDisabled}
                onClick={onHistory}
              />
              <SettingsRow
                label="Project authentication"
                hint="Default credentials requests inherit from"
                disabled={authDisabled}
                onClick={onAuth}
              />
              <SettingsRow
                label={importOpenApiLabel}
                hint="Add requests from an OpenAPI document"
                disabled={importDisabled}
                onClick={onImportOpenApi}
              />
              <SettingsRow
                label="Import Postman…"
                hint="Add requests from a Postman collection"
                disabled={importDisabled}
                onClick={onImportPostman}
              />
              <SettingsRow
                label="Export OpenAPI…"
                hint="Save this project as an OpenAPI document"
                disabled={exportDisabled}
                onClick={onExport}
              />
            </>
          ) : (
            <p className="settings-empty">Open a project to see project settings.</p>
          )}
        </section>
      </div>
    </Modal>
  );
}
