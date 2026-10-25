import { useEffect, useState } from "react";
import { PET_ATLAS, PET_ROWS, petInfo, type PetId, type PetRow } from "@/lib/pet";

export interface PetSpriteProps {
  pet: PetId;
  row: PetRow;
  /** Drawn width in CSS px; height follows the 192:208 cell. */
  width?: number;
  /** prefers-reduced-motion: hold idle's first frame (animation-rows.md). */
  still?: boolean;
}

/**
 * One cell of a Codex pet atlas, animated.
 *
 * "The webview animation uses CSS background positions from the fixed row and
 * column counts" (openai/skills hatch-pet references/codex-pet-contract.md),
 * so that is what this is: the atlas as a background, scaled so one cell is
 * the box, and a frame index stepped on each row's own per-frame durations
 * (animation-rows.md). The durations are uneven (idle holds 280 ms then
 * flicks through 110s), which is why this steps with a timer like the
 * corgi's `build/state-tester.html` tick rather than a CSS `steps()`
 * animation, which can only do even frames.
 */
export const PetSprite = ({ pet, row, width = 96, still = false }: PetSpriteProps) => {
  const info = petInfo(pet);
  const spec = PET_ROWS[still ? "idle" : row];
  const [frame, setFrame] = useState(0);

  useEffect(() => {
    setFrame(0);
    if (still) return;
    let index = 0;
    let id: number;
    const tick = () => {
      index = (index + 1) % spec.durations.length;
      setFrame(index);
      id = window.setTimeout(tick, spec.durations[index]);
    };
    id = window.setTimeout(tick, spec.durations[0]);
    return () => window.clearTimeout(id);
  }, [spec, still]);

  const height = Math.round((width * PET_ATLAS.cellHeight) / PET_ATLAS.cellWidth);
  const col = still ? 0 : Math.min(frame, spec.durations.length - 1);

  return (
    <span
      aria-hidden
      data-testid="pet-sprite"
      data-pet={pet}
      data-row={still ? "idle" : row}
      data-frame={col}
      className="pointer-events-none block select-none"
      style={{
        width,
        height,
        backgroundImage: `url(${info.sheet})`,
        backgroundRepeat: "no-repeat",
        backgroundSize: `${width * PET_ATLAS.columns}px ${height * PET_ATLAS.rows}px`,
        backgroundPosition: `${-col * width}px ${-spec.row * height}px`,
        imageRendering: info.pixelated ? "pixelated" : "auto",
      }}
    />
  );
};
