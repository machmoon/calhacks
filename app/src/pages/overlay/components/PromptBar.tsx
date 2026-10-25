import { useRef, useState } from "react";
import { CornerDownLeftIcon, XIcon } from "lucide-react";
import { Button, Input } from "@/components";
import { cn } from "@/lib/utils";
import type { EngineHealth } from "@/hooks";
import { useSilkscreenRun, type RunRequestDraft } from "@/contexts";
import type { SummaryMode } from "@/lib/silkscreen/types";
import { useIsSpeaking } from "@/hooks/useIsSpeaking";
import { VoiceControl } from "./VoiceControl";

// There is no status indicator in this row, and that is the whole design.
// The row is: field, microphone, ⏎ — and the two controls the page puts
// around it (collapse on the left, dashboard then drag handle on the right).
// Six things, in that order, in both bar sizes as far as each has them.
// A status dot lived here, then a living orb replaced it; both were removed
// on 2026-09-08 because a permanent indicator at the head of a 58px strip is
// the first thing the eye lands on and, resting, it says nothing.
//
// Nothing became invisible with them. An engine that is not answering puts
// "The engine is not answering; a run would fail immediately" on the ⏎
// control's own label (`submitReason` below) and raises the full-width
// engine-down banner in the card (`index.tsx`, `engine-down-reason`), which
// is a sentence at the width of the window rather than eight pixels of hue.
// The microphone draws its own state, because it is the organ that has one.

/**
 * What the strip says where the field was while I talk, in the first person.
 *
 * It names the microphone because the microphone is the one control on the
 * strip and, while I talk, clicking it stops me.
 */
export const SPEAKING_LINE = "I’m talking: click the mic to stop me.";

/**
 * The speaking state, in the field's own place and at the field's own size.
 *
 * It must never be taller than the input it stands in for — the whole point
 * is that the strip does not move. It carries no button: clicking the
 * microphone beside it stops me.
 */
const SpeakingPanel = () => {
  return (
    <div
      // `kv-swap-in` and not `kv-settle`: this is a substitution, not an
      // arrival. The panel stands in the field's own slot at the field's own
      // height, so it must fade *through* it rather than rise into it — a
      // transform here would make the two cross past each other. See §1 of
      // src/motion.css.
      className="kv-swap-in flex h-9 min-w-0 flex-1 items-center gap-2 rounded-md border border-input/50 bg-muted/30 px-3"
      data-testid="prompt-speaking"
      data-state="speaking"
      role="status"
    >
      <span className="min-w-0 flex-1 truncate text-[12px] leading-tight">{SPEAKING_LINE}</span>
    </div>
  );
};

/**
 * Three boards the engine is known to build, offered under an empty field.
 *
 * Pressing one fills the field and nothing else: the sentence is then the
 * person's to edit and to submit, and the ⏎ control keeps saying what that
 * costs. They show only while the field is empty and has focus, so the
 * resting strip over KiCad is still one row; the window grows to fit them
 * through the same raise-only measurement that fits a banner
 * (`overlayStateFor` in index.tsx, `sizeFor` in lib/overlay-size.ts).
 */
export const EXAMPLE_PROMPTS: readonly string[] = [
  "A 3.3 V LDO board off USB-C with a power LED",
  "A 555 timer LED blinker on a 9 V battery",
  "An ESP32 dev board with USB-C and a reset button",
];

/** Everything that decides what the ⏎ control promises. */
export interface SubmitState {
  /** The field's contents, untrimmed. */
  intent: string;
  /** The engine answered its last probe. */
  engineOk: boolean;
  /** A run is in flight in either state machine. */
  busy: boolean;
  /** A step run is holding for approval. */
  awaitingApproval: boolean;
}

