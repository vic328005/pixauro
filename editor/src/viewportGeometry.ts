export type ViewportGeometry = Readonly<{
  left: number;
  top: number;
  width: number;
  height: number;
}>;

/** Initial/duplicate layout notifications must not interrupt a captured stroke. */
export function viewportGeometryChanged(
  previous: ViewportGeometry | null,
  next: ViewportGeometry,
): boolean {
  return (
    previous !== null &&
    (previous.left !== next.left ||
      previous.top !== next.top ||
      previous.width !== next.width ||
      previous.height !== next.height)
  );
}
