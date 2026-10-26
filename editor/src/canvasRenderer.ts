import type { StrokePreview } from './brush';
import type { DocumentSnapshot, RgbaColor } from './generated/bindings';
import type { CelMove, ColorDraft, LayerOpacityPreview } from './editorUiState';
import { celAt, framePixels, sameCel } from './animation';

type RenderLayer = { id: string; pixels: Map<number, string>; opacity: number; editable: boolean };
function write(bytes: Uint8ClampedArray, index: number, c: RgbaColor) {
  const offset = index * 4;
  bytes[offset] = c.red;
  bytes[offset + 1] = c.green;
  bytes[offset + 2] = c.blue;
  bytes[offset + 3] = c.alpha;
}
/** Cache current/adjacent sparse pixels once per document or frame; pointer moves patch only coverage. */
export class CanvasRenderer {
  private document: DocumentSnapshot | null = null;
  private draft: ColorDraft | null = null;
  private opacity: LayerOpacityPreview | null = null;
  private move: CelMove | null = null;
  private frame = '';
  private onion = false;
  private context: CanvasRenderingContext2D | null = null;
  private data: ImageData | null = null;
  private colors = new Map<string, RgbaColor>();
  private layers: RenderLayer[] = [];
  private preview: StrokePreview | null = null;
  private composite(index: number, stroke?: StrokePreview): RgbaColor {
    let r = 0,
      g = 0,
      b = 0,
      a = 0;
    for (const layer of this.layers) {
      const id =
        stroke && layer.editable && layer.id === stroke.layerId
          ? stroke.colorId
          : layer.pixels.get(index);
      const c = id ? this.colors.get(id) : undefined;
      if (!c) continue;
      const alpha = (c.alpha / 255) * layer.opacity;
      r = c.red * alpha + r * (1 - alpha);
      g = c.green * alpha + g * (1 - alpha);
      b = c.blue * alpha + b * (1 - alpha);
      a = alpha + a * (1 - alpha);
    }
    return a
      ? {
          red: Math.round(r / a),
          green: Math.round(g / a),
          blue: Math.round(b / a),
          alpha: Math.round(a * 255),
        }
      : { red: 0, green: 0, blue: 0, alpha: 0 };
  }
  render(
    context: CanvasRenderingContext2D,
    doc: DocumentSnapshot,
    draft: ColorDraft | null,
    stroke: StrokePreview | null,
    opacity: LayerOpacityPreview | null,
    frame: string,
    onion: boolean,
    move: CelMove | null,
  ) {
    const rebuild =
      this.document?.sessionId !== doc.sessionId ||
      this.document?.layers !== doc.layers ||
      this.document?.animations !== doc.animations ||
      this.document?.palette !== doc.palette ||
      this.document?.width !== doc.width ||
      this.document?.height !== doc.height ||
      this.context !== context ||
      this.draft !== draft ||
      this.opacity !== opacity ||
      this.frame !== frame ||
      this.onion !== onion ||
      this.move !== move;
    this.document = doc;
    this.context = context;
    if (rebuild) {
      this.draft = draft;
      this.opacity = opacity;
      this.frame = frame;
      this.onion = onion;
      this.move = move;
      this.colors = new Map(doc.palette.map((e) => [e.id, e.color]));
      if (draft?.id) this.colors.set(draft.id, draft.color);
      const current = doc.layers
        .filter((l) => l.visible)
        .map((l) => ({
          id: l.id,
          pixels: framePixels(
            doc,
            l,
            frame,
            move?.sessionId === doc.sessionId && move.layerId === l.id && move.frameId === frame
              ? move
              : undefined,
          ),
          opacity:
            (opacity?.sessionId === doc.sessionId && opacity.layerId === l.id
              ? opacity.opacity
              : l.opacity) / 255,
          editable: true,
        }));
      const ghosts: RenderLayer[] = [];
      const frames =
        doc.animations.find((a) => a.frames.some((f) => f.frameId === frame))?.frames ?? [];
      const index = frames.findIndex((f) => f.frameId === frame);
      if (onion)
        for (const [i, alpha] of [
          [index - 1, 0.3],
          [index + 1, 0.14],
        ]) {
          const adjacent = frames[i];
          if (!adjacent) continue;
          // Composite each adjacent frame first, then apply its onion opacity once.
          const changed = doc.layers.filter(
            (l) => l.visible && !sameCel(celAt(l, adjacent.frameId), celAt(l, frame)),
          );
          const sources = changed.map((l) => ({
            pixels: framePixels(doc, l, adjacent.frameId),
            opacity: l.opacity / 255,
          }));
          const positions = new Set(sources.flatMap((s) => [...s.pixels.keys()]));
          const pixels = new Map<number, string>();
          for (const pos of positions) {
            let r = 0,
              g = 0,
              b = 0,
              a = 0;
            for (const s of sources) {
              const id = s.pixels.get(pos);
              const c = id ? this.colors.get(id) : undefined;
              if (!c) continue;
              const n = (c.alpha / 255) * s.opacity;
              r = c.red * n + r * (1 - n);
              g = c.green * n + g * (1 - n);
              b = c.blue * n + b * (1 - n);
              a = n + a * (1 - n);
            }
            if (a) {
              const id = `onion:${i}:${pos}`;
              this.colors.set(id, { red: r / a, green: g / a, blue: b / a, alpha: a * 255 });
              pixels.set(pos, id);
            }
          }
          ghosts.push({ id: `onion:${i}`, pixels, opacity: alpha, editable: false });
        }
      // Only a full, opaque, unchanged bottom layer is treated as a backdrop.
      // Otherwise ghosts sit below all artwork, including single-layer animations.
      const bottom = doc.layers[0],
        base = current[0];
      const backdrop =
        onion &&
        doc.layers.length > 1 &&
        bottom?.id === base?.id &&
        base.opacity === 1 &&
        base.pixels.size === doc.width * doc.height &&
        [...base.pixels.values()].every((id) => this.colors.get(id)?.alpha === 255) &&
        [index - 1, index + 1].every(
          (i) => !frames[i] || sameCel(celAt(bottom, frames[i].frameId), celAt(bottom, frame)),
        );
      this.layers = backdrop
        ? [current[0], ...ghosts, ...current.slice(1)]
        : [...ghosts, ...current];
      if (!this.data || this.data.width !== doc.width || this.data.height !== doc.height)
        this.data = context.createImageData(doc.width, doc.height);
      for (let i = 0; i < doc.width * doc.height; i++) write(this.data.data, i, this.composite(i));
      this.preview = null;
    }
    const data = this.data;
    if (!data) return;
    const next = stroke?.sessionId === doc.sessionId && stroke.frameId === frame ? stroke : null;
    const previous = this.preview;
    const continuing =
      previous && next && previous.key === next.key && previous.pixelCount <= next.pixelCount;
    let left = doc.width,
      top = doc.height,
      right = -1,
      bottom = -1;
    const patch = (i: number, s?: StrokePreview) => {
      write(data.data, i, this.composite(i, s));
      const x = i % doc.width,
        y = Math.floor(i / doc.width);
      left = Math.min(left, x);
      right = Math.max(right, x);
      top = Math.min(top, y);
      bottom = Math.max(bottom, y);
    };
    if (previous && !continuing) previous.forEachPixel((i) => patch(i));
    if (next) next.forEachPixel((i) => patch(i, next), continuing ? previous.pixelCount : 0);
    this.preview = next;
    context.imageSmoothingEnabled = false;
    if (rebuild) context.putImageData(data, 0, 0);
    else if (right >= left)
      context.putImageData(data, 0, 0, left, top, right - left + 1, bottom - top + 1);
  }
}
