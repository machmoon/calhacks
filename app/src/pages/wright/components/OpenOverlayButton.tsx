import { useState } from "react";
import { Button } from "@/components/ui/button";
import { wrightShowOverlay } from "../api";

export function OpenOverlayButton() {
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const open = async () => {
    setOpening(true);
    setError(null);
    try { await wrightShowOverlay(); }
    catch (e) { setError(`Could not open overlay: ${String(e)}`); }
    finally { setOpening(false); }
  };
  return <div className="flex items-center gap-2">
    <Button size="sm" variant="outline" disabled={opening} onClick={() => void open()}>{opening ? "Opening…" : "Open overlay"}</Button>
    {error && <span role="alert" className="text-xs text-destructive">{error}</span>}
  </div>;
}
