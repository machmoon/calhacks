// Whether the microphone is open: the push-to-talk recorder's own state.
//
// The overlay must never open a second `getUserMedia` stream just to find out
// (on macOS that is a second permission prompt and a chance of two streams
// fighting over the input device). `VoiceControl` owns the recorder and calls
// `setPushToTalk`; the orb and anything else that draws "mic on" subscribes.
// Nothing here ever touches the microphone.

import { useEffect, useState } from "react";

// Push-to-talk records from a stream of its own, so it announces itself here:
// the code that owns the microphone is the only code allowed to say whether
// it is open.

let pushToTalk = false;
const pushSubscribers = new Set<(on: boolean) => void>();

/** Called by the one control that records: true while the recorder is open. */
export function setPushToTalk(on: boolean): void {
  if (pushToTalk === on) return;
  pushToTalk = on;
  for (const notify of pushSubscribers) notify(on);
}

/** Is push-to-talk recording right now? */
export function isPushToTalk(): boolean {
  return pushToTalk;
}

/** Subscribe to push-to-talk. Re-renders only the caller. */
export function usePushToTalk(): boolean {
  const [on, setOn] = useState(pushToTalk);
  useEffect(() => {
    setOn(pushToTalk);
    pushSubscribers.add(setOn);
    return () => {
      pushSubscribers.delete(setOn);
    };
  }, []);
  return on;
}
