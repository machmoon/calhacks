import { listen } from "@tauri-apps/api/event";
import { WRIGHT_RUN_STARTED } from "./dashboard-handoff";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  wrightControl,
  wrightEnvStatus,
  wrightListRuns,
  wrightStart,
  wrightStatus,
  wrightStop,
  wrightTail,
  type ControlAction,
} from "./api";
import { fold, parseLines } from "./reducer";
import type { Domain, EnvStatus, RunListing, WrightEvent } from "./types";

export const POLL_MS = 500;
const REPLAY_STEP_MS = 120;

export type Mode = "idle" | "live" | "replay";

/**
 * One run on the page: either a live child (events polled from its
 * events.jsonl every 500 ms, the way a log tail works) or a recording
 * stepped through by a cursor. State is always `fold(events[0..cursor])`.
 */
export function useWrightRun() {
  const [mode, setMode] = useState<Mode>("idle");
  const [runDir, setRunDir] = useState<string | null>(null);
  const [events, setEvents] = useState<WrightEvent[]>([]);
  const [cursor, setCursor] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [running, setRunning] = useState(false);
  const starting = useRef(false);
  const [gate, setGate] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [runs, setRuns] = useState<RunListing[]>([]);
  const [env, setEnv] = useState<EnvStatus | null>(null);
  const [envChecking, setEnvChecking] = useState(false);
  const nextLine = useRef(0);

  const checkEnv = useCallback(async () => {
    setEnvChecking(true);
    try {
      setEnv(await wrightEnvStatus());
    } catch (e) {
      setError(String(e));
    } finally {
      setEnvChecking(false);
    }
  }, []);

  const refreshRuns = useCallback(async () => {
    try {
      setRuns(await wrightListRuns());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refreshRuns();
    void checkEnv();
    // Re-attach to a run the app already started (page left and re-entered).
    wrightStatus()
      .then((s) => {
        if (s.running && s.run_dir) attach(s.run_dir);
      })
      .catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const attach = (dir: string) => {
    nextLine.current = 0;
    setEvents([]);
    setCursor(0);
    setRunDir(dir);
    setMode("live");
    setRunning(true);
    setGate(null);
  };

  useEffect(() => {
    let disposed = false; let stop: (() => void) | undefined;
    listen<{ runDir: string | null }>(WRIGHT_RUN_STARTED, ({ payload }) => {
      if (payload.runDir) attach(payload.runDir);
    }).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; }).catch(() => undefined);
    return () => { disposed = true; stop?.(); };
    // attach only writes stable React setters and the cursor ref.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Live tail.
  useEffect(() => {
    if (mode !== "live" || !runDir) return;
    let stop = false;
    const tick = async () => {
      try {
        const r = await wrightTail(runDir, nextLine.current);
        if (stop) return;
        nextLine.current = r.next;
        if (r.lines.length) {
          const { events: fresh } = parseLines(r.lines);
          if (fresh.length) {
            setEvents((prev) => [...prev, ...fresh]);
            setCursor((c) => c + fresh.length);
          }
        }
        setGate(r.status?.state ?? null);
        setRunning(r.running);
        // The child ended with an error: say why, once, and stop polling.
        if (r.failure) {
          setError(r.failure);
          setMode("idle");
          void checkEnv();
        }
      } catch (e) {
        if (!stop) setError(String(e));
      }
    };
    void tick();
    const id = setInterval(tick, POLL_MS);
    return () => {
      stop = true;
      clearInterval(id);
    };
  }, [mode, runDir]);

  // Replay stepper.
  useEffect(() => {
    if (mode !== "replay" || !playing) return;
    const id = setInterval(() => {
      setCursor((c) => {
        if (c >= events.length) {
          setPlaying(false);
          return c;
        }
        return c + 1;
      });
    }, REPLAY_STEP_MS);
    return () => clearInterval(id);
  }, [mode, playing, events.length]);

  const start = useCallback(
    async (domain: Domain, request: string, attachmentIds: string[] = []) => {
      if (starting.current) return;
      starting.current = true;
      setError(null);
      try {
        const s = await wrightStart(domain, request, undefined, attachmentIds);
        if (s.run_dir) attach(s.run_dir);
        void refreshRuns();
      } catch (e) {
        setError(String(e));
      } finally { starting.current = false; }
    },
    [refreshRuns]
  );

  const replay = useCallback(async (dir: string, animate: boolean) => {
    setError(null);
    try {
      const r = await wrightTail(dir, 0);
      const { events: all, skipped } = parseLines(r.lines);
      setMode("replay");
      setRunDir(dir);
      setRunning(false);
      setGate(null);
      setEvents(all);
      setCursor(animate ? 0 : all.length);
      setPlaying(animate);
      if (skipped) setError(`${skipped} line(s) of events.jsonl were not events and were skipped.`);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const control = useCallback(
    async (action: ControlAction, note = "") => {
      if (!runDir) return;
      try {
        await wrightControl(runDir, action, note);
      } catch (e) {
        setError(String(e));
      }
    },
    [runDir]
  );

  const kill = useCallback(async () => {
    try {
      await wrightStop();
      setRunning(false);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const state = useMemo(() => fold(events.slice(0, cursor)), [events, cursor]);

  return {
    mode, runDir, state, events, cursor, setCursor, playing, setPlaying,
    running, gate: gate ?? state.gate, error, setError, runs, refreshRuns,
    env, envChecking, checkEnv,
    start, replay, control, kill,
  };
}
