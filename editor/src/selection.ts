import { framePixels } from './animation';
import type { Point } from './brush';
import type { DocumentSnapshot, LayerSnapshot, RgbaColor } from './generated/bindings';

/** Canvas-space selection; never part of the saved document or command history. */
export type PixelSelection = {
  sessionId: string;
  layerId: string;
  frameId: string;
  width: number;
  height: number;
  bitmap: Uint8Array;
  pixelCount: number;
  outline: string;
};

export function containsPixel(bitmap: Uint8Array, index: number): boolean {
  return (bitmap[index >>> 3] & (1 << (index & 7))) !== 0;
}

export function selectionMatches(
  selection: PixelSelection | null,
  doc: DocumentSnapshot,
  layerId: string | null,
  frameId: string | null,
): selection is PixelSelection {
  return (
    selection !== null &&
    selection.sessionId === doc.sessionId &&
    selection.layerId === layerId &&
    selection.frameId === frameId &&
    selection.width === doc.width &&
    selection.height === doc.height
  );
}

function outlineFor(bitmap: Uint8Array, width: number, height: number): string {
  const edges: string[] = [];
  const selected = (x: number, y: number) =>
    x >= 0 && y >= 0 && x < width && y < height && containsPixel(bitmap, y * width + x);
  // Merge collinear pixel edges so a solid background needs only a few SVG segments.
  for (let y = 0; y <= height; y++) {
    let start = -1;
    for (let x = 0; x <= width; x++) {
      const edge = x < width && selected(x, y - 1) !== selected(x, y);
      if (edge && start < 0) start = x;
      if (!edge && start >= 0) {
        edges.push(`M${start} ${y}H${x}`);
        start = -1;
      }
    }
  }
  for (let x = 0; x <= width; x++) {
    let start = -1;
    for (let y = 0; y <= height; y++) {
      const edge = y < height && selected(x - 1, y) !== selected(x, y);
      if (edge && start < 0) start = y;
      if (!edge && start >= 0) {
        edges.push(`M${x} ${start}V${y}`);
        start = -1;
      }
    }
  }
  return edges.join('');
}

export function magicWandSelection(
  doc: DocumentSnapshot,
  layer: LayerSnapshot,
  frameId: string,
  point: Point,
  tolerance: number,
  contiguous: boolean,
  previous: PixelSelection | null,
  mode: 'replace' | 'add' | 'subtract',
): PixelSelection | null {
  const { width, height } = doc;
  const pixels = framePixels(doc, layer, frameId);
  const colors = new Map(doc.palette.map((entry) => [entry.id, entry.color]));
  const transparent: RgbaColor = { red: 0, green: 0, blue: 0, alpha: 0 };
  const seed = point.y * width + point.x;
  const sample = colors.get(pixels.get(seed) ?? '') ?? transparent;
  const matches = (color: RgbaColor) => {
    if (sample.alpha === 0 || color.alpha === 0) return sample.alpha === color.alpha;
    return (
      Math.max(
        Math.abs(sample.red - color.red),
        Math.abs(sample.green - color.green),
        Math.abs(sample.blue - color.blue),
        Math.abs(sample.alpha - color.alpha),
      ) <= tolerance
    );
  };
  // Match actual RGBA values, including duplicate palette entries with different IDs.
  const matchingIds = new Set(doc.palette.filter((entry) => matches(entry.color)).map((e) => e.id));
  const area = width * height;
  const candidates = new Uint8Array(Math.ceil(area / 8));
  const matchesEmpty = matches(transparent);
  for (let i = 0; i < area; i++) {
    const id = pixels.get(i);
    if (id ? matchingIds.has(id) : matchesEmpty) candidates[i >>> 3] |= 1 << (i & 7);
  }
  const region = contiguous ? new Uint8Array(candidates.length) : candidates;
  if (contiguous) {
    // Iterative scanline flood fill: four-connected, no recursive call-stack limit.
    const stack = [seed];
    while (stack.length) {
      const index = stack.pop()!;
      if (!containsPixel(candidates, index)) continue;
      const rowStart = Math.floor(index / width) * width;
      let left = index;
      let right = index;
      while (left > rowStart && containsPixel(candidates, left - 1)) left--;
      while (right + 1 < rowStart + width && containsPixel(candidates, right + 1)) right++;
      for (let i = left; i <= right; i++) {
        const bit = 1 << (i & 7);
        candidates[i >>> 3] &= ~bit;
        region[i >>> 3] |= bit;
      }
      for (const offset of [-width, width]) {
        let inRun = false;
        for (let i = left + offset; i <= right + offset; i++) {
          const eligible = i >= 0 && i < area && containsPixel(candidates, i);
          if (eligible && !inRun) stack.push(i);
          inRun = eligible;
        }
      }
    }
  }
  const prior = selectionMatches(previous, doc, layer.id, frameId) ? previous.bitmap : null;
  if (mode !== 'replace') {
    for (let i = 0; i < region.length; i++) {
      region[i] = mode === 'add' ? region[i] | (prior?.[i] ?? 0) : (prior?.[i] ?? 0) & ~region[i];
    }
  }
  let pixelCount = 0;
  for (const byte of region) {
    let bits = byte;
    while (bits) {
      bits &= bits - 1;
      pixelCount++;
    }
  }
  if (!pixelCount) return null;
  return {
    sessionId: doc.sessionId,
    layerId: layer.id,
    frameId,
    width,
    height,
    bitmap: region,
    pixelCount,
    outline: outlineFor(region, width, height),
  };
}
