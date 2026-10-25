import { useEffect, useRef, useState } from "react";
import { Input } from "@/components";
import type { EngineHealth } from "@/hooks";
import { VoiceOrb } from "./VoiceOrb";

/**
 * The overlay as an orb: at rest there is an orb and nothing else.
 *
 * The brief was a sentence about a screenshot — "how about the new designs
 * for the bar … i think that is cool bc like its an orb and that could be a
 * diff style" — held against the standing rule that "the overlay isnt meant
 * to have so much visual clutter". The catalogue row is the contract:
 * *"Collapses to an orb; the field appears when you click."* So the
 * resting frame has no field, no microphone button, no options popover and
 * no run control. Everything the bar shows in a row,
 * this shows only once it has been asked for.
 *
 * One rule outranks the aesthetic, and it is why this file is mostly
 * composition rather than art:
 *
 * **A control that spends money is visibly paid, and absent — not greyed
 * — when pressing it would not be valid.** So `orb-submit` is not rendered
 * at all unless there is something to run, the engine answered its last
 * probe, and no run is already in flight.
 *
 * On animation: nothing here is ported from LiveKit's aura shader. That file
 * is Polyform Non-Resale 1.0.0, not Apache-2.0, and cannot ship (recorded in
 * `docs/overlay-skins.md`). The orb's own motion stops entirely under
 * `prefers-reduced-motion` — an always-on-top overlay that animates forever
 * is a battery and an attention cost.
 */

/** How much bigger the skin's orb is than the strip's 18 px indicator. */
const ORB_SCALE = 2.4;

/**
 * True when the OS asks for less motion. Falls back to "no" where unaskable.
 *
 * Duplicated from `VoiceOrb.tsx` rather than imported because that copy is
 * module-private there; the `reducedMotion` prop is the seam tests use, so
 * this function is only the default.
 */
function prefersReducedMotion(): boolean {
  const mm = (globalThis as { matchMedia?: (q: string) => MediaQueryList }).matchMedia;
  if (typeof mm !== "function") return false;
  try {
    return mm.call(globalThis, "(prefers-reduced-motion: reduce)").matches === true;
  } catch {
    return false;
  }
}

export interface OrbSkinProps {
  /** Engine health, carried straight to the orb (it owns the engine dot's job). */
  engine: EngineHealth;
  baseUrl: string;
  /** The draft. Owned by the caller, so the text survives the field closing. */
  value: string;
  onChange: (value: string) => void;
  /** Starts a paid run. Only ever reachable from the field's own control. */
  onSubmit: () => void;
  /** A run is in flight. */
  busy?: boolean;
  /** The run state machine's own guard (`canStart`). */
  canStart?: boolean;
  /** The overlay is hidden by the global shortcut; keep focus out of it. */
  hidden?: boolean;
  /**
   * Controlled disclosure. Omit it and the skin owns it: the field opens on
   * a click, on speech, and while there is a draft to see.
   */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** Test seams, both passed through to the orb. */
  reducedMotion?: boolean;
  isSpeaking?: () => boolean;
}

/** The caption under the orb while the field is closed. */
export function orbCaption(busy: boolean): string {
  return busy ? "Working on it." : "Click to type.";
}

