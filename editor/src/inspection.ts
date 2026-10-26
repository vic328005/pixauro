import type { DocumentSnapshot } from './generated/bindings';

/** Inspection is independent of the active drawing layer, color and playback frame. */
export type InspectionTarget =
  | { kind: 'animation'; id: string }
  | { kind: 'layer'; id: string }
  | { kind: 'frames'; ids: string[] }
  | { kind: 'cels'; layerId: string; frameIds: string[] };

/** Retain live identities through edits/undo, without silently inspecting a neighbor. */
export function reconcileInspection(
  target: InspectionTarget | null,
  doc: DocumentSnapshot,
): InspectionTarget | null {
  if (!target) return null;
  switch (target.kind) {
    case 'animation':
      return doc.animations.some((a) => a.id === target.id) ? target : null;
    case 'layer':
      return doc.layers.some((layer) => layer.id === target.id) ? target : null;
    case 'frames': {
      const ids = target.ids.filter((id) => doc.frames.some((frame) => frame.id === id));
      return !ids.length ? null : ids.length === target.ids.length ? target : { ...target, ids };
    }
    case 'cels': {
      if (!doc.layers.some((layer) => layer.id === target.layerId)) return null;
      const frameIds = target.frameIds.filter((id) => doc.frames.some((frame) => frame.id === id));
      return !frameIds.length
        ? null
        : frameIds.length === target.frameIds.length
          ? target
          : { ...target, frameIds };
    }
  }
}
