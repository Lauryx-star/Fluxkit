type IconName =
  'plus' | 'arrow' | 'chevron' | 'folder' | 'close' | 'code' | 'history' | 'bolt' | 'settings';
const paths: Record<IconName, string | string[]> = {
  plus: 'M12 5v14M5 12h14',
  arrow: 'M5 12h14m-6-6 6 6-6 6',
  chevron: 'm9 5 7 7-7 7',
  folder: 'M3 7V5h6l2 2h10v12H3V7Z',
  close: 'm6 6 12 12M6 18 18 6',
  code: 'm8 7-5 5 5 5m8-10 5 5-5 5m-3-14-2 18',
  history: 'M3 11a9 9 0 1 1 2 7M3 4v7h7m2-4v6l4 2',
  bolt: 'm14 2-9 12h6l-1 8 9-12h-6l1-8Z',
  settings: [
    'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z',
    'M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82A1.65 1.65 0 0 0 3 13.09H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9c0 .69.4 1.32 1 1.51H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z',
  ],
};

export function Icon({ name, size = 16 }: { name: IconName; size?: number }) {
  const d = paths[name];
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {(Array.isArray(d) ? d : [d]).map((part) => (
        <path key={part} d={part} />
      ))}
    </svg>
  );
}
