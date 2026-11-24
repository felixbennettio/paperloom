import { describe, expect, it } from "vitest";
import {
  closeInPanes,
  closeTab,
  dropInPanes,
  focusedTab,
  goBack,
  initialPanes,
  initialTabs,
  joinPanes,
  moveAcross,
  moveTab,
  openInPanes,
  openTab,
  selectInPanes,
  selectTab,
  selectedTab,
  tabId,
  visit,
  type Tab,
} from "./workbench";

const paper = (id: string): Tab => ({ kind: "item", id, title: `Paper ${id}` });

describe("workbench tabs", () => {
  it("starts with the library in front", () => {
    expect(selectedTab(initialTabs)).toEqual({ kind: "library" });
  });

  it("opens a tab once and brings it to the front", () => {
    let s = openTab(initialTabs, paper("a"));
    s = openTab(s, { kind: "notes" });
    s = openTab(s, paper("a"));
    expect(s.tabs.map(tabId)).toEqual(["library", "item:a", "notes"]);
    expect(s.selected).toBe("item:a");
  });

  it("never closes the library", () => {
    expect(closeTab(initialTabs, "library")).toBe(initialTabs);
  });

  it("selects the neighbour when the front tab closes", () => {
    let s = [paper("a"), paper("b"), paper("c")].reduce(openTab, initialTabs);
    s = selectTab(s, "item:b");
    s = closeTab(s, "item:b");
    expect(s.selected).toBe("item:c");
    s = closeTab(s, "item:c");
    expect(s.selected).toBe("item:a");
    expect(closeTab(s, "missing")).toBe(s);
  });

  it("keeps the selection when a background tab closes", () => {
    let s = [paper("a"), paper("b")].reduce(openTab, initialTabs);
    s = closeTab(s, "item:a");
    expect(s.selected).toBe("item:b");
  });

  it("moves tabs", () => {
    const s = [paper("a"), paper("b")].reduce(openTab, initialTabs);
    expect(moveTab(s, "item:b", 0).tabs.map(tabId)).toEqual(["item:b", "library", "item:a"]);
    expect(moveTab(s, "library", 99).tabs.map(tabId)).toEqual(["item:a", "item:b", "library"]);
    expect(selectTab(s, "nope")).toBe(s);
  });
});

describe("split panes", () => {
  const two = [paper("a"), paper("b")].reduce(openInPanes, initialPanes);

  it("opens tabs where the focus is and keeps each tab in one pane", () => {
    expect(two.main.tabs.map(tabId)).toEqual(["library", "item:a", "item:b"]);
    let p = moveAcross(two, "item:b");
    expect(p.main.tabs.map(tabId)).toEqual(["library", "item:a"]);
    expect(p.side?.tabs.map(tabId)).toEqual(["item:b"]);
    expect(p.focus).toBe("side");
    expect(focusedTab(p)).toEqual(paper("b"));
    p = openInPanes(p, paper("c"));
    expect(p.side?.tabs.map(tabId)).toEqual(["item:b", "item:c"]);
    p = openInPanes(selectInPanes(p, "item:a"), paper("b"));
    expect(p.side?.selected).toBe("item:b");
    expect(p.focus).toBe("side");
    expect(moveAcross(p, "library")).toBe(p);
  });

  it("drops tabs across panes and closes an empty side pane", () => {
    let p = moveAcross(two, "item:b");
    p = dropInPanes(p, "item:a", "side", 0);
    expect(p.side?.tabs.map(tabId)).toEqual(["item:a", "item:b"]);
    expect(p.main.tabs.map(tabId)).toEqual(["library"]);
    p = dropInPanes(p, "item:b", "side", 0);
    expect(p.side?.tabs.map(tabId)).toEqual(["item:b", "item:a"]);
    p = closeInPanes(closeInPanes(p, "item:a"), "item:b");
    expect(p.focus).toBe("main");
    expect(p.side).toBeNull();
  });

  it("joins the side pane back", () => {
    const p = joinPanes(moveAcross(two, "item:b"));
    expect(p.side).toBeNull();
    expect(p.main.tabs.map(tabId)).toEqual(["library", "item:a", "item:b"]);
    expect(p.main.selected).toBe("item:b");
    expect(joinPanes(initialPanes)).toBe(initialPanes);
  });
});

describe("going back on a phone", () => {
  const item: Tab = { kind: "item", id: "a", title: "A" };
  const pdf: Tab = { kind: "pdf", id: "p", title: "P", itemId: "a" };

  it("closes the page in front and shows the one seen before", () => {
    let panes = openInPanes(openInPanes(initialPanes, item), pdf);
    const history = ["library", "item:a", "pdf:p"].reduce(visit, [] as string[]);
    panes = goBack(panes, history)!;
    expect(tabId(focusedTab(panes))).toBe("item:a");
    expect(panes.main.tabs.map(tabId)).toEqual(["library", "item:a"]);
    expect(tabId(focusedTab(goBack(panes, history)!))).toBe("library");
  });

  it("skips tabs closed since", () => {
    const panes = openInPanes(openInPanes(initialPanes, { kind: "notes" }), { kind: "settings" });
    const history = ["library", "item:gone", "notes", "settings"].reduce(visit, [] as string[]);
    const back = goBack(panes, history)!;
    expect(tabId(focusedTab(back))).toBe("notes");
    expect(tabId(focusedTab(goBack(back, visit(history, "notes"))!))).toBe("library");
  });

  it("does nothing on the library", () => {
    expect(goBack(initialPanes, ["library"])).toBeNull();
    expect(visit(["a", "b", "a"].reduce(visit, [] as string[]), "b")).toEqual(["a", "b"]);
  });
});
