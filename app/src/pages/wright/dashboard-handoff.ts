import { invoke } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";
export const WRIGHT_RUN_STARTED = "wright-run-started";
/** Show the full native dashboard, never route the floating main NSPanel. */
export async function handoffWrightRun(runDir: string | null) {
  await invoke("open_dashboard");
  await emitTo("dashboard", WRIGHT_RUN_STARTED, { runDir });
}
