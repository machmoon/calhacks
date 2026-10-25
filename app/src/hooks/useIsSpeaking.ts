// "Am I talking?", as a React value.
//
// The strip swaps its field for a speaking panel while I talk, so speaking is
// its own fact and is published as one.

import { useEffect, useState } from "react";
import { isAnnouncing, subscribeSpeaking } from "@/lib/speech";

export function useIsSpeaking(): boolean {
  const [speaking, setSpeaking] = useState(isAnnouncing);
  useEffect(() => {
    // Re-read on mount: an utterance may have started between the initial
    // state and the subscription.
    setSpeaking(isAnnouncing());
    return subscribeSpeaking(setSpeaking);
  }, []);
  return speaking;
}
