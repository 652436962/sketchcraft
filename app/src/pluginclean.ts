// What a plugin hands the interface (a result to show, a form to fill in) is
// untrusted: it is checked and cut down here before React sees it, so a wrong
// shape or a huge table can't blank or freeze the app. Pure, so checks/run.mjs
// can test it on its own.

export interface PluginResult {
  title?: string;
  text?: string;
  table?: { columns: string[]; rows: (string | number)[][] };
}

/** A form a plugin asks you to fill in (ctx.ask). Lengths are in meters. */
export interface PluginForm {
  title?: string;
  text?: string;
  ok?: string;
  fields: PluginField[];
}
export interface PluginField {
  id: string;
  label: string;
  /** "length" (typed in the model's units, answered in meters), "number", "text", "choice" or "check" */
  type?: "length" | "number" | "text" | "choice" | "check";
  value?: string | number | boolean;
  options?: string[];
  min?: number;
  max?: number;
  hint?: string;
}

// ---- cleaning -------------------------------------------------------------------

const str = (v: unknown, max: number): string | undefined => {
  if (typeof v === "string") return v.slice(0, max);
  if (v == null) return undefined;
  try {
    return String(v).slice(0, max);
  } catch {
    return undefined; // an object with no usable text
  }
};

/** A plugin's result, cleaned: strings and numbers only, within limits. */
export function cleanResult(r: unknown): PluginResult {
  const o = (r && typeof r === "object" ? r : {}) as Record<string, unknown>;
  const out: PluginResult = { title: str(o.title, 120), text: str(o.text, 20_000) };
  const t = o.table as { columns?: unknown; rows?: unknown } | undefined;
  if (t && Array.isArray(t.columns) && Array.isArray(t.rows)) {
    const columns = t.columns.slice(0, 40).map((c) => str(c, 80) ?? "");
    const rows = t.rows.slice(0, 2000).map((row) => (Array.isArray(row) ? row.slice(0, columns.length).map((c) => (typeof c === "number" && Number.isFinite(c) ? c : (str(c, 400) ?? ""))) : []));
    out.table = { columns, rows };
  }
  return out;
}

/** A plugin's form, cleaned the same way. */
export function cleanForm(f: unknown): PluginForm {
  const o = (f && typeof f === "object" ? f : {}) as Record<string, unknown>;
  const types = ["length", "number", "text", "choice", "check"];
  const fields: PluginField[] = (Array.isArray(o.fields) ? o.fields : [])
    .slice(0, 40)
    .filter((x): x is Record<string, unknown> => !!x && typeof x === "object" && typeof (x as Record<string, unknown>).id === "string")
    .map((x) => ({
      id: String(x.id).slice(0, 80),
      label: str(x.label, 120) ?? String(x.id).slice(0, 80),
      type: types.includes(x.type as string) ? (x.type as PluginField["type"]) : "text",
      value: typeof x.value === "number" && Number.isFinite(x.value) ? x.value : typeof x.value === "boolean" ? x.value : (str(x.value, 400) ?? ""),
      options: Array.isArray(x.options) ? x.options.slice(0, 100).map((s) => str(s, 80) ?? "") : undefined,
      min: typeof x.min === "number" && Number.isFinite(x.min) ? x.min : undefined,
      max: typeof x.max === "number" && Number.isFinite(x.max) ? x.max : undefined,
      hint: str(x.hint, 200),
    }));
  return { title: str(o.title, 120), text: str(o.text, 2000), ok: str(o.ok, 30), fields };
}
