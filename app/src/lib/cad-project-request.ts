/** Explicit project briefs go to the native CAD recipe runner, never PCB synthesis. */
export function cadProjectBrief(request: string): string | null {
  const token = request.split(/\s+/).map((s) => s.replace(/^[`'".]+|[`'".]+$/g, ""))
    .find((s) => s.startsWith("cad/projects/") && s.endsWith("/BUILD_PROMPT.txt"));
  return token ?? null;
}

/** An attached specification always enters native CAD, including an empty prompt. */
export function nativeCadSubmission(request: string, attachmentIds: string[] = []) {
  if (!attachmentIds.length && !cadProjectBrief(request)) return null;
  return { domain: "aircraft" as const, request: request.trim() || "Build the VTOL from the attached specification", attachmentIds };
}
