import { useCallback, useEffect, useImperativeHandle, useRef, useState, type Ref } from 'react';
import {
  commands,
  type PngRaster,
  type LocateAxis,
  type LocateIssue,
  type LocateItem,
} from '../generated/bindings';
import type { Slice } from '../spriteImport';
import { Button } from './ui/button';

const issueLabels: Record<LocateIssue, string> = {
  background: 'Background is too varied to identify reliably.',
  resolution: 'Slice is too small in the source preview.',
  missing: 'No reliable subject found nearby.',
  ambiguous: 'More than one possible subject or competing slice.',
  size: 'The subject does not fit within this frame and movement range.',
  blocked: 'No clear position on the chosen axes. Try Both axes or adjust manually.',
};
const geometryKey = (slices: Slice[]) =>
  JSON.stringify(
    slices.map((s) => [s.id, s.row, s.x, s.y, s.width, s.height, s.enabled, s.autoLocked]),
  );
type LocateResult = { items: LocateItem[]; geometry: string; before: Slice[] };

export function AutoLocateSlices({
  source,
  slices,
  busy,
  onBusyChange,
  onApply,
  onSelect,
  cancelRef,
}: {
  source: PngRaster | null;
  slices: Slice[];
  busy: boolean;
  onBusyChange: (value: boolean) => void;
  onApply: (slices: Slice[]) => void;
  onSelect: (id: string) => void;
  cancelRef: Ref<{ cancel: () => void }>;
}) {
  const [axis, setAxis] = useState<LocateAxis>('both');
  const [progress, setProgress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<LocateResult | null>(null);
  const jobRef = useRef<string | null>(null);
  const cancelledRef = useRef<string | null>(null);
  useEffect(
    () => () => {
      const job = jobRef.current;
      jobRef.current = null;
      if (job) void commands.cancelPngImport(job).catch(() => {});
    },
    [],
  );
  const currentResult = result?.geometry === geometryKey(slices) ? result : null;
  const moved = currentResult?.items.filter((item) => item.status === 'moved').length ?? 0;
  const review = currentResult?.items.filter((item) => item.status === 'review') ?? [];
  const locked = slices.filter((s) => s.enabled && s.autoLocked).length;
  const eligible = slices.some((s) => s.enabled && !s.autoLocked);

  const cancel = useCallback(() => {
    const job = jobRef.current;
    if (job) {
      cancelledRef.current = job;
      setProgress('Cancelling…');
      void commands
        .cancelPngImport(job)
        .then((result) => {
          if (jobRef.current === job && result.status === 'error') setError(result.error);
        })
        .catch((reason: unknown) => {
          if (jobRef.current === job) setError(String(reason));
        });
    }
  }, []);
  useImperativeHandle(cancelRef, () => ({ cancel }), [cancel]);
  async function start() {
    if (!source || busy || jobRef.current || !eligible) return;
    const job = crypto.randomUUID();
    jobRef.current = job;
    cancelledRef.current = null;
    const before = slices;
    setError(null);
    setProgress('Locating slices…');
    onBusyChange(true);
    try {
      const result = await commands.autoLocateSlices(job, {
        width: source.width,
        height: source.height,
        sourceWidth: source.sourceWidth,
        sourceHeight: source.sourceHeight,
        rgba: source.rgba,
        slices,
        axis,
      });
      if (jobRef.current !== job || cancelledRef.current === job) return;
      if (result.status === 'error') throw new Error(result.error);
      if (!result.data) return;
      const items = result.data;
      const byId = new Map(items.map((item) => [item.id, item]));
      const next = before.map((s) => {
        const item = byId.get(s.id);
        return item?.status === 'moved' ? { ...s, x: item.x, y: item.y } : s;
      });
      if (items.some((item) => item.status === 'moved')) onApply(next);
      setResult({ before, items, geometry: geometryKey(next) });
    } catch (reason) {
      if (jobRef.current === job) setError(String(reason));
    } finally {
      if (jobRef.current === job) {
        jobRef.current = null;
        setProgress(null);
        onBusyChange(false);
      }
    }
  }
  return (
    <section className="import-auto-locate" aria-label="Automatic slice positioning">
      <strong>Automatic positioning</strong>
      <label className="png-import-field">
        <span>Move along</span>
        <select
          className="timeline-input"
          value={axis}
          onChange={(e) => setAxis(e.target.value as LocateAxis)}
          disabled={busy}
        >
          <option value="both">Both axes</option>
          <option value="x">Horizontal only · X</option>
          <option value="y">Vertical only · Y</option>
        </select>
      </label>
      <Button disabled={busy || !source || !eligible} onClick={start}>
        Auto locate slices
      </Button>
      <p className="muted">
        Best for transparent or simple backgrounds. Keeps frame sizes and order; uncertain slices
        stay in place. {locked > 0 && `${locked} locked.`}
      </p>
      <p className="muted">Moving slices resets the pixel preview and cleanup corrections.</p>
      {progress && <p role="status">{progress}</p>}
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
      {currentResult && (
        <>
          <p role="status">
            {moved} moved · {review.length} need review ·{' '}
            {currentResult.items.filter((item) => item.status === 'unchanged').length} already
            positioned
          </p>
          <Button
            variant="secondary"
            disabled={busy || moved === 0}
            onClick={() => {
              const before = new Map(currentResult.before.map((s) => [s.id, s]));
              onApply(
                slices.map((s) => ({ ...s, x: before.get(s.id)!.x, y: before.get(s.id)!.y })),
              );
              setResult(null);
              setError(null);
            }}
          >
            Undo auto positioning
          </Button>
          {review.length > 0 && (
            <details open>
              <summary>Review unchanged slices</summary>
              <ul className="import-locate-review">
                {review.map((item) => {
                  const slice = slices.find((s) => s.id === item.id)!;
                  const number =
                    slices.filter((s) => s.row === slice.row).findIndex((s) => s.id === slice.id) +
                    1;
                  return (
                    <li key={item.id}>
                      <button disabled={busy} onClick={() => onSelect(item.id)}>
                        Row {slice.row + 1} · Frame {number}
                      </button>
                      <small className="muted">{item.issue && issueLabels[item.issue]}</small>
                    </li>
                  );
                })}
              </ul>
            </details>
          )}
        </>
      )}
    </section>
  );
}
