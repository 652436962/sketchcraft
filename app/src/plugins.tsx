// Plugins: small scripts that add commands to SketchCraft (see docs/PLUGINS.md).
// Each run gets a fresh Web Worker: no page, no network, no files. Its only
// way out is ctx.run(), and the app refuses commands that read or write files.
import { useRef, useState } from "react";
import { confirm } from "./ask";
import { exec, type Reply } from "./engine";
import { pickSave } from "./files";

export interface PluginCommand {
  id: string;
  label: string;
  hint?: string;
  /** its icon (a data URL), for the tool bar */
  icon?: string | null;
}

export interface Plugin {
  id: string;
  name: string;
  version?: string;
  author?: string;
  description?: string;
  commands: PluginCommand[];
  code: string;
  builtin: boolean;
  /** off: its commands don't show anywhere */
  enabled?: boolean;
  folder?: string;
}

export interface PluginResult {
  title?: string;
  text?: string;
  table?: { columns: string[]; rows: (string | number)[][] };
}

export interface PluginList {
  dir: string;
  plugins: Plugin[];
  problems: { id: string; error: string }[];
}

export async function loadPlugins(): Promise<PluginList> {
  const r = await exec({ cmd: "plugins" });
  if (!r.ok) return { dir: "", plugins: [], problems: [] };
  const plugins = ((r.plugins as Plugin[]) ?? []).filter((p) => p && typeof p.code === "string" && Array.isArray(p.commands));
  return { dir: String(r.dir ?? ""), plugins, problems: (r.problems as PluginList["problems"]) ?? [] };
}

/** Commands a plugin may not send: anything that reads or writes files. */
const FILE_KEYS = ["path", "out", "dir", "file", "folder", "paths"];
const FILE_CMDS = new Set(["save", "open", "reveal", "save_text", "save_image", "save_skin", "save_to_library", "library_add", "insert_file", "animate", "sheets", "blueprint", "timeline_frames", "video_frame", "video_finish", "plugins", "library"]);
export function allowed(cmd: Record<string, unknown>): string | null {
  const name = String(cmd?.cmd ?? "");
  if (!name) return "a command needs a cmd";
  if (FILE_CMDS.has(name) || name.startsWith("import_") || name.startsWith("export_")) return `plugins can't use "${name}" (it reads or writes files)`;
  if (FILE_KEYS.some((k) => k in cmd)) return `plugins can't pass file paths ("${name}")`;
  return null;
}

// What runs in the worker before the plugin: the sketchcraft API, and no
// way to reach the network or load more code.
const PRELUDE = `
"use strict";
for (const k of ["fetch", "XMLHttpRequest", "WebSocket", "EventSource", "importScripts", "indexedDB", "caches"]) {
  try { Object.defineProperty(self, k, { value: undefined, configurable: false }); } catch {}
}
const __commands = Object.create(null);
const __waiting = new Map();
let __rid = 0;
const sketchcraft = Object.freeze({
  /** Add a command: fn(ctx) runs when it's picked. */
  command(id, fn) { __commands[String(id)] = fn; },
});
self.onmessage = async (e) => {
  const m = e.data || {};
  if (m.type === "reply") {
    const w = __waiting.get(m.rid);
    __waiting.delete(m.rid);
    if (w) w(m.reply);
    return;
  }
  if (m.type !== "invoke") return;
  const fn = __commands[m.id];
  if (typeof fn !== "function") return postMessage({ type: "error", error: "this plugin has no command " + JSON.stringify(m.id) });
  const ctx = Object.freeze({
    selection: m.selection,
    units: m.units,
    run: (cmd) => new Promise((res) => { const rid = ++__rid; __waiting.set(rid, res); postMessage({ type: "exec", rid, cmd: JSON.parse(JSON.stringify(cmd)) }); }),
    show: (r) => postMessage({ type: "show", result: JSON.parse(JSON.stringify(r)) }),
    toast: (t) => postMessage({ type: "toast", text: String(t) }),
  });
  try { await fn(ctx); postMessage({ type: "done" }); }
  catch (err) { postMessage({ type: "error", error: String((err && err.message) || err) }); }
};
`;

