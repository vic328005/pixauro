import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useEditor } from '../editorController';
import type { LayerSnapshot } from '../generated/bindings';

const adjustmentKeys = new Set([
  'ArrowLeft',
  'ArrowRight',
  'ArrowUp',
  'ArrowDown',
  'Home',
  'End',
  'PageUp',
  'PageDown',
]);

/** Native input owns thumb movement; React and canvas previews update once per frame. */
export function LayerOpacityControl({
  layer,
  value,
  disabled,
  blocked,
}: {
  layer: LayerSnapshot | undefined;
  value: number;
  disabled: boolean;
  blocked: boolean;
}) {
  const { previewLayerOpacity, commitLayerOpacity, cancelLayerOpacity } = useEditor();
  const [initialValue] = useState(value);
  const input = useRef<HTMLInputElement>(null);
  const output = useRef<HTMLOutputElement>(null);
  const interacting = useRef(false);
  const display = useCallback((opacity: number) => {
    if (input.current) {
      if (input.current.value !== String(Math.round((opacity / 255) * 100)))
        input.current.value = String(Math.round((opacity / 255) * 100));
      input.current.setAttribute('aria-valuetext', `${Math.round((opacity / 255) * 100)}%`);
    }
    if (output.current) output.current.value = `${Math.round((opacity / 255) * 100)}%`;
  }, []);
  useLayoutEffect(() => {
    // A queued canvas frame must never push an older value back into an active
    // native drag. Outside a gesture, follow undo/redo, cancellation and IPC errors.
    if (!interacting.current) display(value);
  }, [value, display]);
  const finish = useCallback(() => {
    interacting.current = false;
    void commitLayerOpacity();
  }, [commitLayerOpacity]);
  useEffect(() => {
    const pointerUp = () => {
      if (interacting.current) finish();
    };
    const hidden = () => {
      if (document.hidden) finish();
    };
    window.addEventListener('pointerup', pointerUp);
    window.addEventListener('blur', finish);
    document.addEventListener('visibilitychange', hidden);
    return () => {
      window.removeEventListener('pointerup', pointerUp);
      window.removeEventListener('blur', finish);
      document.removeEventListener('visibilitychange', hidden);
      finish();
    };
  }, [finish]);
  const cancel = () => {
    interacting.current = false;
    cancelLayerOpacity();
    display(layer?.opacity ?? 255);
  };
  return (
    <div className="pxl-opacity">
      <label htmlFor="layer-opacity">Opacity</label>
      <input
        ref={input}
        id="layer-opacity"
        type="range"
        min={0}
        max={100}
        step={1}
        defaultValue={Math.round((initialValue / 255) * 100)}
        disabled={disabled}
        aria-disabled={blocked || undefined}
        onPointerDown={(event) => {
          if (blocked) {
            event.preventDefault();
            return;
          }
          // Let the browser keep its own slider/thumb capture and hit testing.
          interacting.current = true;
        }}
        onChange={(event) => {
          if (blocked || !layer) {
            display(value);
            return;
          }
          const opacity = Math.round((Number(event.currentTarget.value) * 255) / 100);
          display(opacity);
          previewLayerOpacity(layer.id, opacity);
          // Accessibility input may change the value without pointer/key events.
          if (!interacting.current) finish();
        }}
        onPointerCancel={cancel}
        onBlur={finish}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            cancel();
          } else if (adjustmentKeys.has(event.key)) {
            if (blocked) event.preventDefault();
            else interacting.current = true;
          }
        }}
        onKeyUp={(event) => {
          if (adjustmentKeys.has(event.key)) finish();
        }}
      />
      <output ref={output} htmlFor="layer-opacity" />
    </div>
  );
}
