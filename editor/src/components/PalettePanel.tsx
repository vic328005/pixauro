import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties, KeyboardEvent, PointerEvent, ReactNode } from 'react';
import { useEditor } from '../editorController';
import { useUi } from '../editorUiState';
import { useShallow } from 'zustand/react/shallow';
import type { ColorDraft, RGBA } from '../editorUiState';
import type { PaletteEntry, PaletteSource } from '../generated/bindings';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from './ui/dialog';

const PAGE_SIZE = 16;
const byte = (value: number) => Math.max(0, Math.min(255, Math.round(value)));
const unit = (value: number) => Math.max(0, Math.min(1, value));

export function rgbaCss(color: RGBA): string {
  return `rgba(${color.red}, ${color.green}, ${color.blue}, ${color.alpha / 255})`;
}

export function colorHex(color: RGBA): string {
  return [color.red, color.green, color.blue, color.alpha]
    .map((channel) => byte(channel).toString(16).padStart(2, '0'))
    .join('')
    .toUpperCase();
}

/** Six digits preserve the current opacity; eight explicitly replace it. */
export function parseHex(value: string, alpha: number): RGBA | null {
  const hex = value.trim().replace(/^#/, '');
  if (!/^(?:[\da-f]{6}|[\da-f]{8})$/i.test(hex)) return null;
  return {
    red: Number.parseInt(hex.slice(0, 2), 16),
    green: Number.parseInt(hex.slice(2, 4), 16),
    blue: Number.parseInt(hex.slice(4, 6), 16),
    alpha: hex.length === 8 ? Number.parseInt(hex.slice(6, 8), 16) : alpha,
  };
}

type HSV = { hue: number; saturation: number; value: number };

/** RGB is deliberately unpremultiplied, including when alpha is zero. */
export function rgbToHsv(color: RGBA, previous?: HSV): HSV {
  const red = color.red / 255;
  const green = color.green / 255;
  const blue = color.blue / 255;
  const maximum = Math.max(red, green, blue);
  const minimum = Math.min(red, green, blue);
  const delta = maximum - minimum;
  let hue = previous?.hue ?? 0;
  if (delta > 0) {
    if (maximum === red) hue = ((green - blue) / delta) % 6;
    else if (maximum === green) hue = (blue - red) / delta + 2;
    else hue = (red - green) / delta + 4;
    hue = (hue * 60 + 360) % 360;
  }
  return {
    hue,
    saturation: maximum === 0 ? (previous?.saturation ?? 0) : delta / maximum,
    value: maximum,
  };
}

export function hsvToRgb(hsv: HSV, alpha: number): RGBA {
  const hue = (((hsv.hue % 360) + 360) % 360) / 60;
  const chroma = unit(hsv.value) * unit(hsv.saturation);
  const x = chroma * (1 - Math.abs((hue % 2) - 1));
  const minimum = unit(hsv.value) - chroma;
  const channels =
    hue < 1
      ? [chroma, x, 0]
      : hue < 2
        ? [x, chroma, 0]
        : hue < 3
          ? [0, chroma, x]
          : hue < 4
            ? [0, x, chroma]
            : hue < 5
              ? [x, 0, chroma]
              : [chroma, 0, x];
  return {
    red: byte((channels[0] + minimum) * 255),
    green: byte((channels[1] + minimum) * 255),
    blue: byte((channels[2] + minimum) * 255),
    alpha,
  };
}

function sameColor(a: RGBA, b: RGBA): boolean {
  return a.red === b.red && a.green === b.green && a.blue === b.blue && a.alpha === b.alpha;
}

function makeDraft(color: RGBA, id: string | null): ColorDraft {
  return {
    id,
    original: { ...color },
    color: { ...color },
    hex: colorHex(color),
    invalidHex: false,
  };
}

function ColorSwatch({ color }: { color: RGBA }) {
  return (
    <span className="checker" aria-hidden="true">
      <span className="swatch-fill" style={{ backgroundColor: rgbaCss(color) }} />
    </span>
  );
}

function SwatchGrid({
  className,
  label,
  children,
}: {
  className: string;
  label: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const grid = ref.current;
    if (!grid) return;
    // WebKit can retain stale intrinsic grid row heights during dock resizing.
    // Measure the resolved column width so replacement swatches stay square.
    const measure = () => {
      const width = grid.firstElementChild?.getBoundingClientRect().width ?? 0;
      if (width <= 0) return;
      const size = `${width}px`;
      if (grid.style.getPropertyValue('--swatch-size') !== size) {
        grid.style.setProperty('--swatch-size', size);
      }
    };
    const observer = new ResizeObserver(measure);
    observer.observe(grid);
    measure();
    return () => observer.disconnect();
  }, []);
  return (
    <div ref={ref} className={className} aria-label={label}>
      {children}
    </div>
  );
}

