import { Select } from '../ui/Select';
import { useState } from 'react';
import { Modal } from '../projects/Dialogs';
import type { CollectionView } from '../projects/types';

export function PostmanImportPicker({
  collections,
  onChoose,
  onCancel,
}: {
  collections: CollectionView[];
  onChoose: (collectionId: string | null) => void;
  onCancel: () => void;
}) {
  const targets = collections.filter((c) => c.data.postman);
  const [target, setTarget] = useState(targets[0]?.data.id ?? '');
  return (
    <Modal label="Import Postman">
      <div
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            onCancel();
          }
        }}
      >
        <h2>Import from Postman</h2>
        <p>
          Choose a Collection v2.1 JSON export, or add an environment to a collection imported
          earlier.
        </p>
        <button className="primary-button" onClick={() => onChoose(null)}>
          Choose collection JSON…
        </button>
        <label className="dialog-label">
          Collection for environment mapping
          <Select
            aria-label="Collection for environment mapping"
            value={target}
            disabled={!targets.length}
            onValueChange={(selectedValue) => setTarget(selectedValue)}
          >
            {!targets.length && <option value="">Import a Postman collection first</option>}
            {targets.map((c) => (
              <option key={c.data.id} value={c.data.id}>
                {c.data.name}
              </option>
            ))}
          </Select>
        </label>
        <p>
          The environment applies to the whole project. The chosen collection supplies the
          source-name to variable mapping; its requests are not modified.
        </p>
        <div className="dialog-actions">
          <button className="secondary-button" onClick={onCancel}>
            Cancel
          </button>
          <button className="primary-button" disabled={!target} onClick={() => onChoose(target)}>
            Choose environment JSON…
          </button>
        </div>
      </div>
    </Modal>
  );
}
