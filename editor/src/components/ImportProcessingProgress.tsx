import type { ImportProgress } from '../generated/bindings';

const phases = ['Read source', 'Pixelate frames', 'Preview'];

export function importProgressLabel(progress: ImportProgress | null, cancelling: boolean): string {
  if (cancelling) return 'Cancelling…';
  if (!progress) return 'Starting frame processing…';
  if (progress.stage === 'decoding') return 'Reading original image…';
  if (progress.stage === 'committing') return 'Adding frames and colors…';
  return `${progress.completed} / ${progress.total} frames complete · ${progress.active} processing`;
}

/** Only finished frames count toward progress; iterations may converge early. */
export function ImportProcessingProgress({
  progress,
  complete,
  cancelling,
}: {
  progress: ImportProgress | null;
  complete: boolean;
  cancelling: boolean;
}) {
  const finished = progress?.stage === 'frames' && progress.completed === progress.total;
  const active = complete || finished ? 2 : !progress || progress.stage === 'decoding' ? 0 : 1;
  const percent = complete
    ? 100
    : progress?.stage === 'frames' && progress.total > 0
      ? Math.min(99, Math.floor((progress.completed / progress.total) * 100))
      : 0;
  return (
    <div className="import-processing-progress">
      <ol className="import-processing-phases" aria-label="Image processing stages">
        {phases.map((phase, index) => (
          <li
            key={phase}
            data-state={
              complete || index < active ? 'complete' : index === active ? 'active' : 'pending'
            }
            aria-current={!complete && index === active ? 'step' : undefined}
          >
            <span aria-hidden="true">{complete || index < active ? '✓' : index + 1}</span> {phase}
          </li>
        ))}
      </ol>
      <div
        className="import-processing-track"
        data-cancelling={cancelling}
        role="progressbar"
        aria-label="Frame processing"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-valuetext={complete ? 'Preview complete' : importProgressLabel(progress, cancelling)}
      >
        <span style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}