function PaletteSection({
  source,
  entries,
  selectedId,
  blocked,
  onSelect,
  onEdit,
}: {
  source: PaletteSource;
  entries: PaletteEntry[];
  selectedId: string;
  blocked: boolean;
  onSelect: (id: string) => void;
  onEdit: (id: string) => void;
}) {
  const pageKey = source === 'custom' ? 'palettePage' : 'externalPalettePage';
  const storedPage = useUi((state) => state[pageKey]);
  const set = useUi((state) => state.set);
  const lastPage = Math.max(0, Math.ceil(entries.length / PAGE_SIZE) - 1);
  const page = Math.max(0, Math.min(storedPage, lastPage));
  const label = source === 'custom' ? 'Custom colors' : 'External colors';
  useEffect(() => {
    if (storedPage !== page) set({ [pageKey]: page });
  }, [storedPage, page, pageKey, set]);

  return (
    <section className="palette-section" aria-label={label}>
      <div className="palette-section-heading">
        <h3>{label}</h3>
        <span className="muted">{entries.length}</span>
      </div>
      {entries.length === 0 ? (
        <p className="muted palette-empty">
          {source === 'external'
            ? 'Drop a PNG onto the canvas to import colors.'
            : 'Add your own palette colors.'}
        </p>
      ) : (
        <div className="palette-grid" aria-label={label}>
          {entries.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE).map((entry) => (
            <Button
              variant="secondary"
              size="sm"
              type="button"
              key={entry.id}
              className="palette-swatch"
              aria-label={`${label} · Color ${entry.id}, #${colorHex(entry.color)}`}
              aria-pressed={entry.id === selectedId}
              title={`Color ${entry.id} · #${colorHex(entry.color)} · ${entry.usage} pixels`}
              disabled={blocked}
              onClick={() => onSelect(entry.id)}
              onDoubleClick={() => onEdit(entry.id)}
            >
              <ColorSwatch color={entry.color} />
            </Button>
          ))}
        </div>
      )}
      {lastPage > 0 && (
        <div className="palette-pagination" aria-label={`${label} pages`}>
          <Button
            variant="secondary"
            size="sm"
            type="button"
            aria-label={`Previous ${label.toLowerCase()} page`}
            disabled={blocked || page === 0}
            onClick={() => set({ [pageKey]: page - 1 })}
          >
            ‹
          </Button>
          <span className="muted">
            {page + 1} / {lastPage + 1}
          </span>
          <Button
            variant="secondary"
            size="sm"
            type="button"
            aria-label={`Next ${label.toLowerCase()} page`}
            disabled={blocked || page === lastPage}
            onClick={() => set({ [pageKey]: page + 1 })}
          >
            ›
          </Button>
        </div>
      )}
    </section>
  );
}