/**
 * What the ⏎ control will do, in one sentence, for the tooltip and the
 * accessible name.
 *
 * This function is the whole reason a glyph is allowed here. The button used
 * to say "Generate" or "Send" and carry the money warning in a title beside
 * the word; with the word gone, the title is the only copy left, so it is
 * pure, exported, and pinned by test rather than being prose inside JSX.
 *
 * Order matters and is deliberate. The mid-run case is answered first because
 * it is the one that changes what the button *costs*: it does not spend, and
 * saying "costs money" over a keystroke that only parks a sentence would be
 * the same lie in the other direction. Nothing was dropped — the engine and
 * empty-field reasons are still here, in their old words.
 */
export function submitReason(state: SubmitState): string {
  if (!state.intent.trim()) {
    if (state.busy) return "Say what you want changed and I will hold it until this run lands";
    return state.awaitingApproval
      ? "Answer the step above. Say “go” to approve it."
      : "Type what you want on the board first";
  }
  // Spends nothing: the sentence is held until the run in flight lands, and
  // then offered. This is checked before the engine probe on purpose —
  // holding a sentence works whether or not the engine is answering.
  if (state.busy) return "Held until the current run ends. Nothing is spent.";
  if (!state.engineOk) return "The engine is not answering; a run would fail immediately";
  return state.awaitingApproval
    ? "Send this to the run in progress"
    : "Generate a board: this calls the model and costs money";
}

export interface PromptBarProps {
  request: RunRequestDraft;
  onRequestChange: (patch: Partial<RunRequestDraft>) => void;
  onSubmit: () => void;
  onCancel: () => void;
  /** The state machine's own guard: an in-flight run makes this false. */
  canStart: boolean;
  submitDisabled?: boolean;
  nativeCad?: boolean;
  /** A run is in flight. */
  busy: boolean;
  /** The overlay is hidden by the global shortcut; keep focus out of it. */
  hidden: boolean;
  /**
   * True when cancelling reaches the engine — a step session exists, so the
   * run can be closed to further steps, which is where the money is.
   *
   * False for the two cases where all the button can do is stop this client
   * waiting: a one-shot run, and the first phase of a step run (`POST /steps`
   * is synchronous and there is no session id until it returns — and that is
   * the most model-expensive phase there is). The label says which, because
   * "nothing more is charged" is simply untrue of a model call already in
   * flight.
   */
  cancelReaches?: boolean;
  engine: EngineHealth;
  baseUrl: string;
  /** See RunOptions: routes the submit to the step-by-step KiCad flow. */
  stepMode?: boolean;
  onStepModeChange?: (enabled: boolean) => void;
  /**
   * See RunOptions: how the finished run summarises itself. Passed straight
   * through, like `stepMode` — the bar owns neither, it only carries them to
   * the options popover, and the page folds this one into the request it
   * starts.
   */
  summary?: SummaryMode;
  onSummaryChange?: (mode: SummaryMode) => void;
  /**
   * A step run is open and holding for approval, so what is typed here is a
   * reply to it rather than a new board. The bar says so, because the only
   * place that behaviour was written down was the code that implements it.
   */
  awaitingApproval?: boolean;
  /** The imperative for the next approvable stage, e.g. "Route copper". */
  nextAction?: string | null;
  /**
   * Where a transcript goes. The default drops it in the draft for the user to
   * read and edit. The page overrides it while a stage waits, so speech can
   * arm an approval — and only arm it. Nothing spoken has ever reached the
   * submit path and nothing here may change that.
   */
  onTranscript?: (text: string) => void;
  /** Test seam for the speech poll; production reads the module speaker. */
  isSpeaking?: () => boolean;
}

