import { useState } from "react";
import { ExternalLinkIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { wrightOpenIn } from "../api";

const NAMES = { kicad: "KiCad", freecad: "FreeCAD" } as const;

/**
 * "Open in KiCad" / "Open in FreeCAD": the page shows the summary, the real
 * tool opens on demand (`open -a`, src-tauri/src/wright_tools.rs). A failure
 * is one sentence beside the button.
 */
export const OpenIn = ({ app, path, label, variant = "outline" }: { app: "kicad" | "freecad"; path: string | null | undefined; label?: string; variant?: "outline" | "default" }) => {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!path) return null;
  return (
    <span className="inline-flex items-center gap-2">
      <Button
        size="sm"
        variant={variant}
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          setError(null);
          try {
            await wrightOpenIn(app, path);
          } catch (e) {
            setError(String(e));
          } finally {
            setBusy(false);
          }
        }}
        title={path}
      >
        <ExternalLinkIcon /> {label ?? `Open in ${NAMES[app]}`}
      </Button>
      {error ? <span role="alert" className="text-xs text-red-600 dark:text-red-400">{error}</span> : null}
    </span>
  );
};
