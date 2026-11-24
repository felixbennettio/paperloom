import type { CitationWork } from "./document";

export const CONNECTOR_URL = "http://127.0.0.1:23121";

export interface TokenStore {
  get(): string | null;
  set(token: string): void;
  clear(): void;
}

export class MemoryTokenStore implements TokenStore {
  constructor(private token: string | null = null) {}
  get(): string | null { return this.token; }
  set(token: string): void { this.token = token; }
  clear(): void { this.token = null; }
}

export class LocalStorageTokenStore extends MemoryTokenStore {
  private storage: Storage | undefined;

  constructor(storage?: Storage, private readonly key = "paperloom.addin.token") {
    super();
    try {
      this.storage = storage ?? globalThis.localStorage;
      const token = this.storage?.getItem(key);
      if (token) super.set(token);
    } catch {
      // Private browsing can deny storage; pairing still lasts for this pane.
    }
  }

  override set(token: string): void {
    super.set(token);
    try { this.storage?.setItem(this.key, token); } catch { /* Keep the session token. */ }
  }

  override clear(): void {
    super.clear();
    try { this.storage?.removeItem(this.key); } catch { /* Never reuse a rejected session token. */ }
  }
}

export type ConnectorErrorCode = "not-running" | "not-connected" | "declined" | "busy" | "other";

export class ConnectorError extends Error {
  constructor(public readonly code: ConnectorErrorCode, message: string, public readonly status?: number) {
    super(message);
    this.name = "ConnectorError";
  }
}

export interface CitationStyle { id: string; name: string }
export interface SearchItem { id: string; key: string; title: string; cite: string; year?: number | null }
export interface CitationCluster { works: CitationWork[] }
export interface FormatResult { citations: string[]; bibliography: string[]; missing: string[] }
export interface PingResult { app: string; version: string; cite: number }
/** A `[@key]` marker in a document's text and the works it names (none when
 * the library lacks one of them, which `missing` lists). */
export interface CiteMarker { text: string; works: CitationWork[]; missing: string[] }

export interface CiteConnector {
  hasToken(): boolean;
  ping(): Promise<PingResult>;
  pair(client: string): Promise<string>;
  styles(): Promise<CitationStyle[]>;
  search(query: string, limit?: number): Promise<SearchItem[]>;
  format(styleId: string, clusters: CitationCluster[], html: boolean): Promise<FormatResult>;
  markers(text: string): Promise<CiteMarker[]>;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function strings(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((entry) => typeof entry === "string");
}

function invalidResponse(): ConnectorError {
  return new ConnectorError("other", "Paperloom returned an invalid response.");
}

export class ConnectorClient implements CiteConnector {
  constructor(
    private readonly tokens: TokenStore,
    private readonly fetcher: typeof fetch = (input, init) => globalThis.fetch(input, init),
    private readonly baseUrl = CONNECTOR_URL,
  ) {}

  hasToken(): boolean { return !!this.tokens.get(); }

  private async request(path: string, body?: unknown, authenticated = true): Promise<unknown> {
    const headers: Record<string, string> = { Accept: "application/json" };
    if (authenticated) {
      const token = this.tokens.get();
      if (!token) throw new ConnectorError("not-connected", "Not connected to Paperloom.");
      headers.Authorization = `Bearer ${token}`;
    }
    if (body !== undefined) headers["Content-Type"] = "application/json";
    let response: Response;
    try {
      response = await this.fetcher(`${this.baseUrl}${path}`, {
        method: body === undefined ? "GET" : "POST",
        headers,
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
        credentials: "omit",
        cache: "no-store",
      });
    } catch {
      throw new ConnectorError("not-running", "Paperloom is not running on this computer.");
    }
    let data: unknown;
    try { data = await response.json(); } catch { data = null; }
    if (!response.ok) {
      const message = record(data) && typeof data.error === "string" ? data.error : "The Paperloom request failed.";
      const code: ConnectorErrorCode = response.status === 401 ? "not-connected"
        : response.status === 403 ? "declined" : response.status === 409 ? "busy" : "other";
      if (response.status === 401) this.tokens.clear();
      throw new ConnectorError(code, message, response.status);
    }
    return data;
  }

  async ping(): Promise<PingResult> {
    const data = await this.request("/ping", undefined, false);
    if (!record(data) || data.app !== "Paperloom" || typeof data.version !== "string") throw invalidResponse();
    if (typeof data.cite !== "number" || !Number.isFinite(data.cite) || data.cite < 1) {
      throw new ConnectorError("other", "Update Paperloom to use writing plugins.");
    }
    return { app: data.app, version: data.version, cite: data.cite };
  }

  async pair(client: string): Promise<string> {
    const data = await this.request("/cite/pair", { client }, false);
    if (!record(data) || typeof data.token !== "string" || !/^[a-f0-9]{64}$/i.test(data.token)) throw invalidResponse();
    this.tokens.set(data.token);
    return data.token;
  }

  async styles(): Promise<CitationStyle[]> {
    const data = await this.request("/cite/styles");
    if (!Array.isArray(data) || !data.every((s: unknown) => record(s) && typeof s.id === "string" && typeof s.name === "string")) {
      throw invalidResponse();
    }
    return data as CitationStyle[];
  }

  async search(query: string, limit = 20): Promise<SearchItem[]> {
    const data = await this.request("/cite/search", { query, limit });
    if (!Array.isArray(data) || !data.every((s: unknown) => record(s)
      && typeof s.id === "string" && typeof s.key === "string" && typeof s.title === "string" && typeof s.cite === "string"
      && (s.year === undefined || s.year === null || typeof s.year === "number"))) throw invalidResponse();
    return data as SearchItem[];
  }

  async format(styleId: string, clusters: CitationCluster[], html: boolean): Promise<FormatResult> {
    const data = await this.request("/cite/format", { styleId, clusters, html });
    if (!record(data) || !strings(data.citations) || !strings(data.bibliography) || !strings(data.missing)
      || data.citations.length !== clusters.length) throw invalidResponse();
    return { citations: data.citations, bibliography: data.bibliography, missing: data.missing };
  }

  async markers(text: string): Promise<CiteMarker[]> {
    let data: unknown;
    try {
      data = await this.request("/cite/markers", { text });
    } catch (error) {
      // Paperloom before cite API 2.
      if (error instanceof ConnectorError && error.status === 404) {
        throw new ConnectorError("other", "Update Paperloom to convert citation markers.");
      }
      throw error;
    }
    if (!Array.isArray(data) || !data.every((m: unknown) => record(m) && typeof m.text === "string"
      && strings(m.missing) && Array.isArray(m.works) && m.works.every((w: unknown) => record(w)
        && typeof w.itemId === "string" && (w.locator === undefined || w.locator === null || typeof w.locator === "string")))) {
      throw invalidResponse();
    }
    return (data as CiteMarker[]).map(({ text: marker, works, missing }) => ({
      text: marker,
      works: works.map(({ itemId, locator }) => (locator ? { itemId, locator } : { itemId })),
      missing,
    }));
  }
}
