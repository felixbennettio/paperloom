import { describe, expect, it, vi } from "vitest";

// The geometry helpers do not need pdf.js itself.
vi.mock("pdfjs-dist", () => ({ GlobalWorkerOptions: {}, getDocument: vi.fn() }));
vi.mock("pdfjs-dist/legacy/build/pdf.worker.min.mjs?url", () => ({ default: "worker.js" }));

const { arrowHead, intersects, loadSignature, mergeLines, placeSignature, rectFromPoints, stampFontSize, stampRect } = await import("./pdf");

describe("mergeLines", () => {
  it("joins the spans of a line and keeps lines apart", () => {
    const lines = mergeLines([
      { left: 10, top: 100, right: 50, bottom: 112 },
      { left: 52, top: 101, right: 90, bottom: 112 },
      { left: 10, top: 116, right: 70, bottom: 128 },
      { left: 0, top: 0, right: 0.2, bottom: 10 },
    ]);
    expect(lines).toEqual([
      { left: 10, top: 100, right: 90, bottom: 112 },
      { left: 10, top: 116, right: 70, bottom: 128 },
    ]);
  });
});

describe("rects", () => {
  it("normalises corners and tests overlap", () => {
    const a = rectFromPoints([100, 700], [20, 680]);
    expect(a).toEqual({ x: 20, y: 680, width: 80, height: 20 });
    expect(intersects(a, { x: 90, y: 690, width: 50, height: 5 })).toBe(true);
    expect(intersects(a, { x: 101, y: 690, width: 50, height: 5 })).toBe(false);
  });
});

describe("arrowheads", () => {
  it("open back from the head on both sides of the shaft", () => {
    const [a, b] = arrowHead([0, 0], [10, 0], 2);
    expect(a[0]).toBeCloseTo(10 - 2 * Math.cos((25 * Math.PI) / 180));
    expect(b[0]).toBeCloseTo(a[0]);
    expect(a[1]).toBeCloseTo(-b[1]);
    expect(Math.abs(a[1])).toBeCloseTo(2 * Math.sin((25 * Math.PI) / 180));
  });
});

describe("stamps and signatures", () => {
  it("size stamps to their words", () => {
    const box = stampRect("Approved", 100, 200);
    expect(box.height).toBe(36);
    expect(box.x + box.width / 2).toBeCloseTo(100);
    expect(stampRect("OK", 0, 0).width).toBe(80);
    expect(stampFontSize("Approved", box.width, box.height)).toBeLessThanOrEqual(36 * 0.55);
  });

  it("place a signature on the page, the right way up", () => {
    const signature = { width: 200, height: 50, strokes: [[{ x: 0, y: 0 }, { x: 200, y: 50 }]] };
    const [stroke] = placeSignature(signature, 300, 400, 100);
    expect(stroke[0]).toEqual({ x: 250, y: 412.5 });
    expect(stroke[1]).toEqual({ x: 350, y: 387.5 });
    expect(loadSignature({ getItem: () => JSON.stringify(signature) })).toEqual(signature);
    expect(loadSignature({ getItem: () => "{}" })).toBeNull();
    expect(loadSignature(null)).toBeNull();
  });
});
