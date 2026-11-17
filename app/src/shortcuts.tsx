// The keyboard shortcuts sheet.
import { Sheet } from "./sheet";

/** Every shortcut, laid out like SketchUp's defaults (Mac keys; Windows uses Ctrl for ⌘ and Option). */
export const SHORTCUTS: [string, [string, string][]][] = [
  [
    "Tools",
    [
      ["Space", "Select"],
      ["L", "Line"],
      ["A", "Arc"],
      ["R", "Rectangle"],
      ["C", "Circle"],
      ["N", "Polygon"],
      ["P", "Push/Pull"],
      ["F", "Offset"],
      ["J", "Follow Me"],
      ["M", "Move"],
      ["Q", "Rotate"],
      ["S", "Scale"],
      ["T", "Tape Measure"],
      ["D", "Dimension"],
      ["W", "Text"],
      ["B", "Paint Bucket"],
      ["E", "Eraser"],
      ["X", "Section Plane"],
    ],
  ],
  [
    "Camera",
    [
      ["O", "Orbit (or middle-drag)"],
      ["H", "Pan (or Shift + middle-drag)"],
      ["Z", "Zoom"],
      ["⇧Z", "Zoom Extents"],
      ["⌘1 – ⌘6", "Iso, Top, Front, Right, Back, Left"],
      ["K", "Back Edges"],
    ],
  ],
  [
    "Edit",
    [
      ["⌘Z", "Undo"],
      ["⇧⌘Z  /  Ctrl+Y", "Redo"],
      ["⌘X  ⌘C  ⌘V", "Cut, Copy, Paste"],
      ["⌘A", "Select All"],
      ["⌘T", "Select None"],
      ["⌫", "Delete"],
      ["G", "Make Component"],
      ["⌘N  ⌘O  ⌘S  ⇧⌘S", "New, Open, Save, Save As"],
      ["⌘I", "Import"],
    ],
  ],
  [
    "While using a tool",
    [
      ["Type + Enter", "Exact length, size, angle, or 24s for sides"],
      ["→  ←  ↑", "Lock to the red, green, blue axis"],
      ["Hold ⇧", "Lock the direction you're inferring"],
      ["Esc", "Cancel, or close the group you're in"],
      ["Option / Ctrl", "Move and Rotate: copy"],
      ["x5 after a copy", "Make 5 copies in a row"],
      ["Double-click", "Push/Pull: repeat the last distance; Select: open a group"],
    ],
  ],
  [
    "Selecting",
    [
      ["⇧ click", "Add or remove"],
      ["Option / Ctrl click", "Add"],
      ["⇧ Option click", "Remove"],
      ["⌘ / Option click", "Paint Bucket: sample a material"],
      ["⇧ click", "Paint Bucket: paint every matching face"],
      ["Option / Ctrl click", "Eraser: soften (⇧ too: unsoften)"],
      ["⇧ click", "Eraser: hide"],
    ],
  ],
];

export function ShortcutSheet({ onClose }: { onClose: () => void }) {
  return (
    <Sheet onClose={onClose} label="Keyboard shortcuts">
      <div className="sheet-head">
        <b>Keyboard shortcuts</b>
        <span className="muted">SketchUp's defaults. On Windows, Ctrl stands in for ⌘.</span>
        <button className="btn small ghost" onClick={onClose}>
          Close
        </button>
      </div>
      <div className="sheet-cols">
        {SHORTCUTS.map(([title, rows]) => (
          <div key={title} className="sheet-sec">
            <div className="section-title">{title}</div>
            {rows.map(([k, what]) => (
              <div key={k + what} className="sheet-row">
                <kbd>{k}</kbd>
                <span>{what}</span>
              </div>
            ))}
          </div>
        ))}
      </div>
    </Sheet>
  );
}