export const OrbSkin = ({
  engine,
  baseUrl,
  value,
  onChange,
  onSubmit,
  busy = false,
  canStart = true,
  hidden = false,
  open,
  onOpenChange,
  reducedMotion,
  isSpeaking,
}: OrbSkinProps) => {
  const [selfOpen, setSelfOpen] = useState(false);
  const field = useRef<HTMLInputElement | null>(null);
  const still = reducedMotion ?? prefersReducedMotion();

  // A draft opens the field: text the user already put there must never be
  // hidden behind a collapse they did not ask for.
  const derived = selfOpen || value.trim() !== "";
  const showField = open ?? derived;

  const setOpen = (next: boolean) => {
    setSelfOpen(next);
    onOpenChange?.(next);
  };

  useEffect(() => {
    if (showField && !hidden) field.current?.focus();
  }, [showField, hidden]);

  const gate: SubmitInput = {
    value,
    canStart,
    busy,
    hidden,
    engineOk: engine.ok,
    engineKnown: engine.lastCheckedAt !== null,
  };
  const paid = submittable(gate);

  return (
    <div
      className="flex flex-col items-center gap-2 py-1"
      data-testid="orb-skin"
      data-open={showField ? "true" : "false"}
      data-motion={still ? "still" : "animated"}
    >
      {/*
        The orb is `VoiceOrb`, scaled: not a second orb. That component owns
        the one rule this skin must not get wrong, and a copy of it here
        would be a second place for the lie to come back. The inner element
        is a real button (engine re-check, same testid the strip has always
        used); this wrapper takes the same click by bubbling and opens the
        field, so one click both re-checks and opens, and a keyboard user
        reaches it by Tab like they always could.
      */}
      <div
        className="relative flex size-[52px] cursor-text items-center justify-center"
        title="Click to type"
        onClick={() => setOpen(true)}
        data-testid="orb-skin-orb"
      >
        <span
          className="relative"
          style={{ transform: `scale(${ORB_SCALE})`, transformOrigin: "center" }}
        >
          <VoiceOrb
            engine={engine}
            baseUrl={baseUrl}
            thinking={busy}
            reducedMotion={still}
            isSpeaking={isSpeaking}
          />
        </span>
      </div>

      {showField ? (
        <div className="flex w-full min-w-0 items-center gap-1.5" data-testid="orb-skin-field">
          <Input
            ref={field}
            placeholder="What do you need built?"
            value={value}
            // Deliberately never `disabled`, matching SpotlightSkin: a
            // disabled input greys itself, drops the caret, and — the part
            // that actually breaks — is skipped by `focusPromptIn`, whose
            // selector is `input:not([disabled])`. So the focus shortcut
            // would stop finding the field exactly while a run is on screen,
            // which is when someone is most likely to reach for it.
            readOnly={hidden}
            className="h-8 min-w-0 flex-1 text-[13px]"
            data-testid="orb-skin-input"
            onChange={(e) => onChange(e.target.value)}
            onKeyDown={(e) => {
              // Escape puts the skin back to what it is meant to be: an orb.
              if (e.key === "Escape") {
                e.preventDefault();
                setOpen(false);
                return;
              }
              if (e.key === "Enter" && !e.shiftKey && paid) {
                e.preventDefault();
                onSubmit();
              }
            }}
          />
          {/*
            The paid control, and the only element in this skin that spends
            money. It is ABSENT rather than greyed whenever pressing it would
            not be valid: nothing typed, the engine unreachable or never
            probed, a run already in flight, the overlay hidden. A greyed
            button is an invitation with the reason hidden inside a disabled
            attribute; no button at all is the honest resting frame, and it
            is why the orb sits alone with nothing filled beside it.
          */}
          {paid ? (
            <button
              type="button"
              className="h-7 shrink-0 rounded-md bg-primary px-2.5 text-[11px] font-medium text-primary-foreground"
              onClick={onSubmit}
              title="Generate a board: this calls the model and costs money"
              data-testid="orb-submit"
            >
              Generate
            </button>
          ) : null}
        </div>
      ) : (
        <span
          className="text-[11px] leading-none text-muted-foreground"
          role="status"
          data-testid="orb-skin-caption"
        >
          {orbCaption(busy)}
        </span>
      )}
    </div>
  );
};

/** Everything the paid control's existence depends on. */
export interface SubmitInput {
  value: string;
  canStart: boolean;
  busy: boolean;
  hidden: boolean;
  engineOk: boolean;
  /** A probe has landed at all; false means "we do not know yet". */
  engineKnown: boolean;
}

/**
 * May the run control exist in this frame?
 *
 * Pure and exported so the rule is pinned by test on its own, rather than
 * only through the DOM: this is the money gate, and "absent, not greyed"
 * means a false here removes the element, never disables it. The
 * unknown-engine case is deliberate — before the first probe lands nobody
 * knows whether a run would reach anything, and offering to spend on a guess
 * is the third engine state being flattened into two.
 */
export function submittable(input: SubmitInput): boolean {
  if (!input.value.trim()) return false;
  if (input.busy || input.hidden || !input.canStart) return false;
  return input.engineKnown && input.engineOk;
}
