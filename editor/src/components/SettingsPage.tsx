import { useEffect, useRef } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { ArrowLeft } from 'lucide-react';
import { useEditor } from '../editorController';
import { useUi } from '../editorUiState';
import { AppHeader } from './AppHeader';
import { Button } from './ui/button';
import './settings.css';

export function SettingsPage() {
  const editor = useEditor();
  const ui = useUi();
  const navigate = useNavigate();
  const back = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    useUi.getState().set({ playing: false });
    back.current?.focus();
  }, []);
  const doc = editor.document;
  if (!doc) return null;
  const disabled = editor.busy || !!ui.draft || !!ui.deletion || ui.newProjectOpen;
  return (
    <div className="settings-page">
      <AppHeader>
        <nav aria-label="Settings navigation">
          <Button
            ref={back}
            variant="ghost"
            onClick={() => void navigate({ to: '/editor', replace: true })}
          >
            <ArrowLeft aria-hidden="true" /> Back to editor
          </Button>
        </nav>
      </AppHeader>
      <div className="settings-scroll">
        <div className="settings-content">
          <h1>Settings</h1>
          <section className="settings-section" aria-labelledby="settings-canvas-heading">
            <h2 id="settings-canvas-heading">Canvas view</h2>
            <p className="muted">Changes apply immediately and do not change the artwork.</p>
            <label className="settings-row" htmlFor="settings-grid">
              <span>Pixel grid</span>
              <input
                id="settings-grid"
                type="checkbox"
                checked={ui.showGrid}
                disabled={disabled}
                onChange={(event) => ui.set({ showGrid: event.currentTarget.checked })}
              />
            </label>
            <label className="settings-row" htmlFor="settings-backdrop">
              <span>Transparency backdrop</span>
              <select
                id="settings-backdrop"
                value={ui.backdrop}
                disabled={disabled}
                onChange={(event) =>
                  ui.set({ backdrop: event.currentTarget.value === 'light' ? 'light' : 'dark' })
                }
              >
                <option value="dark">Dark</option>
                <option value="light">Light</option>
              </select>
            </label>
          </section>
          <section className="settings-section" aria-labelledby="settings-project-heading">
            <h2 id="settings-project-heading">Project details</h2>
            <dl className="settings-details">
              <dt>Name</dt>
              <dd>{doc.name}</dd>
              <dt>Canvas</dt>
              <dd>
                {doc.width} × {doc.height} pixels
              </dd>
              <dt>Palette</dt>
              <dd>{doc.palette.length} RGBA colors</dd>
              <dt>Layers</dt>
              <dd>{doc.layers.length}</dd>
              <dt>Animations</dt>
              <dd>{doc.animations.length}</dd>
              <dt>Frames</dt>
              <dd>{doc.frames.length}</dd>
              <dt>Project ID</dt>
              <dd>{doc.id}</dd>
              <dt>File</dt>
              <dd>{doc.path ?? 'Not saved yet'}</dd>
            </dl>
          </section>
        </div>
      </div>
    </div>
  );
}
