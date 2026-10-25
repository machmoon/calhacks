// The words on the folded pill.
//
// The pill used to be three icons — a dot, a mic, an arrow — and Pat's verdict
// was that it was too small to be worth folding to: it could not answer "is
// anything happening". So the pill carries a word for it, a state and never a
// mood.

export interface RunWordInput {
  /** The live run's status; anything but `idle` has something on screen. */
  status: string;
  /** The step flow's status. */
  stepsStatus: string;
  /** `/healthz` answered ok on the last probe. */
  engineOk: boolean;
  /** False until the first probe lands: "I do not know yet" is its own answer. */
  engineSettled: boolean;
}

/**
 * What the run is doing, as a phrase.
 *
 * The engine's state outranks the run's: a strip folded down beside a dead
 * engine would otherwise read "Nothing running", which is true and useless.
 * And "Nothing running" is printed rather than left blank, because a blank
 * space is the state a folded overlay is accused of being in when a paid run
 * finished invisibly.
 */
export function runWord(input: RunWordInput): string {
  if (!input.engineSettled) return "Checking the engine";
  if (!input.engineOk) return "Engine down";
  if (input.status === "running" || input.stepsStatus === "running") return "Working";
  if (input.stepsStatus === "waiting") return "Waiting for you";
  if (input.stepsStatus === "error" || input.status === "error") return "A stage failed";
  if (input.status === "done" || input.stepsStatus === "done") return "Run finished";
  return "Nothing running";
}
