import { describe, expect, it, vi } from "vitest";

// plugins.tsx reaches the engine module, which asks `window` whether it runs inside Tauri when it loads
vi.stubGlobal("window", {});
const { csvCell } = await import("../src/plugins");

describe("csvCell (the plugin result table as CSV)", () => {
  it("leaves plain text and numbers alone", () => {
    expect(csvCell("hello")).toBe("hello");
    expect(csvCell(3.5)).toBe("3.5");
    expect(csvCell(-5)).toBe("-5");
    expect(csvCell("-5")).toBe("-5");
    expect(csvCell("3.5")).toBe("3.5");
    expect(csvCell("1e5")).toBe("1e5");
  });
  it("quotes cells with commas, quotes and line breaks", () => {
    expect(csvCell("a,b")).toBe('"a,b"');
    expect(csvCell('say "hi"')).toBe('"say ""hi"""');
    expect(csvCell("a\nb")).toBe('"a\nb"');
    expect(csvCell("a\r\nb")).toBe('"a\r\nb"');
  });
  it("defuses a text cell that a spreadsheet would run as a formula", () => {
    expect(csvCell("+1+2")).toBe("'+1+2");
    expect(csvCell("@SUM(A1)")).toBe("'@SUM(A1)");
    expect(csvCell("-1+cmd")).toBe("'-1+cmd");
    expect(csvCell("=HYPERLINK(\"http://x\")")).toBe(`"'=HYPERLINK(""http://x"")"`);
    expect(csvCell("\tx")).toBe("'\tx");
    expect(csvCell("\rx")).toBe(`"'\rx"`);
  });
  it("turns missing values into empty cells", () => {
    expect(csvCell(undefined as unknown as string)).toBe("");
    expect(csvCell(null as unknown as string)).toBe("");
  });
});
