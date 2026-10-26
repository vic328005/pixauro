/** Icons supplied with the Layers design, kept as native SVG/React elements. */
const icons = {
  eye: (
    <>
      <path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z" />
      <circle cx="8" cy="8" r="2" />
    </>
  ),
  eyeOff: (
    <>
      <path d="M2.5 9.5C4 11.5 5.8 12.5 8 12.5s4-1 5.5-3" />
      <path d="M4 11l-1 1.5M8 12.5V14M12 11l1 1.5" />
    </>
  ),
  lock: (
    <>
      <rect x="3.5" y="7" width="9" height="6.5" rx="1" />
      <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" />
    </>
  ),
  unlock: (
    <>
      <rect x="3.5" y="7" width="9" height="6.5" rx="1" />
      <path d="M5.5 7V5a2.5 2.5 0 0 1 4.9-.7" />
    </>
  ),
  plus: <path d="M8 3v10M3 8h10" />,
  duplicate: (
    <>
      <rect x="5.5" y="5.5" width="8" height="8" rx="1" />
      <path d="M3 10.5V3.5a1 1 0 0 1 1-1h6.5" />
    </>
  ),
  mergeDown: (
    <>
      <path d="M8 2.5v7.5M5 7l3 3 3-3" />
      <path d="M3 13.5h10" />
    </>
  ),
  trash: <path d="M2.5 4.5h11M6.5 4.5V3h3v1.5M4 4.5l.7 9h6.6l.7-9" />,
};
export function LayerIcon({ name }: { name: keyof typeof icons }) {
  return (
    <svg
      width={16}
      height={16}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {icons[name]}
    </svg>
  );
}
