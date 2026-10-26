/** A canvas coordinate, never a screen coordinate. */
export type Point = { x: number; y: number };

/** Immutable UI view of one append-only coverage prefix, without copying its pixels. */
export type StrokePreview = Readonly<{
  key: object;
  sessionId: string;
  revision: string;
  layerId: string;
  frameId: string;
  brushSize: number;
  colorId: string | null;
  pixelCount: number;
  forEachPixel: (visit: (index: number) => void, start?: number) => void;
  coverage: () => number[];
}>;

export const BRUSH_SIZES = [1, 2, 4, 8] as const;
const PIXEL_CHUNK_SIZE = 1024;

/** The pointer handler owns this mutable accumulator; React only sees snapshot(). */
export class StrokeAccumulator {
  readonly sessionId: string;
  readonly revision: string;
  readonly layerId: string;
  readonly frameId: string;
  readonly brushSize: number;
  readonly colorId: string | null;
  readonly width: number;
  readonly height: number;
  private readonly key = {};
  private readonly bitmap: Uint8Array;
  // Each pixel is retained once, so storage is bounded by the canvas, not sample count.
  // The append-only prefix also lets the renderer process only new coverage each frame.
  private readonly chunks: Uint32Array[] = [];
  private count = 0;
  private previous: Point | null = null;
  private readonly selection: Uint8Array | undefined;

  constructor(
    sessionId: string,
    revision: string,
    layerId: string,
    frameId: string,
    brushSize: number,
    colorId: string | null,
    width: number,
    height: number,
    selection?: Uint8Array,
  ) {
    if (!(BRUSH_SIZES as readonly number[]).includes(brushSize)) {
      throw new Error('Unsupported brush size');
    }
    this.sessionId = sessionId;
    this.revision = revision;
    this.layerId = layerId;
    this.frameId = frameId;
    this.brushSize = brushSize;
    this.colorId = colorId;
    this.width = width;
    this.height = height;
    this.selection = selection;
    this.bitmap = new Uint8Array(Math.ceil((width * height) / 8));
  }

  get pixelCount(): number {
    return this.count;
  }

  /** Coverage/index storage only; remains bounded even for arbitrarily long strokes. */
  get retainedBytes(): number {
    return this.bitmap.byteLength + this.chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
  }

  private stamp(x: number, y: number): void {
    // An even brush size extends toward the bottom/right, matching the engine.
    const offset = Math.floor((this.brushSize - 1) / 2);
    const right = Math.min(this.width, x - offset + this.brushSize);
    const bottom = Math.min(this.height, y - offset + this.brushSize);
    for (let row = Math.max(0, y - offset); row < bottom; row += 1) {
      for (let column = Math.max(0, x - offset); column < right; column += 1) {
        const index = row * this.width + column;
        const byte = index >>> 3;
        const bit = 1 << (index & 7);
        if (this.selection && (this.selection[byte] & bit) === 0) continue;
        if ((this.bitmap[byte] & bit) !== 0) continue;
        this.bitmap[byte] |= bit;
        const chunkIndex = Math.floor(this.count / PIXEL_CHUNK_SIZE);
        const chunkOffset = this.count % PIXEL_CHUNK_SIZE;
        if (chunkOffset === 0) {
          this.chunks.push(
            new Uint32Array(Math.min(PIXEL_CHUNK_SIZE, this.width * this.height - this.count)),
          );
        }
        this.chunks[chunkIndex][chunkOffset] = index;
        this.count += 1;
      }
    }
  }

  /** null and invalid coordinates interrupt interpolation outside the artwork. */
  append(point: Point | null): boolean {
    if (
      point === null ||
      !Number.isInteger(point.x) ||
      !Number.isInteger(point.y) ||
      point.x < 0 ||
      point.y < 0 ||
      point.x >= this.width ||
      point.y >= this.height
    ) {
      this.previous = null;
      return false;
    }
    const previous = this.previous;
    if (previous?.x === point.x && previous.y === point.y) return false;
    const before = this.count;
    let { x, y } = previous ?? point;
    const dx = Math.abs(point.x - x);
    const dy = -Math.abs(point.y - y);
    const sx = x < point.x ? 1 : -1;
    const sy = y < point.y ? 1 : -1;
    let error = dx + dy;
    for (;;) {
      this.stamp(x, y);
      if (x === point.x && y === point.y) break;
      const twice = 2 * error;
      if (twice >= dy) {
        error += dy;
        x += sx;
      }
      if (twice <= dx) {
        error += dx;
        y += sy;
      }
    }
    this.previous = { x: point.x, y: point.y };
    return this.count !== before;
  }

  snapshot(): StrokePreview {
    const count = this.count;
    const forEachPixel = (visit: (index: number) => void, start = 0) => {
      for (let offset = start; offset < count; offset += 1) {
        visit(this.chunks[Math.floor(offset / PIXEL_CHUNK_SIZE)][offset % PIXEL_CHUNK_SIZE]);
      }
    };
    return Object.freeze({
      key: this.key,
      sessionId: this.sessionId,
      revision: this.revision,
      layerId: this.layerId,
      frameId: this.frameId,
      brushSize: this.brushSize,
      colorId: this.colorId,
      pixelCount: count,
      forEachPixel,
      coverage: () => {
        if (count === this.count) return Array.from(this.bitmap);
        // Older UI snapshots remain exact even after the accumulator advances.
        const bytes = new Array<number>(this.bitmap.length).fill(0);
        forEachPixel((index) => {
          bytes[index >>> 3] |= 1 << (index & 7);
        });
        return bytes;
      },
    });
  }
}

/** Append in place; publication is explicitly batched by the pointer handler. */
export function appendStroke(stroke: StrokeAccumulator, point: Point | null): boolean {
  return stroke.append(point);
}

/** Lock the document guard, target and brush size at pointerdown. */
export function beginStroke(
  sessionId: string,
  revision: string,
  layerId: string,
  frameId: string,
  brushSize: number,
  colorId: string | null,
  point: Point,
  width: number,
  height: number,
  selection?: Uint8Array,
): StrokeAccumulator {
  const stroke = new StrokeAccumulator(
    sessionId,
    revision,
    layerId,
    frameId,
    brushSize,
    colorId,
    width,
    height,
    selection,
  );
  stroke.append(point);
  return stroke;
}
