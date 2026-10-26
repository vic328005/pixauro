import { Button } from './ui/button';
import { Minus, Plus } from 'lucide-react';
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties, PointerEvent as ReactPointerEvent } from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import { appendStroke, beginStroke } from '../brush';
import type { Point, StrokeAccumulator } from '../brush';
import { CanvasRenderer } from '../canvasRenderer';
import { CanvasBackdrop, CursorGuides, PixelGrid } from './CanvasGuides';
import { magicWandSelection, selectionMatches } from '../selection';
import { viewportGeometryChanged, type ViewportGeometry } from '../viewportGeometry';
import { registerStrokeFlusher, useEditor } from '../editorController';
import { useUi, type CelMove } from '../editorUiState';
import { animationDocument, selectedAnimation, celAt, linkedFrames } from '../animation';
import type { LayerSnapshot } from '../generated/bindings';

type ViewportSize = { width: number; height: number };
type Transform = { left: number; top: number; zoom: number };
type PanGesture = {
  pointerId: number;
  clientX: number;
  clientY: number;
  originX: number;
  originY: number;
  button: number;
};

const MIN_ZOOM = 1 / 32;
const MAX_ZOOM = 64;
const VIEWPORT_PADDING = 32;

function layerDrawingBlock(layer: LayerSnapshot | undefined): string | null {
  if (!layer) return 'Select a layer to draw.';
  if (layer.locked) return `“${layer.name}” is locked. Unlock it to draw.`;
  if (!layer.visible) return `“${layer.name}” is hidden. Show it to draw.`;
  if (layer.opacity === 0) return `“${layer.name}” is transparent. Increase opacity to draw.`;
  return null;
}

