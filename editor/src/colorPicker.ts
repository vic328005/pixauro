import { celAt } from './animation';
import type { DocumentSnapshot, RgbaColor } from './generated/bindings';

/** Sample artwork only, excluding onion skins, checkerboards and tool previews. */
export function pickFrameColor(
  doc: DocumentSnapshot,
  frame: string,
  x: number,
  y: number,
): RgbaColor | null {
  if (x < 0 || y < 0 || x >= doc.width || y >= doc.height) return null;
  const colors = new Map(doc.palette.map((entry) => [entry.id, entry.color]));
  let red = 0,
    green = 0,
    blue = 0,
    alpha = 0;
  for (const layer of doc.layers) {
    if (!layer.visible || layer.opacity === 0) continue;
    const cel = celAt(layer, frame);
    if (!cel) continue;
    const pixel = layer.images
      .find((image) => image.id === cel.imageId)
      ?.pixels.find(([px, py]) => px === x - cel.x && py === y - cel.y);
    const color = pixel ? colors.get(pixel[2]) : undefined;
    if (!color) continue;
    const a = (color.alpha * layer.opacity) / 65025;
    red = color.red * a + red * (1 - a);
    green = color.green * a + green * (1 - a);
    blue = color.blue * a + blue * (1 - a);
    alpha = a + alpha * (1 - a);
  }
  if (Math.round(alpha * 255) === 0) return null;
  return {
    red: Math.round(red / alpha),
    green: Math.round(green / alpha),
    blue: Math.round(blue / alpha),
    alpha: Math.round(alpha * 255),
  };
}
