// Making a SubD object from the app: Subdivide (a group or component you already modeled)
// and the SubD box starter. Both end in SubD mode with the cage ready to shape.
import { useState } from "react";
import { Sheet, SheetHead } from "./sheet";
import { type Run } from "./panelkit";

/** The hint every entry point repeats: how a SubD object is shaped, and left. */
export const SUBD_HOW = "Tab to shape, Enter when done";

/** A cube cage, 2 m on a side with its base on the ground: eight points, six quads. */
export const SUBD_BOX = {
  points: [[-1, -1, 0], [1, -1, 0], [1, 1, 0], [-1, 1, 0], [-1, -1, 2], [1, -1, 2], [1, 1, 2], [-1, 1, 2]],
  faces: [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]],
};

/** What Subdivide asks: how smooth, and whether the sharp edges stay sharp. */
export interface SubdivideOptions {
  levels: number;
  keepSharp: boolean;
}

export function SubdivideDialog({ name, component, onGo, onClose }: { name: string; component: boolean; onGo: (o: SubdivideOptions) => void; onClose: () => void }) {
  const [levels, setLevels] = useState(2);
  const [keepSharp, setKeepSharp] = useState(false);
  return (
    <Sheet onClose={onClose} label="Subdivide" width={420}>
      <SheetHead title="Subdivide" note={`Smooth “${name}” into a soft SubD shape.`} />
      <label className="field">
        <span>Smoothness</span>
        <select value={levels} onChange={(e) => setLevels(Number(e.target.value))} aria-label="Smoothness (levels)">
          <option value={1}>1 · light</option>
          <option value={2}>2 · smooth (usual)</option>
          <option value={3}>3 · very smooth</option>
        </select>
      </label>
      <label className="field check">
        <input type="checkbox" checked={keepSharp} onChange={(e) => setKeepSharp(e.target.checked)} />
        <span>Keep sharp edges (corners that are sharp now stay sharp)</span>
      </label>
      <p className="muted small" style={{ margin: "8px 0 0" }}>
        {component ? "The component is made a group first (just this one). " : ""}
        You land in SubD mode on its cage: {SUBD_HOW}.
      </p>
      <div className="row gap" style={{ marginTop: 12 }}>
        <button className="btn small primary" autoFocus onClick={() => onGo({ levels, keepSharp })}>
          Subdivide
        </button>
        <button className="btn small ghost" onClick={onClose}>
          Cancel
        </button>
      </div>
    </Sheet>
  );
}

/** Make the SubD box; resolves with its group id. */
export async function makeSubdBox(run: Run): Promise<number | null> {
  const r = await run({ cmd: "subd", name: "SubD box", points: SUBD_BOX.points, faces: SUBD_BOX.faces, levels: 2 });
  return r.ok ? (r.group as number) : null;
}

/** Subdivide a group (or a component: this one is exploded to a group first); resolves with the new group id. */
export async function subdivideEntity(run: Run, id: number, component: boolean, o: SubdivideOptions): Promise<number | null> {
  let group = id;
  if (component) {
    const e = await run({ cmd: "explode", id });
    if (!e.ok) return null;
    group = e.group as number;
  }
  const r = await run({ cmd: "subdivide", id: group, levels: o.levels, keep_sharp: o.keepSharp });
  if (r.ok) return r.group as number;
  // it didn't take (faces with holes...): leave the component as it was
  if (component) await run({ cmd: "undo" });
  return null;
}
