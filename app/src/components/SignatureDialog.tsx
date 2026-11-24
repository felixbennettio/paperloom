import { useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { useI18n } from "../i18n";
import type { Signature } from "../lib/pdf";

const WIDTH = 480;
const HEIGHT = 160;

/** The strokes moved to the corner of the box around them. */
function cropped(strokes: { x: number; y: number }[][]): Signature {
  const all = strokes.flat();
  const left = Math.min(...all.map((p) => p.x));
  const top = Math.min(...all.map((p) => p.y));
  const width = Math.max(1, Math.max(...all.map((p) => p.x)) - left);
  const height = Math.max(1, Math.max(...all.map((p) => p.y)) - top);
  return { width, height, strokes: strokes.map((stroke) => stroke.map((p) => ({ x: p.x - left, y: p.y - top }))) };
}

interface Props {
  onSave: (signature: Signature) => void;
  onClose: () => void;
}

/** A pad to write a signature on once; it is kept on this device and put
 * on pages with the Sign tool. */
export function SignatureDialog({ onSave, onClose }: Props) {
  const { t } = useI18n();
  const [strokes, setStrokes] = useState<{ x: number; y: number }[][]>([]);
  const drawing = useRef(false);

  const point = (e: ReactPointerEvent<SVGSVGElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    return { x: ((e.clientX - box.left) / box.width) * WIDTH, y: ((e.clientY - box.top) / box.height) * HEIGHT };
  };

  return (
    <div className="dialog-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog wide"
        role="dialog"
        aria-modal="true"
        aria-labelledby="signature-title"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <h2 id="signature-title">{t("Signature")}</h2>
        <p className="hint">{t("Write your signature; it is kept on this device and put on pages with Sign.")}</p>
        <svg
          className="signature-pad"
          viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            drawing.current = true;
            setStrokes((all) => [...all, [point(e)]]);
          }}
          onPointerMove={(e) => {
            if (!drawing.current) return;
            const at = point(e);
            setStrokes((all) => [...all.slice(0, -1), [...(all[all.length - 1] ?? []), at]]);
          }}
          onPointerUp={() => {
            drawing.current = false;
          }}
        >
          <line x1={24} y1={HEIGHT - 32} x2={WIDTH - 24} y2={HEIGHT - 32} className="signature-line" />
          {strokes.map((stroke, i) => (
            <polyline key={i} points={stroke.map((p) => `${p.x},${p.y}`).join(" ")} />
          ))}
        </svg>
        <div className="row">
          <button type="button" disabled={strokes.length === 0} onClick={() => setStrokes([])}>
            {t("Clear")}
          </button>
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button
            type="button"
            className="primary"
            disabled={strokes.length === 0}
            onClick={() => onSave(cropped(strokes))}
          >
            {t("Save")}
          </button>
        </div>
      </div>
    </div>
  );
}
