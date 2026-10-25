"""The part-library contract: a part is a plain build123d function with typed,
documented parameters, tagged with `@part`.

    from typing import Annotated as A
    from build123d import *
    from wright_cad import part, Param

    @part(material="al6061_t6", color="#b8bcc2", kind="machined")
    def injector_plate(
        od: A[float, Param("mm", "plate outer diameter", min=20, max=400)] = 76.0,
        holes: A[int, Param("", "orifices in the ring", min=1, max=360)] = 24,
    ) -> Part:
        "One line of what the part is. More lines: where its numbers come from."
        with BuildPart() as p:
            ...
            RigidJoint("chamber", p.part, Location((0, 0, 0)))
        return p.part

Conventions (from build123d's own examples and joints.py):
* millimetres and degrees, Z up, the part at its own origin; placement is
  the assembly's job (build123d `Location`, `RigidJoint.connect_to`);
* every interface another part attaches to is a named `RigidJoint`
  (build123d `joints.py`): the assembly connects joints, never raw numbers;
* the file `cad/parts/<domain>/<name>.py` holds one public function `<name>`;
  helpers start with `_`. Part ids are `<domain>.<name>`;
* a part imports only build123d, `wright_cad` and `cad/parts/common` --
  the cache key hashes the part's file, the domain's `_*.py`, `common/`
  and `wright_cad/`, so other imports would not invalidate it.

`Param` is to a part what a JSON-schema property is to a tool: unit, doc
and bounds the worker validates before building and agents read through
`describe`.
"""
from __future__ import annotations

import importlib.util
import inspect
import os
import re
import math
import sys
import typing
from dataclasses import dataclass, field
from pathlib import Path

CAD_ROOT = Path(__file__).resolve().parent.parent
PARTS_ROOT = CAD_ROOT / "parts"

KINDS = ("printed", "machined", "cut", "bought", "molded", "laid_up")


@dataclass(frozen=True)
class Param:
    """Unit ("mm", "deg", "", "count"...), one-line doc, optional bounds."""

    unit: str
    doc: str
    min: float | None = None
    max: float | None = None
    choices: tuple | None = None


@dataclass
class PartMeta:
    id: str
    material: str
    color: str
    kind: str
    version: str
    doc: str
    params: list = field(default_factory=list)


def part(*, material: str, color: str = "#9aa0a6", kind: str = "printed", version: str = "1"):
    """Tag a function as a Wright part. `material` is a key of
    `wright_cad.materials.MATERIALS` (density for mass properties; an
    assembly may override per instance), `color` "#rrggbb", `kind` one of
    KINDS (printed parts also get an STL for slicing)."""
    if kind not in KINDS:
        raise ValueError(f"kind {kind!r} not in {KINDS}")

    def deco(fn):
        fn.__wright_part__ = dict(material=material, color=color, kind=kind, version=version)
        return fn

    return deco


def _param_rows(fn) -> list[dict]:
    hints = typing.get_type_hints(fn, include_extras=True)
    rows = []
    for name, p in inspect.signature(fn).parameters.items():
        h = hints.get(name)
        meta = None
        base = h
        if typing.get_origin(h) is typing.Annotated:
            base, *extra = typing.get_args(h)
            meta = next((e for e in extra if isinstance(e, Param)), None)
        if meta is None:
            raise TypeError(f"{fn.__name__}: parameter {name!r} needs Annotated[type, Param(unit, doc)]")
        if p.default is inspect.Parameter.empty:
            raise TypeError(f"{fn.__name__}: parameter {name!r} needs a default (a part must build from defaults)")
        rows.append(dict(name=name, type=getattr(base, "__name__", str(base)), default=p.default,
                         unit=meta.unit, doc=meta.doc, min=meta.min, max=meta.max,
                         choices=list(meta.choices) if meta.choices else None))
    return rows


_modules: dict[str, tuple[float, object]] = {}


def part_file(part_id: str) -> Path:
    domain, _, name = part_id.partition(".")
    if not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*\.[A-Za-z][A-Za-z0-9_]*", part_id):
        raise ValueError(f"part id {part_id!r} is not '<domain>.<name>'")
    return PARTS_ROOT / domain / f"{name}.py"


