import { useEffect, useRef, useState, type ReactNode, type PointerEvent } from 'react';
import { clamp, rasterCanvas, type ImportFrame, type Slice } from '../spriteImport';
import { Button } from './ui/button';

export function ImportViewport({
  width,
  height,
  children,
}: {
  width: number;
  height: number;
  children: (scale: number) => ReactNode;
}) {
  const container = useRef<HTMLDivElement>(null);
  const pan = useRef<{ x: number; y: number; left: number; top: number } | null>(null);
  const [bounds, setBounds] = useState({ width: 600, height: 480 });
  const [zoom, setZoom] = useState(1);
  useEffect(() => {
    const element = container.current!;
    const observer = new ResizeObserver(([entry]) => setBounds(entry.contentRect));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const scale = Math.min((bounds.width - 48) / width, (bounds.height - 48) / height) * zoom;
  return (
    <div className="import-viewport-wrap">
      <div className="import-viewport-tools">
        <Button
          variant="ghost"
          onClick={() => setZoom((z) => clamp(z / 1.5, 0.25, 16))}
          aria-label="Zoom out"
        >
          −
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            setZoom(1);
            container.current?.scrollTo(0, 0);
          }}
        >
          Fit
        </Button>
        <Button
          variant="ghost"
          onClick={() => setZoom((z) => clamp(z * 1.5, 0.25, 16))}
          aria-label="Zoom in"
        >
          +
        </Button>
        <span className="muted">{Math.round(scale * 100)}% · Middle drag to pan</span>
      </div>
      <div
        ref={container}
        className="import-viewport"
        onPointerDown={(e) => {
          if (e.button !== 1) return;
          e.preventDefault();
          e.currentTarget.setPointerCapture(e.pointerId);
          pan.current = {
            x: e.clientX,
            y: e.clientY,
            left: e.currentTarget.scrollLeft,
            top: e.currentTarget.scrollTop,
          };
        }}
        onPointerMove={(e) => {
          if (pan.current)
            e.currentTarget.scrollTo(
              pan.current.left + pan.current.x - e.clientX,
              pan.current.top + pan.current.y - e.clientY,
            );
        }}
        onPointerUp={() => {
          pan.current = null;
        }}
        onPointerCancel={() => {
          pan.current = null;
        }}
      >
        <div
          className="import-surface"
          style={{ minWidth: width * scale + 48, minHeight: height * scale + 48 }}
        >
          {children(Math.max(0.01, scale))}
        </div>
      </div>
    </div>
  );
}

const sliceRowColors = ['#ffc47a', '#d4adff', '#b4df87', '#ffadc5'];
const sliceRowDashes = [
  [8, 4],
  [3, 3],
  [10, 3, 2, 3],
  [12, 4],
];

type MoveAxis = 'x' | 'y' | 'free';
type SliceDrag = {
  slice: Slice;
  x: number;
  y: number;
  corner: string | null;
  axis: MoveAxis;
  pointerId: number;
};

/** Keep handles a constant screen size as the source image zooms. */
function SliceMoveGizmo({
  slice,
  scale,
  active,
}: {
  slice: Slice;
  scale: number;
  active: MoveAxis | null;
}) {
  return (
    <g
      className="import-move-gizmo"
      transform={`translate(${slice.x + slice.width / 2} ${slice.y + slice.height / 2}) scale(${1 / scale})`}
    >
      {(['x', 'y'] as const).map((axis) => {
        const horizontal = axis === 'x';
        const path = horizontal ? 'M12 0H54M46 -6L54 0L46 6' : 'M0 12V54M-6 46L0 54L6 46';
        return (
          <g
            key={axis}
            className="import-move-axis"
            data-slice={slice.id}
            data-axis={axis}
            data-active={active === axis}
            style={{ cursor: horizontal ? 'ew-resize' : 'ns-resize' }}
          >
            <title>{horizontal ? 'Move horizontally · X only' : 'Move vertically · Y only'}</title>
            <path
              d={path}
              fill="none"
              stroke="transparent"
              strokeWidth={22}
              pointerEvents="stroke"
            />
            <path d={path} className="import-move-axis-outline" pointerEvents="none" />
            <path d={path} className="import-move-axis-line" pointerEvents="none" />
            <text x={horizontal ? 38 : 13} y={horizontal ? -12 : 40} pointerEvents="none">
              {axis.toUpperCase()}
            </text>
          </g>
        );
      })}
      <rect
        className="import-move-center"
        data-slice={slice.id}
        data-axis="free"
        data-active={active === 'free'}
        x={-7}
        y={-7}
        width={14}
        height={14}
      >
        <title>Move freely · X and Y</title>
      </rect>
    </g>
  );
}

