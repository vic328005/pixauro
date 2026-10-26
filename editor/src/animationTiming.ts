// Keep these limits aligned with the engine's Animation constants.
export const DEFAULT_ANIMATION_FPS = 10;
export const MIN_ANIMATION_FPS = 0.2;
export const MAX_ANIMATION_FPS = 100;

export function animationDuration(frameCount: number, fps: number) {
  return `${Number((frameCount / fps).toFixed(3))} s`;
}

/** Advance from elapsed time so fractional frame durations never round to integer milliseconds. */
export function startAnimationPlayback(fps: number, advance: (steps: number) => void) {
  const start = performance.now();
  let previous = 0;
  let request = 0;
  const tick = (now: number) => {
    const elapsed = Math.floor(((now - start) * fps) / 1000);
    if (elapsed > previous) {
      advance(elapsed - previous);
      previous = elapsed;
    }
    request = requestAnimationFrame(tick);
  };
  request = requestAnimationFrame(tick);
  return () => cancelAnimationFrame(request);
}
