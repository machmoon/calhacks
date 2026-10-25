import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { WRIGHT_RUN_STARTED } from "./dashboard-handoff";
/** Existing dashboard windows may currently be showing another settings page. */
export function DashboardRunSync() {
  const navigate = useNavigate(); const { pathname } = useLocation();
  const label = isTauri() ? getCurrentWebviewWindow().label : null;
  const dashboard = label ? label === "dashboard" : pathname !== "/" && !pathname.startsWith("/welcome");
  useEffect(() => {
    // Repair a main panel left on /wright by the old in-panel navigation.
    if (label === "main" && pathname === "/wright") navigate("/", { replace: true });
  }, [label, pathname, navigate]);
  useEffect(() => {
    if (!dashboard) return;
    let disposed = false; let stop: (() => void) | undefined;
    listen(WRIGHT_RUN_STARTED, () => navigate("/wright"))
      .then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; })
      .catch((error) => console.error("Could not listen for Wright dashboard handoff", error));
    return () => { disposed = true; stop?.(); };
  }, [dashboard, navigate]);
  return null;
}
