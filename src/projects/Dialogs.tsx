import { useEffect, useRef, useState, type ReactNode } from 'react';

export function Modal({ children, label }: { children: ReactNode; label: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    ref.current?.querySelector<HTMLElement>('input:not(:disabled), button:not(:disabled)')?.focus();
    return () => {
      if (previous instanceof HTMLElement && document.contains(previous)) {
        previous.focus();
      }
    };
  }, []);
  return (
    <div className="modal-backdrop">
      <div
        ref={ref}
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label={label}
        onKeyDown={(event) => {
          if (event.key !== 'Tab') {
            return;
          }
          const controls = Array.from(
            event.currentTarget.querySelectorAll<HTMLElement>(
              'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled)',
            ),
          );
          const first = controls[0],
            last = controls.at(-1);
          if (event.shiftKey && document.activeElement === first) {
            event.preventDefault();
            last?.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first?.focus();
          }
        }}
      >
        {children}
      </div>
    </div>
  );
}

export function NameDialog({
  title,
  initial,
  onSubmit,
  onCancel,
}: {
  title: string;
  initial: string;
  onSubmit: (name: string) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(initial);
  return (
    <Modal label={title}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (name.trim()) {
            onSubmit(name.trim());
          }
        }}
      >
        <h2>{title}</h2>
        <label className="dialog-label">
          Name
          <input
            aria-label="Name"
            value={name}
            maxLength={200}
            required
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                onCancel();
              }
            }}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        {title === 'Create project' && <p>Next, choose an empty folder for the project files.</p>}
        <div className="dialog-actions">
          <button type="button" className="secondary-button" onClick={onCancel}>
            Cancel
          </button>
          <button className="primary-button" disabled={!name.trim()} type="submit">
            {title.startsWith('Rename') ? 'Rename' : 'Create'}
          </button>
        </div>
      </form>
    </Modal>
  );
}
export function ConfirmDialog({
  message,
  onAnswer,
}: {
  message: string;
  onAnswer: (yes: boolean) => void;
}) {
  return (
    <Modal label="Confirm action">
      <h2>Confirm action</h2>
      <p>{message}</p>
      <div className="dialog-actions">
        <button className="secondary-button" onClick={() => onAnswer(false)}>
          Cancel
        </button>
        <button className="primary-button" onClick={() => onAnswer(true)}>
          Continue
        </button>
      </div>
    </Modal>
  );
}