export function PalettePanel() {
  const { document, loading, removeColor, prunePalette, withSettledDocument } = useEditor();
  const removingRef = useRef(false);
  const { colorId, draft, deletion, set } = useUi(
    useShallow((state) => ({
      colorId: state.colorId,
      draft: state.draft,
      deletion: state.deletion,
      set: state.set,
    })),
  );
  const palette = document?.palette ?? [];
  const selected = palette.find((entry) => entry.id === colorId) ?? palette[0];
  const blocked = loading || draft !== null || deletion !== null;

  if (!document || !selected) return null;

  async function removeSelected() {
    if (blocked || removingRef.current || palette.length <= 1 || !selected) return;
    removingRef.current = true;
    try {
      let unusedId: string | null = null;
      const ready = await withSettledDocument((doc) => {
        const ui = useUi.getState();
        if (ui.draft || ui.deletion || doc.palette.length <= 1) return;
        const entry = doc.palette.find((entry) => entry.id === selected.id);
        if (!entry) return;
        if (entry.usage > 0) {
          set({ deletion: { id: entry.id, replacement: null } });
        } else {
          unusedId = entry.id;
        }
      });
      if (ready && unusedId !== null) await removeColor(unusedId, null);
    } finally {
      removingRef.current = false;
    }
  }

  function openColorDialog(id: string, add = false) {
    if (blocked) return;
    void withSettledDocument((doc) => {
      const ui = useUi.getState();
      if (ui.draft || ui.deletion) return;
      const entry = doc.palette.find((entry) => entry.id === id);
      if (entry)
        set({
          colorId: id,
          draft: makeDraft(entry.color, add ? null : id),
        });
    });
  }

  return (
    <section className="palette-panel panel-body" aria-label="Palette">
      <div className="panel-heading">
        <h2>Palette</h2>
        <span className="muted">{palette.length}</span>
      </div>
      {(['custom', 'external'] as const).map((source) => (
        <PaletteSection
          key={source}
          source={source}
          entries={palette.filter((entry) => entry.source === source)}
          selectedId={selected.id}
          blocked={blocked}
          onSelect={(id) =>
            void withSettledDocument((doc) => {
              if (doc.palette.some((entry) => entry.id === id)) set({ colorId: id });
            })
          }
          onEdit={openColorDialog}
        />
      ))}
      <div className="palette-actions">
        <Button
          variant="secondary"
          size="sm"
          type="button"
          disabled={blocked}
          onClick={() => openColorDialog(selected.id, true)}
        >
          + Custom color
        </Button>
        <Button
          variant="secondary"
          size="sm"
          type="button"
          disabled={blocked || palette.length <= 1}
          title={palette.length <= 1 ? 'Keep at least one palette color.' : 'Remove selected color'}
          onClick={() => void removeSelected()}
        >
          Remove
        </Button>
        <Button
          variant="secondary"
          size="sm"
          type="button"
          disabled={blocked || palette.length <= 1}
          title="Remove unused colors across all animations and layers. Keep at least one color."
          className="palette-prune-button"
          onClick={async () => {
            if (blocked || removingRef.current) return;
            removingRef.current = true;
            try {
              await prunePalette();
            } finally {
              removingRef.current = false;
            }
          }}
        >
          Remove unused
        </Button>
      </div>
      <div className="selected-color">
        <div className="selected-color-heading">
          <ColorSwatch color={selected.color} />
          <div>
            <strong>Color {selected.id}</strong>
            <span className="color-hex">#{colorHex(selected.color).slice(0, 6)}</span>
          </div>
        </div>
        <p className="muted">
          Opacity {Math.round((selected.color.alpha / 255) * 100)}% · {selected.usage} pixels
        </p>
        <Button
          variant="secondary"
          size="sm"
          className="edit-color-button"
          type="button"
          disabled={blocked}
          onClick={() => openColorDialog(selected.id)}
        >
          Edit color…
        </Button>
      </div>
    </section>
  );
}

/** The shadcn/Radix modal traps focus and keeps draft dismissal explicit. */
function Modal({
  title,
  description,
  busy,
  onCancel,
  children,
  className = '',
}: {
  title: string;
  description: string;
  busy: boolean;
  onCancel: () => void;
  children: ReactNode;
  className?: string;
}) {
  const focusBeforeOpen = useRef<HTMLElement | null>(null);
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !busy) onCancel();
      }}
    >
      <DialogContent
        className={`modal palette-modal ${className}`}
        showCloseButton={false}
        aria-busy={busy}
        onOpenAutoFocus={() => {
          if (
            !focusBeforeOpen.current &&
            globalThis.document.activeElement instanceof HTMLElement
          ) {
            focusBeforeOpen.current = globalThis.document.activeElement;
          }
        }}
        onCloseAutoFocus={(event) => {
          if (focusBeforeOpen.current?.isConnected) {
            event.preventDefault();
            focusBeforeOpen.current.focus();
          }
        }}
        onInteractOutside={(event) => event.preventDefault()}
        onEscapeKeyDown={(event) => {
          if (busy) event.preventDefault();
        }}
      >
        <DialogHeader>
          <div className="modal-header">
            <DialogTitle>{title}</DialogTitle>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label="Close color dialog"
              className="modal-close"
              disabled={busy}
              onClick={onCancel}
            >
              ×
            </Button>
          </div>
          <DialogDescription className="muted">{description}</DialogDescription>
        </DialogHeader>
        {children}
      </DialogContent>
    </Dialog>
  );
}

