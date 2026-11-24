// Mind map notes: the properties the map view writes.

/** The English names of the map themes; the view translates them. */
export const THEME_NAMES: Record<string, string> = {
  classic: "Classic",
  ocean: "Ocean",
  forest: "Forest",
  sunset: "Sunset",
  mono: "Monochrome",
  dark: "Dark",
};

/** A map note with its `mindmap-theme` property set, beside its `mindmap`
 * property; the text unchanged when it is not a map. */
export function withMapTheme(text: string, theme: string): string {
  if (/^mindmap-theme\s*:/im.test(text)) return text.replace(/^(mindmap-theme\s*:).*$/im, `$1 ${theme}`);
  if (/^mindmap\s*:/im.test(text)) return text.replace(/^(mindmap\s*:.*)$/im, `$1\nmindmap-theme: ${theme}`);
  return text;
}

/** A topic as presentations place it. */
interface Placed {
  id: string;
  depth: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The steps of a presentation, as XMind presents a map: the whole map,
 * then each main branch in turn. */
export function presentationSteps(topics: Placed[]): string[] {
  return ["0", ...topics.filter((t) => t.depth === 1).map((t) => t.id)];
}

/** Whether a topic shows on a step: the centre's step shows them all,
 * a branch's step the centre and that branch. */
export function onStep(id: string, step: string): boolean {
  return step === "0" || id === "0" || id === step || id.startsWith(`${step}.`);
}

/** The box a step shows: the whole map, or a branch and what is under it. */
export function stepBounds(topics: Placed[], step: string, width: number, height: number) {
  if (step === "0") return { left: 0, top: 0, right: width, bottom: height };
  const shown = topics.filter((t) => t.id !== "0" && onStep(t.id, step));
  if (!shown.length) return { left: 0, top: 0, right: width, bottom: height };
  return {
    left: Math.min(...shown.map((t) => t.x)),
    top: Math.min(...shown.map((t) => t.y)),
    right: Math.max(...shown.map((t) => t.x + t.width)),
    bottom: Math.max(...shown.map((t) => t.y + t.height)),
  };
}

/** The pan and zoom that fit a box in a frame, with room around it, at
 * most twice the size. */
export function fitView(
  box: { left: number; top: number; right: number; bottom: number },
  frame: { width: number; height: number },
  room = 48,
): { x: number; y: number; zoom: number } {
  const w = Math.max(1, box.right - box.left);
  const h = Math.max(1, box.bottom - box.top);
  const zoom = Math.min(2, (frame.width - 2 * room) / w, (frame.height - 2 * room) / h);
  return { zoom, x: (frame.width - w * zoom) / 2 - box.left * zoom, y: (frame.height - h * zoom) / 2 - box.top * zoom };
}

/** The view pans just enough to show a topic with room around it, keeping the zoom. */
export function revealTopic(
  topic: { x: number; y: number; width: number; height: number },
  frame: { width: number; height: number },
  view: { x: number; y: number; zoom: number },
  room = 24,
): { x: number; y: number; zoom: number } {
  if (frame.width <= 0 || frame.height <= 0) return view;
  const shift = (start: number, length: number, size: number) => {
    const edge = size - room - length;
    // A topic larger than the frame stays still while it fills the view.
    const low = Math.min(room, edge);
    const high = Math.max(room, edge);
    return Math.max(low, Math.min(high, start)) - start;
  };
  const x = view.x + shift(view.x + topic.x * view.zoom, topic.width * view.zoom, frame.width);
  const y = view.y + shift(view.y + topic.y * view.zoom, topic.height * view.zoom, frame.height);
  return x === view.x && y === view.y ? view : { ...view, x, y };
}
