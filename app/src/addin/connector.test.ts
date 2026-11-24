import { describe, expect, it, vi } from "vitest";
import { CONNECTOR_URL, ConnectorClient, LocalStorageTokenStore, MemoryTokenStore } from "./connector";

function setup(token: string | null = "saved-token") {
  const tokens = new MemoryTokenStore(token);
  const fetcher = vi.fn<typeof fetch>();
  return { tokens, fetcher, client: new ConnectorClient(tokens, fetcher) };
}

function json(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), { status, headers: { "Content-Type": "application/json" } });
}

describe("cite connector", () => {
  it("pings without a token and requires cite API version 1 or newer", async () => {
    const { client, fetcher } = setup(null);
    fetcher.mockResolvedValueOnce(json({ app: "Paperloom", version: "0.1.0", cite: 1 }));
    expect(await client.ping()).toEqual({ app: "Paperloom", version: "0.1.0", cite: 1 });
    expect(fetcher.mock.calls[0][0]).toBe(`${CONNECTOR_URL}/ping`);
    expect(fetcher.mock.calls[0][1]?.headers).not.toHaveProperty("Authorization");
    for (const cite of [undefined, 0]) {
      fetcher.mockResolvedValueOnce(json({ app: "Paperloom", version: "0.1.0", cite }));
      await expect(client.ping()).rejects.toMatchObject({ code: "other", message: "Update Paperloom to use writing plugins." });
    }
    fetcher.mockResolvedValueOnce(json({ app: "Paperloom", version: "0.2.0", cite: 2 }));
    expect((await client.ping()).cite).toBe(2);
  });

  it("pairs without authorization and stores the allowed token", async () => {
    const { client, tokens, fetcher } = setup(null);
    const token = "a".repeat(64);
    fetcher.mockResolvedValueOnce(json({ token }));
    expect(await client.pair("Word")).toBe(token);
    expect(tokens.get()).toBe(token);
    expect(client.hasToken()).toBe(true);
    expect(fetcher).toHaveBeenCalledWith(`${CONNECTOR_URL}/cite/pair`, expect.objectContaining({
      method: "POST", body: JSON.stringify({ client: "Word" }),
      headers: { Accept: "application/json", "Content-Type": "application/json" },
    }));
  });

  it("uses the contract endpoints, bearer token and JSON bodies", async () => {
    const { client, fetcher } = setup();
    const styles = [{ id: "numeric", name: "Numeric" }];
    const items = [{ id: "one", key: "one2026", title: "One", cite: "Author 2026", year: 2026 }];
    const clusters = [{ works: [{ itemId: "one", locator: "12" }] }];
    const formatted = { citations: ["<sup>1</sup>"], bibliography: ["[1] <i>One</i>"], missing: [] };
    fetcher.mockResolvedValueOnce(json(styles)).mockResolvedValueOnce(json(items)).mockResolvedValueOnce(json(formatted));
    expect(await client.styles()).toEqual(styles);
    expect(await client.search("author", 10)).toEqual(items);
    expect(await client.format("numeric", clusters, true)).toEqual(formatted);
    expect(fetcher.mock.calls.map(([url]) => url)).toEqual([
      `${CONNECTOR_URL}/cite/styles`, `${CONNECTOR_URL}/cite/search`, `${CONNECTOR_URL}/cite/format`,
    ]);
    for (const [, init] of fetcher.mock.calls) expect(init?.headers).toHaveProperty("Authorization", "Bearer saved-token");
    expect(fetcher.mock.calls[1][1]?.body).toBe(JSON.stringify({ query: "author", limit: 10 }));
    expect(fetcher.mock.calls[2][1]?.body).toBe(JSON.stringify({ styleId: "numeric", clusters, html: true }));
  });

  it("reports not connected without sending an unauthenticated cite request", async () => {
    const { client, fetcher } = setup(null);
    await expect(client.search("")).rejects.toMatchObject({ code: "not-connected" });
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("clears a rejected token on 401 and reports not connected", async () => {
    const { client, tokens, fetcher } = setup();
    fetcher.mockResolvedValueOnce(json({ error: "Removed in the app" }, 401));
    await expect(client.styles()).rejects.toMatchObject({ code: "not-connected", status: 401, message: "Removed in the app" });
    expect(tokens.get()).toBeNull();
    expect(client.hasToken()).toBe(false);
    await expect(client.search("")).rejects.toMatchObject({ code: "not-connected" });
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("still clears a token when a 401 body is not JSON", async () => {
    const { client, tokens, fetcher } = setup();
    fetcher.mockResolvedValueOnce(new Response("Unauthorized", { status: 401 }));
    await expect(client.styles()).rejects.toMatchObject({ code: "not-connected" });
    expect(tokens.get()).toBeNull();
  });

  it("maps a failed fetch to not running without discarding the token", async () => {
    const { client, tokens, fetcher } = setup();
    fetcher.mockRejectedValueOnce(new TypeError("Failed to fetch"));
    await expect(client.ping()).rejects.toMatchObject({ code: "not-running" });
    expect(tokens.get()).toBe("saved-token");
  });

  it.each([[403, "declined"], [409, "busy"], [500, "other"]] as const)("maps HTTP %s to %s and retains the server message", async (status, code) => {
    const { client, tokens, fetcher } = setup(null);
    fetcher.mockResolvedValueOnce(json({ error: "Explanation from Paperloom" }, status));
    await expect(client.pair("WPS")).rejects.toMatchObject({ code, status, message: "Explanation from Paperloom" });
    expect(tokens.get()).toBeNull();
  });

  it("rejects malformed success bodies before they can change the document or token", async () => {
    const { client, tokens, fetcher } = setup();
    fetcher.mockResolvedValueOnce(json({ token: "" }));
    await expect(client.pair("Word")).rejects.toMatchObject({ code: "other" });
    expect(tokens.get()).toBe("saved-token");
    fetcher.mockResolvedValueOnce(json({ citations: [], bibliography: [], missing: [] }));
    await expect(client.format("numeric", [{ works: [{ itemId: "one" }] }], true)).rejects.toMatchObject({ code: "other" });
  });

  it("reads a document's citation markers, and asks for a newer Paperloom without the endpoint", async () => {
    const { client, fetcher } = setup();
    fetcher.mockResolvedValueOnce(json([
      { text: "[@a, p. 3]", works: [{ itemId: "a", locator: "3" }], missing: [] },
      { text: "[@b]", works: [{ itemId: "b", locator: null }], missing: [] },
      { text: "[@c]", works: [], missing: ["c"] },
    ]));
    expect(await client.markers("A [@a, p. 3] [@b] [@c]")).toEqual([
      { text: "[@a, p. 3]", works: [{ itemId: "a", locator: "3" }], missing: [] },
      { text: "[@b]", works: [{ itemId: "b" }], missing: [] },
      { text: "[@c]", works: [], missing: ["c"] },
    ]);
    expect(fetcher).toHaveBeenLastCalledWith(`${CONNECTOR_URL}/cite/markers`, expect.objectContaining({
      method: "POST", body: JSON.stringify({ text: "A [@a, p. 3] [@b] [@c]" }),
    }));
    fetcher.mockResolvedValueOnce(json({ error: "There is nothing here." }, 404));
    await expect(client.markers("[@a]")).rejects.toMatchObject({ code: "other", message: "Update Paperloom to convert citation markers." });
    fetcher.mockResolvedValueOnce(json([{ text: "[@a]", works: "a", missing: [] }]));
    await expect(client.markers("[@a]")).rejects.toMatchObject({ message: "Paperloom returned an invalid response." });
  });

  it("defaults search to 20 newest items for an empty query", async () => {
    const { client, fetcher } = setup();
    fetcher.mockResolvedValueOnce(json([]));
    await client.search("");
    expect(fetcher.mock.calls[0][1]?.body).toBe(JSON.stringify({ query: "", limit: 20 }));
  });
});

describe("token storage", () => {
  it("persists and removes only its own localStorage key", () => {
    const values = new Map<string, string>([["unrelated", "keep"]]);
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
      removeItem: (key: string) => { values.delete(key); },
    } as Storage;
    const tokens = new LocalStorageTokenStore(storage);
    tokens.set("paired");
    expect(new LocalStorageTokenStore(storage).get()).toBe("paired");
    tokens.clear();
    expect(tokens.get()).toBeNull();
    expect(new LocalStorageTokenStore(storage).get()).toBeNull();
    expect(values.get("unrelated")).toBe("keep");
  });

  it("keeps working for this session when storage is denied", () => {
    const denied = () => { throw new Error("Storage denied"); };
    const tokens = new LocalStorageTokenStore({ getItem: denied, setItem: denied, removeItem: denied } as unknown as Storage);
    tokens.set("paired");
    expect(tokens.get()).toBe("paired");
    tokens.clear();
    expect(tokens.get()).toBeNull();
  });
});
