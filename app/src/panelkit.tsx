// What the side panels share: the command runner type, a length field and the area/volume text.
import { useState } from "react";
import { type Reply } from "./engine";
import { formatLength, parseLength } from "./units";

export type Run = (cmd: Record<string, unknown>) => Promise<Reply>;

export { Outliner } from "./outliner";

export function LengthInput({ value, units, onChange, label }: { value: number; units: string; onChange: (m: number) => void; label: string }) {
  const [text, setText] = useState<string | null>(null);
  return (
    <label className="field">
      <span>{label}</span>
      <input
        value={text ?? formatLength(value, units)}
        onFocus={(e) => (setText(formatLength(value, units)), e.target.select())}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => {
          const v = parseLength(text ?? "", units);
          if (!Number.isNaN(v) && v > 0) onChange(v);
          setText(null);
        }}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
    </label>
  );
}

export function formatVolume(m3: number, units: string): string {
  if (units === "ft" || units === "in") return `${(m3 / 0.028316846592).toFixed(2)} ft³`;
  if (units === "mm" || units === "cm") return `${(m3 * 1e6).toFixed(0)} cm³`;
  return `${m3.toFixed(3)} m³`;
}

export function formatArea(m2: number, units: string): string {
  if (units === "ft" || units === "in") return `${(m2 / 0.09290304).toFixed(1)} ft²`;
  if (units === "mm" || units === "cm") return `${(m2 * 10000).toFixed(0)} cm²`;
  return `${m2.toFixed(2)} m²`;
}
