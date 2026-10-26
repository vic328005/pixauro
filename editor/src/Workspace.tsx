import { Button } from './components/ui/button';
import { useEffect, useRef, useState } from 'react';
import { Outlet, useLocation, useNavigate } from '@tanstack/react-router';
import {
  DockviewReact,
  type DockviewApi,
  type DockviewReadyEvent,
  type SerializedDockview,
} from 'dockview-react';
import { useEditor } from './editorController';
import { useUi } from './editorUiState';
import { useShallow } from 'zustand/react/shallow';
import { CanvasPanel } from './components/CanvasPanel';
import { TimelinePanel } from './components/TimelinePanel';
import { AnimationPanel } from './components/AnimationPanel';
import { Inspector } from './components/Inspector';
import { useAnimationPlayback } from './animation';
import { PalettePanel } from './components/PalettePanel';
import { AppHeader } from './components/AppHeader';
import {
  ChevronDown,
  Eraser,
  Pipette,
  House,
  Move,
  MousePointer2,
  Pencil,
  Undo2,
  Redo2,
  WandSparkles,
} from 'lucide-react';

const layoutKey = 'pixauro.dock-layout.v8';
const panels = {
  canvas: { title: 'Canvas', component: 'canvas' },
  palette: { title: 'Palette', component: 'palette' },
  tools: { title: 'Tools', component: 'tools' },
  animation: { title: 'Animation', component: 'animation' },
  timeline: { title: 'Timeline', component: 'timeline' },
  inspector: { title: 'Inspector', component: 'inspector' },
  history: { title: 'History', component: 'history' },
} as const;
type PanelId = keyof typeof panels;
function defaultLayout(api: DockviewApi) {
  api.clear();
  api.addPanel({ id: 'canvas', ...panels.canvas, minimumWidth: 240 });
  api.addPanel({
    id: 'palette',
    ...panels.palette,
    initialWidth: 300,
    minimumWidth: 200,
    position: { referencePanel: 'canvas', direction: 'left' },
  });
  api.addPanel({
    id: 'tools',
    ...panels.tools,
    initialHeight: 370,
    minimumHeight: 200,
    position: { referencePanel: 'palette', direction: 'below' },
  });
  api.addPanel({
    id: 'history',
    ...panels.history,
    initialWidth: 320,
    minimumWidth: 240,
    position: { referencePanel: 'canvas', direction: 'right' },
  });
  api.addPanel({
    id: 'inspector',
    ...panels.inspector,
    initialHeight: 620,
    minimumHeight: 200,
    position: { referencePanel: 'history', direction: 'above' },
  });
  api.addPanel({
    id: 'timeline',
    ...panels.timeline,
    initialHeight: 308,
    minimumHeight: 160,
    position: { referencePanel: 'canvas', direction: 'below' },
  });
  api.addPanel({
    id: 'animation',
    ...panels.animation,
    initialWidth: 240,
    minimumWidth: 200,
    position: { referencePanel: 'timeline', direction: 'left' },
  });
  // Apply final sizes after every split exists; initial sizes alone are rescaled by later splits.
  api.getPanel('palette')?.api.setSize({ width: 300, height: Math.min(416, api.height - 234) });
  api.getPanel('inspector')?.api.setSize({ width: 320, height: Math.min(620, api.height - 234) });
  api.getPanel('timeline')?.api.setSize({ height: Math.max(160, Math.min(308, api.height - 300)) });
  api.getPanel('canvas')?.api.setActive();
}
const components = {
  canvas: CanvasPanel,
  palette: PalettePanel,
  tools: ToolsPanel,
  history: HistoryPanel,
  animation: AnimationPanel,
  timeline: TimelinePanel,
  inspector: Inspector,
};
export function Workspace() {
  const editor = useEditor();
  const navigate = useNavigate();
  const settingsOpen = useLocation({
    select: (location) => location.pathname === '/editor/settings',
  });
  useAnimationPlayback(editor.document);
  const apiRef = useRef<DockviewApi | null>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const [dockApi, setDockApi] = useState<DockviewApi | null>(null);
  const draft = useUi((state) => state.draft);
  const deletion = useUi((state) => state.deletion);
  const newProjectOpen = useUi((state) => state.newProjectOpen);
  const hasStroke = useUi((state) => state.stroke !== null || state.movePreview !== null);
  const locked = editor.loading || !!draft || !!deletion || newProjectOpen;
  const onReady = ({ api }: DockviewReadyEvent) => {
    apiRef.current = api;
    if (hostRef.current) api.layout(hostRef.current.clientWidth, hostRef.current.clientHeight);
    setDockApi(api);
    try {
      const saved = localStorage.getItem(layoutKey);
      if (saved) {
        const parsed = JSON.parse(saved) as SerializedDockview;
        // Only these local components are valid; corrupted/obsolete layouts reset safely.
        if (
          !parsed.grid ||
          !Number.isFinite(parsed.grid.width) ||
          !Number.isFinite(parsed.grid.height) ||
          parsed.grid.width <= 0 ||
          parsed.grid.height <= 0 ||
          !parsed.panels ||
          Object.values(parsed.panels).some(
            (panel) => !Object.keys(panels).includes(panel.contentComponent ?? ''),
          )
        )
          throw new Error('Invalid saved layout');
        // Restore at the saved dimensions first; our resize observer then preserves rail sizes.
        api.layout(parsed.grid.width, parsed.grid.height);
        api.fromJSON(parsed);
      } else defaultLayout(api);
    } catch {
      defaultLayout(api);
      useUi.getState().set({
        notice: 'The saved panel layout could not be restored. The default layout is ready.',
      });
    }
  };
  useEffect(() => {
    if (!dockApi) return;
    const subscription = dockApi.onDidLayoutChange(() => {
      try {
        localStorage.setItem(layoutKey, JSON.stringify(dockApi.toJSON()));
      } catch {
        /* Storage may be unavailable; the current session remains usable. */
      }
    });
    // StrictMode replays effects independently of Dockview's onReady event.
    // Dispose only this subscription; the ready API remains valid for menus.
    return () => subscription.dispose();
  }, [dockApi]);
  useEffect(() => {
    const host = hostRef.current;
    if (!dockApi || !host) return;
    const observer = new ResizeObserver(() => {
      const width = host.clientWidth,
        height = host.clientHeight;
      if (width <= 0 || height <= 0 || (width === dockApi.width && height === dockApi.height))
        return;
      const palette = dockApi.getPanel('palette'),
        tools = dockApi.getPanel('tools');
      const inspector = dockApi.getPanel('inspector'),
        history = dockApi.getPanel('history');
      const canvas = dockApi.getPanel('canvas'),
        timeline = dockApi.getPanel('timeline'),
        animation = dockApi.getPanel('animation');
      const rect = (panel: typeof palette) => panel?.group.element.getBoundingClientRect();
      const p = rect(palette),
        t = rect(tools),
        r = rect(inspector),
        h = rect(history),
        c = rect(canvas),
        a = rect(timeline),
        an = rect(animation);
      // Preserve user-adjusted rail sizes on window resize. Freely rearranged docks use Dockview's layout.
      const rails =
        p &&
        t &&
        r &&
        h &&
        c &&
        a &&
        an &&
        Math.abs(p.left - t.left) < 2 &&
        Math.abs(r.left - h.left) < 2 &&
        Math.abs(c.left - an.left) < 2 &&
        Math.abs(c.right - a.right) < 2 &&
        Math.abs(an.right - a.left) < 2 &&
        p.right <= c.left + 2 &&
        c.right <= r.left + 2;
      dockApi.layout(width, height);
      if (rails) {
        palette?.api.setSize({ width: p.width, height: Math.min(p.height, height - 234) });
        inspector?.api.setSize({ width: r.width, height: Math.min(r.height, height - 234) });
        animation?.api.setSize({ width: an.width });
        timeline?.api.setSize({ height: Math.max(160, Math.min(a.height, height - 300)) });
      }
    });
    observer.observe(host);
    return () => observer.disconnect();
  }, [dockApi]);
  const showPanel = (id: PanelId) => {
    const api = apiRef.current;
    if (!api) return;
    const existing = api.getPanel(id);
    if (existing) existing.api.setActive();
    else
      api.addPanel({
        id,
        ...panels[id],
        position: { direction: id === 'canvas' ? 'left' : 'right' },
      });
  };
  if (!editor.document) return null;
  const doc = editor.document;
  return (
    <>
      <div className="workspace" hidden={settingsOpen}>
        <AppHeader>
          <nav className="workspace-toolbar" aria-label="File">
            <div className="toolbar-group">
              <Button
                variant="ghost"
                disabled={locked}
                onClick={() => void editor.home()}
                title="Close project · ⌘/Ctrl W"
              >
                <House aria-hidden="true" /> Home
              </Button>
              <Button variant="ghost" disabled={locked} onClick={() => void editor.newProject()}>
                New
              </Button>
              <Button variant="ghost" disabled={locked} onClick={() => void editor.openProject()}>
                Open
              </Button>
              <Button
                variant="ghost"
                disabled={locked || hasStroke}
                onClick={() =>
                  void editor.withSettledDocument(() => {
                    void navigate({ to: '/editor/settings' });
                  })
                }
              >
                Settings
              </Button>
            </div>
            <details className="toolbar-menu">
              <summary>
                Export <ChevronDown aria-hidden="true" />
              </summary>
              <div className="menu-popup">
                <Button
                  variant="ghost"
                  disabled={locked}
                  onClick={() => void editor.exportPng(false)}
                >
                  Artwork PNG…
                </Button>
                <Button
                  variant="ghost"
                  disabled={locked}
                  title="Export every frame of the selected animation as a sprite sheet and JSON"
                  onClick={() => void editor.exportSpriteSheet(false)}
                >
                  Current animation PNG + JSON…
                </Button>
                <Button
                  variant="ghost"
                  disabled={locked}
                  title="Export all animations into one sprite sheet and JSON"
                  onClick={() => void editor.exportSpriteSheet(true)}
                >
                  All animations PNG + JSON…
                </Button>
                <Button
                  variant="ghost"
                  disabled={locked}
                  onClick={() => void editor.exportPng(true)}
                >
                  Palette PNG…
                </Button>
              </div>
            </details>
            <details className="toolbar-menu">
              <summary>
                Panels <ChevronDown aria-hidden="true" />
              </summary>
              <div className="menu-popup">
                {(Object.keys(panels) as PanelId[]).map((id) => (
                  <Button
                    variant="ghost"
                    key={id}
                    disabled={locked || hasStroke}
                    onClick={() => showPanel(id)}
                  >
                    {panels[id].title}
                  </Button>
                ))}
                <hr />
                <Button
                  variant="ghost"
                  disabled={locked || hasStroke}
                  onClick={() => {
                    if (apiRef.current) defaultLayout(apiRef.current);
                  }}
                >
                  Reset layout
                </Button>
              </div>
            </details>
          </nav>
        </AppHeader>
        <div
          ref={hostRef}
          className={`dock-host ${draft || deletion || newProjectOpen ? 'editor-modal-open' : ''}`}
        >
          <DockviewReact
            className="dockview-theme-dark pixauro-dock"
            disableAutoResizing
            components={components}
            onReady={onReady}
            dndStrategy="pointer"
            disableFloatingGroups
            disableDnd={editor.busy || locked || hasStroke}
          />
        </div>
        <footer className="workspace-status">
          <span className="save-state" role="status">
            {doc.modified && <span className="dirty-dot" aria-hidden="true" />}
            {doc.modified
              ? 'Unsaved changes'
              : doc.path
                ? 'All changes saved'
                : 'Not saved to a file yet'}
          </span>
          <span className="document-summary">
            {doc.width} × {doc.height} px · {doc.layers.length} total layers ·{' '}
            {doc.animations.length} animations · {doc.frames.length} frames · {doc.palette.length}{' '}
            colors
          </span>
        </footer>
      </div>
      <Outlet />
    </>
  );
}
function ToolsPanel() {
  const { loading, deleteSelection } = useEditor();
  const ui = useUi(
    useShallow((state) => ({
      tool: state.tool,
      brushSize: state.brushSize,
      draft: state.draft,
      deletion: state.deletion,
      selection: state.selection,
      wandTolerance: state.wandTolerance,
      wandContiguous: state.wandContiguous,
      set: state.set,
    })),
  );
  const disabled = loading || !!ui.draft || !!ui.deletion;
  return (
    <section className="tools-panel panel-body" aria-label="Drawing tools">
      <h2 className="panel-heading">Drawing tools</h2>
      <div className="tool-buttons">
        {(
          [
            ['brush', Pencil, 'Brush', 'B'],
            ['eraser', Eraser, 'Eraser', 'E'],
            ['eyedropper', Pipette, 'Eyedropper', 'I'],
            ['wand', WandSparkles, 'Magic wand', 'W'],
            ['move', MousePointer2, 'Move cel', 'V'],
            ['pan', Move, 'Pan', 'H'],
          ] as const
        ).map(([tool, Icon, label, key]) => (
          <Button
            key={tool}
            variant="ghost"
            aria-pressed={ui.tool === tool}
            disabled={disabled}
            onClick={() => ui.set({ tool })}
            title={`${label} · ${key}`}
          >
            <Icon aria-hidden="true" />
            {label}
            <kbd>{key}</kbd>
          </Button>
        ))}
      </div>
      {ui.tool === 'wand' ? (
        <div className="wand-options">
          <label>
            <span className="field-label">
              Tolerance <span>{ui.wandTolerance}</span>
            </span>
            <input
              type="range"
              min={0}
              max={255}
              step={1}
              value={ui.wandTolerance}
              disabled={disabled}
              onChange={(event) => ui.set({ wandTolerance: Number(event.target.value) })}
            />
          </label>
          <label>
            <input
              type="checkbox"
              checked={ui.wandContiguous}
              disabled={disabled}
              onChange={(event) => ui.set({ wandContiguous: event.target.checked })}
            />{' '}
            Contiguous
          </label>
          <p className="panel-hint">
            Selects colors on the current layer. Increase tolerance for similar shades; turn off
            Contiguous to select matching colors everywhere.
            <br />
            Shift-click adds · Alt-click subtracts
          </p>
        </div>
      ) : ui.tool === 'eyedropper' ? (
        <p className="panel-hint">
          Click to pick the visible artwork color, including opacity. New colors are added to the
          custom palette. Fully transparent pixels are ignored.
        </p>
      ) : (
        <>
          <h3 className="field-label">
            Size <span>{ui.brushSize} px</span>
          </h3>
          <div className="segmented size-buttons" role="group" aria-label="Brush size">
            {[1, 2, 4, 8].map((size) => (
              <Button
                key={size}
                disabled={disabled}
                variant="ghost"
                aria-pressed={ui.brushSize === size}
                onClick={() => ui.set({ brushSize: size })}
              >
                {size}
              </Button>
            ))}
          </div>
        </>
      )}
      {ui.selection && (
        <div className="wand-options">
          <span className="field-label">
            {ui.selection.pixelCount.toLocaleString()} pixels selected
          </span>
          <Button disabled={disabled} onClick={() => void deleteSelection()}>
            Clear selected pixels · ⌫
          </Button>
          <Button variant="ghost" disabled={disabled} onClick={() => ui.set({ selection: null })}>
            Deselect · Esc
          </Button>
        </div>
      )}
      <p className="panel-hint">
        {(ui.tool === 'brush' || ui.tool === 'eraser') && (
          <>
            Right-click color to paint · Right-click blank to erase
            <br />
          </>
        )}
        Middle-drag to pan · Scroll to zoom
        <br />B brush · E eraser · I eyedropper · W wand · V move · [ ] resize
        <br />, and . step frames · Space plays / pauses
      </p>
    </section>
  );
}
function HistoryPanel() {
  const { document: doc, loading, historyJump, stepHistory } = useEditor();
  const draft = useUi((state) => state.draft);
  const deletion = useUi((state) => state.deletion);
  const currentRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    currentRef.current?.scrollIntoView({ block: 'nearest' });
  }, [doc?.revision, doc?.history.position]);
  if (!doc) return null;
  const total = doc.history.entries.length;
  return (
    <section className="history-panel panel-body" aria-label="History">
      <div className="panel-heading">
        <h2>History</h2>
        <div className="history-heading-actions">
          <span className="muted">
            {doc.history.position} / {total}
          </span>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="Undo"
            title="Undo"
            disabled={loading || !!draft || !!deletion || !doc.canUndo}
            onClick={() => void stepHistory(-1)}
          >
            <Undo2 />
          </Button>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="Redo"
            title="Redo"
            disabled={loading || !!draft || !!deletion || !doc.canRedo}
            onClick={() => void stepHistory(1)}
          >
            <Redo2 />
          </Button>
        </div>
      </div>
      <div className="history-list">
        {[...Array(total + 1)].map((_, row) => {
          const step = total - row;
          const active = step === doc.history.position;
          const future = step > doc.history.position;
          return (
            <Button
              key={step}
              variant="ghost"
              ref={active ? currentRef : undefined}
              className={`history-row ${active ? 'current' : ''} ${future ? 'future' : ''}`}
              disabled={loading || !!draft || !!deletion}
              aria-current={active ? 'step' : undefined}
              title={`Go to step ${step}`}
              onClick={() => {
                if (!active || useUi.getState().stroke) void historyJump(step);
              }}
            >
              <span className="step-number">{String(step).padStart(2, '0')}</span>
              <span className="truncate">
                {step ? doc.history.entries[step - 1] : 'Initial state'}
              </span>
              {future && <span className="redo-tag">redo</span>}
            </Button>
          );
        })}
      </div>
      {total === 0 && (
        <p className="panel-hint history-hint">
          Draw your first pixels. Every stroke and palette edit appears here.
        </p>
      )}
    </section>
  );
}