const LIMIT_MS = 60_000;

/** Run one plugin command. `run` sends engine commands (and refreshes the model). */
export function runPlugin(
  plugin: Plugin,
  commandId: string,
  env: {
    selection: { faces: number[]; edges: number[]; groups: number[]; instances: number[] };
    units: string;
    run: (cmd: Record<string, unknown>) => Promise<Reply>;
    show: (r: PluginResult, plugin: Plugin) => void;
    toast: (text: string, bad?: boolean) => void;
  },
): Promise<void> {
  return new Promise((resolve) => {
    const url = URL.createObjectURL(new Blob([PRELUDE, "\n;\n", plugin.code], { type: "text/javascript" }));
    let worker: Worker;
    try {
      worker = new Worker(url);
    } catch (e) {
      URL.revokeObjectURL(url);
      env.toast(`${plugin.name} couldn't start: ${String(e)}`, true);
      return resolve();
    }
    const finish = () => {
      clearTimeout(timer);
      worker.terminate();
      URL.revokeObjectURL(url);
      resolve();
    };
    const timer = setTimeout(() => {
      env.toast(`${plugin.name} took over a minute, so it was stopped.`, true);
      finish();
    }, LIMIT_MS);
    worker.onerror = (e) => {
      env.toast(`${plugin.name}: ${e.message || "it hit an error"}`, true);
      finish();
    };
    worker.onmessage = async (e) => {
      const m = e.data ?? {};
      if (m.type === "exec") {
        const why = allowed(m.cmd);
        const reply: Reply = why ? { ok: false, error: why } : await env.run(m.cmd);
        worker.postMessage({ type: "reply", rid: m.rid, reply });
      } else if (m.type === "show") env.show(m.result as PluginResult, plugin);
      else if (m.type === "toast") env.toast(String(m.text));
      else if (m.type === "error") {
        env.toast(`${plugin.name}: ${m.error}`, true);
        finish();
      } else if (m.type === "done") finish();
    };
    worker.postMessage({ type: "invoke", id: commandId, selection: env.selection, units: env.units });
  });
}

const csvCell = (v: string | number) => {
  const s = String(v ?? "");
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
};

