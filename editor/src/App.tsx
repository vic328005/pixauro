import { Button } from './components/ui/button';
import { useEffect } from 'react';
import { Outlet, useLocation, useNavigate } from '@tanstack/react-router';
import { useEditor } from './editorController';
import { useUi } from './editorUiState';
import { stepFrame } from './animation';
import { PaletteDialogs } from './components/PalettePanel';
import { ArrowDownToLine, X } from 'lucide-react';
import { AppHeader } from './components/AppHeader';
import { NewProjectDialog } from './components/NewProjectDialog';
import { PngImportDialog } from './components/PngImportDialog';

export function AppShell() {
  const editor = useEditor();
  const navigate = useNavigate();
  const pathname = useLocation({ select: (location) => location.pathname });
  const error = useUi((state) => state.error);
  const notice = useUi((state) => state.notice);
  const dragging = useUi((state) => state.draggingFile);
  const draggingPng = useUi((state) => state.draggingPng);
  const newProjectOpen = useUi((state) => state.newProjectOpen);
  const importDialog = useUi((state) => state.importDialog);
  const documentRoute = editor.document ? '/editor' : '/';
  useEffect(() => {
    const settingsAllowed = documentRoute === '/editor' && pathname === '/editor/settings';
    if (pathname !== documentRoute && !settingsAllowed)
      void navigate({ to: documentRoute, replace: true });
  }, [documentRoute, pathname, navigate]);
  useEffect(() => {
    // Suppress the WebView menu after components handle their own context menus.
    // Bubble so Radix triggers can still handle events that are not yet prevented.
    const preventNativeContextMenu = (event: MouseEvent) => event.preventDefault();
    window.addEventListener('contextmenu', preventNativeContextMenu);
    return () => window.removeEventListener('contextmenu', preventNativeContextMenu);
  }, []);
  useEffect(() => {
    // WebKit may treat Backspace (or Shift+Backspace) as history navigation.
    // Catch it before menu/dialog handlers can stop propagation, while allowing
    // native deletion in writable inputs and contenteditable elements.
    const preventBackspaceNavigation = (event: KeyboardEvent) => {
      if (event.key !== 'Backspace') return;
      const target = event.target;
      if (target instanceof HTMLElement && target.isContentEditable) return;
      if (target instanceof HTMLTextAreaElement && !target.readOnly && !target.disabled) return;
      if (
        target instanceof HTMLInputElement &&
        !target.readOnly &&
        !target.disabled &&
        ![
          'button',
          'checkbox',
          'color',
          'file',
          'hidden',
          'image',
          'radio',
          'range',
          'reset',
          'submit',
        ].includes(target.type)
      )
        return;
      event.preventDefault();
      if (!(target instanceof HTMLElement)) return;
      if (target.closest('input, textarea, select, [role=menu], dialog, [role=dialog]')) return;
      if (
        pathname !== '/editor' ||
        event.isComposing ||
        event.repeat ||
        event.ctrlKey ||
        event.metaKey ||
        event.altKey ||
        event.shiftKey ||
        editor.drawingBlocked
      )
        return;
      void editor.deleteSelection();
    };
    window.addEventListener('keydown', preventBackspaceNavigation, true);
    return () => window.removeEventListener('keydown', preventBackspaceNavigation, true);
  }, [editor, pathname]);
  useEffect(() => {
    // Capture before React, Dockview and portalled menus handle new actions.
    // Keep controls and focus intact instead of flashing disabled styles during I/O.
    const blockFileInteraction = (event: Event) => {
      if (!useUi.getState().flowPending) return;
      event.preventDefault();
      event.stopImmediatePropagation();
    };
    const events = [
      'pointerdown',
      'mousedown',
      'click',
      'dblclick',
      'contextmenu',
      'keydown',
      'beforeinput',
      'wheel',
    ] as const;
    for (const type of events) {
      window.addEventListener(type, blockFileInteraction, { capture: true, passive: false });
    }
    return () => {
      for (const type of events) window.removeEventListener(type, blockFileInteraction, true);
    };
  }, []);
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return;
      const target = event.target as HTMLElement;
      if (target.closest('input, textarea, select, [role=menu]') || target.isContentEditable)
        return;
      const command = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      if (command && key === 'a') {
        event.preventDefault();
        return;
      }
      if (target.closest('dialog, [role="dialog"]')) return;
      const ui = useUi.getState();
      if (ui.draft || ui.deletion || ui.newProjectOpen || ui.importDialog) return;
      const inSettings = pathname === '/editor/settings';
      if (key === ' ' && !command && !event.altKey && !event.shiftKey) {
        if (inSettings || !editor.document) return;
        // Selection buttons retain focus after clicks; Space must still control playback.
        // Other controls keep their native keyboard activation.
        const playbackTarget = target.closest(
          '.animation-item, .timeline-frame, .timeline-cel, .timeline-layer-name, .timeline-toolbar button[aria-label="Play"], .timeline-toolbar button[aria-label="Pause"]',
        );
        if (target.closest('button, a, [role=button], summary') && !playbackTarget) return;
        event.preventDefault();
        if (event.repeat || editor.busy) return;
        if (ui.playing) ui.set({ playing: false });
        else void editor.withSettledDocument(() => useUi.getState().set({ playing: true }));
        return;
      }
      if (event.repeat) return;
      if (inSettings && (key === 'escape' || (command && key === 'w'))) {
        event.preventDefault();
        void navigate({ to: '/editor', replace: true });
        return;
      }
      if (command && ['s', 'o', 'n', 'z', 'y', 'w'].includes(key)) {
        event.preventDefault();
        if (editor.busy) return;
        if (key === 's') {
          if (editor.document) void editor.save(event.shiftKey);
        }
        if (key === 'o') void editor.openProject();
        if (key === 'n') void editor.newProject();
        if (key === 'w') {
          if (editor.document) void editor.home();
        }
        if (!inSettings && editor.document && (key === 'z' || key === 'y')) {
          const forward = key === 'y' || event.shiftKey;
          void editor.stepHistory(forward ? 1 : -1);
        }
        return;
      }
      if (!inSettings && command && key === 'd') {
        event.preventDefault();
        ui.set({ selection: null });
        return;
      }
      if (inSettings || command || event.altKey || editor.busy || !editor.document) return;
      if (key === 'escape') ui.set({ selection: null });
      if (key === 'w') ui.set({ tool: 'wand' });
      if (key === 'b') ui.set({ tool: 'brush' });
      if (key === 'e') ui.set({ tool: 'eraser' });
      if (key === 'i') ui.set({ tool: 'eyedropper' });
      if (key === 'v') ui.set({ tool: 'move' });
      if (key === ',' || key === '.') {
        event.preventDefault();
        void editor.withSettledDocument((d) => stepFrame(d, key === ',' ? -1 : 1));
      }
      if (key === 'h') ui.set({ tool: 'pan' });
      if (key === 'g') ui.set({ showGrid: !ui.showGrid });
      if (key === '0') {
        void editor.withSettledDocument(() => ui.set({ zoom: null, pan: { x: 0, y: 0 } }));
      }
      if (key === '[' || key === ']') {
        const sizes = [1, 2, 4, 8];
        ui.set({
          brushSize:
            sizes[Math.max(0, Math.min(3, sizes.indexOf(ui.brushSize) + (key === ']' ? 1 : -1)))],
        });
      }
    };
    window.addEventListener('keydown', keydown);
    const preventDrop = (event: DragEvent) => {
      event.preventDefault();
    };
    window.addEventListener('dragover', preventDrop);
    window.addEventListener('drop', preventDrop);
    return () => {
      window.removeEventListener('keydown', keydown);
      window.removeEventListener('dragover', preventDrop);
      window.removeEventListener('drop', preventDrop);
    };
  }, [editor, pathname, navigate]);
  return (
    <div className="app-shell" aria-busy={editor.busy}>
      {!editor.document && <AppHeader home />}
      <div className="app-notifications">
        {!editor.desktop && (
          <div className="banner notice">
            Browser preview. Open the desktop app to create, edit, or save local projects.
          </div>
        )}
        {editor.loadError && (
          <div className="banner error" role="alert">
            {editor.loadError}
            <Button onClick={editor.reload}>Retry</Button>
          </div>
        )}
        {error && !newProjectOpen && !importDialog && (
          <div className="banner error" role="alert">
            <span>{error}</span>
            <Button
              aria-label="Dismiss error"
              onClick={() => useUi.getState().set({ error: null })}
            >
              <X aria-hidden="true" />
            </Button>
          </div>
        )}
        {notice && (
          <div className="banner notice" role="status">
            <span>{notice}</span>
            <Button
              aria-label="Dismiss notice"
              onClick={() => useUi.getState().set({ notice: null })}
            >
              <X aria-hidden="true" />
            </Button>
          </div>
        )}
      </div>
      <main className="route-content">
        <Outlet />
      </main>
      <PaletteDialogs />
      {newProjectOpen && <NewProjectDialog />}
      {importDialog && <PngImportDialog target={importDialog} />}
      {dragging && (!draggingPng || !editor.document) && (
        <div className="drop-overlay">
          <div>
            <ArrowDownToLine className="drop-symbol" aria-hidden="true" />
            <strong>
              {draggingPng ? 'Create or open a project first' : 'Open a Pixauro project'}
            </strong>
            <p>{draggingPng ? 'Then drop a PNG onto the canvas' : 'Drop one .json file here'}</p>
          </div>
        </div>
      )}
    </div>
  );
}
