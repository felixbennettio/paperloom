// Pictures the web view draws for saving.

/** An SVG drawn into a PNG (or a JPEG on white, for a PDF page) at twice
 * its size (less for a huge map), in base64. */
export async function pictureOf(svg: string, type: "image/png" | "image/jpeg" = "image/png"): Promise<string> {
  const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
  try {
    const image = new Image();
    image.src = url;
    await image.decode();
    const scale = Math.min(2, 16000 / Math.max(image.naturalWidth, image.naturalHeight, 1));
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(image.naturalWidth * scale);
    canvas.height = Math.round(image.naturalHeight * scale);
    const context = canvas.getContext("2d");
    if (!context) throw new Error("This system can't draw pictures.");
    if (type === "image/jpeg") {
      context.fillStyle = "#ffffff";
      context.fillRect(0, 0, canvas.width, canvas.height);
    }
    context.scale(scale, scale);
    context.drawImage(image, 0, 0);
    return canvas.toDataURL(type, 0.92).replace(/^data:image\/(png|jpeg);base64,/, "");
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** A picture from a file, no bigger than 1600 pixels a side, as a `data:` URL. */
export async function readPicture(file: File): Promise<{ url: string; width: number; height: number }> {
  const original = URL.createObjectURL(file);
  try {
    const image = new Image();
    image.src = original;
    await image.decode();
    const scale = Math.min(1, 1600 / Math.max(image.naturalWidth, image.naturalHeight, 1));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(image.naturalWidth * scale));
    canvas.height = Math.max(1, Math.round(image.naturalHeight * scale));
    canvas.getContext("2d")?.drawImage(image, 0, 0, canvas.width, canvas.height);
    const type = file.type === "image/png" ? "image/png" : "image/jpeg";
    return { url: canvas.toDataURL(type, 0.9), width: canvas.width, height: canvas.height };
  } finally {
    URL.revokeObjectURL(original);
  }
}
