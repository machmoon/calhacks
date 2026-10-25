import { useEffect, useRef, useState } from "react";
import { CheckIcon, Loader2Icon, MicIcon, PencilIcon, XIcon } from "lucide-react";
import { Button, Card, DragButton } from "@/components";
import { useVoiceInput } from "@/hooks/useVoiceInput";
import { useReducedMotion } from "@/hooks/useReducedMotion";
import { takeMicFloor } from "@/lib/speech";
import { MAX_RECORDING_MS } from "@/lib/silkscreen/voice";
import { petMood, type PetId } from "@/lib/pet";
import { PetSprite } from "./PetSprite";

const MAX_RECORDING_S = Math.floor(MAX_RECORDING_MS / 1000);

export interface PetBarProps {
  pet: PetId;
  baseUrl: string;
  token?: string;
  /** The overlay is hidden by the global shortcut; keep focus out of it. */
  hidden: boolean;
  /** Clicking the pet itself: back to the full bar. */
  onExpand: () => void;
  /**
   * A Speak transcript landed. Same rule as the pill's microphone: the page
   * puts it in the draft and opens the bar; speech never reaches submit.
   */
  onTranscript: (text: string) => void;
  /** Write's text, on Enter. The page sends it through the bar's own submit. */
  onWrite: (text: string) => void;
  /** Write's field is open; the page widens the window for it. */
  writing: boolean;
  onWritingChange: (writing: boolean) => void;
  /** Run state, so the pet plays what Codex's pet plays for it. */
  busy: boolean;
  awaitingApproval: boolean;
  status: string;
}

/**
 * The overlay folded all the way down, as a desktop pet.
 *
 * Shaped on the OpenAI Codex app's pets (a companion that floats over the
 * desktop and acts out the agent's state; atlas and rows from openai/skills
 * hatch-pet, see `@/lib/pet`). Hovering it, or tabbing into it, shows two
 * actions beside it:
 *
 *  - Speak: the overlay's existing push-to-talk recorder (`useVoiceInput`,
 *    the hook VoiceControl's hold uses, with the same mic floor around it). Tap to start, tap the check to stop.
 *  - Write: a one-line field. Enter hands the text to the page, which sends
 *    it the way the bar's own Enter does.
 *
 * Clicking the pet opens the full bar. The drag handle is the third action,
 * since the pet itself is a button and cannot also be a drag region.
 *
 * Nothing here takes focus on its own: the field is focused only after the
 * user pressed Write.
 */
