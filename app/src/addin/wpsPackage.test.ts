import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

function source(name: string): string {
  return readFileSync(new URL(`../../../integrations/wps/${name}`, import.meta.url), "utf8");
}

describe("WPS ribbon package", () => {
  it.each([undefined, 7])("creates one pane beside index.html, docks it right and toggles it (enum %s)", (dock) => {
    const pane = { Visible: false, DockPosition: 0, ID: "pane-1" };
    const CreateTaskPane = vi.fn(() => pane);
    const context = {
      window: { location: { href: "file:///home/test/jsaddons/Paperloom_0.1.0/index.html" } },
      URL,
      wps: { CreateTaskPane, Enum: dock === undefined ? undefined : { JSKsoEnum_msoCTPDockPositionRight: dock } },
      OnAddinLoad: (_ribbon: unknown) => false,
      OnAction: (_control: { Id: string }) => false,
      paperloomRibbon: undefined as unknown,
    };
    runInNewContext(source("main.js"), context);
    const ribbon = {};
    expect(context.OnAddinLoad(ribbon)).toBe(true);
    expect(context.paperloomRibbon).toBe(ribbon);
    context.OnAction({ Id: "unrelated" });
    expect(CreateTaskPane).not.toHaveBeenCalled();
    context.OnAction({ Id: "paperloomCite" });
    expect(CreateTaskPane).toHaveBeenCalledWith("file:///home/test/jsaddons/Paperloom_0.1.0/pane.html");
    expect(pane.Visible).toBe(true);
    expect(pane.DockPosition).toBe(dock ?? 2);
    context.OnAction({ Id: "paperloomCite" });
    expect(pane.Visible).toBe(false);
    context.OnAction({ Id: "paperloomCite" });
    expect(pane.Visible).toBe(true);
    expect(CreateTaskPane).toHaveBeenCalledOnce();
  });

  it("uses local classic scripts and matches the ribbon callbacks", () => {
    const index = source("index.html");
    const pane = source("pane.html");
    expect(index).toContain('<script src="main.js"></script>');
    expect(pane).toContain('<script src="pane.js"></script>');
    expect(pane).toContain('href="pane.css"');
    expect(index + pane).not.toMatch(/type=["']module|https?:\/\//);
    const ribbon = source("ribbon.xml");
    expect(ribbon).toContain('onLoad="OnAddinLoad"');
    expect(ribbon).toContain('onAction="OnAction"');
    expect(ribbon).toContain('id="paperloomCite" label="Cite"');
  });
});