export function SliceCanvas({
  width,
  height,
  url,
  slices,
  selected,
  onSelect,
  onChange,
  editable,
}: {
  width: number;
  height: number;
  url: string;
  slices: Slice[];
  selected: string;
  onSelect: (id: string) => void;
  onChange: (slice: Slice) => void;
  editable: boolean;
}) {
  const drag = useRef<SliceDrag | null>(null);
  const [activeAxis, setActiveAxis] = useState<MoveAxis | null>(null);
  const finishDrag = () => {
    drag.current = null;
    setActiveAxis(null);
  };
  const point = (e: PointerEvent<SVGSVGElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    return {
      x: ((e.clientX - rect.left) * width) / rect.width,
      y: ((e.clientY - rect.top) * height) / rect.height,
    };
  };
  return (
    <ImportViewport width={width} height={height}>
      {(scale) => (
        <svg
          className="import-sheet"
          width={width * scale}
          height={height * scale}
          viewBox={`0 0 ${width} ${height}`}
          aria-label="Sprite sheet slices"
          onPointerDown={(e) => {
            if (e.button !== 0 || drag.current) return;
            const element = (e.target as SVGElement).closest<SVGElement>('[data-slice]');
            const id = element?.dataset.slice;
            const slice = slices.find((s) => s.id === id);
            if (!slice || !element) return;
            e.preventDefault();
            onSelect(slice.id);
            if (!editable) return;
            e.currentTarget.setPointerCapture(e.pointerId);
            const axis =
              element.dataset.axis === 'x' || element.dataset.axis === 'y'
                ? element.dataset.axis
                : 'free';
            const corner = element.dataset.corner ?? null;
            drag.current = { slice, ...point(e), corner, axis, pointerId: e.pointerId };
            setActiveAxis(corner ? null : axis);
          }}
          onPointerMove={(e) => {
            const d = drag.current;
            if (!d || d.pointerId !== e.pointerId) return;
            const p = point(e),
              dx = Math.round(p.x - d.x),
              dy = Math.round(p.y - d.y);
            const s = d.slice;
            if (!d.corner)
              onChange({
                ...s,
                x: d.axis === 'y' ? s.x : clamp(s.x + dx, 0, width - s.width),
                y: d.axis === 'x' ? s.y : clamp(s.y + dy, 0, height - s.height),
              });
            else {
              let left = s.x,
                top = s.y,
                right = s.x + s.width,
                bottom = s.y + s.height;
              if (d.corner.includes('w')) left = clamp(left + dx, 0, right - 1);
              else right = clamp(right + dx, left + 1, width);
              if (d.corner.includes('n')) top = clamp(top + dy, 0, bottom - 1);
              else bottom = clamp(bottom + dy, top + 1, height);
              onChange({ ...s, x: left, y: top, width: right - left, height: bottom - top });
            }
          }}
          onPointerUp={(e) => {
            if (drag.current?.pointerId === e.pointerId) finishDrag();
          }}
          onPointerCancel={(e) => {
            if (drag.current?.pointerId === e.pointerId) finishDrag();
          }}
          onLostPointerCapture={(e) => {
            if (drag.current?.pointerId === e.pointerId) finishDrag();
          }}
        >
          <image
            href={url}
            width={width}
            height={height}
            style={{ imageRendering: editable ? 'auto' : 'pixelated' }}
          />
          {[...slices]
            .sort((a, b) => Number(a.id === selected) - Number(b.id === selected))
            .map((s) => {
              const isSelected = selected === s.id;
              const frameNumber =
                slices.filter((item) => item.row === s.row).findIndex((item) => item.id === s.id) +
                1;
              const label = `${isSelected ? 'Selected · ' : ''}${s.row + 1} · ${frameNumber}${s.enabled ? '' : ' ×'}${s.autoLocked ? ' · L' : ''}`;
              const labelWidth = label.length * 7 + 16;
              const dash = isSelected
                ? undefined
                : (s.enabled ? sliceRowDashes[s.row % sliceRowDashes.length] : [2, 5])
                    .map((n) => n / scale)
                    .join(' ');
              return (
                <g
                  key={s.id}
                  className="import-slice"
                  data-selected={isSelected}
                  opacity={s.enabled || isSelected ? 1 : 0.5}
                  style={{
                    color: isSelected
                      ? 'var(--import-selected-color)'
                      : s.enabled
                        ? sliceRowColors[s.row % sliceRowColors.length]
                        : '#c5c5ca',
                  }}
                >
                  <rect
                    className="import-slice-outline"
                    x={s.x}
                    y={s.y}
                    width={s.width}
                    height={s.height}
                    fill="none"
                    stroke="#101114"
                    strokeWidth={(isSelected ? 7 : 4) / scale}
                    strokeDasharray={dash}
                    pointerEvents="none"
                  />
                  <rect
                    data-slice={s.id}
                    x={s.x}
                    y={s.y}
                    width={s.width}
                    height={s.height}
                    fill="transparent"
                    stroke="currentColor"
                    strokeWidth={(isSelected ? 3 : 1.5) / scale}
                    strokeDasharray={dash}
                    style={{ cursor: editable ? 'move' : 'pointer' }}
                  />
                  <g
                    className="import-slice-label"
                    data-slice={s.id}
                    transform={`translate(${s.x + 10 / scale} ${s.y + 10 / scale}) scale(${1 / scale})`}
                  >
                    <rect width={labelWidth} height={22} rx={3} />
                    <text x={8} y={15} pointerEvents="none">
                      {label}
                    </text>
                  </g>
                  {editable && isSelected && (
                    <SliceMoveGizmo slice={s} scale={scale} active={activeAxis} />
                  )}
                  {editable &&
                    isSelected &&
                    ['nw', 'ne', 'sw', 'se'].map((corner) => (
                      <g
                        key={corner}
                        className="import-slice-handle"
                        data-slice={s.id}
                        data-corner={corner}
                        transform={`translate(${corner.includes('w') ? s.x : s.x + s.width} ${corner.includes('n') ? s.y : s.y + s.height}) scale(${1 / scale})`}
                        style={{ cursor: `${corner}-resize` }}
                      >
                        <rect
                          className="import-slice-handle-base"
                          x={-9}
                          y={-9}
                          width={18}
                          height={18}
                          rx={2}
                          pointerEvents="none"
                        />
                        <rect
                          className="import-slice-handle-core"
                          x={-6}
                          y={-6}
                          width={12}
                          height={12}
                          rx={1}
                          pointerEvents="none"
                        />
                        <rect
                          x={-12}
                          y={-12}
                          width={24}
                          height={24}
                          fill="transparent"
                          pointerEvents="all"
                        />
                      </g>
                    ))}
                </g>
              );
            })}
        </svg>
      )}
    </ImportViewport>
  );
}

