import { useEffect, useMemo, useRef, useState } from 'react';
import {
  commands,
  type PngRaster,
  type SpritePreview,
  type PngImportOptions,
} from '../generated/bindings';
import { useEditor } from '../editorController';
import { animationNameError } from '../inputValidation';
import {
  DEFAULT_ANIMATION_FPS,
  MIN_ANIMATION_FPS,
  MAX_ANIMATION_FPS,
  startAnimationPlayback,
} from '../animationTiming';
import { useUi, type PngImportDialog as ImportTarget } from '../editorUiState';
import {
  clamp,
  cleanFrame,
  previewFrames,
  gridSlices,
  rasterUrl,
  type ImportGroup,
  type Slice,
} from '../spriteImport';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { CommitInput } from './ui/commit-input';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from './ui/dialog';
import { FrameCanvas, SliceCanvas } from './SpriteImportCanvas';
import { AutoLocateSlices } from './AutoLocateSlices';
import { ImportProcessingProgress, importProgressLabel } from './ImportProcessingProgress';
import './png-import.css';

const steps = ['Slice', 'Pixelate', 'Background', 'Review'];
const initialOptions: PngImportOptions = {
  pixelSize: 64,
  colors: 16,
  workingSize: 1024,
  saturation: 1.1,
  compactness: 45,
  smoothing: 0.4,
  colorSigma: 3,
  maxIterations: 500,
};
function NumberField({
  label,
  value,
  min = 0,
  max = 8192,
  step = 1,
  onChange,
}: {
  label: string;
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onChange: (value: number) => void;
}) {
  return (
    <label className="png-import-field">
      <span>{label}</span>
      <CommitInput
        label={label}
        min={min}
        max={max}
        value={String(value)}
        integer={step === 1}
        onCommit={(text) => onChange(Number(text))}
      />
    </label>
  );
}

