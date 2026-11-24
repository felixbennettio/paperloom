import type { DocumentAdapter } from "./document";
import { LocalStorageTokenStore, type TokenStore } from "./connector";
import { MemoryDocument } from "./memoryDocument";
import { OfficeDocument } from "./officeDocument";
import { WpsDocument } from "./wpsDocument";

export interface AddinHost {
  clientName: string;
  tokens: TokenStore;
  document: DocumentAdapter;
  preview?: MemoryDocument;
  displayLanguage?: string;
}

let officeScript: Promise<void> | undefined;

function loadOfficeScript(): Promise<void> {
  officeScript ??= new Promise<void>((resolve, reject) => {
    const script = document.createElement("script");
    script.src = "https://appsforoffice.microsoft.com/lib/1/hosted/office.js";
    script.onload = () => resolve();
    script.onerror = () => {
      script.remove();
      officeScript = undefined;
      reject(new Error("Office.js could not be loaded."));
    };
    document.head.append(script);
  });
  return officeScript;
}

function createPreviewHost(): AddinHost {
  const document = new MemoryDocument();
  return { clientName: "Paperloom", tokens: new LocalStorageTokenStore(), document, preview: document };
}

export async function createHost(): Promise<AddinHost> {
  if (window.wps) {
    return {
      clientName: "WPS",
      tokens: new LocalStorageTokenStore(undefined, "paperloom.addin.wps.token"),
      document: new WpsDocument(window.wps.WpsApplication()),
      displayLanguage: navigator.language,
    };
  }
  // Loading Office.js in a browser preview can stall initialization.
  if (new URLSearchParams(window.location.search).has("_host_Info")) {
    await loadOfficeScript();
    const ready = await Office.onReady();
    if (ready.host !== "Word" || !Office.context.requirements.isSetSupported("WordApi", "1.1")) {
      throw new Error("This add-in requires Word with WordApi 1.1.");
    }
    return {
      clientName: "Word",
      tokens: new LocalStorageTokenStore(undefined, "paperloom.addin.word.token"),
      document: new OfficeDocument(),
      displayLanguage: Office.context.displayLanguage,
    };
  }
  return createPreviewHost();
}