export function FrameCanvas({
  frame,
  width,
  height,
  tool,
  onPaint,
  onMove,
  baseline,
  onStart,
}: {
  frame: ImportFrame;
  width: number;
  height: number;
  tool: string;
  onPaint: (x: number, y: number, last: { x: number; y: number } | null) => void;
  onMove: (dx: number, dy: number) => void;
  baseline: number | null;
  onStart: () => void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const drag = useRef<{ last: { x: number; y: number } } | null>(null);
  useEffect(() => {
    const ctx = canvas.current!.getContext('2d')!;
    ctx.clearRect(0, 0, width, height);
    ctx.drawImage(rasterCanvas(frame), frame.x, frame.y);
  }, [frame, width, height]);
  const point = (e: PointerEvent<HTMLCanvasElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    return {
      x: Math.floor(((e.clientX - rect.left) * width) / rect.width),
      y: Math.floor(((e.clientY - rect.top) * height) / rect.height),
    };
  };
  return (
    <ImportViewport width={width} height={height}>
      {(scale) => (
        <div
          className="import-frame-surface"
          style={{ width: width * scale, height: height * scale }}
        >
          <canvas
            ref={canvas}
            width={width}
            height={height}
            style={{
              width: width * scale,
              height: height * scale,
              cursor: tool === 'move' ? 'move' : 'crosshair',
            }}
            aria-label="Animation frame preview"
            onPointerDown={(e) => {
              if (e.button !== 0 || tool === 'preview') return;
              e.preventDefault();
              e.currentTarget.setPointerCapture(e.pointerId);
              const p = point(e);
              onStart();
              drag.current = { last: p };
              if (tool !== 'move') onPaint(p.x - frame.x, p.y - frame.y, null);
            }}
            onPointerMove={(e) => {
              const d = drag.current;
              if (!d) return;
              const p = point(e);
              if (tool === 'move') onMove(p.x - d.last.x, p.y - d.last.y);
              else if (tool === 'erase' || tool === 'restore')
                onPaint(p.x - frame.x, p.y - frame.y, {
                  x: d.last.x - frame.x,
                  y: d.last.y - frame.y,
                });
              d.last = p;
            }}
            onPointerUp={() => {
              drag.current = null;
            }}
            onPointerCancel={() => {
              drag.current = null;
            }}
          />
          {baseline !== null && (
            <div className="import-baseline" style={{ top: baseline * scale }} />
          )}
        </div>
      )}
    </ImportViewport>
  );
}
