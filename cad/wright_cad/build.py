"""Build a declarative assembly with content-addressed, per-part reuse.

Usage: PYTHONPATH=cad <cad-python> -m wright_cad.build plan.json -o out/cad
This entry point keeps one kernel process alive for the complete assembly.
"""
import argparse
import hashlib
import json
from pathlib import Path
import time
from . import library
from .validation import validate_instances


def cache_key(part, params, material=None, density=None, kind=None):
    digest = hashlib.sha256()
    for path in sorted(set(library._dependency_files(library.part_file(part)))):
        digest.update(str(path.relative_to(library.CAD_ROOT)).encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    digest.update(json.dumps(dict(part=part, params=params, material=material, density=density, kind=kind),
                             sort_keys=True, allow_nan=False, separators=(",", ":")).encode())
    # Kernel changes require a new cache; build123d and OCP upgrades do as well.
    from importlib.metadata import version
    for package in ("build123d", "cadquery-ocp-novtk"):
        digest.update(version(package).encode())
    return digest.hexdigest()


def build_plan(plan, out_dir):
    from . import kernel
    started = time.perf_counter()
    validate_instances(plan["instances"])
    out = Path(out_dir).resolve()
    cache = out / "cache"
    instances = []
    hits = misses = 0
    for raw in plan["instances"]:
        inst = dict(raw)
        params = library.check_params(inst["part"], inst.get("params", {}))
        key = cache_key(inst["part"], params, inst.get("material"), inst.get("density"), inst.get("kind"))
        metadata = cache / (key + ".json")
        hit = False
        try:
            record = json.loads(metadata.read_text())
            required = [".brep", ".step"] + ([".stl"] if record.get("stl") else [])
            hit = record.get("key") == key and record.get("valid") is True and all(
                (cache / (key + suffix)).is_file() for suffix in required)
        except (OSError, ValueError):
            pass
        if hit:
            hits += 1
        else:
            kernel.build(inst["part"], params, key, str(cache),
                         material=inst.get("material"), density=inst.get("density"), kind=inst.get("kind"))
            misses += 1
        inst.update(key=key, cache=str(cache), params=params)
        instances.append(inst)
    report = kernel.assemble(plan["name"], instances, str(out))
    report["cache"] = dict(hits=hits, misses=misses)
    report["total_ms"] = (time.perf_counter() - started) * 1000
    (out / "assembly.json").write_text(json.dumps(report, indent=2, allow_nan=False))
    (out / "plan.json").write_text(json.dumps(plan, indent=2, allow_nan=False))
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plan", type=Path)
    parser.add_argument("-o", "--out", required=True)
    args = parser.parse_args()
    report = build_plan(json.loads(args.plan.read_text()), args.out)
    print(json.dumps({k: report[k] for k in ("passed", "n_parts", "mass_kg", "cache", "total_ms", "clashes", "floating")}, indent=2))
    raise SystemExit(0 if report["passed"] else 1)


if __name__ == "__main__":
    main()
