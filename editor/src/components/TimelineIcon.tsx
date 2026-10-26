/** Timeline icons follow the reference's 16 px, 1.5 px stroke artwork. */
const icons = {
  first: <path d="M3.5 3v10M12.5 3.5 7 8l5.5 4.5z" />,
  previous: <path d="M11 3.5 5.5 8l5.5 4.5z" />,
  next: <path d="M5 3.5 10.5 8 5 12.5z" />,
  last: <path d="M12.5 3v10M3.5 3.5 9 8l-5.5 4.5z" />,
  play: <path d="M5 3.5v9l7-4.5z" fill="currentColor" />,
  pause: <path d="M5.5 3.5v9M10.5 3.5v9" strokeWidth="2" />,
  onion: (
    <>
      <circle cx="6" cy="8" r="4" />
      <circle cx="10" cy="8" r="4" strokeDasharray="2 2" />
    </>
  ),
  link: (
    <path d="M6.5 9.5l3-3M7 4.5l1-1a2.5 2.5 0 0 1 3.5 3.5l-1 1M9 11.5l-1 1a2.5 2.5 0 0 1-3.5-3.5l1-1" />
  ),
};
export function TimelineIcon({ name }: { name: keyof typeof icons }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {icons[name]}
    </svg>
  );
}
