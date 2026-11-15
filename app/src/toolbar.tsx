// The tool bar along the bottom of the model. When the window is too narrow
// for every tool, the ones that don't fit go behind a "+N more" button; its
// list can be searched by typing a tool's name or shortcut.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Icons } from "./icons";
import { TOOL_INFO } from "./workspace";

type ToolId = keyof typeof TOOL_INFO;

const info = (id: string) => TOOL_INFO[id as ToolId];
const icon = (id: string) => Icons[id as keyof typeof Icons]?.();

export function Toolbar({
  groups,
  tool,
  choose,
  vertical,
  onCustomize,
  children,
}: {
  groups: string[][];
  tool: string;
  choose: (id: string) => void;
  /** docked down the left side: it wraps into columns instead */
  vertical: boolean;
  onCustomize: () => void;
  /** after the tools (the paint color chip) */
  children?: ReactNode;
}) {
  const nav = useRef<HTMLElement>(null);
  const [room, setRoom] = useState(Infinity);
  const [open, setOpen] = useState(false);

  // how wide the model area is (the bar's parent)
  useLayoutEffect(() => {
    const host = nav.current?.parentElement;
    if (!host || vertical) {
      setRoom(Infinity);
      return;
    }
    const ro = new ResizeObserver(() => setRoom(host.clientWidth));
    ro.observe(host);
    setRoom(host.clientWidth);
    return () => ro.disconnect();
  }, [vertical]);

  // which tools fit: sizes measured from the bar itself, so skins can differ
  const { shown, hidden } = useMemo(() => {
    const all = groups.flat();
    if (!Number.isFinite(room)) return { shown: groups, hidden: [] as string[] };
    const el = nav.current;
    const btn = el?.querySelector<HTMLElement>(".tool:not(.more)");
    const per = (btn?.offsetWidth ?? 34) + 1;
    const groupPad = 9;
    const chrome = 14 + 44 + 32; // bar padding, color chip, margins either side
    const moreW = 64;
    let left = room - chrome;
    // everything fits?
    const full = all.length * per + groups.length * groupPad;
    if (full <= left) return { shown: groups, hidden: [] as string[] };
    left -= moreW;
    const shown: string[][] = [];
    const hidden: string[] = [];
    for (const g of groups) {
      const keep: string[] = [];
      for (const id of g) {
        const cost = per + (keep.length === 0 ? groupPad : 0);
        if (hidden.length === 0 && cost <= left) {
          keep.push(id);
          left -= cost;
        } else hidden.push(id);
      }
      if (keep.length) shown.push(keep);
    }
    return { shown, hidden };
  }, [groups, room]);

  const activeHidden = hidden.includes(tool);
  return (
    <nav className="toolbar" ref={nav} aria-label="Tools">
      {shown.map((g, i) => (
        <div key={i} className="tool-group">
          {g.map((id) => {
            const t = info(id);
            return (
              <button key={id} className={`tool ${tool === id ? "on" : ""}`} onClick={() => choose(id)} title={t.key ? `${t.name} (${t.key})` : t.name} aria-label={t.name}>
                {icon(id)}
              </button>
            );
          })}
        </div>
      ))}
      {hidden.length > 0 && (
        <div className="tool-group more-wrap">
          <button
            className={`tool more ${activeHidden ? "on" : ""}`}
            onClick={() => setOpen((o) => !o)}
            aria-expanded={open}
            aria-haspopup="dialog"
            title={activeHidden ? `${info(tool).name}, and ${hidden.length - 1} more tools` : `${hidden.length} more tools: type to find one`}
          >
            {activeHidden ? (
              <>
                {icon(tool)}
                <span className="more-n">+{hidden.length - 1}</span>
              </>
            ) : (
              <span className="more-label">+{hidden.length} more</span>
            )}
          </button>
          {open && (
            <MoreTools
              ids={hidden}
              tool={tool}
              choose={(id) => {
                choose(id);
                setOpen(false);
              }}
              onCustomize={() => {
                setOpen(false);
                onCustomize();
              }}
              onClose={() => setOpen(false)}
            />
          )}
        </div>
      )}
      {children}
    </nav>
  );
}

/** The tools that didn't fit, with a search box: type, then Enter for the first match. */
function MoreTools({ ids, tool, choose, onCustomize, onClose }: { ids: string[]; tool: string; choose: (id: string) => void; onCustomize: () => void; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [at, setAt] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "");
  const list = ids.filter((id) => {
    const t = info(id);
    const n = norm(q);
    return !n || norm(t.name).includes(n) || norm(t.key) === n;
  });
  useEffect(() => setAt(0), [q]);
  // close on a click anywhere else
  useEffect(() => {
    const away = (e: PointerEvent) => {
      const wrap = box.current?.parentElement;
      if (wrap && !wrap.contains(e.target as Node)) onClose();
    };
    window.addEventListener("pointerdown", away, true);
    return () => window.removeEventListener("pointerdown", away, true);
  }, [onClose]);
  return (
    <div className="more-tools" ref={box} role="dialog" aria-label="More tools">
      <input
        autoFocus
        placeholder="Type to find a tool…"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={(e) => {
          // keep tool shortcuts from firing while typing here
          e.stopPropagation();
          if (e.key === "Escape") onClose();
          if (e.key === "ArrowDown") (e.preventDefault(), setAt((a) => Math.min(list.length - 1, a + 1)));
          if (e.key === "ArrowUp") (e.preventDefault(), setAt((a) => Math.max(0, a - 1)));
          if (e.key === "Enter" && list[at]) choose(list[at]);
        }}
        aria-label="Find a tool"
      />
      <div className="more-list" role="listbox">
        {list.map((id, i) => {
          const t = info(id);
          return (
            <button key={id} role="option" aria-selected={i === at} className={`${i === at ? "at" : ""} ${tool === id ? "on" : ""}`} onMouseEnter={() => setAt(i)} onClick={() => choose(id)}>
              {icon(id)}
              <span>{t.name}</span>
              {t.key && <kbd>{t.key}</kbd>}
            </button>
          );
        })}
        {list.length === 0 && <p className="muted small">No tool here matches “{q}”. Shift+S searches every command.</p>}
      </div>
      <button className="more-customize" onClick={onCustomize}>
        + Add or remove tools…
      </button>
    </div>
  );
}
