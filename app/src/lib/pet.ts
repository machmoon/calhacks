/**
 * The folded overlay as a desktop pet, the way Codex does it.
 *
 * The OpenAI Codex app's pets float over the desktop and follow the state of
 * the current thread (running, waiting for input, ready for review). The app
 * itself is closed source, but its pet *format* is published: openai/skills,
 * `skills/.curated/hatch-pet/references/codex-pet-contract.md` (Apache-2.0)
 * fixes the atlas at 8 columns x 9 rows of 192x208 cells, and
 * `references/animation-rows.md` fixes which row is which state, how many
 * frames each row uses and how long each frame shows. Everything below is that
 * table, verbatim, so any Codex pet (`pet.json` + `spritesheet.webp`) drops in.
 *
 * The two pets shipped (both MIT, see app/NOTICE.md):
 *  - corgi: "Corgi Companion" by cxian0928-afk,
 *    https://github.com/cxian0928-afk/corgi-codex-pet (build/package/).
 *  - bear: Baluc by Adarsh Jha, https://github.com/Adarsh-Jha/baluc-desktop-pet,
 *    whose canvas-drawn bear was baked into the same atlas layout by
 *    `src/assets/pets/bear/bake-atlas.html`.
 *
 * Pure and DOM-free, like `overlay-skin.ts`: the catalogue, the stored choice,
 * the row table and the state mapping. The sprite and the bar read it.
 */

import { WRIGHT_STORAGE_KEYS } from "@/config/wright.constants";
import { safeLocalStorage } from "@/lib/storage/helper";
import corgiSheet from "@/assets/pets/corgi/spritesheet.webp";
import bearSheet from "@/assets/pets/bear/spritesheet.png";

export type PetId = "corgi" | "bear";
/** `off` keeps the three-control pill as the folded shape. */
export type PetChoice = PetId | "off";

export interface PetInfo {
  id: PetId;
  name: string;
  sheet: string;
  /** Pixel art scales with nearest-neighbour; painted art does not. */
  pixelated: boolean;
  credit: string;
}

export const PETS: readonly PetInfo[] = [
  {
    id: "corgi",
    name: "Corgi",
    sheet: corgiSheet,
    pixelated: false,
    credit: "Corgi Companion by cxian0928-afk (MIT)",
  },
  {
    id: "bear",
    name: "Bear",
    sheet: bearSheet,
    pixelated: true,
    credit: "Baluc by Adarsh Jha (MIT)",
  },
] as const;

export const DEFAULT_PET: PetChoice = "corgi";

export function isPetChoice(value: unknown): value is PetChoice {
  return value === "off" || PETS.some((pet) => pet.id === value);
}

export function getPet(): PetChoice {
  const stored = safeLocalStorage.getItem(WRIGHT_STORAGE_KEYS.OVERLAY_PET);
  return isPetChoice(stored) ? stored : DEFAULT_PET;
}

export function setPet(choice: PetChoice): void {
  safeLocalStorage.setItem(WRIGHT_STORAGE_KEYS.OVERLAY_PET, choice);
}

export function petInfo(id: PetId): PetInfo {
  return PETS.find((pet) => pet.id === id) ?? PETS[0];
}

// ------------------------------------------------------------- atlas contract

/** codex-pet-contract.md: 1536x1872, 8 columns x 9 rows, 192x208 cells. */
export const PET_ATLAS = { columns: 8, rows: 9, cellWidth: 192, cellHeight: 208 } as const;

export type PetRow =
  | "idle"
  | "running-right"
  | "running-left"
  | "waving"
  | "jumping"
  | "failed"
  | "waiting"
  | "running"
  | "review";

/** animation-rows.md, row for row: index, then each used frame's duration. */
export const PET_ROWS: Readonly<Record<PetRow, { row: number; durations: readonly number[] }>> = {
  idle: { row: 0, durations: [280, 110, 110, 140, 140, 320] },
  "running-right": { row: 1, durations: [120, 120, 120, 120, 120, 120, 120, 220] },
  "running-left": { row: 2, durations: [120, 120, 120, 120, 120, 120, 120, 220] },
  waving: { row: 3, durations: [140, 140, 140, 280] },
  jumping: { row: 4, durations: [140, 140, 140, 140, 280] },
  failed: { row: 5, durations: [140, 140, 140, 140, 140, 140, 140, 240] },
  waiting: { row: 6, durations: [150, 150, 150, 150, 150, 260] },
  running: { row: 7, durations: [120, 120, 120, 120, 120, 220] },
  review: { row: 8, durations: [150, 150, 150, 150, 150, 280] },
};

// ------------------------------------------------------------------ the mood

export interface PetMoodInput {
  /** Pointer over the pet, or keyboard focus inside it. */
  hovered: boolean;
  /** The pet's own microphone: recording or transcribing. */
  voice: "idle" | "recording" | "transcribing" | "error";
  /** A run is in flight in either state machine. */
  busy: boolean;
  /** The step flow is parked on a stage that needs an answer. */
  awaitingApproval: boolean;
  /** The live run's status. */
  status: string;
}

/**
 * Which row plays, by the Codex app's own meanings for its rows:
 * `running` is the agent working, `waiting` is blocked on the user, `review`
 * is ready to look at, `failed` is failed. The overlay-specific ones are the
 * microphone (open: `waiting`, the pet is listening for you; transcribing:
 * `running`) and the hover greeting (`waving`). Ordered so that a fault or a
 * question beats a wave: hovering must not hide that something went wrong.
 */
export function petMood(input: PetMoodInput): PetRow {
  if (input.voice === "recording") return "waiting";
  if (input.voice === "transcribing") return "running";
  if (input.voice === "error" || input.status === "error") return "failed";
  if (input.busy) return "running";
  if (input.awaitingApproval) return "waiting";
  if (input.hovered) return "waving";
  if (input.status === "done") return "review";
  return "idle";
}
