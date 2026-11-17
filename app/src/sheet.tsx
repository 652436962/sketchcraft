// The frame every dialog shares: a dimmed backdrop (a click on it closes) around a sheet.
import type { ReactNode } from "react";

/** The dialog's room on narrow windows: `width` pixels, never wider than the window. */
const sheetWidth = (width: number) => `min(${width}px, calc(100vw - 32px))`;

export function Sheet({ onClose, label, className, width, children }: { onClose: () => void; label: string; className?: string; width?: number; children: ReactNode }) {
  return (
    <div className="sheet-backdrop" onClick={onClose}>
      <div className={className ? `sheet ${className}` : "sheet"} style={width ? { width: sheetWidth(width) } : undefined} onClick={(e) => e.stopPropagation()} role="dialog" aria-label={label}>
        {children}
      </div>
    </div>
  );
}

/** The title line of a dialog: a name and what it is for. */
export function SheetHead({ title, note }: { title: string; note: string }) {
  return (
    <div className="sheet-head">
      <b>{title}</b>
      <span className="muted">{note}</span>
    </div>
  );
}
