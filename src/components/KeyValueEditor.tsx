import { Icon } from './Icon';
import { VariableInput } from '../environments/VariableInput';
import { historyKey } from '../environments/textHistory';

export interface Entry {
  id: number;
  enabled: boolean;
  key: string;
  value: string;
}

export function KeyValueEditor({
  label,
  entries,
  onChange,
  historyScope,
}: {
  label: string;
  entries: Entry[];
  onChange: (entries: Entry[]) => void;
  /** Enables undo/redo per key and value field, e.g. the scope of the open tab. */
  historyScope?: string;
}) {
  const field = (id: number, part: 'key' | 'value') =>
    historyScope ? historyKey(historyScope, `${label.toLowerCase()}:${id}:${part}`) : undefined;
  function update(id: number, patch: Partial<Entry>) {
    onChange(entries.map((entry) => (entry.id === id ? { ...entry, ...patch } : entry)));
  }

  return (
    <div className="pair-editor">
      <div className="pair-head">
        <span />
        <span>KEY</span>
        <span>VALUE</span>
        <span />
      </div>
      {entries.map((entry, index) => (
        <div className={`pair-row ${entry.enabled ? '' : 'muted-row'}`} key={entry.id}>
          <input
            type="checkbox"
            checked={entry.enabled}
            aria-label={`Enable ${label} ${index + 1}`}
            onChange={(event) => update(entry.id, { enabled: event.target.checked })}
          />
          <VariableInput
            label={`${label} key ${index + 1}`}
            placeholder="Key"
            value={entry.key}
            onChange={(key) => update(entry.id, { key })}
            historyKey={field(entry.id, 'key')}
          />
          <VariableInput
            label={`${label} value ${index + 1}`}
            placeholder="Value"
            value={entry.value}
            onChange={(value) => update(entry.id, { value })}
            historyKey={field(entry.id, 'value')}
          />
          <button
            className="icon-button"
            aria-label={`Remove ${label} ${index + 1}`}
            onClick={() => onChange(entries.filter((item) => item.id !== entry.id))}
          >
            <Icon name="close" size={12} />
          </button>
        </div>
      ))}
      <button
        className="text-button add-row"
        onClick={() =>
          onChange([
            ...entries,
            {
              id: Math.max(0, ...entries.map((entry) => entry.id)) + 1,
              enabled: true,
              key: '',
              value: '',
            },
          ])
        }
      >
        <Icon name="plus" size={13} /> Add {label.toLowerCase()}
      </button>
    </div>
  );
}
