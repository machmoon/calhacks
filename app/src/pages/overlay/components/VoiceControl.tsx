import { useEffect, useRef, useState } from "react";
import {
  CheckIcon,
  Loader2Icon,
  MicIcon,
  Volume2Icon,
  VolumeXIcon,
  XIcon,
} from "lucide-react";
import { Button, Popover, PopoverAnchor, PopoverContent } from "@/components";
import { useIsSpeaking } from "@/hooks/useIsSpeaking";
import { useVoiceInput } from "@/hooks/useVoiceInput";
import { setPushToTalk } from "@/hooks/useMicLevel";
import {
  isVoiceEnabled,
  saveVoiceEnabled,
  speaker,
  takeMicFloor,
} from "@/lib/speech";
import { MAX_RECORDING_MS } from "@/lib/silkscreen/voice";
import { cn } from "@/lib/utils";

const MAX_RECORDING_S = Math.floor(MAX_RECORDING_MS / 1000);

/** How long the button has to be held before it becomes push-to-talk. */
export const HOLD_MS = 400;

/** What the microphone button says at rest, in the first person. */
export const MIC_TITLE =
  "Click or press and hold to talk to me. The words land in the field for you to check.";

export interface VoiceControlProps {
  baseUrl: string;
  token?: string;
  disabled?: boolean;
  /** Receives the transcript for the intent draft. Never submits anything. */
  onTranscript: (text: string) => void;
  /**
   * Am I talking right now? The bar already knows (it swaps its field out for
   * the length of a reply), so it passes its own answer down rather than
   * letting two subscriptions to the same fact disagree on screen. Left out,
   * this control subscribes for itself.
   */
  speaking?: boolean;
}

/**
 * The strip's one voice control: push-to-talk.
 *
 * A tap starts a recording and the next tap (or the check) stops it; a hold
 * records for as long as it is held. While I am talking, a click stops me
 * instead. The transcript lands in the field for the engineer to read before
 * anything is sent.
 */
