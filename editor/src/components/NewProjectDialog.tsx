import { useRef, useState, type FormEvent } from 'react';
import { useEditor } from '../editorController';
import { nameError } from '../inputValidation';
import { useUi } from '../editorUiState';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Dialog, DialogContent, DialogFooter, DialogTitle } from './ui/dialog';

// Keep these limits aligned with Project::validate_name and Canvas::MAX_SIDE.
const maxSide = 4096;
const sizePresets = [32, 64] as const;
function validDimension(value: string) {
  const number = Number(value);
  return Number.isInteger(number) && number >= 1 && number <= maxSide;
}

export function NewProjectDialog() {
  const editor = useEditor();
  const error = useUi((state) => state.error);
  const [name, setName] = useState('Untitled');
  const [width, setWidth] = useState('32');
  const [height, setHeight] = useState('32');
  const [pending, setPending] = useState(false);
  const submitting = useRef(false);
  const focusBeforeOpen = useRef<HTMLElement | null>(null);
  const trimmedName = name.trim();
  const nameValidationError = nameError(name);
  const validName = !nameValidationError;
  const dimensionsValid = validDimension(width) && validDimension(height);
  const blocked = editor.busy || pending;
  const canCreate = validName && dimensionsValid && !blocked;

  function cancel() {
    if (!blocked) useUi.getState().set({ newProjectOpen: false });
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canCreate || submitting.current) return;
    submitting.current = true;
    setPending(true);
    try {
      // accept() closes this dialog only when a new document session is created.
      // Canceling the native save prompt leaves the entered values available.
      await editor.createProject({
        name: trimmedName,
        width: Number(width),
        height: Number(height),
      });
    } finally {
      submitting.current = false;
      setPending(false);
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && cancel()}>
      <DialogContent
        className="new-project-dialog"
        showCloseButton={false}
        aria-busy={blocked}
        aria-describedby={undefined}
        onOpenAutoFocus={() => {
          if (globalThis.document.activeElement instanceof HTMLElement) {
            focusBeforeOpen.current = globalThis.document.activeElement;
          }
        }}
        onCloseAutoFocus={(event) => {
          if (focusBeforeOpen.current?.isConnected) {
            event.preventDefault();
            focusBeforeOpen.current.focus();
          }
        }}
        onInteractOutside={(event) => event.preventDefault()}
        onEscapeKeyDown={(event) => {
          if (blocked) event.preventDefault();
        }}
      >
        <DialogTitle className="sr-only">New project</DialogTitle>
        <form className="new-project-form" onSubmit={(event) => void submit(event)}>
          <label className="new-project-field">
            <span>Project name</span>
            <Input
              value={name}
              onChange={(event) => setName(event.target.value)}
              onFocus={(event) => event.target.select()}
              placeholder="Untitled"
              required
              disabled={blocked}
              aria-invalid={!validName}
              aria-describedby={!validName ? 'new-project-name-help' : undefined}
            />
            {nameValidationError && (
              <span id="new-project-name-help" className="field-error" role="alert">
                {nameValidationError}
              </span>
            )}
          </label>
          <div className="new-project-field">
            <span id="new-project-presets-label">Canvas size</span>
            <div
              className="new-project-presets"
              role="group"
              aria-labelledby="new-project-presets-label"
            >
              {sizePresets.map((size) => {
                const selected = Number(width) === size && Number(height) === size;
                return (
                  <Button
                    key={size}
                    type="button"
                    variant={selected ? 'default' : 'secondary'}
                    aria-pressed={selected}
                    disabled={blocked}
                    onClick={() => {
                      setWidth(String(size));
                      setHeight(String(size));
                    }}
                  >
                    {size} × {size}
                  </Button>
                );
              })}
            </div>
          </div>
          <div className="new-project-dimensions">
            {(
              [
                ['Width', width, setWidth],
                ['Height', height, setHeight],
              ] as const
            ).map(([label, value, onChange]) => (
              <label className="new-project-field" key={label}>
                <span>{label} (px)</span>
                <Input
                  type="number"
                  min={1}
                  max={maxSide}
                  step={1}
                  required
                  value={value}
                  onChange={(event) => onChange(event.target.value)}
                  disabled={blocked}
                  aria-invalid={!validDimension(value)}
                  aria-describedby={
                    !validDimension(value) ? `new-project-${label}-error` : undefined
                  }
                />
                {!validDimension(value) && (
                  <span id={`new-project-${label}-error`} className="field-error" role="alert">
                    Enter a whole number from 1 to {maxSide}.
                  </span>
                )}
              </label>
            ))}
          </div>
          {error && (
            <p className="field-error" role="alert">
              {error}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="secondary" disabled={blocked} onClick={cancel}>
              Cancel
            </Button>
            <Button type="submit" disabled={!canCreate}>
              {blocked ? 'Creating…' : 'Create'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
