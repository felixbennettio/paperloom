// Recording from the microphone into notes, as OneNote and Obsidian's audio
// recorder do: the recording is kept in the vault and embedded with `![[…]]`.

/** A recording under way. */
export interface Recording {
  /** Stops it; the sound and the file extension it takes. */
  stop: () => Promise<{ data: Blob; extension: string }>;
}

/** Starts recording the microphone, in a format the system can write. */
export async function startRecording(): Promise<Recording> {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  const type =
    ["audio/webm;codecs=opus", "audio/webm", "audio/mp4", "audio/ogg"].find((t) => MediaRecorder.isTypeSupported?.(t)) ?? "";
  const recorder = new MediaRecorder(stream, type ? { mimeType: type } : undefined);
  const chunks: Blob[] = [];
  recorder.ondataavailable = (e) => {
    if (e.data.size) chunks.push(e.data);
  };
  recorder.start();
  return {
    stop: () =>
      new Promise((resolve) => {
        recorder.onstop = () => {
          stream.getTracks().forEach((track) => track.stop());
          const mime = recorder.mimeType || type || "audio/webm";
          resolve({ data: new Blob(chunks, { type: mime }), extension: extensionFor(mime) });
        };
        recorder.stop();
      }),
  };
}

/** The file extension a recording of a type takes. */
export function extensionFor(mime: string): string {
  if (mime.includes("mp4") || mime.includes("aac")) return "m4a";
  if (mime.includes("ogg")) return "ogg";
  return "webm";
}

/** A name for a recording made at a time: `Recording 2026-10-13 05.30.12`. */
export function recordingName(at: Date): string {
  const two = (n: number) => String(n).padStart(2, "0");
  return `Recording ${at.getFullYear()}-${two(at.getMonth() + 1)}-${two(at.getDate())} ${two(at.getHours())}.${two(at.getMinutes())}.${two(at.getSeconds())}`;
}

/** A blob's bytes in base64. */
export async function base64Of(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(binary);
}

/** The MIME type of an audio file, by its extension, for playing it. */
export function audioType(path: string): string {
  const extension = path.toLowerCase().split(".").pop() ?? "";
  const types: Record<string, string> = {
    m4a: "audio/mp4",
    mp3: "audio/mpeg",
    wav: "audio/wav",
    webm: "audio/webm",
    ogg: "audio/ogg",
    oga: "audio/ogg",
    flac: "audio/flac",
    aac: "audio/aac",
  };
  return types[extension] ?? "audio/*";
}
