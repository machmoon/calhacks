import { useEffect, useState } from "react";
import { wrightReadFile, wrightResolve } from "../api";

/** Several recorded paths found on this machine; undefined while looking. */
export function useResolvedMany(runDir: string | null, recorded: (string | undefined)[]): (string | null | undefined)[] {
  const key = `${runDir}|${recorded.join("|")}`;
  const [paths, setPaths] = useState<(string | null | undefined)[]>(recorded.map(() => undefined));
  useEffect(() => {
    let off = false;
    setPaths(recorded.map(() => undefined));
    Promise.all(
      recorded.map((r) => (r && runDir ? wrightResolve(runDir, r).catch(() => null) : Promise.resolve(null)))
    ).then((p) => !off && setPaths(p));
    return () => {
      off = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return paths;
}

/** A JSON file the run wrote, parsed; null when absent or unreadable. */
export function useJson(path: string | null | undefined): unknown {
  const [value, setValue] = useState<unknown>(null);
  useEffect(() => {
    setValue(null);
    if (!path) return;
    let off = false;
    wrightReadFile(path)
      .then((b) => !off && setValue(JSON.parse(new TextDecoder().decode(b))))
      .catch(() => !off && setValue(null));
    return () => {
      off = true;
    };
  }, [path]);
  return value;
}

const MIME: Record<string, string> = { svg: "image/svg+xml", png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg" };

/** An object URL for an image file; revoked on change. */
export function useImageUrl(path: string | null | undefined): { url: string | null; error: string | null } {
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setUrl(null);
    setError(null);
    if (!path) return;
    let off = false;
    let made: string | null = null;
    wrightReadFile(path)
      .then((bytes) => {
        if (off) return;
        made = URL.createObjectURL(new Blob([bytes as BlobPart], { type: MIME[path.split(".").pop()?.toLowerCase() ?? ""] ?? "image/png" }));
        setUrl(made);
      })
      .catch((e) => !off && setError(String(e)));
    return () => {
      off = true;
      if (made) URL.revokeObjectURL(made);
    };
  }, [path]);
  return { url, error };
}