function ColorEditorDialog({ draft }: { draft: ColorDraft }) {
  const { document, busy, addColor, editColor } = useEditor();
  const { set, stroke, error } = useUi();
  const [hsv, setHsv] = useState(() => rgbToHsv(draft.color));
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState(false);
  const applying = useRef(false);
  const blocked = busy || pending || stroke !== null;
  const used =
    draft.id === null ? 0 : (document?.palette.find((entry) => entry.id === draft.id)?.usage ?? 0);
  const changed = !sameColor(draft.color, draft.original);
  const canApply = !blocked && !draft.invalidHex && (draft.id === null || changed);

  function cancel() {
    if (!blocked) set({ draft: null });
  }

  function updateColor(color: RGBA, nextHsv?: HSV) {
    if (blocked) return;
    setFailed(false);
    setHsv((previous) => nextHsv ?? rgbToHsv(color, previous));
    set({ draft: { ...draft, color, hex: colorHex(color), invalidHex: false } });
  }

  function updateHsv(next: HSV) {
    updateColor(hsvToRgb(next, draft.color.alpha), next);
  }

  function pickShade(event: PointerEvent<HTMLDivElement>) {
    if (blocked) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    if (bounds.width === 0 || bounds.height === 0) return;
    updateHsv({
      ...hsv,
      saturation: unit((event.clientX - bounds.left) / bounds.width),
      value: unit(1 - (event.clientY - bounds.top) / bounds.height),
    });
  }

  function shadeKey(event: KeyboardEvent<HTMLDivElement>) {
    const step = event.shiftKey ? 0.1 : 0.01;
    let next: HSV;
    if (event.key === 'ArrowLeft') next = { ...hsv, saturation: unit(hsv.saturation - step) };
    else if (event.key === 'ArrowRight') next = { ...hsv, saturation: unit(hsv.saturation + step) };
    else if (event.key === 'ArrowUp') next = { ...hsv, value: unit(hsv.value + step) };
    else if (event.key === 'ArrowDown') next = { ...hsv, value: unit(hsv.value - step) };
    else return;
    event.preventDefault();
    if (!blocked) updateHsv(next);
  }

  async function apply() {
    if (!canApply || applying.current) return;
    applying.current = true;
    setPending(true);
    setFailed(false);
    try {
      const saved =
        draft.id === null
          ? await addColor({ ...draft.color })
          : await editColor(draft.id, { ...draft.color });
      if (saved) set({ draft: null });
      else setFailed(true);
    } catch {
      // The controller owns the detailed error. Keep the draft available for retry.
      setFailed(true);
    } finally {
      applying.current = false;
      setPending(false);
    }
  }

  return (
    <Modal
      title={draft.id === null ? 'Add palette color' : `Edit color · #${draft.id}`}
      description={
        draft.id === null
          ? 'Choose a color to add to your custom colors.'
          : `Updates ${used} pixels using this palette color.`
      }
      busy={blocked}
      onCancel={cancel}
    >
      <div className="color-editor-grid">
        <div className="color-picker">
          <div
            className="sv-picker"
            role="slider"
            tabIndex={blocked ? -1 : 0}
            aria-label="Saturation and brightness"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(hsv.saturation * 100)}
            aria-valuetext={`Saturation ${Math.round(hsv.saturation * 100)} percent, brightness ${Math.round(hsv.value * 100)} percent. Left and right change saturation; up and down change brightness.`}
            aria-disabled={blocked}
            style={{
              background: `linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, hsl(${hsv.hue} 100% 50%))`,
              touchAction: 'none',
            }}
            onKeyDown={shadeKey}
            onPointerDown={(event) => {
              if (blocked || event.button !== 0) return;
              event.preventDefault();
              event.currentTarget.focus();
              event.currentTarget.setPointerCapture(event.pointerId);
              pickShade(event);
            }}
            onPointerMove={(event) => {
              if (event.currentTarget.hasPointerCapture(event.pointerId)) pickShade(event);
            }}
            onPointerUp={(event) => {
              if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                pickShade(event);
                event.currentTarget.releasePointerCapture(event.pointerId);
              }
            }}
          >
            <span
              className="sv-cursor"
              style={{ left: `${hsv.saturation * 100}%`, top: `${(1 - hsv.value) * 100}%` }}
            />
          </div>
          <label className="hue-label" htmlFor="palette-hue">
            Hue <span className="muted">{Math.round(hsv.hue)}°</span>
          </label>
          <Input
            id="palette-hue"
            className="hue-slider"
            type="range"
            min={0}
            max={360}
            step={1}
            value={hsv.hue}
            disabled={blocked}
            onChange={(event) => updateHsv({ ...hsv, hue: Number(event.currentTarget.value) })}
          />
          <p className="muted picker-help">Drag to choose hue and shade</p>
        </div>
        <div className="color-values">
          <div className="color-comparison">
            <div className="color-preview">
              <span className="muted">Before</span>
              <ColorSwatch color={draft.original} />
            </div>
            <div className="color-preview">
              <span className="muted">After</span>
              <ColorSwatch color={draft.color} />
            </div>
          </div>
          <label className="hex-label" htmlFor="palette-hex">
            HEX · RRGGBB / RRGGBBAA
          </label>
          <Input
            id="palette-hex"
            className="hex-input"
            type="text"
            value={draft.hex}
            autoComplete="off"
            autoCapitalize="characters"
            spellCheck={false}
            aria-invalid={draft.invalidHex}
            aria-describedby={draft.invalidHex ? 'palette-hex-error' : 'palette-hex-help'}
            disabled={blocked}
            onChange={(event) => {
              const hex = event.currentTarget.value;
              const color = parseHex(hex, draft.color.alpha);
              setFailed(false);
              if (color) setHsv((previous) => rgbToHsv(color, previous));
              set({
                draft: { ...draft, hex, invalidHex: color === null, color: color ?? draft.color },
              });
            }}
          />
          {draft.invalidHex ? (
            <p className="field-error" id="palette-hex-error">
              Enter 6 or 8 hex digits, with an optional #.
            </p>
          ) : (
            <p className="muted hex-help" id="palette-hex-help">
              6 digits keep opacity; 8 digits set it.
            </p>
          )}
          <div className="channel-inputs">
            {(['red', 'green', 'blue', 'alpha'] as const).map((channel) => (
              <label key={channel}>
                {channel[0].toUpperCase()}
                <Input
                  type="number"
                  min={0}
                  max={255}
                  step={1}
                  aria-label={channel[0].toUpperCase() + channel.slice(1)}
                  value={draft.color[channel]}
                  disabled={blocked}
                  onChange={(event) => {
                    const value = event.currentTarget.valueAsNumber;
                    if (Number.isFinite(value))
                      updateColor({ ...draft.color, [channel]: byte(value) });
                  }}
                />
              </label>
            ))}
          </div>
          <label className="opacity-label" htmlFor="palette-opacity">
            Opacity · {Math.round((draft.color.alpha / 255) * 100)}%
          </label>
          <Input
            id="palette-opacity"
            className="opacity-slider"
            type="range"
            min={0}
            max={255}
            step={1}
            value={draft.color.alpha}
            disabled={blocked}
            style={{ '--opacity-color': rgbaCss({ ...draft.color, alpha: 255 }) } as CSSProperties}
            onChange={(event) =>
              updateColor({ ...draft.color, alpha: Number(event.currentTarget.value) }, hsv)
            }
          />
        </div>
      </div>
      {failed && (
        <p className="field-error" role="alert">
          {error ?? 'Could not save this color. Your draft is still here; try again.'}
        </p>
      )}
      <div className="modal-actions">
        <Button
          variant="secondary"
          size="sm"
          type="button"
          disabled={blocked || (!changed && !draft.invalidHex)}
          onClick={() => updateColor({ ...draft.original }, rgbToHsv(draft.original))}
        >
          Reset
        </Button>
        <span className="action-spacer" />
        <Button variant="secondary" size="sm" type="button" disabled={blocked} onClick={cancel}>
          Cancel
        </Button>
        <Button
          size="sm"
          type="button"
          variant="default"
          disabled={!canApply}
          onClick={() => void apply()}
        >
          {pending ? 'Saving…' : draft.id === null ? 'Add color' : 'Apply'}
        </Button>
      </div>
    </Modal>
  );
}

