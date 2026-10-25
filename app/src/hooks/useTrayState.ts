// The overlay's half of the menu bar icon (src-tauri/src/tray.rs).
//
// On Windows the hide shortcut blanks the webview rather than hiding the
// window, so the tray cannot see it; this hook reports it with
// `tray_set_state` every time it changes, and the tray's show/hide label
// follows. Elsewhere the tray reads the window itself.
//
// A failed report is a console.warn: a stale menu bar beats a crashed
// overlay, the same rule useOverlayHeight and useOverlayDock apply.

import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface TrayStateInput {
  /** The overlay is on screen (on Windows, not blanked by the hide shortcut). */
  visible: boolean;
}

export function useTrayState({ visible }: TrayStateInput): void {
  useEffect(() => {
    invoke("tray_set_state", { visible }).catch((error) =>
      console.warn("[wright tray] could not report state:", error)
    );
  }, [visible]);
}
