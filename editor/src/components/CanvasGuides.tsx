import { memo, useLayoutEffect, useMemo, useRef } from 'react';

type Dimensions = { width: number; height: number };

export function CursorGuides({
  width,
  height,
  left,
  top,
  x,
  y,
}: Dimensions & { left: number; top: number; x: number; y: number }) {
  return (
    <div className="canvas-cursor-guides" style={{ left, top, width, height }} aria-hidden="true">
      <span className="canvas-cursor-guide-horizontal" style={{ top: y - 1.5 }} />
      <span className="canvas-cursor-guide-vertical" style={{ left: x - 1.5 }} />
    </div>
  );
}

export const CanvasBackdrop = memo(function CanvasBackdrop({
  width,
  height,
  backdrop,
}: Dimensions & { backdrop: 'dark' | 'light' }) {
  const ref = useRef<HTMLCanvasElement>(null);

  useLayoutEffect(() => {
    const canvas = ref.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) return;
    const style = getComputedStyle(canvas);
    const tile = document.createElement('canvas');
    tile.width = tile.height = 4;
    const tileContext = tile.getContext('2d');
    if (!tileContext) return;
    tileContext.fillStyle = style.getPropertyValue('--checker-a').trim();
    tileContext.fillRect(0, 0, 4, 4);
    tileContext.fillStyle = style.getPropertyValue('--checker-b').trim();
    tileContext.fillRect(2, 0, 2, 2);
    tileContext.fillRect(0, 2, 2, 2);
    const pattern = context.createPattern(tile, 'repeat');
    if (!pattern) return;
    // Tile only at integer artwork coordinates, then scale the whole bitmap just
    // like the artwork. Fractional CSS background tiles accumulate rounding drift.
    context.imageSmoothingEnabled = false;
    context.fillStyle = pattern;
    context.fillRect(0, 0, width, height);
  }, [width, height, backdrop]);

  return (
    <canvas ref={ref} className="canvas-checker" width={width} height={height} aria-hidden="true" />
  );
});

export const PixelGrid = memo(function PixelGrid({ width, height }: Dimensions) {
  const path = useMemo(() => {
    const lines: string[] = [];
    for (let x = 0; x <= width; x++) lines.push(`M${x} 0V${height}`);
    for (let y = 0; y <= height; y++) lines.push(`M0 ${y}H${width}`);
    return lines.join('');
  }, [width, height]);

  return (
    <svg
      className="canvas-grid"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      aria-hidden="true"
      focusable="false"
    >
      {/* Every edge is independently transformed from the same artwork origin. */}
      <path d={path} vectorEffect="non-scaling-stroke" />
    </svg>
  );
});