function DeleteColorDialog() {
  const { document, busy, removeColor } = useEditor();
  const { deletion, stroke, set, error } = useUi();
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState(false);
  const applying = useRef(false);
  const blocked = busy || pending || stroke !== null;
  const target = document?.palette.find((entry) => entry.id === deletion?.id);
  const alternatives = document?.palette.filter((entry) => entry.id !== deletion?.id) ?? [];
  const replacement = alternatives.find((entry) => entry.id === deletion?.replacement);

  // A replaced project cannot leave a modal referring to a missing color.
  useEffect(() => {
    if (deletion && !target && !busy) set({ deletion: null });
  }, [deletion, target, busy, set]);

  if (!deletion || !target) return null;

  function cancel() {
    if (!blocked) set({ deletion: null });
  }

  async function remove() {
    if (blocked || applying.current || !deletion || !replacement || alternatives.length === 0)
      return;
    applying.current = true;
    setPending(true);
    setFailed(false);
    try {
      if (await removeColor(deletion.id, replacement.id)) set({ deletion: null });
      else setFailed(true);
    } catch {
      setFailed(true);
    } finally {
      applying.current = false;
      setPending(false);
    }
  }

  return (
    <Modal
      title="Remove palette color"
      description="Choose a replacement. Only pixels using this color will be replaced."
      busy={blocked}
      onCancel={cancel}
      className="delete-color-modal"
    >
      <div className="delete-color-summary">
        <ColorSwatch color={target.color} />
        <strong>
          Color {target.id} · {target.usage} pixels
        </strong>
      </div>
      <SwatchGrid className="replacement-grid" label="Replacement colors">
        {alternatives.map((entry) => (
          <Button
            variant="secondary"
            size="sm"
            key={entry.id}
            type="button"
            className="palette-swatch"
            aria-label={`Replace with color ${entry.id}, #${colorHex(entry.color)}`}
            aria-pressed={replacement?.id === entry.id}
            disabled={blocked}
            title={`Color ${entry.id} · #${colorHex(entry.color)}`}
            onClick={() => {
              setFailed(false);
              set({ deletion: { ...deletion, replacement: entry.id } });
            }}
          >
            <ColorSwatch color={entry.color} />
          </Button>
        ))}
      </SwatchGrid>
      <p className="replacement-choice" aria-live="polite">
        {replacement ? `Replace with Color ${replacement.id}` : 'No replacement selected'}
      </p>
      {failed && (
        <p className="field-error" role="alert">
          {error ?? 'Could not remove this color. Try again.'}
        </p>
      )}
      <div className="modal-actions">
        <Button variant="secondary" size="sm" type="button" disabled={blocked} onClick={cancel}>
          Cancel
        </Button>
        <Button
          variant="destructive"
          size="sm"
          type="button"
          disabled={blocked || !replacement || alternatives.length === 0}
          onClick={() => void remove()}
        >
          {pending ? 'Removing…' : 'Replace & remove'}
        </Button>
      </div>
    </Modal>
  );
}

export function PaletteDialogs() {
  const { document } = useEditor();
  const { draft, deletion } = useUi();
  if (!document) return null;
  if (draft) return <ColorEditorDialog draft={draft} />;
  if (deletion) return <DeleteColorDialog />;
  return null;
}
