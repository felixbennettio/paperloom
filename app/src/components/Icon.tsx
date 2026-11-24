// A small stroke icon set drawn for Paperloom (24×24 grid).

const PATHS = {
  library: "M4 4h4v16H4zM10 4h4v16h-4zM16.5 4.5l3.5 1-4 15-3.5-1z",
  note: "M6 3h9l4 4v14H6zM15 3v4h4M9 12h7M9 16h5",
  settings:
    "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  plus: "M12 5v14M5 12h14",
  close: "M6 6l12 12M18 6L6 18",
  folder: "M3 6h6l2 2h10v11H3z",
  search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4",
  trash: "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13",
  link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
  doc: "M6 3h9l4 4v14H6zM15 3v4h4",
  latex: "M6 3h9l4 4v14H6zM15 3v4h4M15 11H9.5l3 3.5-3 3.5H15",
  globe: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c2.5 2.6 3.8 5.6 3.8 9s-1.3 6.4-3.8 9c-2.5-2.6-3.8-5.6-3.8-9s1.3-6.4 3.8-9z",
  star: "M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z",
  tag: "M3 12V4h8l10 10-8 8zM7.5 7.5h.01",
  restore: "M4 12a8 8 0 1 0 2.3-5.7L4 8.6M4 4v4.6h4.6",
  edit: "M4 20h4L19 9l-4-4L4 16zM14 6l4 4",
  compass: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM15.5 8.5l-2 5-5 2 2-5z",
  calendar: "M4 6h16v14H4zM4 10h16M8 3v4M16 3v4",
  template: "M6 3h12v18H6zM9 7h6M9 11h6M9 15h3",
  canvas: "M3 4h7v6H3zM14 4h7v10h-7zM3 14h7v6H3zM10 7h4M6.5 10v4",
  import: "M12 4v11M7 10l5 5 5-5M4 20h16",
  duplicate: "M9 9h11v11H9zM5 15H4V4h11v1",
  picture: "M3 5h18v14H3zM3 16l5-5 4 4 3-3 6 6M15.5 8.5h.01",
  mic: "M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3zM5 11a7 7 0 0 0 14 0M12 18v3",
  ruler: "M3 15.5L15.5 3 21 8.5 8.5 21zM6.5 12l2 2M9 9.5l1.5 1.5M11.5 7l2 2M14 4.5l1.5 1.5",
  protractor: "M3 18h18M3 18a9 9 0 0 1 18 0M12 18v-3M7.5 12.5l1.5 1.5M16.5 12.5l-1.5 1.5",
  diagram: "M3 4h7v5H3zM14 15h7v5h-7zM6.5 9v3.5h11V15",
  pen: "M4 20l4.5-1L19 8.5 15.5 5 5 15.5zM13.5 7l3.5 3.5",
  highlighter: "M7 14l8-8 3 3-8 8H7zM5 20h14",
  underline: "M7 4v7a5 5 0 0 0 10 0V4M5 20h14",
  eraser: "M8 20h11M5 15l8.5-9.5 6 6L12 20H9.5zM9.5 10.5l6 6",
  lasso: "M12 4c4.4 0 8 2.5 8 5.5S16.4 15 12 15s-8-2.5-8-5.5S7.6 4 12 4zM8 14.5c-1.5 2-.5 4.5 2 5",
  shapes: "M3 4h7v7H3zM17 3l4 7h-8zM14 17.5a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0",
  text: "M5 5h14M12 5v14M9 19h6",
  line: "M5 19L19 5",
  arrow: "M5 19L19 5M10 5h9v9",
  undo: "M9 14L4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3",
  redo: "M15 14l5-5-5-5M20 9H10a6 6 0 0 0 0 12h3",
  mindmap: "M9 10h6v4H9zM15 12c2 0 2-6 4-6M15 12c2 0 2 6 4 6M9 12c-2 0-2-6-4-6M9 12c-2 0-2 6-4 6",
  graph: "M4 6a2 2 0 1 0 4 0a2 2 0 1 0-4 0M16 8a2 2 0 1 0 4 0a2 2 0 1 0-4 0M10 18a2 2 0 1 0 4 0a2 2 0 1 0-4 0M8 6.3l8 1.4M7 7.7l4.2 8.5M17 9.8l-4 6.4",
  back: "M15 5l-7 7 7 7",
  forward: "M9 5l7 7-7 7",
  panelLeft: "M3 4h18v16H3zM9 4v16",
  panelRight: "M3 4h18v16H3zM15 4v16",
  split: "M3 4h18v16H3zM12 4v16",
};

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
