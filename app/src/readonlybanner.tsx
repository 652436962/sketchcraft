// A model saved by a newer SketchCraft opens read-only (the engine refuses to change or save it, see
// core/src/format.rs). This bar says so for as long as it stays that way: a toast would be gone in seconds.
import { useEffect, useState } from "react";
import { exec } from "./engine";

export const READ_ONLY_NOTICE = "Saved by a newer SketchCraft — update to edit. You can look, but changes won't be saved here.";

export function ReadOnlyBanner() {
  const [why, setWhy] = useState<string[] | null>(null);
  useEffect(() => {
    let stop = false;
    const look = async () => {
      try {
        const r = await exec({ cmd: "info" });
        if (!stop) setWhy(Array.isArray(r.read_only) ? (r.read_only as string[]) : null);
      } catch {
        /* the engine is busy or away: keep what the bar says */
      }
    };
    void look();
    const id = window.setInterval(look, 1500);
    return () => {
      stop = true;
      window.clearInterval(id);
    };
  }, []);
  if (!why) return null;
  return (
    <div className="readonly-bar" role="status" data-testid="read-only-notice" title={why.join("; ")}>
      {READ_ONLY_NOTICE}
    </div>
  );
}
