import { describe, expect, it } from "vitest";
import { colorOk, fontOk, lengthOk, skinValueOk } from "../src/skinvalue";
import { iconOk, isDark, readSkin, SKINS, TOKENS } from "../src/skins";

describe("skin values", () => {
  it("accepts plain colors, lengths and fonts", () => {
    for (const c of ["#fff", "#ffff", "#ffffff", "#12345678", "rgb(1,2,3)", "rgba(32,18,63,0.95)", "hsl(200 50% 40%)", "rgb(10 20 30 / 50%)", "white", "transparent"]) expect(colorOk(c), c).toBe(true);
    for (const l of ["4px", "0", "0.5rem", ".5em", "50%", "4px 8px", "1px 2px 3px 4px"]) expect(lengthOk(l), l).toBe(true);
    for (const f of [`"Bricolage Grotesque Variable", system-ui`, "Georgia, serif", `'IBM Plex Mono', monospace`, "-apple-system, BlinkMacSystemFont"]) expect(fontOk(f), f).toBe(true);
  });
  it("refuses anything that could load or run something", () => {
    for (const c of ["url(https://x/y.png)", "image-set(\"a\" 1x)", "-webkit-image-set(\"a\" 1x)", "u\\72l(x)", "red; background: url(x)", "var(--x)", "expression(alert(1))", "#12", "#12345", "rgb(1,2,3) url(x)", "", "calc(1px)"]) expect(colorOk(c), c).toBe(false);
    for (const l of ["calc(1px + 2px)", "4px url(x)", "red", "", "1 2 3 4 5"]) expect(lengthOk(l), l).toBe(false);
    for (const f of [`"a", url(x)`, `"a\\"b"`, "a; b", "a{b}", `"x", image-set("a" 1x)`, "", "1abc"]) expect(fontOk(f), f).toBe(false);
  });
  it("applies each token's own rule", () => {
    expect(skinValueOk("radius", "4px")).toBe(true);
    expect(skinValueOk("radius", "red")).toBe(false);
    expect(skinValueOk("bg", "4px")).toBe(false);
    expect(skinValueOk("font", "Georgia, serif")).toBe(true);
    expect(skinValueOk("font", "#fff")).toBe(false);
  });
  it("refuses non-strings, empty and very long values", () => {
    for (const v of [5, null, undefined, {}, [], true, ""]) expect(skinValueOk("bg", v)).toBe(false);
    expect(skinValueOk("bg", "a".repeat(200))).toBe(false);
  });
});

describe("the built-in skins", () => {
  it("have unique ids and every token with a valid value", () => {
    expect(new Set(SKINS.map((s) => s.id)).size).toBe(SKINS.length);
    for (const s of SKINS) for (const k of TOKENS) expect(skinValueOk(k, s.tokens[k]), `${s.id}.${k} = ${s.tokens[k]}`).toBe(true);
  });
  it("say whether they are dark by their panels", () => {
    for (const s of SKINS) expect(s.dark, s.id).toBe(isDark(s.tokens.panel));
  });
});

describe("readSkin (a theme file someone shared)", () => {
  const good = { name: "Mine", blurb: "b", tokens: { bg: "#101010", panel: "#202020", radius: "2px" }, backdrop: "night" };
  it("reads a good skin and fills the gaps from the first built-in", () => {
    const s = readSkin(good, "file")!;
    expect(s.id).toBe("file:Mine");
    expect(s.tokens.bg).toBe("#101010");
    expect(s.tokens.accent).toBe(SKINS[0].tokens.accent);
    expect(s.dark).toBe(true);
    expect(s.backdrop).toBe("night");
  });
  it("drops token values that are not plain, keeping the built-in's", () => {
    const s = readSkin({ tokens: { bg: "url(https://evil/x.png)", font: "a; b", radius: "calc(1px)", accent: "#ff0000" } }, "f")!;
    expect(s.tokens.bg).toBe(SKINS[0].tokens.bg);
    expect(s.tokens.font).toBe(SKINS[0].tokens.font);
    expect(s.tokens.radius).toBe(SKINS[0].tokens.radius);
    expect(s.tokens.accent).toBe("#ff0000");
  });
  it("names it, cutting long names, falling back to the file name", () => {
    expect(readSkin({ tokens: {}, name: "x".repeat(100) }, "f")!.name).toHaveLength(40);
    expect(readSkin({ tokens: {}, name: "   " }, "from-file")!.name).toBe("from-file");
    expect(readSkin({ tokens: {}, name: 7 }, "from-file")!.name).toBe("from-file");
    expect(readSkin({ tokens: {}, blurb: "y".repeat(500) }, "f")!.blurb).toHaveLength(120);
  });
  it("ignores a backdrop it does not know", () => {
    expect(readSkin({ tokens: {}, backdrop: "nowhere" }, "f")!.backdrop).toBeUndefined();
  });
  it("is not a skin without tokens", () => {
    for (const raw of [null, undefined, 5, "x", [], {}, { tokens: null }, { tokens: "x" }]) expect(readSkin(raw, "f"), JSON.stringify(raw)).toBeNull();
  });
  it("keeps only safe icons, under safe names", () => {
    const png = "data:image/png;base64,iVBORw0KGgo=";
    const s = readSkin({ tokens: {}, icons: { line: png, "bad name": png, rect: "https://x/y.png", circle: "data:text/html;base64,PGI+", arc: "data:image/svg+xml;utf8,<svg/>" } }, "f")!;
    expect(s.icons).toEqual({ line: png });
    expect(readSkin({ tokens: {}, icons: { rect: "javascript:1" } }, "f")!.icons).toBeUndefined();
    expect(readSkin({ tokens: {}, icons: [png] }, "f")!.icons).toBeUndefined();
  });
});

describe("iconOk", () => {
  it("takes base64 SVG, PNG, JPEG and WebP data URLs only", () => {
    for (const k of ["svg+xml", "png", "jpeg", "webp"]) expect(iconOk(`data:image/${k};base64,AAAA`), k).toBe(true);
    for (const v of ["data:image/gif;base64,AAAA", "data:image/png,AAAA", "https://x/y.png", "data:image/png;base64,AA AA", 5, null, "data:image/png;base64,"]) expect(iconOk(v), String(v)).toBe(false);
  });
  it("limits the size", () => {
    expect(iconOk("data:image/png;base64," + "A".repeat(350_000))).toBe(false);
  });
});

describe("isDark", () => {
  it("judges a #rrggbb color by its lightness", () => {
    expect(isDark("#000000")).toBe(true);
    expect(isDark("#202020")).toBe(true);
    expect(isDark("#ffffff")).toBe(false);
    expect(isDark("#f7f7f7")).toBe(false);
  });
  it("calls anything else light", () => {
    for (const v of ["rgb(0,0,0)", "#000", "black", "", "#00000"]) expect(isDark(v), v).toBe(false);
  });
});