def load(part_id: str):
    """The part function for `<domain>.<name>`, re-imported when its file changed."""
    path = part_file(part_id)
    if not path.exists():
        raise FileNotFoundError(f"no part file {path}")
    stamp = max(f.stat().st_mtime for f in _dependency_files(path))
    hit = _modules.get(part_id)
    if hit and hit[0] == stamp:
        return hit[1]
    for k in [k for k in sys.modules if k.startswith("wright_parts.")]:
        del sys.modules[k]  # helpers may have changed too
    if str(PARTS_ROOT.parent) not in sys.path:
        sys.path.insert(0, str(PARTS_ROOT.parent))
    spec = importlib.util.spec_from_file_location(f"wright_parts.{part_id}", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    domain, _, name = part_id.partition(".")
    fn = getattr(mod, name, None)
    if fn is None or not hasattr(fn, "__wright_part__"):
        raise TypeError(f"{path} must define @part def {name}(...)")
    _modules[part_id] = (stamp, fn)
    return fn


def _dependency_files(path: Path) -> list[Path]:
    """What a part's cache key covers (the Rust side hashes the same list:
    crates/wright-cad/src/cache.rs `dependency_files`)."""
    files = [path]
    files += sorted(path.parent.glob("_*.py"))
    files += sorted((PARTS_ROOT / "common").glob("*.py"))
    files += sorted((CAD_ROOT / "wright_cad").glob("*.py"))
    return files


def describe(part_id: str) -> dict:
    fn = load(part_id)
    m = fn.__wright_part__
    return dict(id=part_id, doc=inspect.getdoc(fn) or "", params=_param_rows(fn), **m)


def all_parts(domain: str | None = None) -> list[str]:
    out = []
    for d in sorted(p for p in PARTS_ROOT.iterdir() if p.is_dir() and not p.name.startswith(("_", "."))):
        if domain and d.name not in (domain, "common"):
            continue
        for f in sorted(d.glob("*.py")):
            if not f.name.startswith("_") and f.stem != "assembly":
                out.append(f"{d.name}.{f.stem}")
    return out


def check_params(part_id: str, params: dict) -> dict:
    """Defaults filled in; unknown names, wrong types and out-of-bounds values
    refused with the parameter's name, unit and bounds."""
    fn = load(part_id)
    rows = {r["name"]: r for r in _param_rows(fn)}
    unknown = set(params) - set(rows)
    if unknown:
        raise ValueError(f"{part_id}: unknown parameter(s) {sorted(unknown)}; has {sorted(rows)}")
    full = {}
    for name, r in rows.items():
        v = params.get(name, r["default"])
        if r["type"] in ("float", "int"):
            if isinstance(v, bool) or not isinstance(v, (int, float)):
                raise ValueError(f"{part_id}.{name} must be {r['type']}")
            if r["type"] == "int" and (not math.isfinite(v) or int(v) != v):
                raise ValueError(f"{part_id}.{name} must be an integer")
            if v != v or v in (float("inf"), float("-inf")):
                raise ValueError(f"{part_id}.{name} = {v} is not finite")
            if r["min"] is not None and v < r["min"] or r["max"] is not None and v > r["max"]:
                raise ValueError(f"{part_id}.{name} = {v} {r['unit']} outside [{r['min']}, {r['max']}] ({r['doc']})")
            v = int(round(v)) if r["type"] == "int" else float(v)
        if r["type"] == "list":
            if not isinstance(v, (list, tuple)):
                raise ValueError(f"{part_id}.{name} must be a list")
            from .validation import finite_tree
            finite_tree(v, f"{part_id}.{name}")
        if r["type"] == "bool" and not isinstance(v, bool):
            raise ValueError(f"{part_id}.{name} must be a boolean")
        if r["type"] == "str" and not isinstance(v, str):
            raise ValueError(f"{part_id}.{name} must be a string")
        if r["choices"] and v not in r["choices"]:
            raise ValueError(f"{part_id}.{name} = {v!r} not one of {r['choices']}")
        full[name] = v
    return full
