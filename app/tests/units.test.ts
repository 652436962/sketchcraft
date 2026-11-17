import { describe, expect, it } from "vitest";
import { formatLength, parseLength, parseLengths } from "../src/units";

const INCH = 0.0254;
const close = (a: number, b: number) => expect(a).toBeCloseTo(b, 9);

describe("parseLength", () => {
  it("reads a bare number in the current unit", () => {
    close(parseLength("2.5", "m"), 2.5);
    close(parseLength("250", "cm"), 2.5);
    close(parseLength("2500", "mm"), 2.5);
    close(parseLength("96", "in"), 96 * INCH);
    close(parseLength("8", "ft"), 8 * 12 * INCH);
  });
  it("reads explicit units whatever the current one is", () => {
    close(parseLength("2.5m", "ft"), 2.5);
    close(parseLength("250cm", "m"), 2.5);
    close(parseLength("2500mm", "in"), 2.5);
    close(parseLength('96"', "m"), 96 * INCH);
    close(parseLength("96in", "m"), 96 * INCH);
    close(parseLength("96 inches", "m"), 96 * INCH);
  });
  it("reads feet and inches", () => {
    close(parseLength("8'", "m"), 8 * 12 * INCH);
    close(parseLength("8ft", "m"), 8 * 12 * INCH);
    close(parseLength("10'6\"", "m"), (10 * 12 + 6) * INCH);
    close(parseLength("10' 6 1/2\"", "m"), (10 * 12 + 6.5) * INCH);
    close(parseLength("10ft 6in", "m"), (10 * 12 + 6) * INCH);
    close(parseLength("-3'", "m"), -36 * INCH);
    close(parseLength("-3' 2\"", "m"), -(36 + 2) * INCH);
  });
  it("reads fractions", () => {
    close(parseLength("1/2", "in"), 0.5 * INCH);
    close(parseLength("6 1/2", "in"), 6.5 * INCH);
    close(parseLength("-6 1/2", "in"), -6.5 * INCH);
  });
  it("ignores case and surrounding space", () => {
    close(parseLength("  250CM ", "m"), 2.5);
    close(parseLength("8FT", "m"), 8 * 12 * INCH);
  });
  it("gives NaN for what is not a length", () => {
    for (const bad of ["", "  ", "abc", "5 parsecs", "1/", "--3", "3..4m", "m", "'", "10'x"]) expect(parseLength(bad, "m"), JSON.stringify(bad)).toBeNaN();
  });
  it("falls back to meters for an unknown current unit", () => {
    close(parseLength("3", "furlongs"), 3);
  });
});

describe("parseLengths", () => {
  it("splits on commas and semicolons", () => {
    expect(parseLengths("8,12", "m")).toEqual([8, 12]);
    expect(parseLengths("8; 12", "m")).toEqual([8, 12]);
    expect(parseLengths("4,6", "cm").map((x) => Math.round(x * 1000))).toEqual([40, 60]);
  });
  it("splits on spaces only where a new number follows a unit mark", () => {
    const [a, b] = parseLengths("8' 12'", "m");
    close(a, 96 * INCH);
    close(b, 144 * INCH);
    const [c, d] = parseLengths("10'6\" 12'", "m");
    close(c, 126 * INCH);
    close(d, 144 * INCH);
  });
  // BUG (found by this test, recorded in the Sprint 0 report): the comment in splitSpaces says "10' 6 1/2\"" is kept
  // together, but it comes back as two lengths (10' and 6 1/2"), so typing it into a Rectangle reads width and height.
  it.fails("keeps feet, a space and inches together as one length", () => {
    expect(parseLengths("10' 6 1/2\"", "m")).toHaveLength(1);
  });
  it("drops what does not parse", () => {
    expect(parseLengths("8, x, 12", "m")).toEqual([8, 12]);
    expect(parseLengths("", "m")).toEqual([]);
  });
});

describe("formatLength", () => {
  it("formats each unit", () => {
    expect(formatLength(2.5, "m")).toBe("2.50 m");
    expect(formatLength(2.5, "cm")).toBe("250.0 cm");
    expect(formatLength(2.5, "mm")).toBe("2500 mm");
    expect(formatLength(96 * INCH, "in")).toBe('96"');
    expect(formatLength((10 * 12 + 6.5) * INCH, "ft")).toBe(`10' 6 1/2"`);
    expect(formatLength(6 * INCH, "ft")).toBe('6"');
    expect(formatLength(12 * INCH, "ft")).toBe(`1' 0"`);
    expect(formatLength(-(10 * 12 + 6) * INCH, "ft")).toBe(`-10' 6"`);
    expect(formatLength(0, "ft")).toBe('0"');
  });
  it("uses meters for an unknown unit", () => {
    expect(formatLength(1, "parsec")).toBe("1.00 m");
  });
  it("rounds to eighths of an inch", () => {
    expect(formatLength(0.3 * INCH, "in")).toBe('1/4"');
    expect(formatLength(1.06 * INCH, "in")).toBe('1"');
  });
  it("parses back what it formats (to the precision it shows)", () => {
    for (const m of [0.0254, 0.3048, 2.4384, 3.2004, 10.668]) {
      const text = formatLength(m, "ft");
      expect(Math.abs(parseLength(text, "ft") - m)).toBeLessThan(INCH / 8 + 1e-9);
    }
  });
});
