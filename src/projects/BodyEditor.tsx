import { Select } from '../ui/Select';
import { KeyValueEditor } from '../components/KeyValueEditor';
import { VariableInput } from '../environments/VariableInput';
import type { Attachment, Draft } from './types';

export function BodyEditor({
  draft,
  onChange,
  onFile,
}: {
  draft: Draft;
  onChange: (patch: Partial<Draft>) => void;
  onFile: (accept: (file: Attachment) => void) => void;
}) {
  return (
    <div className="body-editor">
      <label className="body-format">
        Body type
        <Select
          aria-label="Body type"
          value={draft.bodyFormat}
          onValueChange={(selectedValue) =>
            onChange({ bodyFormat: selectedValue as Draft['bodyFormat'] })
          }
        >
          <option value="none">None</option>
          <option value="json">JSON</option>
          <option value="text">Text</option>
          <option value="urlEncoded">URL-encoded form</option>
          <option value="multipart">Multipart form</option>
          <option value="file">File</option>
        </Select>
      </label>
      {draft.bodyFormat === 'none' && <p className="panel-hint">This request has no body.</p>}
      {(draft.bodyFormat === 'json' || draft.bodyFormat === 'text') && (
        <VariableInput
          label="Request body"
          variant="body"
          placeholder={draft.bodyFormat === 'json' ? '{\n  "hello": "world"\n}' : 'Request content'}
          value={draft.body}
          onChange={(body) => onChange({ body })}
        />
      )}
      {(draft.bodyFormat === 'urlEncoded' || draft.bodyFormat === 'multipart') && (
        <>
          <KeyValueEditor
            label="Field"
            entries={draft.fields}
            onChange={(fields) =>
              onChange({
                fields: fields.map((field) => ({
                  ...field,
                  file: draft.fields.find((old) => old.id === field.id)?.file ?? null,
                })),
              })
            }
          />
          {draft.bodyFormat === 'multipart' &&
            draft.fields.map((field, index) => (
              <div className="attachment-row" key={field.id}>
                <span>
                  Field {index + 1}: {field.file?.name ?? 'Text value'}
                </span>
                <button
                  className="text-button"
                  onClick={() =>
                    onFile((file) =>
                      onChange({
                        fields: draft.fields.map((item) =>
                          item.id === field.id ? { ...item, file } : item,
                        ),
                      }),
                    )
                  }
                >
                  Attach file to field {index + 1}
                </button>
                {field.file && (
                  <button
                    className="text-button"
                    onClick={() =>
                      onChange({
                        fields: draft.fields.map((item) =>
                          item.id === field.id ? { ...item, file: null } : item,
                        ),
                      })
                    }
                  >
                    Use text for field {index + 1}
                  </button>
                )}
              </div>
            ))}
        </>
      )}
      {draft.bodyFormat === 'file' && (
        <div className="attachment-row">
          <span>{draft.file?.name ?? 'No file selected'}</span>
          <button className="text-button" onClick={() => onFile((file) => onChange({ file }))}>
            Choose body file
          </button>
          {draft.file && (
            <button className="text-button" onClick={() => onChange({ file: null })}>
              Remove body file
            </button>
          )}
        </div>
      )}
      {['multipart', 'file'].includes(draft.bodyFormat) && (
        <p className="panel-hint">Selected files are copied into your project for portability.</p>
      )}
    </div>
  );
}
