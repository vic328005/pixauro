import { useEffect, useRef, useState, type PointerEvent } from 'react';
import { useEditor } from '../editorController';

type DropTarget = { id: string; edge: 'before' | 'after'; position: number };
type Gesture = {
  id: string;
  pointerId: number;
  startY: number;
  x: number;
  y: number;
  active: boolean;
  session: string;
  revision: string;
  source: HTMLButtonElement;
};

export function useAnimationReorder(enabled: boolean) {
  const { document: doc, getDocument, editAnimation } = useEditor();
  const listRef = useRef<HTMLDivElement>(null);
  const gestureRef = useRef<Gesture | null>(null);
  const suppressClick = useRef(false);
  const [dragged, setDragged] = useState<string | null>(null);
  const [drop, setDrop] = useState<DropTarget | null>(null);

  useEffect(() => {
    let tick: number | null = null;
    const cancel = () => {
      const gesture = gestureRef.current;
      gestureRef.current = null;
      if (tick !== null) cancelAnimationFrame(tick);
      tick = null;
      if (gesture?.source.hasPointerCapture(gesture.pointerId))
        gesture.source.releasePointerCapture(gesture.pointerId);
      setDragged(null);
      setDrop(null);
    };
    const valid = (gesture: Gesture) => {
      const current = getDocument();
      return (
        enabled && current?.sessionId === gesture.session && current.revision === gesture.revision
      );
    };
    const targetAt = (gesture: Gesture): DropTarget | null => {
      const list = listRef.current;
      if (!list) return null;
      const bounds = list.getBoundingClientRect();
      if (
        gesture.x < bounds.left ||
        gesture.x > bounds.right ||
        gesture.y < bounds.top ||
        gesture.y > bounds.bottom
      )
        return null;
      const rows = [...list.querySelectorAll<HTMLElement>('[data-animation-id]')].filter(
        (row) => row.dataset.animationId !== gesture.id,
      );
      for (const [position, row] of rows.entries()) {
        const rect = row.getBoundingClientRect();
        if (gesture.y < rect.top + rect.height / 2)
          return { id: row.dataset.animationId!, edge: 'before', position };
      }
      const last = rows.at(-1);
      return last ? { id: last.dataset.animationId!, edge: 'after', position: rows.length } : null;
    };
    const update = () => {
      const gesture = gestureRef.current;
      const list = listRef.current;
      if (!gesture?.active || !list || !valid(gesture)) {
        cancel();
        return;
      }
      const bounds = list.getBoundingClientRect();
      if (gesture.x >= bounds.left && gesture.x <= bounds.right) {
        if (gesture.y >= bounds.top && gesture.y < bounds.top + 24) list.scrollTop -= 8;
        else if (gesture.y <= bounds.bottom && gesture.y > bounds.bottom - 24) list.scrollTop += 8;
      }
      const target = targetAt(gesture);
      const from = getDocument()?.animations.findIndex((a) => a.id === gesture.id);
      const next = target?.position === from ? null : target;
      setDrop((previous) =>
        previous?.id === next?.id && previous?.edge === next?.edge ? previous : next,
      );
      tick = requestAnimationFrame(update);
    };
    const move = (event: globalThis.PointerEvent) => {
      const gesture = gestureRef.current;
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      if (!valid(gesture)) {
        cancel();
        return;
      }
      gesture.x = event.clientX;
      gesture.y = event.clientY;
      if (!gesture.active && Math.abs(gesture.y - gesture.startY) >= 4) {
        gesture.active = true;
        suppressClick.current = true;
        setDragged(gesture.id);
        update();
      }
      if (gesture.active) event.preventDefault();
    };
    const up = (event: globalThis.PointerEvent) => {
      const gesture = gestureRef.current;
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      gesture.x = event.clientX;
      gesture.y = event.clientY;
      const target = gesture.active && valid(gesture) ? targetAt(gesture) : null;
      cancel();
      const from = getDocument()?.animations.findIndex((a) => a.id === gesture.id);
      if (target && from !== undefined && from >= 0 && target.position !== from)
        void editAnimation({ kind: 'moveAnimation', id: gesture.id, position: target.position });
    };
    const pointerCancel = (event: globalThis.PointerEvent) => {
      if (gestureRef.current?.pointerId === event.pointerId) cancel();
    };
    const keyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && gestureRef.current) {
        event.preventDefault();
        cancel();
      }
    };
    window.addEventListener('pointermove', move, { passive: false });
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', pointerCancel);
    window.addEventListener('lostpointercapture', pointerCancel);
    window.addEventListener('blur', cancel);
    window.addEventListener('keydown', keyDown);
    return () => {
      cancel();
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', pointerCancel);
      window.removeEventListener('lostpointercapture', pointerCancel);
      window.removeEventListener('blur', cancel);
      window.removeEventListener('keydown', keyDown);
    };
  }, [enabled, doc?.sessionId, doc?.revision, getDocument, editAnimation]);

  const startDrag = (event: PointerEvent<HTMLButtonElement>, id: string) => {
    suppressClick.current = false;
    if (!enabled || !doc || event.button !== 0 || !event.isPrimary) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    gestureRef.current = {
      id,
      pointerId: event.pointerId,
      startY: event.clientY,
      x: event.clientX,
      y: event.clientY,
      active: false,
      session: doc.sessionId,
      revision: doc.revision,
      source: event.currentTarget,
    };
  };
  const blockClick = () => {
    const blocked = suppressClick.current;
    suppressClick.current = false;
    return blocked;
  };
  return { listRef, dragged, drop, startDrag, blockClick };
}