export function PngImportDialog({ target }: { target: ImportTarget }) {
  const { prepareSpriteImport, commitSpriteImport, cancelPngImport, getDocument } = useEditor();
  const {
    importRunning: running,
    importCancelling: cancelling,
    importProgress: progress,
    importMessage: message,
    error,
    set,
  } = useUi();
  const [source, setSource] = useState<PngRaster | null>(null);
  const [originalSource, setOriginalSource] = useState<PngRaster | null>(null);
  const [removeSourceBackground, setRemoveSourceBackground] = useState(false);
  const [sourceBackgroundEdge, setSourceBackgroundEdge] = useState(0);
  const [sourceProcessing, setSourceProcessing] = useState(false);
  const [gridProcessing, setGridProcessing] = useState(false);
  const sourceRequest = useRef(0);
  const sourcePending = useRef(false);
  const [preview, setPreview] = useState<SpritePreview | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);
  const [step, setStep] = useState(0);
  const [rows, setRows] = useState(1),
    [columns, setColumns] = useState(1);
  const [margin, setMargin] = useState(0),
    [gap, setGap] = useState(0);
  const [slices, setSlices] = useState<Slice[]>([]);
  const [groups, setGroups] = useState<ImportGroup[]>([]);
  const [selected, setSelected] = useState('0:0');
  const [options, setOptions] = useState<PngImportOptions>(() => ({
    ...initialOptions,
    pixelSize: Math.min(64, Math.max(target.canvasWidth, target.canvasHeight)),
  }));
  const [removeBackground, setRemoveBackground] = useState(true);
  const [showOriginal, setShowOriginal] = useState(false);
  const [touchUp, setTouchUp] = useState(false);
  const [alignmentOpen, setAlignmentOpen] = useState(false);
  const [overrides, setOverrides] = useState<Record<string, Uint8Array>>({});
  const [undoMasks, setUndoMasks] = useState<Record<string, Uint8Array>[]>([]);
  const [tool, setTool] = useState('restore');
  const [brushSize, setBrushSize] = useState(1);
  const [playing, setPlaying] = useState(false);
  const [baseline, setBaseline] = useState(true);
  const [guideY, setGuideY] = useState(Math.round(target.canvasHeight * 0.875));
  const [asAnimations, setAsAnimations] = useState(false);
  const [committing, setCommitting] = useState(false);
  const offsetUndo = useRef<{ id: string; x: number; y: number } | null>(null);
  const [locating, setLocating] = useState(false);
  const cancelLocate = useRef<{ cancel: () => void } | null>(null);
  const busy = running || committing || locating || sourceProcessing || gridProcessing;
  const fileName = target.path.split(/[/\\]/).at(-1) ?? 'Imported sprites';
  const layerName =
    fileName
      .replace(/\.png$/i, '')
      .trim()
      .slice(0, 128) || 'Imported sprites';

  useEffect(() => {
    let disposed = false;
    void commands
      .pngSourcePreview(target.path, false, 0)
      .then(async (result) => {
        if (disposed) return;
        if (result.status === 'error') {
          setLocalError(result.error);
          return;
        }
        const image = result.data;
        const initialSlices = await gridSlices(image.sourceWidth, image.sourceHeight, 1, 1, 0, 0);
        if (disposed) return;
        setSource(image);
        setOriginalSource(image);
        setSlices(initialSlices);
        const names = new Set(getDocument()?.animations.map((a) => a.name));
        let name = 'Imported 1',
          suffix = 1;
        while (names.has(name)) name = `Imported 1 (${suffix++})`;
        setGroups([{ name, fps: DEFAULT_ANIMATION_FPS }]);
      })
      .catch((reason: unknown) => {
        if (!disposed) setLocalError(String(reason));
      });
    return () => {
      disposed = true;
      sourceRequest.current += 1;
    };
  }, [target.path, getDocument]);

  const enabled = slices.filter((s) => s.enabled);
  const createAnimations = asAnimations || enabled.length > 1;
  const selectedSlice = slices.find((s) => s.id === selected) ?? slices[0];
  const frames = useMemo(
    () => (preview ? previewFrames(preview, slices, target.canvasWidth, target.canvasHeight) : []),
    [preview, slices, target.canvasWidth, target.canvasHeight],
  );
  const cleaned = useMemo(
    () => frames.map((f) => cleanFrame(f, removeBackground, overrides[f.id])),
    [frames, removeBackground, overrides],
  );
  const frame = cleaned.find((f) => f.id === selected) ?? cleaned[0];
  const rawFrame = frames.find((f) => f.id === frame?.id);
  const activeTool =
    playing || busy
      ? 'preview'
      : step === 2 && touchUp && !showOriginal
        ? tool
        : step === 3 && alignmentOpen
          ? 'move'
          : 'preview';
  const currentRow = step >= 1 && frame ? (frame?.row ?? 0) : (selectedSlice?.row ?? 0);
  const rowFrames = cleaned.filter((f) => f.row === currentRow);
  const fps = groups[currentRow]?.fps ?? DEFAULT_ANIMATION_FPS;
  const rowIds = rowFrames.map((f) => f.id).join('|');
  useEffect(() => {
    if (!playing || busy || step < 1) return;
    const ids = rowIds.split('|').filter(Boolean);
    if (!ids.length) return;
    return startAnimationPlayback(fps, (steps) => {
      setSelected((id) => ids[(Math.max(0, ids.indexOf(id)) + steps) % ids.length]);
    });
  }, [playing, busy, step, rowIds, fps]);
  useEffect(() => {
    const stop = () => setPlaying(false);
    window.addEventListener('blur', stop);
    document.addEventListener('visibilitychange', stop);
    return () => {
      window.removeEventListener('blur', stop);
      document.removeEventListener('visibilitychange', stop);
    };
  }, []);

  const sourceUrl = useMemo(() => (source ? rasterUrl(source) : ''), [source]);
  const groupNames = groups
    .filter((_, row) => enabled.some((s) => s.row === row))
    .map((g) => g.name.trim());
  const existingNames = getDocument()?.animations.map((a) => a.name) ?? [];
  const nameErrors = groups.map((group, row) => {
    if (!createAnimations || !enabled.some((slice) => slice.row === row)) return null;
    return (
      animationNameError(group.name, existingNames) ??
      (groupNames.filter((name) => name === group.name.trim()).length > 1
        ? 'Another imported row uses this name. Enter a unique name.'
        : null)
    );
  });
  const badNames = nameErrors.some(Boolean);
  const gridError =
    rows * columns > 256
      ? 'Use at most 256 slices. Reduce rows or columns.'
      : source &&
          (source.sourceWidth - margin * 2 - gap * (columns - 1) < columns ||
            source.sourceHeight - margin * 2 - gap * (rows - 1) < rows)
        ? 'The grid does not fit the image. Reduce rows, columns, outer margin or spacing.'
        : null;
  const clipped =
    frame &&
    (frame.x < 0 ||
      frame.y < 0 ||
      frame.x + frame.width > target.canvasWidth ||
      frame.y + frame.height > target.canvasHeight);

  function invalidate() {
    setPreview(null);
    offsetUndo.current = null;
    setOverrides({});
    setUndoMasks([]);
    setPlaying(false);
    setLocalError(null);
    set({ error: null, importMessage: null });
  }
  function changeSlice(slice: Slice) {
    setSlices((all) => all.map((s) => (s.id === slice.id ? slice : s)));
    invalidate();
  }
  function toggleSourceBackground() {
    if (!source || !originalSource || busy || sourcePending.current) return;
    if (removeSourceBackground) {
      setSource(originalSource);
      setRemoveSourceBackground(false);
      setRemoveBackground(true);
      setShowOriginal(false);
      invalidate();
      return;
    }
    void applySourceBackground(sourceBackgroundEdge);
  }
  async function changeSourceBackgroundEdge(edge: number): Promise<boolean> {
    if (busy || sourcePending.current) return false;
    if (edge === sourceBackgroundEdge) return true;
    if (!removeSourceBackground) {
      setSourceBackgroundEdge(edge);
      return true;
    }
    return applySourceBackground(edge);
  }
  async function applySourceBackground(edge: number): Promise<boolean> {
    if (!source || !originalSource || busy || sourcePending.current) return false;
    const request = ++sourceRequest.current;
    sourcePending.current = true;
    setSourceProcessing(true);
    setLocalError(null);
    try {
      const result = await commands.pngSourcePreview(target.path, true, edge);
      if (request !== sourceRequest.current) return false;
      if (result.status === 'error') {
        setLocalError(result.error);
        return false;
      }
      if (
        result.data.sourceWidth !== originalSource.sourceWidth ||
        result.data.sourceHeight !== originalSource.sourceHeight
      ) {
        setLocalError(
          'The source image dimensions changed. Close this importer and open the PNG again.',
        );
        return false;
      }
      setSource(result.data);
      setRemoveSourceBackground(true);
      setSourceBackgroundEdge(edge);
      // Compare the two stages independently unless the user explicitly enables both.
      if (!removeSourceBackground) setRemoveBackground(false);
      setShowOriginal(false);
      invalidate();
      return true;
    } catch (reason) {
      if (request === sourceRequest.current) setLocalError(String(reason));
      return false;
    } finally {
      if (request === sourceRequest.current) {
        sourcePending.current = false;
        setSourceProcessing(false);
      }
    }
  }
  async function generateGrid() {
    if (!source || busy) return;
    setGridProcessing(true);
    try {
      const next = await gridSlices(
        source.sourceWidth,
        source.sourceHeight,
        rows,
        columns,
        margin,
        gap,
      );
      if (!next.length) return;
      const used = new Set(existingNames);
      const nextGroups = Array.from({ length: rows }, (_, i) => {
        let name = `Imported ${i + 1}`,
          suffix = 1;
        while (used.has(name)) name = `Imported ${i + 1} (${suffix++})`;
        used.add(name);
        return { name, fps: DEFAULT_ANIMATION_FPS };
      });
      setSlices(next);
      setGroups(nextGroups);
      setSelected(next[0].id);
      setAsAnimations(next.length > 1);
      invalidate();
    } catch (reason) {
      setLocalError(String(reason));
    } finally {
      setGridProcessing(false);
    }
  }
  function changeOptions(key: keyof PngImportOptions, value: number) {
    setOptions((o) => ({ ...o, [key]: value }));
    invalidate();
  }
  async function process() {
    setLocalError(null);
    setPlaying(false);
    const result = await prepareSpriteImport(
      options,
      enabled.map(({ id, x, y, width, height }) => ({ id, x, y, width, height })),
      removeSourceBackground,
      sourceBackgroundEdge,
    );
    if (!result) return;
    if (
      result.sourceWidth !== source?.sourceWidth ||
      result.sourceHeight !== source?.sourceHeight
    ) {
      setLocalError(
        'The source image dimensions changed. Close this importer and open the PNG again.',
      );
      return;
    }
    setPreview(result);
    setOverrides({});
    setUndoMasks([]);
    setShowOriginal(false);
    setTouchUp(false);
  }
  function selectFrame(id: string) {
    setPlaying(false);
    setSelected(id);
  }
  function moveFrame(dx: number, dy: number) {
    if (!frame) return;
    setSlices((all) =>
      all.map((s) =>
        s.id === frame.id
          ? {
              ...s,
              offsetX: clamp(s.offsetX + dx, -4096, 4096),
              offsetY: clamp(s.offsetY + dy, -4096, 4096),
            }
          : s,
      ),
    );
  }
  function paint(x: number, y: number, last: { x: number; y: number } | null) {
    if (!rawFrame || x < 0 || y < 0 || x >= rawFrame.width || y >= rawFrame.height) return;
    if (tool !== 'erase' && tool !== 'restore') return;
    setOverrides((all) => {
      const next = all[rawFrame.id]?.slice() ?? new Uint8Array(rawFrame.width * rawFrame.height);
      const start = last ?? { x, y },
        steps = Math.max(Math.abs(x - start.x), Math.abs(y - start.y), 1);
      for (let t = 0; t <= steps; t++) {
        const px = Math.round(start.x + ((x - start.x) * t) / steps),
          py = Math.round(start.y + ((y - start.y) * t) / steps);
        const half = Math.floor(brushSize / 2);
        for (let iy = py - half; iy < py - half + brushSize; iy++)
          for (let ix = px - half; ix < px - half + brushSize; ix++) {
            if (ix >= 0 && iy >= 0 && ix < rawFrame.width && iy < rawFrame.height)
              next[iy * rawFrame.width + ix] = tool === 'erase' ? 1 : 2;
          }
      }
      return { ...all, [rawFrame.id]: next };
    });
  }
  async function commit() {
    if (!preview || badNames || !frames.length) return;
    setCommitting(true);
    setPlaying(false);
    setLocalError(null);
    try {
      await commitSpriteImport(
        layerName,
        preview,
        {
          slices,
          groups,
          removeBackground,
          overrides: Object.fromEntries(
            Object.entries(overrides).map(([id, mask]) => [id, Array.from(mask)]),
          ),
        },
        !createAnimations,
      );
    } finally {
      setCommitting(false);
    }
  }
  function go(next: number) {
    setPlaying(false);
    setStep(next);
    if (next >= 1 && !enabled.some((s) => s.id === selected)) setSelected(enabled[0]?.id ?? '');
  }
  function close() {
    if (!busy) set({ importDialog: null, error: null, importMessage: null });
  }
  const status = gridProcessing
    ? 'Generating slices…'
    : sourceProcessing
      ? 'Removing background from the full source image…'
      : committing
        ? 'Adding frames and colors…'
        : running
          ? importProgressLabel(progress, cancelling)
          : (message ??
            (preview
              ? `${preview.frames.length} frames pixelated. ${preview.frames.filter((f) => !f.raster.converged).length} reached the iteration limit.`
              : 'Adjust slices, then generate a pixel preview.'));

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) close();
      }}
    >
      <DialogContent
        className="png-import-dialog"
        showCloseButton={!busy}
        onPointerDownOutside={(e) => e.preventDefault()}
        onInteractOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => {
          if (busy || e.target instanceof HTMLInputElement) e.preventDefault();
        }}
      >
        <DialogHeader>
          <DialogTitle>Import resource</DialogTitle>
          <DialogDescription>
            {fileName} ·{' '}
            {source ? `${source.sourceWidth} × ${source.sourceHeight}` : 'Reading PNG…'} · Output
            canvas {target.canvasWidth} × {target.canvasHeight}
          </DialogDescription>
        </DialogHeader>
        <nav className="import-steps" aria-label="Import stages">
          {steps.map((label, i) => (
            <Button
              key={label}
              variant="ghost"
              aria-current={step === i ? 'step' : undefined}
              disabled={busy || !source || (i > 0 && !enabled.length) || (i > 1 && !preview)}
              onClick={() => go(i)}
            >
              {i + 1}. {label}
            </Button>
          ))}
        </nav>
        <div className="import-workspace">
          <aside className="import-groups">
            <strong>Animations / frames</strong>
            {groups.map((group, row) => (
              <div key={row} className="import-group">
                <button
                  className="import-group-name"
                  data-invalid={!!nameErrors[row]}
                  aria-describedby={nameErrors[row] ? `import-row-${row}-error` : undefined}
                  disabled={busy}
                  onClick={() => {
                    const first = slices.find(
                      (s) => s.row === row && ((step < 2 && !nameErrors[row]) || s.enabled),
                    );
                    if (nameErrors[row] && step !== 0) go(preview ? 3 : 0);
                    if (first) selectFrame(first.id);
                  }}
                >
                  {group.name.trim() || `Row ${row + 1}`}
                </button>
                {nameErrors[row] && (
                  <span id={`import-row-${row}-error`} className="field-error import-row-error">
                    Check animation name
                  </span>
                )}
                <div className="import-frame-list">
                  {slices
                    .filter((s) => s.row === row)
                    .map((s, i) => (
                      <button
                        key={s.id}
                        className={selected === s.id ? 'selected' : ''}
                        disabled={busy || (step >= 2 && !s.enabled)}
                        aria-pressed={selected === s.id}
                        title={s.enabled ? 'Select frame' : 'Excluded from import'}
                        onClick={() => selectFrame(s.id)}
                      >
                        {i + 1}
                        {!s.enabled && ' ×'}
                      </button>
                    ))}
                </div>
              </div>
            ))}
            <p className="muted">
              {enabled.length} frames · {groupNames.length} rows
            </p>
          </aside>
          <main className="import-main">
            {source && (step === 0 || (step === 1 && !frame)) ? (
              <SliceCanvas
                width={source.sourceWidth}
                height={source.sourceHeight}
                url={sourceUrl}
                slices={slices}
                selected={selected}
                onSelect={selectFrame}
                onChange={changeSlice}
                editable={step === 0 && !busy}
              />
            ) : frame ? (
              <FrameCanvas
                frame={(step === 1 || (step === 2 && showOriginal)) && rawFrame ? rawFrame : frame}
                width={target.canvasWidth}
                height={target.canvasHeight}
                baseline={step === 3 && alignmentOpen && baseline ? guideY : null}
                tool={activeTool}
                onStart={() => {
                  if (activeTool === 'erase' || activeTool === 'restore')
                    setUndoMasks((history) => [...history.slice(-19), overrides]);
                  if (activeTool === 'move') {
                    const s = slices.find((v) => v.id === frame.id)!;
                    offsetUndo.current = { id: s.id, x: s.offsetX, y: s.offsetY };
                  }
                }}
                onPaint={paint}
                onMove={moveFrame}
              />
            ) : (
              <div className="import-empty">{localError ?? 'Reading image…'}</div>
            )}
            {step >= 1 && frame && (
              <div className="import-playback">
                <Button variant="secondary" disabled={busy} onClick={() => setPlaying((v) => !v)}>
                  {playing ? 'Pause' : 'Play row'}
                </Button>
                <span>
                  {groups[currentRow]?.name} · {rowFrames.findIndex((f) => f.id === frame.id) + 1} /{' '}
                  {rowFrames.length} · {fps} FPS
                </span>
                {step === 3 && alignmentOpen && (
                  <label>
                    <input
                      type="checkbox"
                      checked={baseline}
                      onChange={(e) => setBaseline(e.target.checked)}
                    />{' '}
                    Ground guide
                  </label>
                )}
              </div>
            )}
          </main>
          <fieldset className="import-inspector" disabled={busy}>
            <div className="import-slice-settings" hidden={step !== 0}>
              <section className="import-auto-locate" aria-label="Source background removal">
                <strong>Source background</strong>
                <label className="png-import-field">
                  <span>Edge width (source px)</span>
                  <CommitInput
                    label="Source background edge width"
                    value={String(sourceBackgroundEdge)}
                    min={0}
                    integer={false}
                    onCommit={(text) => changeSourceBackgroundEdge(Number(text))}
                  />
                </label>
                <small className="muted">
                  0 disables edge processing; 2.5 is the algorithm default. While removal is on,
                  press Enter or leave the field to update the source preview.
                </small>
                <Button
                  variant="secondary"
                  disabled={!source || busy}
                  aria-pressed={removeSourceBackground}
                  onClick={toggleSourceBackground}
                >
                  {sourceProcessing
                    ? 'Removing background…'
                    : removeSourceBackground
                      ? 'Restore source background'
                      : 'Remove source background'}
                </Button>
                <p className="muted">
                  {removeSourceBackground
                    ? 'Background removed before slicing and pixelation. Later automatic removal is off by default; you can enable it in Background.'
                    : 'Try removing a solid background before slicing and pixelation, then compare with removal in Background.'}
                </p>
                <small className="muted">
                  Applying changes resets the pixel preview and touch-ups. Your source file and
                  slices are kept.
                </small>
              </section>
              <strong>Grid slicing</strong>
              <p className="muted">
                Each row becomes an animation. Drag X or Y to move along one axis, the center to
                move freely, or the corners to resize.
              </p>
              <div
                className="png-import-fields"
                role="group"
                aria-label="Grid dimensions"
                aria-describedby={gridError ? 'import-grid-error' : undefined}
              >
                <NumberField label="Rows" value={rows} min={1} max={64} onChange={setRows} />
                <NumberField
                  label="Columns"
                  value={columns}
                  min={1}
                  max={64}
                  onChange={setColumns}
                />
                <NumberField label="Outer margin" value={margin} onChange={setMargin} />
                <NumberField label="Spacing" value={gap} onChange={setGap} />
              </div>
              {gridError && (
                <p id="import-grid-error" className="field-error" role="alert">
                  {gridError}
                </p>
              )}
              <Button variant="secondary" disabled={!source || !!gridError} onClick={generateGrid}>
                Generate grid
              </Button>
              <small className="muted">Replaces the current slices and manual adjustments.</small>
              <AutoLocateSlices
                key={
                  removeSourceBackground
                    ? `source-background-${sourceBackgroundEdge}`
                    : 'original-source'
                }
                source={source}
                slices={slices}
                busy={busy}
                onBusyChange={setLocating}
                onApply={(next) => {
                  setSlices(next);
                  invalidate();
                }}
                onSelect={selectFrame}
                cancelRef={cancelLocate}
              />
              {selectedSlice && source && (
                <>
                  <hr />
                  <strong>Selected slice</strong>
                  <label>
                    <input
                      type="checkbox"
                      checked={selectedSlice.enabled}
                      onChange={(e) => changeSlice({ ...selectedSlice, enabled: e.target.checked })}
                    />{' '}
                    Include frame
                  </label>
                  <label>
                    <input
                      type="checkbox"
                      checked={selectedSlice.autoLocked}
                      onChange={(e) =>
                        setSlices((all) =>
                          all.map((s) =>
                            s.id === selectedSlice.id ? { ...s, autoLocked: e.target.checked } : s,
                          ),
                        )
                      }
                    />{' '}
                    Lock automatic positioning
                  </label>
                  <div className="png-import-fields">
                    <NumberField
                      label="X"
                      key={`x-${selectedSlice.id}`}
                      value={selectedSlice.x}
                      max={source.sourceWidth - selectedSlice.width}
                      onChange={(x) => changeSlice({ ...selectedSlice, x })}
                    />
                    <NumberField
                      label="Y"
                      key={`y-${selectedSlice.id}`}
                      value={selectedSlice.y}
                      max={source.sourceHeight - selectedSlice.height}
                      onChange={(y) => changeSlice({ ...selectedSlice, y })}
                    />
                    <NumberField
                      label="Width"
                      key={`width-${selectedSlice.id}`}
                      value={selectedSlice.width}
                      min={1}
                      max={source.sourceWidth - selectedSlice.x}
                      onChange={(width) => changeSlice({ ...selectedSlice, width })}
                    />
                    <NumberField
                      label="Height"
                      key={`height-${selectedSlice.id}`}
                      value={selectedSlice.height}
                      min={1}
                      max={source.sourceHeight - selectedSlice.y}
                      onChange={(height) => changeSlice({ ...selectedSlice, height })}
                    />
                  </div>
                  <Button
                    variant="secondary"
                    disabled={slices.length >= 256}
                    onClick={() => {
                      const next = {
                        ...selectedSlice,
                        id: crypto.randomUUID(),
                        offsetX: 0,
                        offsetY: 0,
                        enabled: true,
                        autoLocked: false,
                      };
                      setSlices((all) => [...all, next]);
                      setSelected(next.id);
                      setAsAnimations(true);
                      invalidate();
                    }}
                  >
                    Add frame to row
                  </Button>
                  <div className="import-tool-row">
                    {[-1, 1].map((direction) => (
                      <Button
                        key={direction}
                        variant="ghost"
                        onClick={() => {
                          const all = [...slices],
                            i = all.findIndex((s) => s.id === selected),
                            sameRow = all
                              .map((s, index) => ({ s, index }))
                              .filter(({ s }) => s.row === selectedSlice.row);
                          const next =
                            sameRow[sameRow.findIndex(({ index }) => index === i) + direction]
                              ?.index;
                          if (next === undefined) return;
                          [all[i], all[next]] = [all[next], all[i]];
                          setSlices(all);
                        }}
                      >
                        {direction < 0 ? 'Earlier' : 'Later'}
                      </Button>
                    ))}
                  </div>
                </>
              )}
            </div>
            {step === 1 && (
              <>
                <strong>Pixelate each frame</strong>
                <p className="muted">
                  Each slice is cropped from the original image and pixelated independently.
                  Multiple frames process in parallel.
                </p>
                <NumberField
                  label="Frame grid · longest side"
                  value={options.pixelSize}
                  min={1}
                  max={256}
                  onChange={(n) => changeOptions('pixelSize', n)}
                />
                <NumberField
                  label="Maximum colors · per frame"
                  value={options.colors}
                  min={1}
                  max={256}
                  onChange={(n) => changeOptions('colors', n)}
                />
                <NumberField
                  label="Saturation"
                  value={options.saturation}
                  min={0}
                  max={2}
                  step={0.1}
                  onChange={(n) => changeOptions('saturation', n)}
                />
                <p className="muted">
                  Each frame keeps its aspect ratio and fits the project canvas. Resolution and
                  color limits apply separately to each frame.
                  {rawFrame && ` Selected output: ${rawFrame.width} × ${rawFrame.height}.`}
                </p>
                <details>
                  <summary>Advanced</summary>
                  <NumberField
                    label="Frame input · longest side"
                    value={options.workingSize}
                    min={128}
                    max={2048}
                    onChange={(n) => changeOptions('workingSize', n)}
                  />
                  <NumberField
                    label="Iteration limit"
                    value={options.maxIterations}
                    min={1}
                    max={2000}
                    onChange={(n) => changeOptions('maxIterations', n)}
                  />
                  <NumberField
                    label="Shape regularity"
                    value={options.compactness}
                    min={1}
                    max={100}
                    onChange={(n) => changeOptions('compactness', n)}
                  />
                  <NumberField
                    label="Position smoothing"
                    value={options.smoothing}
                    max={1}
                    step={0.1}
                    onChange={(n) => changeOptions('smoothing', n)}
                  />
                  <NumberField
                    label="Color smoothing"
                    value={options.colorSigma}
                    min={0.1}
                    max={20}
                    step={0.1}
                    onChange={(n) => changeOptions('colorSigma', n)}
                  />
                </details>
                <Button disabled={!enabled.length} onClick={() => void process()}>
                  {preview ? 'Regenerate preview' : 'Generate preview'}
                </Button>
                {preview && (
                  <small className="muted">
                    Regenerating resets manual background corrections.
                  </small>
                )}
              </>
            )}
            {step === 2 && frame && (
              <>
                <strong>Remove background</strong>
                {removeSourceBackground && (
                  <p className="muted">
                    Source background removal is already applied. This step only adds cleanup;
                    compare and restore here cannot recover the source background. Return to Slice
                    and restore it there to compare the two stages.
                  </p>
                )}
                <label>
                  <input
                    type="checkbox"
                    checked={removeBackground}
                    onChange={(e) => setRemoveBackground(e.target.checked)}
                  />{' '}
                  Apply to all frames
                </label>
                <p className="muted">
                  Uses the final pixel outlines to separate the outer background from enclosed
                  character details.
                </p>
                {rawFrame?.foreground === null && (
                  <p role="status" className="muted">
                    The background could not be identified reliably. Automatic removal kept the
                    image unchanged. Use touch-up or keep its background.
                  </p>
                )}
                <Button
                  variant="secondary"
                  aria-pressed={showOriginal}
                  onClick={() => {
                    setShowOriginal((v) => !v);
                    setPlaying(false);
                  }}
                >
                  {showOriginal ? 'Show result' : 'Compare before removal'}
                </Button>
                <details
                  open={touchUp}
                  onToggle={(e) => {
                    setTouchUp(e.currentTarget.open);
                    setPlaying(false);
                  }}
                >
                  <summary>Touch up this frame</summary>
                  <p className="muted">
                    Paint only where needed. Keep restores the original pixels; Remove clears them.
                  </p>
                  <fieldset className="import-touch-up" disabled={showOriginal}>
                    <div className="import-tool-grid">
                      {[
                        ['restore', 'Keep'],
                        ['erase', 'Remove'],
                      ].map(([value, label]) => (
                        <Button
                          key={value}
                          variant="ghost"
                          aria-pressed={tool === value}
                          onClick={() => {
                            setTool(value);
                            setPlaying(false);
                          }}
                        >
                          {label}
                        </Button>
                      ))}
                    </div>
                    <NumberField
                      label="Brush size"
                      value={brushSize}
                      min={1}
                      max={16}
                      onChange={setBrushSize}
                    />
                    <div className="import-tool-row">
                      <Button
                        variant="secondary"
                        disabled={!undoMasks.length}
                        onClick={() => {
                          setOverrides(undoMasks.at(-1)!);
                          setUndoMasks((h) => h.slice(0, -1));
                        }}
                      >
                        Undo stroke
                      </Button>
                      <Button
                        variant="ghost"
                        onClick={() => {
                          setUndoMasks((h) => [...h.slice(-19), overrides]);
                          setOverrides((all) => {
                            const next = { ...all };
                            delete next[frame.id];
                            return next;
                          });
                        }}
                      >
                        Reset frame
                      </Button>
                    </div>
                  </fieldset>
                </details>
              </>
            )}
            {step === 3 && (
              <>
                <strong>Ready to import</strong>
                <p>
                  {enabled.length} frames into{' '}
                  {createAnimations ? `${groupNames.length} new animations` : 'the current frame'}{' '}
                  {createAnimations
                    ? 'with a separate layer for each animation.'
                    : 'on one new layer in the current animation.'}
                </p>
                <p className="muted">
                  Review each row with Play. Existing artwork is kept. One undo removes this entire
                  import.
                </p>
                <details
                  open={alignmentOpen}
                  onToggle={(e) => {
                    setAlignmentOpen(e.currentTarget.open);
                    setPlaying(false);
                  }}
                >
                  <summary>Adjust frame alignment</summary>
                  <p className="muted">Drag the character to move this frame, or enter offsets.</p>
                  <div className="png-import-fields">
                    <NumberField
                      label="Offset X"
                      value={slices.find((s) => s.id === frame.id)?.offsetX ?? 0}
                      min={-4096}
                      max={4096}
                      onChange={(n) =>
                        moveFrame(n - (slices.find((s) => s.id === frame.id)?.offsetX ?? 0), 0)
                      }
                    />
                    <NumberField
                      label="Offset Y"
                      value={slices.find((s) => s.id === frame.id)?.offsetY ?? 0}
                      min={-4096}
                      max={4096}
                      onChange={(n) =>
                        moveFrame(0, n - (slices.find((s) => s.id === frame.id)?.offsetY ?? 0))
                      }
                    />
                  </div>
                  <NumberField
                    label="Ground guide Y"
                    value={guideY}
                    max={target.canvasHeight}
                    onChange={setGuideY}
                  />
                  <Button
                    variant="ghost"
                    onClick={() => {
                      const prev = offsetUndo.current;
                      if (prev)
                        setSlices((all) =>
                          all.map((s) =>
                            s.id === prev.id ? { ...s, offsetX: prev.x, offsetY: prev.y } : s,
                          ),
                        );
                      offsetUndo.current = null;
                    }}
                  >
                    Undo last move
                  </Button>
                </details>
                {enabled.length === 1 && (
                  <label>
                    <input
                      type="checkbox"
                      checked={asAnimations}
                      onChange={(e) => setAsAnimations(e.target.checked)}
                    />{' '}
                    Create a new animation
                  </label>
                )}
              </>
            )}
            {groups[currentRow] && (step === 0 || step === 3) && (
              <>
                <hr />
                <strong>Row {currentRow + 1}</strong>
                <label className="png-import-field">
                  <span>Animation name</span>
                  <Input
                    value={groups[currentRow].name}
                    disabled={
                      !createAnimations || !enabled.some((slice) => slice.row === currentRow)
                    }
                    aria-invalid={!!nameErrors[currentRow]}
                    aria-describedby={
                      nameErrors[currentRow] ? 'import-animation-name-error' : undefined
                    }
                    onChange={(e) =>
                      setGroups((all) =>
                        all.map((g, row) =>
                          row === currentRow ? { ...g, name: e.target.value } : g,
                        ),
                      )
                    }
                  />
                  {nameErrors[currentRow] && (
                    <span id="import-animation-name-error" className="field-error" role="alert">
                      {nameErrors[currentRow]}
                    </span>
                  )}
                </label>
                <NumberField
                  key={`fps-${currentRow}`}
                  label="FPS"
                  value={fps}
                  min={MIN_ANIMATION_FPS}
                  max={MAX_ANIMATION_FPS}
                  step={0.1}
                  onChange={(fps) =>
                    setGroups((all) =>
                      all.map((g, row) => (row === currentRow ? { ...g, fps } : g)),
                    )
                  }
                />
              </>
            )}
            {step === 3 && clipped && (
              <p className="muted">
                Part of this frame lies outside the canvas. Those pixels are kept; adjust the offset
                to bring them back.
              </p>
            )}
          </fieldset>
        </div>
        <footer className="import-footer">
          <div className="import-status" aria-live="polite">
            <span role="status">{status}</span>
            {(localError || error) && (
              <span role="alert" className="field-error">
                {localError ?? error}
              </span>
            )}
            {(running || (step === 1 && preview && !message && !error && !localError)) && (
              <ImportProcessingProgress
                progress={progress}
                complete={!running}
                cancelling={running && cancelling}
              />
            )}
          </div>
          <div className="modal-actions">
            {locating ? (
              <Button variant="secondary" onClick={() => cancelLocate.current?.cancel()}>
                Cancel positioning
              </Button>
            ) : running ? (
              <Button
                variant="secondary"
                disabled={cancelling}
                onClick={() => void cancelPngImport()}
              >
                {cancelling ? 'Cancelling…' : 'Cancel processing'}
              </Button>
            ) : (
              <Button variant="secondary" disabled={busy} onClick={close}>
                Cancel
              </Button>
            )}
            <span className="action-spacer" />
            {step > 0 && (
              <Button variant="secondary" disabled={busy} onClick={() => go(step - 1)}>
                Back
              </Button>
            )}
            {step < 3 ? (
              <Button
                disabled={busy || !source || !enabled.length || (step >= 1 && !preview)}
                onClick={() => go(step + 1)}
              >
                Next · {steps[step + 1]}
              </Button>
            ) : (
              <Button
                disabled={busy || !preview || !enabled.length || badNames}
                onClick={() => void commit()}
              >
                {committing ? 'Importing…' : 'Import resource'}
              </Button>
            )}
          </div>
        </footer>
      </DialogContent>
    </Dialog>
  );
}
