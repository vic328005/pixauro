import type { SpritePreview } from './generated/bindings';

export type { ImportSlice as Slice, ImportGroup } from './generated/bindings';
import { commands, type ImportSlice as Slice } from './generated/bindings';
export type Raster = { width: number; height: number; rgba: Uint8ClampedArray };
export type ImportFrame = Raster & {
  id: string;
  row: number;
  x: number;
  y: number;
  foreground: Uint8Array | null;
};

export const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n));

export async function gridSlices(
  width: number,
  height: number,
  rows: number,
  columns: number,
  margin: number,
  gap: number,
): Promise<Slice[]> {
  const result = await commands.gridSlices(width, height, { rows, columns, margin, gap });
  if (result.status === 'error') throw new Error(result.error);
  return result.data;
}

// 下面只用于即时画布预览；最终蒙版应用、偏移与动画组装由 engine 校验并执行。
export function previewFrames(
  preview: SpritePreview,
  slices: Slice[],
  cw: number,
  ch: number,
): ImportFrame[] {
  const rasters = new Map(preview.frames.map((f) => [f.id, f.raster]));
  return slices
    .filter((s) => s.enabled)
    .flatMap((s) => {
      const raster = rasters.get(s.id);
      if (!raster) return [];
      return [
        {
          id: s.id,
          row: s.row,
          width: raster.width,
          height: raster.height,
          rgba: new Uint8ClampedArray(raster.rgba),
          foreground: raster.foreground ? new Uint8Array(raster.foreground) : null,
          x: Math.floor((cw - raster.width) / 2) + s.offsetX,
          y: Math.floor((ch - raster.height) / 2) + s.offsetY,
        },
      ];
    });
}

export function cleanFrame(
  frame: ImportFrame,
  removeBackground: boolean,
  overrides?: Uint8Array,
): ImportFrame {
  const rgba = frame.rgba.slice();
  for (let i = 0; i < frame.width * frame.height; i++) {
    if (
      overrides?.[i] === 1 ||
      (removeBackground && frame.foreground?.[i] === 0 && overrides?.[i] !== 2)
    )
      rgba[i * 4 + 3] = 0;
  }
  return { ...frame, rgba };
}

export function rasterCanvas(raster: Raster): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = raster.width;
  canvas.height = raster.height;
  canvas
    .getContext('2d')!
    .putImageData(
      new ImageData(new Uint8ClampedArray(raster.rgba), raster.width, raster.height),
      0,
      0,
    );
  return canvas;
}
export function rasterUrl(raster: {
  width: number;
  height: number;
  rgba: ArrayLike<number>;
}): string {
  return rasterCanvas({ ...raster, rgba: new Uint8ClampedArray(raster.rgba) }).toDataURL();
}
