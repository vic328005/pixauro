import { useEffect } from 'react';
import type { CelSnapshot, DocumentSnapshot, LayerSnapshot } from './generated/bindings';
import { useUi } from './editorUiState';
import { startAnimationPlayback } from './animationTiming';

/** An animation view exposes only its own layers and ordered frames. */
const animationViews = new WeakMap<DocumentSnapshot, Map<string, DocumentSnapshot>>();
export function selectedAnimation(doc: DocumentSnapshot, id: string | null) {
  return doc.animations.find((a) => a.id === id) ?? doc.animations[0];
}
export function animationDocument(doc: DocumentSnapshot, id: string | null): DocumentSnapshot {
  const animation = selectedAnimation(doc, id);
  let views = animationViews.get(doc);
  if (!views) {
    views = new Map();
    animationViews.set(doc, views);
  }
  let view = views.get(animation.id);
  if (!view) {
    view = {
      ...doc,
      frames: animation.frames.map((f) => ({ id: f.frameId })),
      layers: doc.layers.filter((layer) => layer.animationId === animation.id),
    };
    views.set(animation.id, view);
  }
  return view;
}

const celIndexes = new WeakMap<LayerSnapshot['cels'], Map<string, CelSnapshot>>();
const groupIndexes = new WeakMap<
  LayerSnapshot['cels'],
  WeakMap<DocumentSnapshot['frames'], Map<string, string[]>>
>();
export function celAt(layer: LayerSnapshot | undefined, frameId: string | null) {
  if (!layer || !frameId) return undefined;
  let index = celIndexes.get(layer.cels);
  if (!index) {
    index = new Map(layer.cels.map((c) => [c.frameId, c]));
    celIndexes.set(layer.cels, index);
  }
  return index.get(frameId);
}
export function linkedFrames(
  doc: DocumentSnapshot,
  layer: LayerSnapshot | undefined,
  frame: string | null,
) {
  const cel = celAt(layer, frame);
  if (!layer || !cel) return [];
  let byFrames = groupIndexes.get(layer.cels);
  if (!byFrames) {
    byFrames = new WeakMap();
    groupIndexes.set(layer.cels, byFrames);
  }
  let groups = byFrames.get(doc.frames);
  if (!groups) {
    groups = new Map();
    for (const f of doc.frames) {
      const c = celAt(layer, f.id);
      if (c) {
        const list = groups.get(c.imageId) ?? [];
        list.push(f.id);
        groups.set(c.imageId, list);
      }
    }
    byFrames.set(doc.frames, groups);
  }
  return groups.get(cel.imageId) ?? [];
}
export function frameNumbers(doc: DocumentSnapshot, ids: string[]) {
  return doc.frames.flatMap((f, i) => (ids.includes(f.id) ? [i + 1] : [])).join(', ');
}
export function sameCel(a: CelSnapshot | undefined, b: CelSnapshot | undefined) {
  return a?.imageId === b?.imageId && a?.x === b?.x && a?.y === b?.y;
}
export function framePixels(
  doc: DocumentSnapshot,
  layer: LayerSnapshot,
  frame: string,
  offset?: { x: number; y: number },
) {
  const cel = celAt(layer, frame);
  const pixels = new Map<number, string>();
  if (!cel) return pixels;
  for (const [px, py, color] of layer.images.find((i) => i.id === cel.imageId)?.pixels ?? []) {
    const x = px + (offset?.x ?? cel.x),
      y = py + (offset?.y ?? cel.y);
    if (x >= 0 && y >= 0 && x < doc.width && y < doc.height) pixels.set(y * doc.width + x, color);
  }
  return pixels;
}
export function stepFrame(document: DocumentSnapshot, step: number | 'first' | 'last') {
  const doc = animationDocument(document, useUi.getState().animationId);
  let i = doc.frames.findIndex((f) => f.id === useUi.getState().frameId);
  if (step === 'first') i = 0;
  else if (step === 'last') i = doc.frames.length - 1;
  else if (i < 0) i = 0;
  else i = (i + step + doc.frames.length) % doc.frames.length;
  const id = doc.frames[i].id;
  const ui = useUi.getState();
  const target = ui.inspectionTarget;
  ui.set({
    frameId: id,
    selectedFrames: [id],
    anchorFrame: id,
    // Explicit stepping follows a frame/cel inspection; playback never changes it.
    inspectionTarget:
      target?.kind === 'frames'
        ? { kind: 'frames', ids: [id] }
        : target?.kind === 'cels' && ui.layerId
          ? { kind: 'cels', layerId: ui.layerId, frameIds: [id] }
          : target,
  });
}
export function useAnimationPlayback(doc: DocumentSnapshot | null) {
  const playing = useUi((s) => s.playing);
  const animationId = useUi((s) => s.animationId);
  useEffect(() => {
    if (!doc || !playing) return;
    const animation = selectedAnimation(doc, animationId);
    const frames = animation.frames;
    let frame = frames.findIndex((f) => f.frameId === useUi.getState().frameId);
    if (frame < 0) frame = 0;
    useUi.getState().set({ frameId: frames[frame].frameId });
    const cancelPlayback = startAnimationPlayback(animation.fps, (steps) => {
      if (!useUi.getState().playing) return;
      frame = (frame + steps) % frames.length;
      if (useUi.getState().frameId !== frames[frame].frameId)
        useUi.getState().set({ frameId: frames[frame].frameId });
    });
    const stop = () => {
      useUi.getState().set({ playing: false });
    };
    const visibility = () => {
      if (document.hidden) stop();
    };
    window.addEventListener('blur', stop);
    document.addEventListener('visibilitychange', visibility);
    return () => {
      cancelPlayback();
      window.removeEventListener('blur', stop);
      document.removeEventListener('visibilitychange', visibility);
    };
  }, [doc, playing, animationId]);
}
