import { useRef, type CSSProperties, type KeyboardEvent, type PointerEvent } from "react";
import { useI18n } from "../i18n";

interface Props {
  /** What it resizes, for screen readers. */
  label: string;
  className: string;
  style?: CSSProperties;
  /** The size it sets now, and its limits, for screen readers. */
  value: number;
  min: number;
  max: number;
  /** A drag (or arrow keys) begins. */
  onStart: () => void;
  /** How far the pointer is from where the drag began, in pixels; positive is rightwards. */
  onMove: (dx: number) => void;
  onEnd: () => void;
  /** Double-click or Enter: back to the default size. */
  onReset: () => void;
}

/** A divider between two parts of the window, dragged to resize them. */
export function Splitter({ label, className, style, value, min, max, onStart, onMove, onEnd, onReset }: Props) {
  const { t } = useI18n();
  const start = useRef<number | null>(null);

  const down = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    start.current = e.clientX;
    // No text selection, and no frame (PDF, plugins) swallowing the pointer.
    document.body.classList.add("resizing");
    onStart();
  };
  const move = (e: PointerEvent<HTMLDivElement>) => {
    if (start.current !== null) onMove(e.clientX - start.current);
  };
  const up = (e: PointerEvent<HTMLDivElement>) => {
    if (start.current === null) return;
    start.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
    document.body.classList.remove("resizing");
    onEnd();
  };
  const key = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      onReset();
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      onStart();
      onMove(e.key === "ArrowLeft" ? -16 : 16);
      onEnd();
    }
  };

  return (
    <div
      className={`splitter ${className}`}
      style={style}
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={value}
      aria-valuemin={min}
      aria-valuemax={max}
      title={t("Drag to resize; double-click to reset")}
      tabIndex={0}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      onDoubleClick={onReset}
      onKeyDown={key}
    />
  );
}
