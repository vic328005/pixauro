import { useId } from 'react';
import { CommitInput } from './ui/commit-input';

type Commit = (value: string) => void | Promise<boolean>;
type FieldBase = { key: string; label: string; disabled?: boolean; help?: string };
/** Explicit editor metadata; document mutations remain typed controller commands. */
export type InspectorField = FieldBase &
  (
    | { kind: 'text'; value: string; commit: Commit; getError?: (value: string) => string | null }
    | {
        kind: 'number';
        value: number | null;
        min: number;
        max: number;
        unit?: string;
        commit: Commit;
      }
    | { kind: 'toggle'; value: boolean; commit: (value: boolean) => void | Promise<boolean> }
    | {
        kind: 'select';
        value: string;
        options: readonly { value: string; label: string }[];
        commit: Commit;
      }
    | { kind: 'readonly'; value: string }
  );

export function InspectorFields({
  fields,
  disabled,
}: {
  fields: InspectorField[];
  disabled: boolean;
}) {
  const prefix = useId();
  return (
    <div className="inspector-fields">
      {fields.map((field) => {
        const blocked = disabled || field.disabled;
        const id = `${prefix}-${field.key}`;
        return (
          <div className="inspector-field" key={field.key}>
            <div className="property-row">
              {field.kind === 'readonly' ? (
                <span className="property-label">{field.label}</span>
              ) : (
                <label className="property-label" htmlFor={id}>
                  {field.label}
                </label>
              )}
              {field.kind === 'readonly' ? (
                <span className="property-value">{field.value}</span>
              ) : field.kind === 'toggle' ? (
                <input
                  id={id}
                  aria-label={field.label}
                  type="checkbox"
                  checked={field.value}
                  disabled={blocked}
                  onChange={(event) => void field.commit(event.currentTarget.checked)}
                />
              ) : field.kind === 'select' ? (
                <select
                  className="timeline-input"
                  id={id}
                  aria-label={field.label}
                  value={field.value}
                  disabled={blocked}
                  onChange={(event) => void field.commit(event.currentTarget.value)}
                >
                  {field.options.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              ) : (
                <>
                  <CommitInput
                    id={id}
                    label={field.label}
                    value={field.value === null ? '' : String(field.value)}
                    placeholder={field.value === null ? 'Mixed' : undefined}
                    min={field.kind === 'number' ? field.min : undefined}
                    max={field.kind === 'number' ? field.max : undefined}
                    getError={field.kind === 'text' ? field.getError : undefined}
                    disabled={blocked}
                    onCommit={field.commit}
                  />
                  {field.kind === 'number' && field.unit && (
                    <span className="muted">{field.unit}</span>
                  )}
                </>
              )}
            </div>
            {field.help && <p className="muted inspector-field-help">{field.help}</p>}
          </div>
        );
      })}
    </div>
  );
}
