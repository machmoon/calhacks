import { STORAGE_KEYS } from "@/config";

export interface CustomizableState {
  appIcon: {
    isVisible: boolean;
  };
  alwaysOnTop: {
    isEnabled: boolean;
  };
  autostart: {
    isEnabled: boolean;
  };
}

// Wright is a control strip floating over KiCad, so it stays on
// top by default: a stage that launches KiCad would otherwise bury it.
export const DEFAULT_CUSTOMIZABLE_STATE: CustomizableState = {
  appIcon: { isVisible: true },
  alwaysOnTop: { isEnabled: true },
  autostart: { isEnabled: true },
};

// Bumped when a default changes in a way stored state must not keep
// overriding. Version 2: always-on-top became the default. Versions 3 and 4
// changed a setting that no longer exists.
export const CUSTOMIZABLE_VERSION = 4;

/**
 * Get customizable state from localStorage
 */
export const getCustomizableState = (): CustomizableState => {
  try {
    const stored = localStorage.getItem(STORAGE_KEYS.CUSTOMIZABLE);
    if (!stored) {
      return DEFAULT_CUSTOMIZABLE_STATE;
    }

    const parsedState = JSON.parse(stored);
    const current = parsedState.version === CUSTOMIZABLE_VERSION;

    return {
      appIcon: parsedState.appIcon || DEFAULT_CUSTOMIZABLE_STATE.appIcon,
      alwaysOnTop:
        (current && parsedState.alwaysOnTop) ||
        DEFAULT_CUSTOMIZABLE_STATE.alwaysOnTop,
      autostart: parsedState.autostart || DEFAULT_CUSTOMIZABLE_STATE.autostart,
    };
  } catch (error) {
    console.error("Failed to get customizable state:", error);
    return DEFAULT_CUSTOMIZABLE_STATE;
  }
};

/**
 * Save customizable state to localStorage
 */
export const setCustomizableState = (state: CustomizableState): boolean => {
  try {
    localStorage.setItem(
      STORAGE_KEYS.CUSTOMIZABLE,
      JSON.stringify({ ...state, version: CUSTOMIZABLE_VERSION })
    );
    return true;
  } catch (error) {
    // Returned, not only logged: the shortcut editor holds preferences a person just set, and a failed save has to reach them.
    console.error("Failed to save customizable state:", error);
    return false;
  }
};

/**
 * Update app icon visibility
 */
export const updateAppIconVisibility = (
  isVisible: boolean
): CustomizableState => {
  const currentState = getCustomizableState();
  const newState = { ...currentState, appIcon: { isVisible } };
  setCustomizableState(newState);
  return newState;
};

/**
 * Update always on top state
 */
export const updateAlwaysOnTop = (isEnabled: boolean): CustomizableState => {
  const currentState = getCustomizableState();
  const newState = { ...currentState, alwaysOnTop: { isEnabled } };
  setCustomizableState(newState);
  return newState;
};

/**
 * Update autostart state
 */
export const updateAutostart = (isEnabled: boolean): CustomizableState => {
  const currentState = getCustomizableState();
  const newState = { ...currentState, autostart: { isEnabled } };
  setCustomizableState(newState);
  return newState;
};
