// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryDocument } from "./memoryDocument";
import { OfficeDocument } from "./officeDocument";
import { WpsDocument } from "./wpsDocument";

describe("task pane hosts", () => {
  beforeEach(() => {
    vi.resetModules();
    window.history.replaceState(null, "", "/addin.html");
    localStorage.clear();
  });

  afterEach(() => {
    delete window.wps;
    document.head.querySelectorAll('script[src*="appsforoffice.microsoft.com"]').forEach((script) => script.remove());
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    window.history.replaceState(null, "", "/addin.html");
  });

  it("keeps the browser preview free of Office.js", async () => {
    const { createHost } = await import("./hosts");
    const preview = await createHost();
    // resetModules gives the host a separate module instance; check its shape too.
    expect(preview.document.constructor.name).toBe(MemoryDocument.name);
    expect(preview.preview).toBe(preview.document);
    expect(preview.clientName).toBe("Paperloom");
    expect(document.head.querySelector('script[src*="office.js"]')).toBeNull();
  });

  it("selects the WPS application, navigator language and a separate persistent token without Office.js", async () => {
    // WPS wins even when a page carries an Office query parameter.
    window.history.replaceState(null, "", "/pane.html?_host_Info=Word%24Mac");
    const application: Wps.Application = {
      ActiveDocument: {
        Content: { Start: 0, End: 0, Text: "", Font: { Italic: false, Bold: false, Superscript: false, Subscript: false }, SetRange: vi.fn() },
        ContentControls: { Count: 0, Item: vi.fn(), Add: vi.fn() },
        Variables: { Item: () => undefined, Add: vi.fn() },
        Range: vi.fn(),
      },
      Selection: { Range: { Start: 0, End: 0, Text: "", Font: {
        Italic: false, Bold: false, Superscript: false, Subscript: false,
      }, SetRange: vi.fn() }, TypeParagraph: vi.fn() },
    };
    const WpsApplication = vi.fn(() => application);
    window.wps = { WpsApplication };
    localStorage.setItem("paperloom.addin.token", "preview-token");
    localStorage.setItem("paperloom.addin.word.token", "word-token");
    localStorage.setItem("paperloom.addin.wps.token", "wps-token");
    const { createHost } = await import("./hosts");
    const host = await createHost();
    expect(host.clientName).toBe("WPS");
    expect(host.document.constructor.name).toBe(WpsDocument.name);
    expect(await host.document.getCitations()).toEqual([]);
    expect(WpsApplication).toHaveBeenCalledOnce();
    expect(host.preview).toBeUndefined();
    expect(host.displayLanguage).toBe(navigator.language);
    expect(host.tokens.get()).toBe("wps-token");
    host.tokens.set("new-wps-token");
    expect((await createHost()).tokens.get()).toBe("new-wps-token");
    host.tokens.clear();
    expect(localStorage.getItem("paperloom.addin.wps.token")).toBeNull();
    expect(localStorage.getItem("paperloom.addin.word.token")).toBe("word-token");
    expect(localStorage.getItem("paperloom.addin.token")).toBe("preview-token");
    expect(document.head.querySelector('script[src*="office.js"]')).toBeNull();
  });

  it("surfaces WPS startup errors instead of using the preview", async () => {
    window.wps = { WpsApplication: () => { throw new Error("No active WPS application"); } };
    const { createHost } = await import("./hosts");
    await expect(createHost()).rejects.toThrow("No active WPS application");
    expect(document.head.querySelector('script[src*="office.js"]')).toBeNull();
  });

  it("loads Office.js only for _host_Info, waits for onReady and isolates the Word token", async () => {
    window.history.replaceState(null, "", "/addin.html?_host_Info=Word%24Mac%2416.01%24fr-FR%24test");
    let ready!: (value: { host: string; platform: string }) => void;
    const onReady = vi.fn(() => new Promise<{ host: string; platform: string }>((resolve) => { ready = resolve; }));
    vi.stubGlobal("Office", { onReady, context: { displayLanguage: "fr-FR",
      requirements: { isSetSupported: vi.fn(() => true) } } });
    // Not really appended: happy-dom would load the script, and fail it.
    const appended: HTMLScriptElement[] = [];
    vi.spyOn(document.head, "append").mockImplementation((...nodes) => {
      const script = nodes[0] as HTMLScriptElement;
      appended.push(script);
      queueMicrotask(() => script.dispatchEvent(new Event("load")));
    });
    localStorage.setItem("paperloom.addin.token", "preview-token");
    const { createHost } = await import("./hosts");
    let resolved = false;
    const pending = createHost().then((host) => { resolved = true; return host; });
    await vi.waitFor(() => expect(onReady).toHaveBeenCalledOnce());
    expect(resolved).toBe(false);
    expect(appended.map(({ src }) => src)).toEqual(["https://appsforoffice.microsoft.com/lib/1/hosted/office.js"]);
    ready({ host: "Word", platform: "Mac" });
    const host = await pending;
    expect(host.clientName).toBe("Word");
    expect(host.document.constructor.name).toBe(OfficeDocument.name);
    expect(host.preview).toBeUndefined();
    expect(host.displayLanguage).toBe("fr-FR");
    expect(host.tokens.get()).toBeNull();
    host.tokens.set("word-token");
    expect(localStorage.getItem("paperloom.addin.word.token")).toBe("word-token");
    expect(localStorage.getItem("paperloom.addin.token")).toBe("preview-token");
    onReady.mockResolvedValue({ host: "Word", platform: "Mac" });
    await createHost();
    expect(appended).toHaveLength(1);
  });

  it("reports script load failures instead of falling back to a memory document", async () => {
    window.history.replaceState(null, "", "/addin.html?_host_Info=Word%24Mac");
    vi.spyOn(document.head, "append").mockImplementation((...nodes) => {
      queueMicrotask(() => (nodes[0] as HTMLScriptElement).dispatchEvent(new Event("error")));
    });
    const { createHost } = await import("./hosts");
    await expect(createHost()).rejects.toThrow("Office.js could not be loaded.");
  });

  it.each(["Excel", "unsupported Word"])("rejects %s instead of silently using the preview", async (host) => {
    window.history.replaceState(null, "", "/addin.html?_host_Info=Word%24Mac");
    vi.stubGlobal("Office", { onReady: async () => ({ host: host === "Excel" ? "Excel" : "Word", platform: "Mac" }),
      context: { requirements: { isSetSupported: () => false } } });
    vi.spyOn(document.head, "append").mockImplementation((...nodes) => {
      queueMicrotask(() => (nodes[0] as HTMLScriptElement).dispatchEvent(new Event("load")));
    });
    const { createHost } = await import("./hosts");
    await expect(createHost()).rejects.toThrow("This add-in requires Word with WordApi 1.1.");
  });
});
