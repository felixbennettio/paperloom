import { backend, isTauri, type TranslatorRequest } from "./api";
import { pluginFrameUrl } from "./plugins";

export const TRANSLATORS_CHANGED = "paperloom-translators-changed";
export interface DetectedTranslator {
  id: string;
  label: string;
  itemType: string;
}
export type ItemPicker = (items: Record<string, string>, signal: AbortSignal) => Promise<string[]>;

type Pending = {
  kind: "translate" | "save";
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
};

/** One isolated translation frame, created only when translators are installed. */
export class TranslatorHost {
  private frame: HTMLIFrameElement | null = null;
  private pending = new Map<number, Pending>();
  private nextId = 1;
  private generation = 0;
  private loaded: Promise<void> | null = null;
  private ready: (() => void) | null = null;
  private cancelReady: ((error: Error) => void) | null = null;
  private loadTimer: ReturnType<typeof setTimeout> | null = null;
  private listening = false;
  private detected = false;
  private saving = false;
  private pickerAbort = new AbortController();
  private picker: ItemPicker = async () => [];

  setPicker(picker: ItemPicker) { this.picker = picker; }

  private receive = (event: MessageEvent) => {
    if (!this.frame || event.source !== this.frame.contentWindow || !event.data || typeof event.data !== "object") return;
    const message = event.data as Record<string, unknown>;
    if (message.type === "loaded") {
      if (this.loadTimer) clearTimeout(this.loadTimer);
      this.loadTimer = null;
      this.ready?.();
      this.ready = null;
      this.cancelReady = null;
    } else if (message.type === "call") {
      void this.handleCall(message, this.frame);
    } else if (message.type === "detected" || message.type === "translated" || message.type === "translate-error" || message.type === "error") {
      const id = typeof message.id === "number" ? message.id : -1;
      const pending = this.pending.get(id);
      if (!pending) return;
      if (message.type === "detected" && pending.kind !== "translate" || message.type === "translated" && pending.kind !== "save") return;
      clearTimeout(pending.timer);
      this.pending.delete(id);
      if (message.type === "translate-error" || message.type === "error") {
        pending.reject(new Error(String(message.message ?? "Translation failed.")));
      } else if (message.type === "detected") {
        const valid = typeof message.translator === "string" && typeof message.label === "string" && typeof message.itemType === "string";
        this.detected = valid;
        pending.resolve(valid ? { id: message.translator, label: message.label, itemType: message.itemType } : null);
      } else if (Array.isArray(message.items) && message.items.every((item) => item && typeof item === "object" && !Array.isArray(item))) {
        pending.resolve(message.items);
      } else {
        pending.reject(new Error("Translator returned invalid items."));
      }
    }
  };

  private async handleCall(message: Record<string, unknown>, frame: HTMLIFrameElement) {
    if (typeof message.id !== "number" || !Array.isArray(message.args)) return;
    const args = message.args as unknown[];
    const reply = (answer: object) => {
      if (this.frame === frame) frame.contentWindow?.postMessage({ type: "reply", id: message.id, ...answer }, "*");
    };
    try {
      let value: unknown;
      switch (message.method) {
        case "translatorCode":
          if (typeof args[0] !== "string") throw new Error("Invalid translator id.");
          value = await backend.translatorCode(args[0]);
          break;
        case "translators":
          value = await backend.translatorCatalog();
          break;
        case "request": {
          const request = args[0] as TranslatorRequest | null;
          if (!request || typeof request.url !== "string") throw new Error("Invalid translator request.");
          const url = new URL(request.url);
          const method = (request.method ?? "GET").toUpperCase();
          if (!["http:", "https:"].includes(url.protocol) || !["GET", "POST"].includes(method)) throw new Error("Invalid translator request.");
          if (request.headers && (typeof request.headers !== "object" || Array.isArray(request.headers) || Object.values(request.headers).some((header) => typeof header !== "string"))) throw new Error("Invalid request headers.");
          if (request.body != null && typeof request.body !== "string") throw new Error("Invalid request body.");
          value = await backend.translatorRequest({ ...request, method });
          break;
        }
        case "selectItems": {
          if (!this.saving) throw new Error("Item selection is available only while saving.");
          const items = args[0];
          if (!items || typeof items !== "object" || Array.isArray(items)) throw new Error("Invalid item selection.");
          const labels = Object.fromEntries(Object.entries(items).map(([key, label]) => [key, String(label)]));
          const keys = await this.picker(labels, this.pickerAbort.signal);
          value = keys.filter((key) => Object.prototype.hasOwnProperty.call(labels, key));
          break;
        }
        default: throw new Error("Unknown translator method.");
      }
      reply({ value });
    } catch (error) {
      reply({ error: error instanceof Error ? error.message : String(error) });
    }
  }

  private start() {
    if (this.loaded) return this.loaded;
    if (!this.listening) {
      window.addEventListener("message", this.receive);
      this.listening = true;
    }
    const frame = document.createElement("iframe");
    frame.setAttribute("sandbox", "allow-scripts");
    frame.hidden = true;
    frame.title = "Zotero translators";
    frame.src = isTauri() ? pluginFrameUrl(navigator.userAgent, "translate.html") : "/translate.html";
    this.frame = frame;
    this.loaded = new Promise<void>((resolve, reject) => {
      this.ready = resolve;
      this.cancelReady = reject;
      this.loadTimer = setTimeout(() => this.reset("Translation frame did not load."), 10000);
    });
    document.body.appendChild(frame);
    return this.loaded;
  }

  private ask(kind: Pending["kind"], values: object): Promise<unknown> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => this.reset("Translation exceeded 60 seconds."), 60000);
      this.pending.set(id, { kind, resolve, reject, timer });
      this.frame?.contentWindow?.postMessage({ type: kind === "translate" ? "translate" : "save", id, ...values }, "*");
    });
  }

  async detect(url: string, html: string): Promise<DetectedTranslator | null> {
    this.reset();
    const generation = this.generation;
    const translators = await backend.webTranslators();
    if (generation !== this.generation || !translators.length) return null;
    if (new TextEncoder().encode(html).length > 5 * 1024 * 1024) throw new Error("Page HTML exceeds 5 MB.");
    await this.start();
    if (generation !== this.generation) return null;
    return this.ask("translate", { url, html, translators }) as Promise<DetectedTranslator | null>;
  }

  async save(): Promise<Record<string, unknown>[]> {
    if (!this.detected || this.saving) throw new Error("No translator ready for saving.");
    this.saving = true;
    this.pickerAbort = new AbortController();
    const abort = this.pickerAbort;
    try { return await this.ask("save", {}) as Record<string, unknown>[]; }
    finally { if (this.pickerAbort === abort) this.saving = false; abort.abort(); }
  }

  reset(reason = "Translation canceled.") {
    this.generation++;
    this.pickerAbort.abort();
    this.frame?.remove();
    this.frame = null;
    this.detected = false;
    this.saving = false;
    if (this.loadTimer) clearTimeout(this.loadTimer);
    this.loadTimer = null;
    this.cancelReady?.(new Error(reason));
    this.ready = null;
    this.cancelReady = null;
    this.loaded = null;
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(new Error(reason));
    }
    this.pending.clear();
  }

  dispose() {
    this.reset();
    if (this.listening) window.removeEventListener("message", this.receive);
    this.listening = false;
  }
}

export const translatorHost = new TranslatorHost();