export const VoiceControl = ({
  baseUrl,
  token,
  disabled,
  onTranscript,
  speaking: speakingProp,
}: VoiceControlProps) => {
  const [menuOpen, setMenuOpen] = useState(false);
  // Hooks run unconditionally; the prop only decides which answer is used.
  const liveSpeaking = useIsSpeaking();
  const speaking = speakingProp ?? liveSpeaking;
  const voice = useVoiceInput({ baseUrl, token, onTranscript });
  const holdRef = useRef<number | null>(null);
  // A hold has already done something; the click that follows it must not
  // also toggle the recording, or letting go of push-to-talk would start a
  // second one.
  const heldRef = useRef(false);
  // Held for as long as push-to-talk is held: while it exists I do not talk,
  // and anything I was saying was cut the moment it was taken. One room, one
  // microphone — a sentence of mine played into a live recording ends up in
  // the transcript as if the engineer had said it.
  const floorRef = useRef<(() => void) | null>(null);
  // Whether I am allowed to talk back. Read once and kept here so the menu's
  // switch and the flag in storage cannot disagree on screen.
  const [speaks, setSpeaks] = useState(isVoiceEnabled);

  useEffect(
    () => () => {
      if (holdRef.current !== null) window.clearTimeout(holdRef.current);
      floorRef.current?.();
      floorRef.current = null;
    },
    []
  );

  const recording = voice.status === "recording";

  // The orb and the mic-level bus learn from here that the recorder is open.
  useEffect(() => {
    setPushToTalk(recording);
    return () => setPushToTalk(false);
  }, [recording]);

  const beginHold = () => {
    heldRef.current = true;
    floorRef.current?.();
    floorRef.current = takeMicFloor();
    void voice.start();
  };

  const endHold = () => {
    if (holdRef.current !== null) {
      window.clearTimeout(holdRef.current);
      holdRef.current = null;
    }
    // Released even when the hold never became a recording: a floor left held
    // by a stray pointer event would mute me for the rest of the session.
    floorRef.current?.();
    floorRef.current = null;
    if (!heldRef.current) return;
    if (voice.status === "recording") voice.stop();
    // Cleared on the next tick so the click this pointer-up produces is
    // swallowed rather than starting another recording.
    window.setTimeout(() => {
      heldRef.current = false;
    }, 0);
  };

  const tap = () => {
    if (heldRef.current) return;
    // While I am talking, this button shuts me up. It is the only control on
    // the strip, so it has to do the thing the strip is currently doing.
    if (speaking) {
      speaker.stop();
      return;
    }
    if (recording) voice.stop();
    else void voice.start();
  };

  /**
   * The other switch, and the copy around it never blurs the two: the
   * microphone decides whether I HEAR you, this decides whether I TALK BACK.
   * They live in one menu because that is where someone goes to make the
   * voice stop, and they are worded as two sentences about two organs
   * because "mute" for both would leave no way to ask for only one.
   */
  const toggleSpeaks = () => {
    const next = !speaks;
    setSpeaks(next);
    saveVoiceEnabled(next);
    // Silencing means stop talking, not finish the sentence first.
    if (!next) speaker.stop();
  };

  const title = speaking ? "I’m talking: click to stop me mid-sentence." : MIC_TITLE;

  if (recording) {
    return (
      <div
        className="strip-control flex shrink-0 items-center gap-1 border border-[var(--strip-field-line)] pl-2.5 pr-1"
        data-testid="voice-recording"
      >
        <span className="size-2 shrink-0 animate-pulse rounded-full bg-[var(--strip-bad)]" />
        <span
          className="strip-mono text-[10.5px] tabular-nums text-[var(--strip-fg-2)]"
          data-testid="voice-elapsed"
        >
          {voice.elapsedS}s/{MAX_RECORDING_S}s
        </span>
        <Button
          variant="ghost"
          size="icon"
          className="size-6"
          onClick={voice.stop}
          title="Stop and transcribe"
          data-testid="voice-stop"
        >
          <CheckIcon className="size-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="size-6"
          onClick={voice.cancel}
          title="Throw this recording away"
          data-testid="voice-cancel"
        >
          <XIcon className="size-3.5" />
        </Button>
      </div>
    );
  }

  return (
    <Popover open={menuOpen} onOpenChange={setMenuOpen}>
      <PopoverAnchor asChild>
        <span
          className="relative flex min-w-0 shrink items-center gap-1"
          data-testid="voice-control"
          onContextMenu={(event) => {
            event.preventDefault();
            setMenuOpen(true);
          }}
        >
          <Button
            variant="ghost"
            size="icon"
            // A circle the height of every other control, with a surface of
            // its own: the bare glyph that was here read as a decoration
            // rather than a target. See .strip-mic.
            className={cn("strip-mic shrink-0 p-0")}
            style={{ color: "var(--strip-fg)" }}
            disabled={disabled || voice.status === "transcribing"}
            title={title}
            aria-label={title}
            onClick={tap}
            onPointerDown={() => {
              heldRef.current = false;
              holdRef.current = window.setTimeout(beginHold, HOLD_MS);
            }}
            onPointerUp={endHold}
            onPointerLeave={endHold}
            data-testid="voice-start"
            // Which organ this click closes, so the guarantee is pinnable by
            // test rather than by reading the handler: never "mic" while a
            // sentence is playing.
            data-stops={speaking ? "voice" : "mic"}
          >
            {voice.status === "transcribing" ? (
              <Loader2Icon className="size-4 animate-spin" />
            ) : speaking ? (
              // A speaker, not a microphone: for the length of a reply this
              // button is about my voice.
              <Volume2Icon className="size-[17px]" />
            ) : (
              <MicIcon className="size-[17px]" />
            )}
          </Button>
        </span>
      </PopoverAnchor>

      <PopoverContent
        align="end"
        side="bottom"
        sideOffset={8}
        className="w-80 border border-input/50 p-3"
        data-testid="voice-menu"
      >
        <div className="flex flex-col gap-2">
          <span className="text-[12px] font-medium">The microphone</span>
          <p className="text-[11px] leading-tight text-[var(--strip-fg-2)]">
            Click it, or press and hold it, to talk to me. I record until you
            click again or let go, and the words land in the field for you to
            read before anything is sent.
          </p>

          {/* The second switch. Separated by a rule and given its own heading
              because the microphone above it and this are opposite
              directions: one is whether I hear you, this is whether I talk
              back, and a menu that called both of them "mute" would leave no
              way to ask for only one of them. */}
          <div className="mt-1 flex flex-col gap-1.5 border-t border-input/40 pt-2">
            <span className="text-[12px] font-medium">My voice</span>
            <p className="text-[11px] leading-tight text-[var(--strip-fg-2)]">
              Separate from the microphone above: that one is whether I hear
              you, this one is whether I say anything back. I speak when a
              stage lands and is waiting for you, when something fails, and
              when I answer a question. Never a running commentary. It costs
              nothing: the voice is your machine&rsquo;s own.
            </p>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 justify-start gap-2 px-2 text-[11px]"
              onClick={toggleSpeaks}
              // Both halves in the label, so the button says which of the two
              // switches it is even when read on its own by a screen reader.
              title={
                speaks
                  ? "I speak at the moments that need an answer. Click to silence my voice; the microphone is unaffected."
                  : "My voice is silenced. I still hear you, I just say nothing back. Click to let me speak."
              }
              aria-label={
                speaks ? "Silence my voice" : "Let me speak"
              }
              aria-pressed={speaks}
              data-testid="voice-speak-toggle"
              data-enabled={speaks ? "1" : "0"}
            >
              {speaks ? (
                <Volume2Icon className="size-3.5" />
              ) : (
                <VolumeXIcon className="size-3.5 text-[var(--strip-fg-3)]" />
              )}
              {/* Shape and words both carry it: the icon changes AND the
                  sentence changes, because colour alone never states a state. */}
              <span data-testid="voice-speak-state">
                {speaks ? "I speak: click to silence me" : "Silenced: click to let me speak"}
              </span>
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
};