function constrainZoom(zoom: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

function fitZoom(size: ViewportSize, width: number, height: number): number {
  if (size.width <= 0 || size.height <= 0) return 1;
  const fit = Math.max(
    0.001,
    Math.min(
      (size.width - VIEWPORT_PADDING * 2) / width,
      (size.height - VIEWPORT_PADDING * 2) / height,
    ),
  );
  return fit >= 1 ? Math.floor(fit) : fit;
}

export function CanvasPanel({ api }: IDockviewPanelProps) {
  const {
    document: documentSnapshot,
    loading,
    drawingBlocked,
    commitStroke,
    commitCelMove,
    pickColor,
    withSettledDocument,
    getDocument,
  } = useEditor();
  const ui = useUi();
  const snapshot = documentSnapshot ? animationDocument(documentSnapshot, ui.animationId) : null;
  const viewportRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const moveGestureRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    origin: CelMove;
  } | null>(null);
  const commitMoveRef = useRef(commitCelMove);
  commitMoveRef.current = commitCelMove;
  const activeStrokeRef = useRef<StrokeAccumulator | null>(null);
  const previewFrameRef = useRef<number | null>(null);
  const rendererRef = useRef<CanvasRenderer | null>(null);
  const strokePointerRef = useRef<number | null>(null);
  const panGestureRef = useRef<PanGesture | null>(null);
  const pendingCommitRef = useRef<Promise<boolean> | null>(null);
  const commitStrokeRef = useRef(commitStroke);
  const snapshotRef = useRef(snapshot);
  const busyRef = useRef(drawingBlocked);
  const [size, setSize] = useState<ViewportSize>({ width: 0, height: 0 });
  const [hover, setHover] = useState<Point | null>(null);
  const [panning, setPanning] = useState(false);
  commitStrokeRef.current = commitStroke;
  snapshotRef.current = snapshot;
  busyRef.current = drawingBlocked;

  const width = snapshot?.width ?? 1;
  const height = snapshot?.height ?? 1;
  const fittedZoom = fitZoom(size, width, height);
  const zoom = ui.zoom === null ? fittedZoom : constrainZoom(ui.zoom);
  const left = (size.width - width * zoom) / 2 + ui.pan.x;
  const top = (size.height - height * zoom) / 2 + ui.pan.y;
  const transformRef = useRef<Transform>({ left, top, zoom });
  transformRef.current = { left, top, zoom };

  const releaseCapture = useCallback((pointerId: number | null) => {
    const viewport = viewportRef.current;
    if (pointerId !== null && viewport?.hasPointerCapture(pointerId)) {
      viewport.releasePointerCapture(pointerId);
    }
  }, []);

  const finishPan = useCallback(() => {
    const gesture = panGestureRef.current;
    panGestureRef.current = null;
    setPanning(false);
    if (gesture) releaseCapture(gesture.pointerId);
  }, [releaseCapture]);

  const publishStroke = useCallback(() => {
    if (previewFrameRef.current !== null) return;
    previewFrameRef.current = requestAnimationFrame(() => {
      previewFrameRef.current = null;
      const stroke = activeStrokeRef.current;
      if (stroke) useUi.getState().set({ stroke: stroke.snapshot() });
    });
  }, []);

  const cancelPublication = useCallback(() => {
    if (previewFrameRef.current !== null) cancelAnimationFrame(previewFrameRef.current);
    previewFrameRef.current = null;
  }, []);

  const flushStroke = useCallback((): Promise<boolean> => {
    finishPan();
    // A second blur, pointerup, or native action waits for the very same commit.
    if (pendingCommitRef.current) return pendingCommitRef.current;
    const gesture = moveGestureRef.current;
    moveGestureRef.current = null;
    const move = useUi.getState().movePreview;
    if (gesture && move) {
      releaseCapture(gesture.pointerId);
      const pending = Promise.resolve()
        .then(() => commitMoveRef.current(move))
        .catch(() => false)
        .finally(() => {
          useUi.getState().set({ movePreview: null });
          pendingCommitRef.current = null;
        });
      pendingCommitRef.current = pending;
      return pending;
    }
    const stroke = activeStrokeRef.current;
    cancelPublication();
    const pointerId = strokePointerRef.current;
    activeStrokeRef.current = null;
    strokePointerRef.current = null;
    releaseCapture(pointerId);
    if (!stroke) return Promise.resolve(true);
    const preview = stroke.snapshot();
    // Finalize before yielding: queued samples can no longer alter this commit.
    useUi.getState().set({ stroke: preview });
    const pending = Promise.resolve()
      .then(() => commitStrokeRef.current(preview))
      .catch(() => false)
      .finally(() => {
        // Keep this preview visible until the authoritative snapshot arrives.
        if (useUi.getState().stroke?.key === preview.key) useUi.getState().set({ stroke: null });
        pendingCommitRef.current = null;
      });
    pendingCommitRef.current = pending;
    return pending;
  }, [cancelPublication, finishPan, releaseCapture]);
  const flushRef = useRef(flushStroke);
  flushRef.current = flushStroke;

  useEffect(() => {
    const unregister = registerStrokeFlusher(flushStroke);
    return () => {
      // A dock-panel remount must not abandon a captured, unsubmitted stroke.
      void flushStroke();
      unregister();
    };
  }, [flushStroke]);

  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    let frame = 0;
    let previousGeometry: ViewportGeometry | null = null;
    const measure = () => {
      const rect = viewport.getBoundingClientRect();
      const geometry = { left: rect.left, top: rect.top, width: rect.width, height: rect.height };
      const changed = viewportGeometryChanged(previousGeometry, geometry);
      previousGeometry = geometry;
      if (changed) {
        void flushRef.current();
        setHover(null);
      }
      // Dockview can construct content while detached/hidden. Ignore that zero
      // measurement and read again when its group is laid out or made visible.
      if (rect.width <= 0 || rect.height <= 0) return;
      setSize((previous) => {
        if (previous.width === rect.width && previous.height === rect.height) return previous;
        return { width: rect.width, height: rect.height };
      });
    };
    const scheduleMeasure = () => {
      // Dockview emits dimensions even when only an option (e.g. stroke DnD lock)
      // changed. Only a real geometry change may interrupt a captured gesture.
      measure();
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(measure);
    };
    const observer = new ResizeObserver(scheduleMeasure);
    observer.observe(viewport);
    const dimensions = api.onDidDimensionsChange(scheduleMeasure);
    const visibility = api.onDidVisibilityChange(({ isVisible }) => {
      // Visibility may be announced before Dockview changes the DOM geometry.
      if (!isVisible) {
        void flushRef.current();
        setHover(null);
      }
      scheduleMeasure();
    });
    window.addEventListener('resize', scheduleMeasure);
    measure();
    scheduleMeasure();
    return () => {
      observer.disconnect();
      dimensions.dispose();
      visibility.dispose();
      window.removeEventListener('resize', scheduleMeasure);
      cancelAnimationFrame(frame);
    };
  }, [api]);

  useEffect(() => {
    const blur = () => {
      setHover(null);
      void flushStroke();
    };
    const visibility = () => {
      if (window.document.visibilityState === 'hidden') blur();
    };
    window.addEventListener('blur', blur);
    window.document.addEventListener('visibilitychange', visibility);
    return () => {
      window.removeEventListener('blur', blur);
      window.document.removeEventListener('visibilitychange', visibility);
    };
  }, [flushStroke]);

  useEffect(() => {
    if ((ui.draft || ui.newProjectOpen) && (activeStrokeRef.current || moveGestureRef.current))
      void flushStroke();
  }, [ui.draft, ui.newProjectOpen, flushStroke]);

  useEffect(() => {
    setHover(null);
    const move = moveGestureRef.current;
    if (move && move.origin.sessionId !== snapshot?.sessionId) {
      moveGestureRef.current = null;
      releaseCapture(move.pointerId);
      useUi.getState().set({ movePreview: null });
    }
    const stroke = activeStrokeRef.current;
    if (stroke && stroke.sessionId !== snapshot?.sessionId) {
      // A stale project preview cannot leak into a newly loaded document.
      activeStrokeRef.current = null;
      cancelPublication();
      const pointerId = strokePointerRef.current;
      strokePointerRef.current = null;
      releaseCapture(pointerId);
      if (useUi.getState().stroke?.sessionId === stroke.sessionId)
        useUi.getState().set({ stroke: null });
    }
  }, [snapshot?.sessionId, cancelPublication, releaseCapture]);

  useLayoutEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext('2d');
    const accepted = getDocument();
    if (!snapshot || !context || accepted?.sessionId !== snapshot.sessionId) return;
    const renderer = rendererRef.current ?? (rendererRef.current = new CanvasRenderer());
    // Preview cleanup can arrive before React Query's render notification. Always
    // draw the accepted pixels, so lifting the pointer never flashes the old image.
    renderer.render(
      context,
      animationDocument(accepted, ui.animationId),
      ui.draft,
      ui.stroke,
      ui.layerOpacity,
      ui.frameId ?? accepted.frames[0].id,
      ui.onion && !ui.playing,
      ui.movePreview,
    );
  }, [
    snapshot,
    ui.draft,
    ui.stroke,
    ui.layerOpacity,
    ui.frameId,
    ui.animationId,
    ui.onion,
    ui.playing,
    ui.movePreview,
    getDocument,
  ]);

  const pixelAt = useCallback((clientX: number, clientY: number): Point | null => {
    const viewport = viewportRef.current;
    const current = snapshotRef.current;
    if (!viewport || !current) return null;
    const rect = viewport.getBoundingClientRect();
    // Pointer capture also delivers moves over a toolbar/outside the viewport.
    if (
      clientX < rect.left ||
      clientX >= rect.right ||
      clientY < rect.top ||
      clientY >= rect.bottom
    )
      return null;
    const transform = transformRef.current;
    const x = (clientX - rect.left - transform.left) / transform.zoom;
    const y = (clientY - rect.top - transform.top) / transform.zoom;
    if (x < 0 || y < 0 || x >= current.width || y >= current.height) return null;
    return { x: Math.floor(x), y: Math.floor(y) };
  }, []);

  const changeZoom = useCallback((nextZoom: number, anchor?: { x: number; y: number }) => {
    const current = snapshotRef.current;
    const viewport = viewportRef.current;
    if (
      !current ||
      !viewport ||
      activeStrokeRef.current ||
      moveGestureRef.current ||
      pendingCommitRef.current
    )
      return;
    const actual = transformRef.current;
    const next = constrainZoom(nextZoom);
    const rect = viewport.getBoundingClientRect();
    const center = anchor ?? { x: rect.width / 2, y: rect.height / 2 };
    const imageX = (center.x - actual.left) / actual.zoom;
    const imageY = (center.y - actual.top) / actual.zoom;
    const pan = {
      x: center.x - imageX * next - (rect.width - current.width * next) / 2,
      y: center.y - imageY * next - (rect.height - current.height * next) / 2,
    };
    // Keep rapid wheel events anchored correctly before React's next render.
    transformRef.current = {
      left: (rect.width - current.width * next) / 2 + pan.x,
      top: (rect.height - current.height * next) / 2 + pan.y,
      zoom: next,
    };
    useUi.getState().set({ zoom: next, pan });
    setHover(null);
  }, []);

  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      if (busyRef.current || panGestureRef.current) return;
      const rect = viewport.getBoundingClientRect();
      const delta = event.deltaMode === 1 ? event.deltaY * 16 : event.deltaY;
      changeZoom(
        transformRef.current.zoom * Math.exp(-Math.max(-300, Math.min(300, delta)) * 0.002),
        {
          x: event.clientX - rect.left,
          y: event.clientY - rect.top,
        },
      );
      // Zoom updates the transform synchronously; keep guides under a stationary pointer.
      setHover(pixelAt(event.clientX, event.clientY));
    };
    viewport.addEventListener('wheel', wheel, { passive: false });
    return () => viewport.removeEventListener('wheel', wheel);
  }, [changeZoom, pixelAt]);

  const appendAt = useCallback(
    (point: Point | null) => {
      const current = snapshotRef.current;
      const stroke = activeStrokeRef.current;
      if (!current || !stroke || current.sessionId !== stroke.sessionId) return;
      if (appendStroke(stroke, point)) publishStroke();
    },
    [publishStroke],
  );

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!event.isPrimary) return;
    // Read the accepted revision directly; React may still be rendering the last commit.
    const snapshot = getDocument();
    const currentUi = useUi.getState();
    if (
      !snapshot ||
      snapshot.sessionId !== snapshotRef.current?.sessionId ||
      drawingBlocked ||
      currentUi.playing ||
      moveGestureRef.current ||
      currentUi.pending > 0 ||
      currentUi.layerOpacity !== null ||
      currentUi.flowPending ||
      currentUi.draft ||
      currentUi.deletion ||
      currentUi.newProjectOpen ||
      pendingCommitRef.current ||
      activeStrokeRef.current ||
      panGestureRef.current
    )
      return;
    const shouldPickColor =
      (event.button === 0 && currentUi.tool === 'eyedropper') ||
      (event.button === 2 && (currentUi.tool === 'brush' || currentUi.tool === 'eraser'));
    if (event.button !== 0 && event.button !== 1 && !shouldPickColor) return;
    const shouldPan = event.button === 1 || currentUi.tool === 'pan';
    const activeLayer = snapshot.layers.find((layer) => layer.id === currentUi.layerId);
    const reason = layerDrawingBlock(activeLayer);
    if (!shouldPan && !shouldPickColor && reason) {
      currentUi.set({ notice: reason });
      return;
    }
    if (!shouldPan && !shouldPickColor && currentUi.tool === 'brush' && !currentUi.colorId) return;
    const point = pixelAt(event.clientX, event.clientY);
    if (!shouldPan && !point) return;
    event.preventDefault();
    event.currentTarget.focus({ preventScroll: true });
    event.currentTarget.setPointerCapture(event.pointerId);
    if (shouldPan) {
      panGestureRef.current = {
        pointerId: event.pointerId,
        clientX: event.clientX,
        clientY: event.clientY,
        originX: currentUi.pan.x,
        originY: currentUi.pan.y,
        button: event.button,
      };
      setPanning(true);
      setHover(null);
      return;
    }
    if (point && shouldPickColor) {
      releaseCapture(event.pointerId);
      void pickColor(point.x, point.y);
      setHover(point);
      return;
    }
    if (!point || !activeLayer) return;
    if (currentUi.tool === 'wand') {
      releaseCapture(event.pointerId);
      currentUi.set({
        selection: magicWandSelection(
          snapshot,
          activeLayer,
          currentUi.frameId ?? snapshot.frames[0].id,
          point,
          currentUi.wandTolerance,
          currentUi.wandContiguous,
          currentUi.selection,
          event.altKey ? 'subtract' : event.shiftKey ? 'add' : 'replace',
        ),
      });
      setHover(point);
      return;
    }
    if (currentUi.tool === 'move') {
      const frameId = currentUi.frameId ?? snapshot.frames[0].id;
      const cel = celAt(activeLayer, frameId);
      if (!cel) {
        releaseCapture(event.pointerId);
        return;
      }
      const origin = {
        sessionId: snapshot.sessionId,
        revision: snapshot.revision,
        layerId: activeLayer.id,
        frameId,
        x: cel.x,
        y: cel.y,
      };
      moveGestureRef.current = {
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        origin,
      };
      currentUi.set({ movePreview: origin });
      setHover(null);
      return;
    }
    const stroke = beginStroke(
      snapshot.sessionId,
      snapshot.revision,
      activeLayer.id,
      currentUi.frameId ?? snapshot.frames[0].id,
      currentUi.brushSize,
      currentUi.tool === 'eraser' ? null : currentUi.colorId,
      point,
      snapshot.width,
      snapshot.height,
      selectionMatches(currentUi.selection, snapshot, activeLayer.id, currentUi.frameId)
        ? currentUi.selection.bitmap
        : undefined,
    );
    activeStrokeRef.current = stroke;
    strokePointerRef.current = event.pointerId;
    // Publish the start immediately so document actions can settle this stroke first.
    // All subsequent (including coalesced) samples share one publication per frame.
    currentUi.set({ stroke: stroke.snapshot() });
    setHover(point);
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const move = moveGestureRef.current;
    if (move) {
      if (move.pointerId !== event.pointerId) return;
      const dx = Math.round((event.clientX - move.startX) / transformRef.current.zoom),
        dy = Math.round((event.clientY - move.startY) / transformRef.current.zoom);
      useUi.getState().set({
        movePreview: {
          ...move.origin,
          x: Math.max(-8192, Math.min(8192, move.origin.x + dx)),
          y: Math.max(-8192, Math.min(8192, move.origin.y + dy)),
        },
      });
      if ((event.buttons & 1) === 0) void flushStroke();
      return;
    }
    const pan = panGestureRef.current;
    if (pan) {
      if (pan.pointerId !== event.pointerId) return;
      if ((event.buttons & (pan.button === 1 ? 4 : 1)) === 0) {
        finishPan();
        return;
      }
      const nextPan = {
        x: pan.originX + event.clientX - pan.clientX,
        y: pan.originY + event.clientY - pan.clientY,
      };
      useUi.getState().set({ pan: nextPan });
      return;
    }
    if (strokePointerRef.current !== null && strokePointerRef.current !== event.pointerId) return;
    const point = pixelAt(event.clientX, event.clientY);
    setHover(point);
    if (!activeStrokeRef.current) return;
    if ((event.buttons & 1) === 0) {
      void flushStroke();
      return;
    }
    // Preserve intermediate pen/mouse samples when the WebView provides them.
    const native = event.nativeEvent;
    const samples =
      typeof native.getCoalescedEvents === 'function' ? native.getCoalescedEvents() : [];
    for (const sample of samples) appendAt(pixelAt(sample.clientX, sample.clientY));
    appendAt(point);
  };

  const onPointerUp = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (moveGestureRef.current?.pointerId === event.pointerId) {
      onPointerMove(event);
      void flushStroke();
      return;
    }
    if (panGestureRef.current?.pointerId === event.pointerId) {
      finishPan();
      return;
    }
    if (strokePointerRef.current !== event.pointerId) return;
    appendAt(pixelAt(event.clientX, event.clientY));
    void flushStroke();
  };

  const selectedLayer = snapshot?.layers.find((layer) => layer.id === ui.layerId);
  const drawingReason = ui.tool === 'eyedropper' ? null : layerDrawingBlock(selectedLayer);
  const frameId = ui.frameId ?? snapshot?.frames[0].id ?? null;
  const frameIndex = snapshot?.frames.findIndex((f) => f.id === frameId) ?? 0;
  const linked = snapshot ? linkedFrames(snapshot, selectedLayer, frameId) : [];
  const previewSize = ui.stroke?.brushSize ?? ui.brushSize;
  const brushOffset = Math.floor((previewSize - 1) / 2);
  const showCursorGuides =
    !loading &&
    !panning &&
    !ui.playing &&
    !ui.movePreview &&
    !ui.draft &&
    !ui.deletion &&
    !ui.newProjectOpen &&
    !ui.importDialog &&
    !ui.draggingFile &&
    ui.tool !== 'pan';
  const canPreview =
    snapshot &&
    !loading &&
    !ui.draft &&
    !ui.layerOpacity &&
    selectedLayer?.visible &&
    !selectedLayer.locked &&
    selectedLayer.opacity > 0 &&
    !ui.deletion &&
    !ui.newProjectOpen &&
    !panning &&
    !ui.playing &&
    ui.tool !== 'move' &&
    ui.tool !== 'wand' &&
    ui.tool !== 'eyedropper' &&
    (ui.stroke !== null || ui.tool !== 'pan');
  const controlsDisabled = !snapshot || loading;
  const guideLeft = Math.max(0, left);
  const guideTop = Math.max(0, top);
  const surfaceStyle: CSSProperties = {
    position: 'absolute',
    left,
    top,
    width: width * zoom,
    height: height * zoom,
    overflow: 'hidden',
    pointerEvents: 'none',
  };

  return (
    <section className="canvas-panel" aria-label="Pixel canvas">
      <header className="canvas-toolbar">
        <div className="canvas-title">
          <span>Canvas</span>
          <span>{snapshot ? `${width} × ${height} px` : 'No project'}</span>
        </div>
        <div className="canvas-zoom-controls">
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Zoom out"
            title="Zoom out"
            disabled={controlsDisabled || zoom <= MIN_ZOOM}
            onClick={() =>
              void withSettledDocument(() => changeZoom(transformRef.current.zoom / 1.25))
            }
          >
            <Minus aria-hidden="true" />
          </Button>
          <span className="canvas-zoom-label" aria-label="Canvas zoom">
            {snapshot ? `${Math.round(zoom * 100)}%` : '—'}
          </span>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Zoom in"
            title="Zoom in"
            disabled={controlsDisabled || zoom >= MAX_ZOOM}
            onClick={() =>
              void withSettledDocument(() => changeZoom(transformRef.current.zoom * 1.25))
            }
          >
            <Plus aria-hidden="true" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-pressed={ui.zoom === null}
            title="Fit artwork to viewport"
            disabled={controlsDisabled}
            onClick={() => {
              void withSettledDocument(() => {
                ui.set({ zoom: null, pan: { x: 0, y: 0 } });
                setHover(null);
              });
            }}
          >
            Fit
          </Button>
          <Button
            type="button"
            aria-label="Toggle pixel grid"
            aria-pressed={ui.showGrid}
            title={zoom < 4 ? 'Pixel grid appears at 400% zoom or higher' : 'Toggle pixel grid'}
            variant="ghost"
            size="sm"
            disabled={!snapshot}
            onClick={() => ui.set({ showGrid: !ui.showGrid })}
          >
            Grid
          </Button>
          <div className="canvas-backdrop-control">
            <span className="toolbar-separator" aria-hidden="true" />
            <span className="muted">Backdrop</span>
            <div className="segmented inline" role="group" aria-label="Transparency backdrop">
              {(['dark', 'light'] as const).map((backdrop) => (
                <Button
                  key={backdrop}
                  variant="ghost"
                  size="xs"
                  aria-pressed={ui.backdrop === backdrop}
                  onClick={() => ui.set({ backdrop })}
                >
                  {backdrop === 'dark' ? 'Dark' : 'Light'}
                </Button>
              ))}
            </div>
          </div>
        </div>
      </header>
      <div
        ref={viewportRef}
        data-png-drop-target
        className={`canvas-viewport${panning ? ' is-panning' : ''}`}
        role="region"
        aria-label="Artwork editing area"
        tabIndex={0}
        style={{
          cursor: panning
            ? 'grabbing'
            : ui.stroke
              ? 'crosshair'
              : ui.tool === 'pan'
                ? 'grab'
                : loading || ui.draft
                  ? 'default'
                  : drawingReason
                    ? 'not-allowed'
                    : ui.tool === 'move'
                      ? 'move'
                      : 'crosshair',
        }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerLeave={() => {
          setHover(null);
          if (activeStrokeRef.current) appendAt(null);
        }}
        onPointerCancel={(event) => {
          if (
            moveGestureRef.current?.pointerId === event.pointerId ||
            strokePointerRef.current === event.pointerId ||
            panGestureRef.current?.pointerId === event.pointerId
          )
            void flushStroke();
        }}
        onLostPointerCapture={(event) => {
          if (
            moveGestureRef.current?.pointerId === event.pointerId ||
            strokePointerRef.current === event.pointerId ||
            panGestureRef.current?.pointerId === event.pointerId
          )
            void flushStroke();
        }}
        onBlur={() => {
          void flushStroke();
        }}
        onContextMenu={(event) => event.preventDefault()}
      >
        {snapshot && ui.draggingFile && ui.draggingPng && (
          <div className="canvas-import-overlay" role="status">
            <strong>Import PNG into a new layer</strong>
            <span>Fit to canvas · Keep proportions · Current frame</span>
          </div>
        )}
        {snapshot ? (
          <div className="canvas-artwork" data-backdrop={ui.backdrop} style={surfaceStyle}>
            <CanvasBackdrop width={width} height={height} backdrop={ui.backdrop} />
            <canvas
              ref={canvasRef}
              width={width}
              height={height}
              aria-label={`${snapshot.name}, ${width} by ${height} pixels`}
              className="canvas-pixels"
            />
            {ui.showGrid && zoom >= 4 && <PixelGrid width={width} height={height} />}
            {selectionMatches(ui.selection, snapshot, ui.layerId, frameId) && !ui.playing && (
              <svg
                className="canvas-selection"
                viewBox={`0 0 ${width} ${height}`}
                preserveAspectRatio="none"
                aria-hidden="true"
              >
                <path d={ui.selection.outline} vectorEffect="non-scaling-stroke" />
                <path d={ui.selection.outline} vectorEffect="non-scaling-stroke" />
              </svg>
            )}
            {canPreview && hover && (
              <div
                className="brush-cursor"
                aria-hidden="true"
                style={{
                  left: (hover.x - brushOffset) * zoom,
                  top: (hover.y - brushOffset) * zoom,
                  width: previewSize * zoom,
                  height: previewSize * zoom,
                }}
              />
            )}
          </div>
        ) : (
          <div className="canvas-empty">Open or create a project to start drawing.</div>
        )}
        {snapshot && showCursorGuides && hover && (
          <CursorGuides
            left={guideLeft}
            top={guideTop}
            width={Math.max(0, Math.min(size.width, left + width * zoom) - guideLeft)}
            height={Math.max(0, Math.min(size.height, top + height * zoom) - guideTop)}
            x={left + (hover.x + 0.5) * zoom - guideLeft}
            y={top + (hover.y + 0.5) * zoom - guideTop}
          />
        )}
      </div>
      <footer className="canvas-footer">
        <span className="canvas-coordinate" title={drawingReason ?? selectedLayer?.name}>
          {ui.playing
            ? `Playing · ${snapshot ? selectedAnimation(snapshot, ui.animationId).name : ''}`
            : ui.tool === 'eyedropper'
              ? `${hover ? `${hover.x}, ${hover.y} · ` : ''}Picking visible color · frame ${frameIndex + 1}`
              : (drawingReason ?? (
                  <>
                    {hover ? `${hover.x}, ${hover.y} · ` : ''}
                    {ui.tool === 'wand'
                      ? 'Selecting on '
                      : ui.tool === 'eraser'
                        ? 'Erasing on '
                        : 'Drawing on '}
                    {selectedLayer?.name ?? '—'} · frame {frameIndex + 1}
                    {linked.length > 1 ? ` · shared with ${linked.length - 1} frames` : ''}
                  </>
                ))}
        </span>
        <span className="muted">
          {ui.selection
            ? `${ui.selection.pixelCount.toLocaleString()} px selected · Backspace to clear · Esc to deselect`
            : ui.tool === 'wand'
              ? 'Click background · Shift adds · Alt subtracts'
              : 'Wheel to zoom · Middle-drag to pan · Space to play / pause'}
        </span>
        <span className="muted">
          {ui.zoom === null ? 'Fit' : 'Manual'}
          {ui.showGrid && zoom < 4 ? ' · Grid hidden below 400%' : ''}
        </span>
      </footer>
    </section>
  );
}
