// The Materials panel: finishes, pictures and the model's materials.
import { confirm } from "./ask";
import { type Scene } from "./engine";
import { Icons } from "./icons";
import { type Sel } from "./selection";
import { prepareImage } from "./textures";
import { useRef } from "react";
import type { Cam } from "./Viewport";
import { LengthInput, type Run } from "./panelkit";

/** Image materials: add one from a photo, paint with it, size it, project it. */
/** How a surface reflects: SketchUp-style presets over physically based values. */
const FINISHES: { label: string; roughness?: number; metalness?: number; glass?: boolean; emission?: number }[] = [
  { label: "Matte", roughness: 0.9, metalness: 0 },
  { label: "Satin", roughness: 0.5, metalness: 0 },
  { label: "Gloss", roughness: 0.12, metalness: 0 },
  { label: "Metal", roughness: 0.25, metalness: 1 },
  { label: "Brushed metal", roughness: 0.55, metalness: 1 },
  { label: "Glass", roughness: 0.02, glass: true },
  { label: "Glow", roughness: 0.9, emission: 3 },
];

/** A picture's size in the model, as people read it. */
function pictureSize(data: string): string {
  const mb = (data.length * 0.75) / (1 << 20);
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.max(1, Math.round((data.length * 0.75) / 1024))} KB`;
}

export function Textures({
  scene,
  paintMaterial,
  choose,
  run,
  camera,
  sel,
  onToast,
}: {
  scene: Scene;
  paintMaterial: string | null;
  choose: (name: string | null) => void;
  run: Run;
  camera: () => Cam;
  sel: Sel;
  onToast: (t: string, bad?: boolean) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const textures = scene.materials.filter((m) => m.texture);
  const active = textures.find((m) => m.name === paintMaterial);
  const add = async (file: File) => {
    try {
      const img = await prepareImage(file);
      let name = file.name.replace(/\.[^.]+$/, "") || "Image";
      while (scene.materials.some((m) => m.name === name)) name += " 2";
      const width = 1;
      const r = await run({ cmd: "texture", name, data: img.data, width, height: width * img.aspect, color: img.color });
      if (r.ok) {
        choose(name);
        onToast(`Added ${name}. Click faces with Paint, or set its size below.`);
      }
    } catch (e) {
      onToast(String(e), true);
    }
  };
  const replacer = useRef<HTMLInputElement>(null);
  // a new picture for a material that exists: every face that uses it shows the new one
  const replace = async (file: File) => {
    if (!active) return;
    try {
      const img = await prepareImage(file);
      const r = await run({ cmd: "texture", name: active.name, data: img.data, color: img.color, width: active.texture!.width, height: active.texture!.width * img.aspect });
      if (r.ok) onToast(`${active.name} now shows ${file.name}. Undo brings the old picture back.`);
    } catch (e) {
      onToast(String(e), true);
    }
  };
  // take a material (and its picture) out of the model; its faces go to another material or back to the plain look
  const remove = async (replaceWith?: string) => {
    if (!active) return;
    const uses = scene.faces.filter((f) => f.material === active.name).length;
    const where = replaceWith ? `They'll use ${replaceWith.replace(/^[a-z]+:/, "")}.` : "They go back to the plain look.";
    if (!(await confirm(`Delete ${active.name}?`, `${uses} face${uses === 1 ? " uses" : "s use"} it. ${where} Its picture (${pictureSize(active.texture!.data)}) is removed from the model. Undo brings it back.`, "Delete"))) return;
    const r = await run(replaceWith ? { cmd: "delete_material", name: active.name, replace_with: replaceWith } : { cmd: "delete_material", name: active.name });
    if (r.ok) {
      choose(null);
      onToast(`Deleted ${active.name}.`);
    }
  };
  const purgeUnused = async () => {
    const r = await run({ cmd: "purge_materials" });
    if (r.ok) onToast(Number(r.removed) ? `Removed ${r.removed} unused material${r.removed === 1 ? "" : "s"}${Number(r.freed_mb) ? ` (${r.freed_mb} MB of pictures)` : ""}. Undo brings them back.` : "Every material is in use.");
  };
  const project = async () => {
    if (!active) return;
    const c = camera();
    const dir = [c.target[0] - c.eye[0], c.target[1] - c.eye[1], c.target[2] - c.eye[2]];
    await run({ cmd: "texture", name: active.name, project: dir, origin: c.target });
    onToast("Projected from this view: paint a whole group and the image runs across it.");
  };
  const ids = [...sel.faces, ...sel.groups];
  return (
    <div className="section">
      <div className="section-title">Finish</div>
      <div className="finishes" title={ids.length ? "Applies to the selection, keeping each face's color or image" : "Select faces or groups first"}>
        {FINISHES.map((f) => (
          <button key={f.label} className="btn small ghost" disabled={!ids.length} onClick={() => run({ cmd: "finish", ids, ...f }).then((r) => r.ok && onToast(`${f.label}: renders, and Faces › Physically based, show it`))}>
            {f.label}
          </button>
        ))}
      </div>
      <div className="section-title">Textures</div>
      <div className="swatches">
        {textures.map((m) => (
          <button
            key={m.name}
            className={`swatch texture ${paintMaterial === m.name ? "on" : ""}`}
            style={{ backgroundImage: `url(${m.texture!.data})` }}
            title={`${m.name} (${pictureSize(m.texture!.data)})`}
            onClick={() => {
              choose(m.name);
              if (ids.length) run({ cmd: "paint", ids, material: m.name });
            }}
          />
        ))}
        <button className="swatch add" title="Add an image (photo of brick, wood, a facade...)" onClick={() => input.current?.click()}>
          <Icons.plus />
        </button>
        <input ref={input} type="file" accept="image/*" hidden onChange={(e) => e.target.files?.[0] && add(e.target.files[0])} />
      </div>
      {textures.length > 0 && (
        <p className="muted small">
          {textures.length} picture{textures.length === 1 ? "" : "s"}, {pictureSize(textures.map((m) => m.texture!.data).join(""))} in all.{" "}
          {[...textures].sort((a, b) => b.texture!.data.length - a.texture!.data.length).slice(0, 3).map((m) => `${m.name} ${pictureSize(m.texture!.data)}`).join(", ")}
          {textures.length > 3 ? "…" : ""}
        </p>
      )}
      <div className="row gap">
        <button className="btn small ghost" title="Remove every material that no face uses (and its picture)" onClick={purgeUnused}>
          Remove unused materials
        </button>
      </div>
      {active?.texture && (
        <div className="build-options">
          <p>
            <b>{active.name}</b>: Paint (B) clicks apply it. {active.texture.project ? "Projected: it runs straight across every face it's painted on." : "Laid along each face it's painted on."}
          </p>
          <LengthInput
            label="Tile width"
            value={active.texture.width}
            units={scene.units || "m"}
            onChange={(w) => run({ cmd: "texture", name: active.name, width: w })}
          />
          <label className="field" title="For reference images: see your model (or a SubD cage) through the picture">
            <span>See-through {Math.round((1 - (active.opacity ?? 1)) * 100)}%</span>
            <input type="range" min={0} max={0.9} step={0.05} value={1 - (active.opacity ?? 1)} onChange={(e) => run({ cmd: "opacity", material: active.name, value: 1 - Number(e.target.value) })} />
          </label>
          <div className="row gap">
            <button className="btn small" onClick={project}>
              Project from this view
            </button>
            {active.texture.project && (
              <button className="btn small ghost" onClick={() => run({ cmd: "texture", name: active.name, project: null })}>
                Lay on faces
              </button>
            )}
          </div>
          <div className="row gap">
            <button className="btn small ghost" title="Show another picture on every face that uses this material" onClick={() => replacer.current?.click()}>
              Replace picture…
            </button>
            <button className="btn small ghost" title="Take this material and its picture out of the model" onClick={() => remove()}>
              Delete
            </button>
            <input ref={replacer} type="file" accept="image/*" hidden onChange={(e) => e.target.files?.[0] && replace(e.target.files[0])} />
          </div>
          {scene.materials.some((m) => m.name !== active.name) && (
            <label className="field" title="Delete this material and put another one on the faces that use it">
              <span>Delete, and use instead</span>
              <select value="" onChange={(e) => e.target.value && remove(e.target.value)}>
                <option value="">Choose a material…</option>
                {scene.materials.filter((m) => m.name !== active.name).map((m) => (
                  <option key={m.name} value={m.name}>
                    {m.name.replace(/^[a-z]+:/, "")}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>
      )}
    </div>
  );
}