export const PromptBar = ({
  request,
  onRequestChange,
  onSubmit,
  onCancel,
  canStart,
  submitDisabled = false,
  nativeCad = false,
  busy,
  hidden,
  cancelReaches = false,
  engine,
  baseUrl,
  awaitingApproval = false,
  nextAction = null,
  onTranscript,
  isSpeaking,
}: PromptBarProps) => {
  // `canStart` is still the guard on *spending*: it is false while a run is in
  // flight. Submitting is wider than spending now — a sentence typed mid-run
  // is parked by the page, which costs nothing — so the control is live
  // whenever the bar is on screen, and the page decides what the sentence
  // does. The one thing that must not happen is a second paid run starting
  // without a click on a button that said it would; that lives in the page.
  const submittable = (canStart || busy) && !hidden && !submitDisabled;
  // What cancelling actually does, which is not the same in the two cases and
  // must never claim the second one. A model call already in flight is paid
  // for whether or not anyone is waiting for it, and the placement solve runs
  // its full budget; what a real cancel buys is the steps that now never run.
  const cancelDetail = cancelReaches
    ? "Close this run. No further step runs. Work already under way (the placement solve, a model call, the case and parts jobs) finishes and is discarded."
    : "Stop waiting on this. It does not stop the work: a model call already in flight is paid for either way, and this run has no session yet to close.";
  const reason = nativeCad ? "Build the native FreeCAD project" : submitReason({
    intent: request.intent,
    engineOk: engine.ok,
    busy,
    awaitingApproval,
  });
  // Am I talking? Its own fact, published by the speech layer. The hook is called unconditionally — the seam overrides its answer, it
  // does not replace the call, or the hook order would change with a prop.
  const live = useIsSpeaking();
  const speaking = isSpeaking ? isSpeaking() : live;
  // Only for the bearer token — the run itself stays behind onSubmit.
  const { token } = useSilkscreenRun();

  // While a stage waits, this field is a conversation with the run: "go"
  // approves, "start over" restarts. Nothing typed here starts a second
  // board, so the control says Send rather than Generate.
  const placeholder = awaitingApproval
    ? nextAction
      ? `Say \u201cgo\u201d to ${nextAction.toLowerCase()}, or ask for a change`
      : "Ask for a change, or describe a different board"
    : busy
      ? "Say what you want changed \u2014 I\u2019ll hold it until this lands"
      : "What do you need built? Drop a VTOL PDF.";

  // While I talk the field steps aside and the speaking state stands in its
  // place, at the same size. Nothing here touches request.intent: the draft
  // lives in the parent, so the text is exactly where it was when the field
  // comes back.
  const showSpeaking = speaking && !hidden;

  // Focus-within, tracked by hand: React's onFocus/onBlur bubble, and a blur
  // whose `relatedTarget` is still inside the row (the field to a chip, a chip
  // to the arrow) is not the person leaving. A chip press is the case that
  // needs care: macOS WebKit does not focus a button on click, so the field
  // would blur with `relatedTarget` null, the row would unmount on mousedown
  // and the click would land on nothing. Each chip prevents the mousedown's
  // default, so the field keeps focus through the press and the click fires.
  const [within, setWithin] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const showExamples =
    within && !request.intent.trim() && !busy && !awaitingApproval && !hidden && !showSpeaking;
  const fill = (example: string) => {
    onRequestChange({ intent: example });
    // The chip that was pressed is about to unmount; the sentence it left is
    // the person's to edit, so the caret goes back to the field.
    inputRef.current?.focus();
  };

  return (
    // `min-w-0` is load-bearing: a flex item's default min-width is `auto`,
    // so without it this row refuses to shrink below its own content and
    // pushes Generate, the dashboard button and the drag handle off the
    // 600px card whenever a status line beside the mic gets long. Seen on
    // screen with a long status line showing.
    <div
      // `flex-wrap` only while the examples are shown: their row has
      // `basis-full`, so it is the one child that wraps, and the field's
      // `flex-1` (basis 0) keeps the controls on the first line as before.
      className={cn("flex min-w-0 flex-1 items-center gap-1.5", showExamples && "flex-wrap")}
      onFocus={() => setWithin(true)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setWithin(false);
      }}
    >
      {showSpeaking ? (
        <SpeakingPanel />
      ) : (
        <Input
          ref={inputRef}
          placeholder={nativeCad ? "Describe the CAD build, or use the attached VTOL PDF" : placeholder}
          value={request.intent}
          // Typing is no longer switched off while a run is in flight. A
          // disabled field is the app refusing a sentence before it has heard
          // it; what the sentence costs is decided when it is submitted, and
          // mid-run it costs nothing — the page parks it.
          disabled={hidden}
          data-testid="prompt-input"
          // The other half of the same substitution; see SpeakingPanel.
          className="kv-swap-in flex-1"
          onChange={(e) => onRequestChange({ intent: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && submittable) {
              e.preventDefault();
              onSubmit();
            }
          }}
        />
      )}

      {/* One microphone, and it is the row's only voice control. Tapping or
          holding it is push-to-talk; tapping it while I am talking stops me. `speaking` is passed rather than
          subscribed so the panel's sentence and the button's job cannot
          disagree about whether a reply is playing. */}
      <VoiceControl
        baseUrl={baseUrl}
        token={token}
        disabled={busy || hidden}
        speaking={speaking}
        // The transcript joins the draft through the same onChange path typing
        // uses, for the user to review and edit. It must never submit.
        onTranscript={(text) =>
          onTranscript
            ? onTranscript(text)
            : onRequestChange({
                intent: request.intent.trim()
                  ? `${request.intent.trimEnd()} ${text}`
                  : text,
              })
        }
      />

      {/* The run-options gear used to sit here. It is out of the row on
          purpose: the strip is meant to be one microphone and one way to
          submit, and a gear in a 58 px bar reads as chrome. The same
          `RunOptions` form is mounted in the card below (index.tsx,
          `run-options-disclosure`), so the solver budget, the datasheets, the
          step-by-step switch and the summary mode are all still reachable —
          they are just no longer in the way. */}

      {/* Cancel comes BEFORE the arrow, so the arrow is the last control in
          the row in every state. See the note on the arrow below. */}
      {busy ? (
        <span title={cancelDetail}>
          <Button
            variant="outline"
            size="icon"
            onClick={onCancel}
            aria-label={cancelDetail}
            data-testid="prompt-cancel"
            data-reaches={String(cancelReaches)}
          >
            <XIcon className="size-3.5" />
          </Button>
        </span>
      ) : null}

      <span
        // The title lives on a wrapper because a disabled element suppresses
        // pointer events in some webviews — the one place the reason lived
        // was the one place it could never show.
        //
        // The control is a glyph now, so this string is the only place the
        // money warning and the refusal reasons are written down. Every
        // sentence that was reachable here still is, plus the mid-run one,
        // and `submitReason` is exported so a test reads the same function
        // the button does rather than a copy of its prose.
        title={reason}
      >
        <Button
          size="sm"
          onClick={onSubmit}
          disabled={!submittable}
          // A glyph-only button reads as "corner down left" without this.
          aria-label={reason}
          // This is the control that spends money — except while a run is in
          // flight, when what it does instead is hold the sentence, and the
          // label says which.
          data-testid="prompt-submit"
          // What it will do with the field: `start` spends a pipeline, `reply`
          // answers the stage that is waiting, `park` spends nothing. Pinned
          // by test so the promise is checked without reading prose.
          data-intent={busy ? "park" : awaitingApproval ? "reply" : "start"}
        >
          <CornerDownLeftIcon className="size-3.5" />
        </Button>
      </span>

      {showExamples ? (
        // Under the field, at the field's own text size. Each one is a whole
        // sentence, not a category, so what lands in the field is a board.
        <div className="flex basis-full flex-wrap gap-1 pl-0.5" data-testid="prompt-examples">
          {EXAMPLE_PROMPTS.map((example) => (
            <button
              key={example}
              type="button"
              className="rounded-full border border-input/50 bg-muted/30 px-2 py-0.5 text-[11px] leading-tight text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-[2px] focus-visible:ring-ring"
              // See the focus-within note above: the field keeps focus
              // through the press, so the row is still here for the click.
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => fill(example)}
              data-testid="prompt-example"
              data-example={example}
            >
              {example}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
};