/** What a plugin showed: its text and table, to copy or save as CSV. */
export function PluginResultDialog({ result, plugin, onClose, onToast }: { result: PluginResult; plugin: Plugin; onClose: () => void; onToast: (t: string, bad?: boolean) => void }) {
  const [copied, setCopied] = useState(false);
  const t = result.table;
  const csv = t ? [t.columns, ...t.rows].map((r) => r.map(csvCell).join(",")).join("\n") + "\n" : "";
  const save = async () => {
    const path = await pickSave(`${(result.title ?? plugin.name).replace(/[^\w -]+/g, "").trim() || "plugin"}.csv`, "csv", "CSV");
    if (!path) return;
    const r = await exec({ cmd: "save_text", path, text: csv });
    if (r.ok) onToast(`Saved ${path.split("/").pop()}`);
    else onToast(r.error ?? "Couldn't save it", true);
  };
  return (
    <div className="sheet-backdrop" onClick={onClose}>
      <div className="sheet plugin-result" onClick={(e) => e.stopPropagation()} role="dialog" aria-label={result.title ?? plugin.name}>
        <div className="sheet-head">
          <b>{result.title ?? plugin.name}</b>
          <span className="muted">{plugin.builtin ? "Built-in plugin" : `Plugin: ${plugin.name}${plugin.version ? ` ${plugin.version}` : ""}`}</span>
        </div>
        {result.text && <p className="plugin-text">{result.text}</p>}
        {t && (
          <div className="plugin-table">
            <table>
              <thead>
                <tr>
                  {t.columns.map((c, i) => (
                    <th key={i}>{c}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {t.rows.map((r, i) => (
                  <tr key={i}>
                    {r.map((c, j) => (
                      <td key={j}>{String(c)}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <div className="row gap">
          {t && (
            <>
              <button
                className="btn small"
                onClick={() =>
                  navigator.clipboard.writeText([t.columns, ...t.rows].map((r) => r.join("\t")).join("\n")).then(
                    () => setCopied(true),
                    () => onToast("Couldn't copy", true),
                  )
                }
              >
                {copied ? "Copied" : "Copy table"}
              </button>
              <button className="btn small ghost" onClick={save}>
                Save CSV…
              </button>
            </>
          )}
          <button className="btn small ghost" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

/** The Plugins window: every plugin, on or off, install from a .zip, remove yours. */
export function PluginsDialog({ list, reload, onClose, onToast }: { list: PluginList; reload: () => Promise<void>; onClose: () => void; onToast: (t: string, bad?: boolean) => void }) {
  const file = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const install = async (f: File | undefined) => {
    if (!f) return;
    setBusy(true);
    const data = await new Promise<string>((res) => {
      const fr = new FileReader();
      fr.onload = () => res(String(fr.result).replace(/^data:[^,]*,/, ""));
      fr.onerror = () => res("");
      fr.readAsDataURL(f);
    });
    const r = await exec({ cmd: "install_plugin", data });
    setBusy(false);
    if (!r.ok) return onToast(`Couldn't install ${f.name}: ${r.error}`, true);
    await reload();
    onToast(`${r.replaced ? "Updated" : "Installed"} ${r.name}. Its commands are in File › Plugins and Shift+S.`);
  };
  const toggle = async (p: Plugin, on: boolean) => {
    const r = await exec({ cmd: "plugin_enable", id: p.id, on });
    if (!r.ok) return onToast(r.error ?? "Couldn't change it", true);
    await reload();
  };
  const remove = async (p: Plugin) => {
    if (!(await confirm(`Remove ${p.name}?`, "It's moved to the .removed folder inside Plugins, so you can put it back.", "Remove"))) return;
    const r = await exec({ cmd: "remove_plugin", id: p.id });
    if (!r.ok) return onToast(r.error ?? "Couldn't remove it", true);
    await reload();
    onToast(`Removed ${p.name}`);
  };
  return (
    <div className="sheet-backdrop" onClick={onClose}>
      <div className="sheet plugins-dlg" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Plugins">
        <div className="sheet-head">
          <b>Plugins</b>
          <span className="muted">Add commands to SketchCraft. They run sandboxed: no network, no files, and what they change can be undone.</span>
        </div>
        <div className="plugin-list">
          {list.plugins.map((p) => (
            <div key={p.id} className={`plugin-row${p.enabled === false ? " off" : ""}`}>
              <label className="plugin-switch" title={p.enabled === false ? "Turn on" : "Turn off"}>
                <input type="checkbox" checked={p.enabled !== false} onChange={(e) => toggle(p, e.target.checked)} aria-label={`${p.name} on`} />
              </label>
              <div className="plugin-about">
                <b>{p.name}</b>
                <span className="muted small">
                  {[p.version && `v${p.version}`, p.author, p.builtin ? "built in" : "yours"].filter(Boolean).join(" · ")}
                </span>
                {p.description && <span className="small">{p.description}</span>}
                <span className="muted small">{p.commands.map((c) => c.label).join(", ")}</span>
              </div>
              {!p.builtin && (
                <button className="btn small ghost" onClick={() => remove(p)}>
                  Remove
                </button>
              )}
            </div>
          ))}
          {list.problems.map((p) => (
            <div key={`x${p.id}`} className="plugin-row off">
              <span />
              <div className="plugin-about">
                <b>{p.id}</b>
                <span className="small warn">{p.error}</span>
              </div>
            </div>
          ))}
        </div>
        <input ref={file} type="file" accept=".zip,application/zip" hidden onChange={(e) => (install(e.target.files?.[0]), (e.target.value = ""))} />
        <div className="row gap" style={{ marginTop: 14 }}>
          <button className="btn small" onClick={() => file.current?.click()} disabled={busy}>
            {busy ? "Installing…" : "Install from file…"}
          </button>
          {list.dir && (
            <button className="btn small ghost" onClick={() => exec({ cmd: "reveal", path: list.dir })}>
              Open plugins folder
            </button>
          )}
          <span className="muted small">A plugin is a .zip with plugin.json and main.js (see docs/PLUGINS.md).</span>
          <span style={{ flex: 1 }} />
          <button className="btn small" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
