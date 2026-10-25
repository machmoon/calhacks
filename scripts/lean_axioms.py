"""The Lean axiom gate for scripts/verify.sh.

Runs `#print axioms` (via `lake env lean`) on every theorem declared in
lean/Wright/*.lean and fails unless each rests only on propext, Quot.sound
and Classical.choice. A `sorry` shows up as `sorryAx`, a declared
`axiom cheat : False` as `cheat`: either fails. Grepping the build log for
"sorry" passed the second (review 4, finding 15).

Usage: python3 -I scripts/lean_axioms.py [LEAN_PROJECT_DIR]
"""
import os, re, subprocess, sys, tempfile

ALLOWED = {"propext", "Quot.sound", "Classical.choice"}


def theorems(src_dir):
    """Fully qualified theorem names, tracking `namespace`/`end`."""
    names, axioms = [], []
    for root, _, files in os.walk(src_dir):
        for fn in sorted(files):
            if not fn.endswith(".lean"):
                continue
            stack = []  # (kind, name) for namespace/section
            for line in open(os.path.join(root, fn), encoding="utf-8"):
                s = line.strip()
                m = re.match(r"namespace\s+(\S+)", s)
                if m:
                    stack.append(("ns", m.group(1)))
                    continue
                m = re.match(r"(?:noncomputable\s+)?section\b", s)
                if m:
                    stack.append(("sec", ""))
                    continue
                if re.match(r"end\b", s) and stack:
                    stack.pop()
                    continue
                m = re.match(r"(?:@\[[^\]]*\]\s*)?(?:(private|protected)\s+)?(theorem|lemma|axiom)\s+([^\s:({\[]+)", s)
                if m:
                    ns = ".".join(n for k, n in stack if k == "ns" and n)
                    full = f"{ns}.{m.group(3)}" if ns else m.group(3)
                    if m.group(2) == "axiom":
                        axioms.append(full)
                    elif m.group(1) == "private":
                        sys.exit(f"axiom gate: private theorem {full} cannot be named from outside; make it public")
                    else:
                        names.append(full)
    return names, axioms


def main():
    proj = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "lean")
    proj = os.path.abspath(proj)
    names, axioms = theorems(os.path.join(proj, "Wright"))
    if not names:
        sys.exit("axiom gate: no theorems found")
    with tempfile.NamedTemporaryFile("w", suffix=".lean", delete=False) as f:
        f.write("import Wright\n")
        for n in names:
            f.write(f"#print axioms {n}\n")
        path = f.name
    try:
        out = subprocess.run(["lake", "env", "lean", path], cwd=proj, capture_output=True, text=True)
    finally:
        os.unlink(path)
    text = out.stdout + out.stderr
    bad, seen = [], set()
    for m in re.finditer(r"'([^']+)' depends on axioms: \[([^\]]*)\]", text):
        seen.add(m.group(1))
        extra = sorted({a.strip() for a in m.group(2).split(",") if a.strip()} - ALLOWED)
        if extra:
            bad.append(f"{m.group(1)} depends on {', '.join(extra)}")
    for m in re.finditer(r"'([^']+)' does not depend on any axioms", text):
        seen.add(m.group(1))
    missing = [n for n in names if n not in seen]
    if out.returncode != 0 or missing:
        print(text.strip()[-3000:])
    for a in axioms:
        print(f"axiom declared: {a}")
    for b in bad:
        print(b)
    for n in missing:
        print(f"no axiom report for {n}")
    ok = out.returncode == 0 and not bad and not missing and not axioms
    print(f"axiom gate: {len(names)} theorems, {len(bad)} with disallowed axioms, {len(axioms)} axioms declared: {'PASS' if ok else 'FAIL'}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
