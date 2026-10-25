// Thin wrappers over the Rust commands in src-tauri/src/wright.rs.
import { invoke } from "@tauri-apps/api/core";
import type { Domain, EnvStatus, RunListing, TailResult, WrightStatus } from "./types";

export const wrightStatus = () => invoke<WrightStatus>("wright_status");

export const wrightStart = (domain: Domain, request: string, runDir?: string, attachmentIds: string[] = []) =>
  invoke<WrightStatus>("wright_start", { domain, request, runDir: runDir ?? null, attachmentIds });

export const wrightStop = () => invoke<WrightStatus>("wright_stop");

export const wrightTail = (runDir: string, fromLine: number) =>
  invoke<TailResult>("wright_tail", { runDir, fromLine });

export type ControlAction = "pause" | "resume" | "approve" | "stop";

export const wrightControl = (runDir: string, action: ControlAction, note = "") =>
  invoke<void>("wright_control", { runDir, action, note });

export const wrightListRuns = () => invoke<RunListing[]>("wright_list_runs");

export const wrightResolve = (runDir: string, recorded: string) =>
  invoke<string | null>("wright_resolve", { runDir, recorded });

export const wrightReadFile = async (path: string): Promise<Uint8Array> =>
  new Uint8Array(await invoke<ArrayBuffer>("wright_read_file", { path }));

export const wrightOpen = (path: string) => invoke<void>("wright_open", { path });

export const wrightEnvStatus = () => invoke<EnvStatus>("wright_env_status");

export interface PdfAttachment { id: string; name: string; sha256: string; pages: number; preview: string; limitations: string[] }
export const wrightAttachPdf = (input: { path?: string; name?: string; bytes?: number[] }) =>
  invoke<PdfAttachment>("wright_attach_pdf", { path: input.path ?? null, name: input.name ?? null, bytes: input.bytes ?? null });

export const wrightShowOverlay = () => invoke<void>("wright_show_overlay");

/** One SVG per KiCad layer (pcb) or one sheet (sch), plotted by kicad-cli. */
export interface KicadSvgLayer { layer: string; svg: string }
export const wrightKicadSvg = (path: string, kind: "pcb" | "sch") =>
  invoke<KicadSvgLayer[]>("wright_kicad_svg", { path, kind });

/** The board as GLB bytes (kicad-cli pcb export glb), for the in-app viewer. */
export const wrightKicadGlb = async (path: string): Promise<Uint8Array> =>
  new Uint8Array(await invoke<ArrayBuffer>("wright_kicad_glb", { path }));

/** Open a file in the real tool: KiCad's PCB/Schematic Editor or FreeCAD. */
export const wrightOpenIn = (app: "kicad" | "freecad", path: string) =>
  invoke<void>("wright_open_in", { app, path });
