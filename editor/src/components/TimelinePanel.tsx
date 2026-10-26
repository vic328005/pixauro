import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent,
  type PointerEvent,
} from 'react';
import { ArrowDown, ArrowUp } from 'lucide-react';
import { TimelineIcon } from './TimelineIcon';
import { LayerIcon } from './LayerIcon';
import { useEditor } from '../editorController';
import { useUi } from '../editorUiState';
import { MAX_ANIMATION_FPS, MIN_ANIMATION_FPS } from '../animationTiming';
import {
  animationDocument,
  selectedAnimation,
  celAt,
  frameNumbers,
  linkedFrames,
  stepFrame,
} from '../animation';
import type { AnimationEdit, DocumentSnapshot, LayerSnapshot } from '../generated/bindings';
import { Button } from './ui/button';
import { CommitInput } from './ui/commit-input';
import { ContextMenu } from './ui/context-menu';
import { TimelineContextMenu, type TimelineMenuTarget } from './TimelineContextMenu';
import './timeline.css';

const FRAME_ROW_HEIGHT = 25;
const HEADER_HEIGHT = FRAME_ROW_HEIGHT;

export function TimelinePanel() {
  const editor = useEditor();
  const ui = useUi();
  const doc = editor.document ? animationDocument(editor.document, ui.animationId) : null;
  const [hover, setHover] = useState<{ layer: string; image: string } | null>(null);
  const [menu, setMenu] = useState<TimelineMenuTarget | null>(null);
  const [rename, setRename] = useState<string | null>(null);
  const [focusLayer, setFocusLayer] = useState<string | null>(null);
  const [dragged, setDragged] = useState<string | null>(null);
  const layerGesture = useRef<{
    id: string;
    pointerId: number;
    startY: number;
    x: number;
    y: number;
    active: boolean;
    session: string;
    revision: string;
  } | null>(null);
  const suppressClick = useRef(false);
  const dragTick = useRef<number | null>(null);
  const { getDocument, editLayer } = editor;
  const scroll = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState({ left: 0, top: 0, width: 800, height: 280 });
  useEffect(() => {
    if (scroll.current) scroll.current.scrollLeft = 0;
    setMenu(null);
    setHover(null);
  }, [ui.animationId]);
  useEffect(() => {
    const node = scroll.current;
    if (!node) return;
    const read = () =>
      setViewport({
        left: node.scrollLeft,
        top: node.scrollTop,
        width: node.clientWidth,
        height: node.clientHeight,
      });
    const observer = new ResizeObserver(read);
    observer.observe(node);
    read();
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const cancel = () => {
      if (dragTick.current !== null) cancelAnimationFrame(dragTick.current);
      dragTick.current = null;
      layerGesture.current = null;
      setDragged(null);
    };
    const tick = () => {
      const gesture = layerGesture.current;
      const node = scroll.current;
      const current = getDocument();
      if (
        !gesture?.active ||
        !node ||
        current?.sessionId !== gesture.session ||
        current.revision !== gesture.revision
      ) {
        cancel();
        return;
      }
      const bounds = node.getBoundingClientRect();
      if (gesture.y < bounds.top + HEADER_HEIGHT + 24) node.scrollTop -= 8;
      else if (gesture.y > bounds.bottom - 24) node.scrollTop += 8;
      dragTick.current = requestAnimationFrame(tick);
    };
    const move = (event: globalThis.PointerEvent) => {
      const gesture = layerGesture.current;
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      gesture.x = event.clientX;
      gesture.y = event.clientY;
      if (!gesture.active && Math.abs(gesture.y - gesture.startY) >= 4) {
        gesture.active = true;
        suppressClick.current = true;
        setDragged(gesture.id);
        dragTick.current = requestAnimationFrame(tick);
      }
      if (gesture.active) event.preventDefault();
    };
    const up = (event: globalThis.PointerEvent) => {
      const gesture = layerGesture.current;
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      const current = getDocument();
      const target = document
        .elementFromPoint(event.clientX, event.clientY)
        ?.closest<HTMLElement>('[data-timeline-layer]')?.dataset.timelineLayer;
      cancel();
      if (
        !gesture.active ||
        !current ||
        current.sessionId !== gesture.session ||
        current.revision !== gesture.revision ||
        !target
      )
        return;
      const owner = current.layers.find((l) => l.id === gesture.id)?.animationId;
      if (owner !== useUi.getState().animationId) return;
      const position = current.layers
        .filter((l) => l.animationId === owner)
        .findIndex((l) => l.id === target);
      if (position >= 0) void editLayer({ kind: 'move', id: gesture.id, position });
    };
    window.addEventListener('pointermove', move, { passive: false });
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', cancel);
    window.addEventListener('blur', cancel);
    return () => {
      cancel();
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('blur', cancel);
    };
  }, [getDocument, editLayer]);
  const disabled = editor.drawingBlocked || !!ui.draft || !!ui.deletion || ui.playing;
  useEffect(() => {
    setMenu(null);
  }, [doc?.sessionId, doc?.revision, ui.playing]);
  useEffect(() => {
    const container = scroll.current;
    const index = doc?.frames.findIndex((f) => f.id === ui.frameId) ?? -1;
    if (!container || index < 0) return;
    const left = 208 + index * 36,
      right = left + 36;
    if (left < container.scrollLeft + 208) container.scrollLeft = Math.max(0, left - 208);
    else if (right > container.scrollLeft + container.clientWidth)
      container.scrollLeft = right - container.clientWidth;
  }, [ui.frameId, doc?.frames]);
  useEffect(() => {
    if (!focusLayer || !doc || !scroll.current) return;
    const index = [...doc.layers].reverse().findIndex((l) => l.id === focusLayer);
    if (index < 0) {
      setFocusLayer(null);
      return;
    }
    const node = scroll.current;
    const top = HEADER_HEIGHT + index * 32;
    if (top < node.scrollTop + HEADER_HEIGHT) node.scrollTop = top - HEADER_HEIGHT;
    else if (top + 32 > node.scrollTop + node.clientHeight)
      node.scrollTop = top + 32 - node.clientHeight;
    const request = requestAnimationFrame(() => {
      const target = node.querySelector<HTMLButtonElement>(
        `[data-timeline-layer="${focusLayer}"] .timeline-layer-name`,
      );
      if (target) {
        target.focus({ preventScroll: true });
        setFocusLayer(null);
      }
    });
    return () => cancelAnimationFrame(request);
  }, [focusLayer, viewport.top, doc]);
  if (!doc) return null;
  const animation = selectedAnimation(doc, ui.animationId);
  const columnStart = Math.max(0, Math.floor(viewport.left / 36) - 2);
  const columnEnd = Math.min(doc.frames.length, columnStart + Math.ceil(viewport.width / 36) + 4);
  const visibleFrames = doc.frames
    .slice(columnStart, columnEnd)
    .map((f, n) => ({ f, i: columnStart + n }));
  const rowStart = Math.max(0, Math.floor((viewport.top - HEADER_HEIGHT) / 32) - 2);
  const visibleLayers = [...doc.layers]
    .reverse()
    .slice(rowStart, rowStart + Math.ceil(viewport.height / 32) + 4);
  const navigateLayer = async (id: string, delta: number, reorder: boolean) => {
    if (disabled) return;
    const position = doc.layers.findIndex((l) => l.id === id) + delta;
    const target = doc.layers[position];
    if (!target) return;
    const ok = reorder
      ? await editor.editLayer({ kind: 'move', id, position })
      : await editor.selectLayer(target.id);
    if (ok) setFocusLayer(reorder ? id : target.id);
  };
  const startLayerDrag = (event: PointerEvent<HTMLButtonElement>, id: string) => {
    if (disabled || event.button !== 0 || !event.isPrimary) return;
    suppressClick.current = false;
    event.currentTarget.setPointerCapture(event.pointerId);
    layerGesture.current = {
      id,
      pointerId: event.pointerId,
      startY: event.clientY,
      x: event.clientX,
      y: event.clientY,
      active: false,
      session: doc.sessionId,
      revision: doc.revision,
    };
  };
  const blockLayerClick = () => {
    const blocked = suppressClick.current;
    suppressClick.current = false;
    return blocked;
  };
  const frame = doc.frames.find((f) => f.id === ui.frameId) ?? doc.frames[0];
  const index = doc.frames.indexOf(frame);
  const layer = doc.layers.find((l) => l.id === ui.layerId) ?? doc.layers.at(-1)!;
  const group = linkedFrames(doc, layer, frame.id);
  const position = doc.layers.indexOf(layer);
  const below = doc.layers[position - 1];
  const canMerge = !!below && layer.visible && below.visible && !layer.locked && !below.locked;
  const edit = (change: AnimationEdit) => {
    setMenu(null);
    void editor.editAnimation(change);
  };
  const select = (
    layerId: string,
    frameId: string,
    shift = false,
    toggle = false,
    kind: 'cels' | 'frames' = 'cels',
  ) => {
    if (disabled) return;
    void editor.withSettledDocument((document) => {
      if (useUi.getState().animationId !== ui.animationId) return;
      const latest = animationDocument(document, ui.animationId);
      if (
        !latest.layers.some((l) => l.id === layerId) ||
        !latest.frames.some((f) => f.id === frameId)
      )
        return;
      const s = useUi.getState();
      let selected = [frameId];
      let anchor = frameId;
      if (s.layerId === layerId && s.inspectionTarget?.kind === kind && (shift || toggle)) {
        anchor = s.anchorFrame ?? frameId;
        if (shift) {
          const a = latest.frames.findIndex((f) => f.id === anchor),
            b = latest.frames.findIndex((f) => f.id === frameId);
          selected = latest.frames.slice(Math.min(a, b), Math.max(a, b) + 1).map((f) => f.id);
        } else if (toggle)
          selected = s.selectedFrames.includes(frameId)
            ? s.selectedFrames.filter((f) => f !== frameId)
            : [...s.selectedFrames, frameId];
      }
      const ids = selected.length ? selected : [frameId];
      s.set({
        inspectionTarget: kind === 'frames' ? { kind, ids } : { kind, layerId, frameIds: ids },
        layerId,
        frameId: ids.includes(frameId) ? frameId : ids.at(-1)!,
        selectedFrames: ids,
        anchorFrame: anchor,
      });
    });
  };
  const context = (event: MouseEvent) => {
    const target = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-context-frame]');
    const frameId = target?.dataset.contextFrame;
    const layerId = target?.dataset.contextLayer;
    if (disabled || !frameId || !layerId || menu) {
      event.preventDefault();
      setMenu(null);
      return;
    }
    const header = target.dataset.contextHeader === 'true';
    target.focus({ preventScroll: true });
    // Let the Radix trigger record the pointer position; open after the stroke settles.
    void editor.withSettledDocument((document) => {
      if (useUi.getState().animationId !== ui.animationId) return;
      const latest = animationDocument(document, ui.animationId);
      if (
        !latest.layers.some((l) => l.id === layerId) ||
        !latest.frames.some((f) => f.id === frameId)
      )
        return;
      const state = useUi.getState();
      const selected =
        state.inspectionTarget?.kind === (header ? 'frames' : 'cels') &&
        state.layerId === layerId &&
        state.selectedFrames.includes(frameId)
          ? latest.frames.filter((f) => state.selectedFrames.includes(f.id)).map((f) => f.id)
          : [frameId];
      state.set({
        layerId,
        frameId,
        selectedFrames: selected,
        anchorFrame: selected.includes(state.anchorFrame ?? '') ? state.anchorFrame : frameId,
        inspectionTarget: header
          ? { kind: 'frames', ids: selected }
          : { kind: 'cels', layerId, frameIds: selected },
      });
      setMenu({ layer: layerId, frame: frameId, frames: selected, header });
    });
  };
  const icon = (label: string, child: React.ReactNode, action: () => void, blocked = false) => (
    <Button
      variant="ghost"
      size="icon-sm"
      title={label}
      aria-label={label}
      disabled={disabled || blocked}
      onClick={action}
    >
      {child}
    </Button>
  );
  return (
    <ContextMenu.Root
      open={!!menu}
      onOpenChange={(open) => {
        if (!open) setMenu(null);
      }}
    >
      <section
        className="timeline-panel"
        aria-label="Animation timeline"
        style={
          {
            '--timeline-frame-row-height': `${FRAME_ROW_HEIGHT}px`,
          } as CSSProperties
        }
      >
        <div className="timeline-toolbar">
          {icon(
            'First frame',
            <TimelineIcon name="first" />,
            () => void editor.withSettledDocument((d) => stepFrame(d, 'first')),
          )}
          {icon(
            'Previous frame · ,',
            <TimelineIcon name="previous" />,
            () => void editor.withSettledDocument((d) => stepFrame(d, -1)),
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={ui.playing ? 'Pause' : 'Play'}
            title="Play / pause · Space"
            aria-keyshortcuts="Space"
            aria-pressed={ui.playing}
            disabled={editor.drawingBlocked || !!ui.draft || !!ui.deletion}
            onClick={() => {
              if (ui.playing) ui.set({ playing: false });
              else void editor.withSettledDocument(() => useUi.getState().set({ playing: true }));
            }}
          >
            {ui.playing ? <TimelineIcon name="pause" /> : <TimelineIcon name="play" />}
          </Button>
          {icon(
            'Next frame · .',
            <TimelineIcon name="next" />,
            () => void editor.withSettledDocument((d) => stepFrame(d, 1)),
          )}
          {icon(
            'Last frame',
            <TimelineIcon name="last" />,
            () => void editor.withSettledDocument((d) => stepFrame(d, 'last')),
          )}
          <span className="timeline-separator" aria-hidden="true" />
          <span className="timeline-readout">
            <span className="timeline-animation-name" title={animation.name}>
              {animation.name}
            </span>
            <span className="muted">Frame</span>
            <span>
              {index + 1} / {doc.frames.length}
            </span>
          </span>
          <label className="timeline-timing">
            <span>FPS</span>
            <CommitInput
              compact
              key={animation.id}
              label="Animation FPS"
              value={String(animation.fps)}
              min={MIN_ANIMATION_FPS}
              max={MAX_ANIMATION_FPS}
              integer={false}
              disabled={disabled}
              onCommit={(value) =>
                editor.editAnimation({
                  kind: 'timing',
                  animation: animation.id,
                  fps: Number(value),
                })
              }
            />
          </label>
          <span className="timeline-spacer" />
          <Button
            variant="ghost"
            size="sm"
            aria-pressed={ui.onion}
            onClick={() => ui.set({ onion: !ui.onion })}
          >
            <TimelineIcon name="onion" /> Onion skin
          </Button>
          <span className="timeline-separator" aria-hidden="true" />
          {icon('New frame', <LayerIcon name="plus" />, () =>
            edit({ kind: 'newFrame', after: frame.id }),
          )}
          {icon('Duplicate frame · independent images', <LayerIcon name="duplicate" />, () =>
            edit({ kind: 'duplicateFrame', frame: frame.id, linked: false }),
          )}
          {icon(
            'Delete frame',
            <LayerIcon name="trash" />,
            () => edit({ kind: 'deleteFrame', frame: frame.id }),
            doc.frames.length === 1 ||
              doc.layers.some((l) => l.locked && celAt(l, frame.id) !== undefined),
          )}
        </div>
        <ContextMenu.Trigger asChild disabled={disabled} onContextMenu={context}>
          <div
            className="timeline-scroll"
            ref={scroll}
            onScroll={(e) => {
              setMenu(null);
              const n = e.currentTarget;
              setViewport({
                left: n.scrollLeft,
                top: n.scrollTop,
                width: n.clientWidth,
                height: n.clientHeight,
              });
            }}
          >
            <div
              className="timeline-grid"
              style={
                {
                  gridTemplateColumns: `208px repeat(${doc.frames.length},36px) minmax(36px, 1fr)`,
                  gridTemplateRows: `${FRAME_ROW_HEIGHT}px repeat(${doc.layers.length},32px)`,
                } as CSSProperties
              }
            >
              <div className="timeline-layer-tools">
                <span>Layers</span>
                {icon(
                  'New layer',
                  <LayerIcon name="plus" />,
                  () =>
                    void editor.editLayer({
                      kind: 'add',
                      animation: animation.id,
                      above: layer.id,
                      name: `layer ${doc.layers.length + 1}`,
                    }),
                )}
                {icon(
                  'Duplicate layer',
                  <LayerIcon name="duplicate" />,
                  () =>
                    void editor.editLayer({
                      kind: 'duplicate',
                      id: layer.id,
                      name: `${layer.name.slice(0, 120)} copy`,
                    }),
                )}
                {icon(
                  'Merge down · current animation',
                  <LayerIcon name="mergeDown" />,
                  () => void editor.editLayer({ kind: 'mergeDown', id: layer.id }),
                  !canMerge,
                )}
                {icon(
                  'Delete layer · current animation',
                  <LayerIcon name="trash" />,
                  () => void editor.editLayer({ kind: 'remove', id: layer.id }),
                  doc.layers.length === 1 || layer.locked,
                )}
              </div>
              <div
                className="timeline-frame-rule"
                style={{ gridColumn: '2 / -1', gridRow: 1 }}
                aria-hidden="true"
              />
              {visibleFrames.map(({ f, i }) => (
                <button
                  key={f.id}
                  className={`timeline-frame ${frame.id === f.id ? 'is-current' : ui.selectedFrames.includes(f.id) ? 'is-selected' : ''}`}
                  style={{ gridColumn: i + 2, gridRow: 1 }}
                  data-current-frame={frame.id === f.id}
                  disabled={disabled}
                  onClick={(event) =>
                    select(layer.id, f.id, event.shiftKey, event.metaKey || event.ctrlKey, 'frames')
                  }
                  data-context-layer={layer.id}
                  data-context-frame={f.id}
                  data-context-header="true"
                >
                  {i + 1}
                </button>
              ))}
              <button
                className="timeline-frame timeline-add-frame"
                style={{ gridColumn: doc.frames.length + 2, gridRow: 1 }}
                aria-label="Add frame after current"
                disabled={disabled}
                onClick={() => edit({ kind: 'newFrame', after: frame.id })}
              >
                +
              </button>
              {visibleLayers.map((l, n) => (
                <TimelineRow
                  key={l.id}
                  doc={doc}
                  visibleFrames={visibleFrames}
                  row={rowStart + n + 2}
                  layer={l}
                  currentFrame={frame.id}
                  group={group}
                  hover={hover}
                  selectedLayer={layer.id}
                  selectedFrames={ui.selectedFrames}
                  disabled={disabled}
                  thumbnails={ui.celView === 'thumbnails'}
                  onHover={setHover}
                  onSelect={select}
                  onNavigate={navigateLayer}
                  rename={rename === l.id}
                  onRename={() => setRename(l.id)}
                  onRenamed={() => setRename(null)}
                  dragged={dragged}
                  onDragStart={startLayerDrag}
                  blockClick={blockLayerClick}
                />
              ))}
            </div>
          </div>
        </ContextMenu.Trigger>
        <div className="timeline-hint">
          Hover a cel to see what shares its image · Shift-click for a range, ⌘/Ctrl-click to add ·
          Right-click a cel or frame number for link options
        </div>
        {menu && (
          <TimelineContextMenu doc={doc} menu={menu} edit={edit} close={() => setMenu(null)} />
        )}
      </section>
    </ContextMenu.Root>
  );
}

type RowProps = {
  visibleFrames: { f: DocumentSnapshot['frames'][number]; i: number }[];
  row: number;
  doc: DocumentSnapshot;
  layer: LayerSnapshot;
  currentFrame: string;
  group: string[];
  hover: { layer: string; image: string } | null;
  selectedLayer: string;
  selectedFrames: string[];
  disabled: boolean;
  thumbnails: boolean;
  onHover: (v: { layer: string; image: string } | null) => void;
  onNavigate: (id: string, delta: number, reorder: boolean) => Promise<void>;
  onSelect: (layer: string, frame: string, shift?: boolean, toggle?: boolean) => void;
  rename: boolean;
  onRename: () => void;
  onRenamed: () => void;
  dragged: string | null;
  onDragStart: (event: PointerEvent<HTMLButtonElement>, id: string) => void;
  blockClick: () => boolean;
};
function TimelineRow(p: RowProps) {
  const editor = useEditor();
  const l = p.layer;
  const current = p.selectedLayer === l.id;
  const pos = p.doc.layers.indexOf(l);
  return (
    <>
      <div
        className={`timeline-layer ${current ? 'is-current' : ''} ${!l.visible ? 'is-hidden' : ''}`}
        style={{ gridColumn: 1, gridRow: p.row }}
        data-timeline-layer={l.id}
        data-dragging={p.dragged === l.id}
      >
        <button
          aria-label={`${l.visible ? 'Hide' : 'Show'} ${l.name}`}
          disabled={p.disabled}
          onClick={() => void editor.updateLayer(l.id, { visible: !l.visible })}
        >
          {l.visible ? <LayerIcon name="eye" /> : <LayerIcon name="eyeOff" />}
        </button>
        <button
          aria-label={`${l.locked ? 'Unlock' : 'Lock'} ${l.name}`}
          aria-pressed={l.locked}
          disabled={p.disabled}
          onClick={() => void editor.updateLayer(l.id, { locked: !l.locked })}
        >
          {l.locked ? <LayerIcon name="lock" /> : <LayerIcon name="unlock" />}
        </button>
        <button
          aria-label={`${l.name}: new frames ${l.autoLink ? 'linked' : 'empty'}`}
          aria-pressed={l.autoLink}
          disabled={p.disabled}
          onClick={() =>
            void editor.editAnimation({ kind: 'autoLink', layer: l.id, on: !l.autoLink })
          }
        >
          <TimelineIcon name="link" />
        </button>
        {p.rename ? (
          <CommitInput
            compact
            label="Layer name"
            value={l.name}
            onCommit={(value) => editor.updateLayer(l.id, { name: value })}
            onFinish={p.onRenamed}
            autoFocus
          />
        ) : (
          <button
            className="timeline-layer-name"
            onPointerDown={(event) => p.onDragStart(event, l.id)}
            disabled={p.disabled}
            onClick={() => {
              if (!p.blockClick()) void editor.selectLayer(l.id);
            }}
            onDoubleClick={p.onRename}
            onKeyDown={(e) => {
              if (e.key === 'F2') {
                e.preventDefault();
                p.onRename();
              }
              if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
                e.preventDefault();
                void p.onNavigate(l.id, e.key === 'ArrowUp' ? 1 : -1, e.altKey);
              }
            }}
          >
            {l.name}
          </button>
        )}
        {current && (
          <span className="timeline-reorder">
            <button
              title="Move layer up"
              aria-label="Move layer up"
              disabled={p.disabled || pos === p.doc.layers.length - 1}
              onClick={() => void editor.editLayer({ kind: 'move', id: l.id, position: pos + 1 })}
            >
              <ArrowUp />
            </button>
            <button
              title="Move layer down"
              aria-label="Move layer down"
              disabled={p.disabled || pos === 0}
              onClick={() => void editor.editLayer({ kind: 'move', id: l.id, position: pos - 1 })}
            >
              <ArrowDown />
            </button>
          </span>
        )}
      </div>
      {p.visibleFrames.map(({ f, i }) => {
        const c = celAt(l, f.id);
        const previous = celAt(l, p.doc.frames[i - 1]?.id ?? null);
        const selected = current && p.selectedFrames.includes(f.id);
        const active = current && p.currentFrame === f.id;
        const bright =
          !!c &&
          ((current && p.group.includes(f.id)) ||
            (p.hover?.layer === l.id && p.hover.image === c.imageId));
        return (
          <button
            key={f.id}
            className={`timeline-cel ${active ? 'is-current' : selected ? 'is-selected' : ''} ${bright ? 'is-linked' : ''} ${!l.visible ? 'is-hidden' : ''} ${c && previous?.imageId === c.imageId ? 'has-link-left' : ''}`}
            aria-label={`${l.name}, frame ${i + 1}, ${c ? `linked frames ${frameNumbers(p.doc, linkedFrames(p.doc, l, f.id))}` : 'empty'}`}
            style={{ gridColumn: i + 2, gridRow: p.row }}
            aria-pressed={selected}
            disabled={p.disabled}
            onMouseEnter={() => p.onHover(c ? { layer: l.id, image: c.imageId } : null)}
            onMouseLeave={() => p.onHover(null)}
            onClick={(e) => p.onSelect(l.id, f.id, e.shiftKey, e.metaKey || e.ctrlKey)}
            data-context-layer={l.id}
            data-context-frame={f.id}
          >
            {p.thumbnails && c ? (
              <CelThumbnail doc={p.doc} layer={l} frameId={f.id} />
            ) : (
              <span className={c ? 'cel-dot' : 'cel-empty'} />
            )}
          </button>
        );
      })}
      <div
        className="timeline-row-end"
        style={{ gridColumn: p.doc.frames.length + 2, gridRow: p.row }}
      />
    </>
  );
}
function CelThumbnail({
  doc,
  layer,
  frameId,
}: {
  doc: DocumentSnapshot;
  layer: LayerSnapshot;
  frameId: string;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const ctx = ref.current?.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, 26, 26);
    ctx.imageSmoothingEnabled = false;
    const cel = celAt(layer, frameId);
    if (!cel) return;
    const colors = new Map(doc.palette.map((c) => [c.id, c.color]));
    const scale = Math.min(26 / doc.width, 26 / doc.height);
    for (const [x, y, id] of layer.images.find((i) => i.id === cel.imageId)?.pixels ?? []) {
      const c = colors.get(id);
      const px = x + cel.x,
        py = y + cel.y;
      if (!c || px < 0 || py < 0 || px >= doc.width || py >= doc.height) continue;
      ctx.fillStyle = `rgba(${c.red},${c.green},${c.blue},${c.alpha / 255})`;
      ctx.fillRect(
        (26 - doc.width * scale) / 2 + px * scale,
        (26 - doc.height * scale) / 2 + py * scale,
        Math.max(1, scale),
        Math.max(1, scale),
      );
    }
  }, [doc.width, doc.height, doc.palette, layer, frameId]);
  return <canvas ref={ref} width={26} height={26} className="cel-thumbnail" />;
}
