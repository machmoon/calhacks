import { useEffect, useState } from "react";
import { WRIGHT_STORAGE_KEYS } from "@/config/wright.constants";
import {
  getSkin,
  getTerminalWright,
  setSkin as persistSkin,
  type SkinId,
} from "@/lib/overlay-skin";
import { getPet, type PetChoice } from "@/lib/pet";

/**
 * The chosen skin, kept live across windows.
 *
 * Settings and the overlay are two separate webviews with separate React
 * trees, so a context cannot carry this. localStorage plus the `storage`
 * event is the bridge this app already uses between them (see
 * `WRIGHT_STORAGE_KEYS.LAST_RUN`), and it has the property that matters here:
 * `storage` fires in the *other* window, which is exactly the direction the
 * change travels — you pick a skin in Settings and the overlay follows
 * without a restart.
 */
export function useOverlaySkin(): {
  skin: SkinId;
  wrightInTerminal: boolean;
  setSkin: (id: SkinId) => void;
  /** The folded overlay's pet, chosen in Settings; "off" keeps the pill. */
  pet: PetChoice;
} {
  const [skin, setSkin] = useState<SkinId>(() => getSkin());
  const [wrightInTerminal, setWright] = useState<boolean>(() => getTerminalWright());
  const [pet, setPetState] = useState<PetChoice>(() => getPet());

  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      // A `null` key means the whole store was cleared; re-read both rather
      // than leaving the overlay on a skin that no longer exists.
      if (event.key === null) {
        setSkin(getSkin());
        setWright(getTerminalWright());
        setPetState(getPet());
        return;
      }
      if (event.key === WRIGHT_STORAGE_KEYS.OVERLAY_SKIN) setSkin(getSkin());
      if (event.key === WRIGHT_STORAGE_KEYS.TERMINAL_WRIGHT) setWright(getTerminalWright());
      if (event.key === WRIGHT_STORAGE_KEYS.OVERLAY_PET) setPetState(getPet());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  // `storage` does not fire in the window that wrote, so a skin changed from
  // *here* has to update local state itself. Settings still hears it.
  const choose = (id: SkinId) => {
    persistSkin(id);
    setSkin(id);
  };

  return { skin, wrightInTerminal, setSkin: choose, pet };
}