export const PetBar = ({
  pet,
  baseUrl,
  token,
  hidden,
  onExpand,
  onTranscript,
  onWrite,
  writing,
  onWritingChange,
  busy,
  awaitingApproval,
  status,
}: PetBarProps) => {
  const still = useReducedMotion();
  const voice = useVoiceInput({ baseUrl, token, onTranscript });
  const [pointerIn, setPointerIn] = useState(false);
  const [focusIn, setFocusIn] = useState(false);
  const [draft, setDraft] = useState("");
  const floorRef = useRef<(() => void) | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const recording = voice.status === "recording";
  const transcribing = voice.status === "transcribing";

  // Give the floor back once the recording is over, however it ended (stop,
  // cancel, the length cap, an error).
  useEffect(() => {
    if (recording) return;
    floorRef.current?.();
    floorRef.current = null;
  }, [recording]);

  useEffect(
    () => () => {
      floorRef.current?.();
      floorRef.current = null;
    },
    []
  );

  useEffect(() => {
    if (writing) inputRef.current?.focus();
  }, [writing]);

  const speak = () => {
    if (recording) {
      voice.stop();
      return;
    }
    floorRef.current?.();
    floorRef.current = takeMicFloor();
    void voice.start();
  };

  const send = () => {
    const text = draft.trim();
    if (!text) return;
    onWrite(text);
    setDraft("");
    onWritingChange(false);
  };

  const row = petMood({
    hovered: pointerIn || focusIn,
    voice: voice.status,
    busy,
    awaitingApproval,
    status,
  });
  const showActions = pointerIn || focusIn || writing || recording || transcribing;

  return (
    <div
      className="kv-shape-in flex items-center gap-1.5"
      data-testid="pet-bar"
      data-motion={still ? "still" : "animated"}
      onPointerEnter={() => setPointerIn(true)}
      onPointerLeave={() => setPointerIn(false)}
      onFocus={() => setFocusIn(true)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocusIn(false);
      }}
    >
      <button
        type="button"
        className="shrink-0 rounded-xl bg-transparent p-0 outline-none focus-visible:ring-2 focus-visible:ring-ring"
        title="Open the prompt"
        aria-label="Open the prompt"
        onClick={onExpand}
        disabled={hidden}
        data-testid="pet-open"
      >
        <PetSprite pet={pet} row={row} width={96} still={still} />
      </button>

      <Card
        className={`flex w-fit flex-row items-center gap-1 px-1.5 py-1 transition-opacity ${
          showActions ? "opacity-100" : "opacity-0"
        }`}
        data-testid="pet-actions"
        data-shown={showActions ? "1" : "0"}
      >
        {recording || transcribing ? (
          <>
            <span className="flex items-center gap-1.5 px-1.5" data-testid="pet-recording">
              {transcribing ? (
                <Loader2Icon className="size-3.5 animate-spin" />
              ) : (
                <span className="size-2 animate-pulse rounded-full bg-[var(--strip-bad)]" />
              )}
              <span className="strip-mono text-[10.5px] tabular-nums">
                {transcribing ? "…" : `${voice.elapsedS}s/${MAX_RECORDING_S}s`}
              </span>
            </span>
            <Button
              variant="ghost"
              size="icon"
              className="size-7"
              onClick={voice.stop}
              disabled={transcribing}
              title="Stop and put the words in the prompt"
              aria-label="Stop and transcribe"
              data-testid="pet-speak-stop"
            >
              <CheckIcon className="size-3.5" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="size-7"
              onClick={voice.cancel}
              title="Throw this recording away"
              aria-label="Cancel recording"
              data-testid="pet-speak-cancel"
            >
              <XIcon className="size-3.5" />
            </Button>
          </>
        ) : writing ? (
          <>
            <input
              ref={inputRef}
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.nativeEvent.isComposing) {
                  event.preventDefault();
                  send();
                } else if (event.key === "Escape") {
                  event.preventDefault();
                  onWritingChange(false);
                }
              }}
              placeholder="Ask Wright… (Enter sends)"
              aria-label="Write to Wright"
              className="h-7 w-56 rounded-md border border-input bg-transparent px-2 text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring"
              data-testid="pet-write-input"
              disabled={hidden}
            />
            <Button
              variant="ghost"
              size="icon"
              className="size-7"
              onClick={() => onWritingChange(false)}
              title="Close (Esc)"
              aria-label="Close the field"
              data-testid="pet-write-close"
            >
              <XIcon className="size-3.5" />
            </Button>
          </>
        ) : (
          <>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 gap-1 px-2 text-[11px]"
              onClick={speak}
              disabled={hidden}
              title="Speak: talk now, the words land in the prompt for you to check"
              aria-label="Speak"
              data-testid="pet-speak"
            >
              <MicIcon className="size-3.5" />
              Speak
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 gap-1 px-2 text-[11px]"
              onClick={() => onWritingChange(true)}
              disabled={hidden}
              title="Write something to Wright"
              aria-label="Write"
              data-testid="pet-write"
            >
              <PencilIcon className="size-3.5" />
              Write
            </Button>
            <DragButton />
          </>
        )}
      </Card>
      {voice.status === "error" && voice.error ? (
        <span className="sr-only" role="status">
          {voice.error}
        </span>
      ) : null}
    </div>
  );
};
