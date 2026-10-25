"""Independent dimensional/volume checks from pinned OpenRocket examples.

This is a part geometry benchmark, NOT a complete importer or flight check.
Every unsupported or unresolved component is reported and fails full coverage.
Run with PYTHONPATH=cad <cad-python> evals/cad/openrocket_parts.py.
"""
import hashlib
import json
import math
from pathlib import Path
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile

from wright_cad import kernel
from build123d import import_step

ROOT = Path(__file__).resolve().parent


def number(element, name):
    text = element.findtext(name)
    if text is None or text.startswith("auto"):
        raise ValueError(f"{name}: missing or automatic dimension requires assembly resolution")
    value = float(text) * 1000  # OpenRocket metres -> CAD millimetres
    if not math.isfinite(value):
        raise ValueError(f"{name}: nonfinite")
    return value


def case(element):
    tag = element.tag
    if tag == "trapezoidfinset":
        if element.findtext("crosssection", "square") != "square":
            raise ValueError("non-square fin cross-section is not implemented")
        if float(element.findtext("filletradius", "0")) != 0:
            raise ValueError("fin fillets are not implemented")
        root, tip, span, sweep, thickness = [number(element,k) for k in ("rootchord","tipchord","height","sweeplength","thickness")]
        tab_height = float(element.findtext("tabheight", "0"))*1000
        tab_length = float(element.findtext("tablength", "0"))*1000 if tab_height else 0
        offset = 0
        if tab_height:
            pos = element.find("tabposition")
            if pos is None or pos.get("relativeto") not in ("center", "middle"):
                raise ValueError("unsupported tab positioning")
            offset = float(pos.text)*1000 + (root-tab_length)/2
            offset = max(0, offset)  # absorb source floating point round-off
        params = dict(root=root, tip=tip, span=span, sweep=sweep, thickness=thickness,
                      tab_height=tab_height, tab_length=tab_length, tab_offset=offset)
        volume = ((root+tip)*span/2 + tab_length*tab_height)*thickness
        return "airframe.trapezoid_fin", params, volume, [max(root,sweep+tip)-min(0,sweep),span+tab_height,thickness]
    length = number(element, "length")
    if tag in ("bodytube", "innertube", "tubecoupler", "launchlug"):
        r = number(element, "radius" if tag == "bodytube" else "outerradius")
        wall = number(element, "thickness")
        if not 0 < wall < r:
            raise ValueError("tube needs 0 < wall < radius")
        params = dict(od=2*r, wall=wall, length=length)
        volume = math.pi * (r*r-(r-wall)**2) * length
        return "common.tube", params, volume, [2*r, 2*r, length]
    if tag in ("bulkhead", "centeringring", "engineblock"):
        r = number(element, "outerradius")
        bore = 0 if tag == "bulkhead" else number(element, "innerradius")
        if not 0 <= bore < r:
            raise ValueError("ring needs 0 <= inner radius < outer radius")
        return "common.disc", dict(od=2*r, bore=2*bore, thickness=length), math.pi*(r*r-bore*bore)*length, [2*r, 2*r, length]
    raise ValueError(f"unsupported component {tag}")


def evaluate():
    sources = json.loads((ROOT / "references/sources.json").read_text())
    designs = []
    with tempfile.TemporaryDirectory(prefix="wright-cad-eval-") as cache:
        for source in sources:
            path = ROOT / "references" / source["file"]
            raw = path.read_bytes()
            if hashlib.sha256(raw).hexdigest() != source["sha256"]:
                raise ValueError(f"source checksum mismatch: {path}")
            with zipfile.ZipFile(path) as archive:
                root = ET.fromstring(archive.read("rocket.ork"))
            records = []
            for parent in root.iter("subcomponents"):
                for element in parent:
                    if element.tag == "stage":
                        continue  # container, not a geometric part
                    row = dict(id=element.findtext("id"), type=element.tag, name=element.findtext("name"))
                    try:
                        part, params, volume, extent = case(element)
                    except ValueError as exc:
                        row.update(status="unsupported", reason=str(exc))
                        records.append(row)
                        continue
                    try:
                        key = hashlib.sha256(json.dumps([part, params], sort_keys=True).encode()).hexdigest()
                        result = kernel.build(part, params, key, cache)
                        delta = abs(result["volume_mm3"]-volume) / volume
                        sizes = [result["bbox_mm"][1][i]-result["bbox_mm"][0][i] for i in range(3)]
                        bbox_error = max(abs(a-b) for a,b in zip(sizes, extent))
                        restored = import_step(Path(cache)/(key+".step"))
                        step_error = abs(restored.volume-volume)/volume
                        passed = result["valid"] and restored.is_valid and delta < 1e-6 and bbox_error < 1e-4 and step_error < 1e-6
                        row.update(status="pass" if passed else "fail", part=part, params=params,
                                   expected_volume_mm3=volume, volume_relative_error=delta,
                                   bbox_max_error_mm=bbox_error, step_volume_relative_error=step_error)
                    except Exception as exc:
                        row.update(status="fail", reason=f"{type(exc).__name__}: {exc}")
                    records.append(row)
            counts = {s: sum(r['status']==s for r in records) for s in ('pass','fail','unsupported')}
            designs.append(dict(source=source, counts=counts, full_geometry_pass=bool(records) and counts['pass']==len(records), components=records))
    report = dict(scope="individual source-dimensioned tubes, rings and square-section fins; no assembly/flight claim", designs=designs,
                  passed=bool(designs) and all(d['full_geometry_pass'] for d in designs))
    (ROOT/'result.json').write_text(json.dumps(report, indent=2, allow_nan=False))
    print(json.dumps({"passed":report['passed'], "designs":[dict(file=d['source']['file'], **d['counts']) for d in designs]},indent=2))
    return report


if __name__ == '__main__':
    report = evaluate()
    sys.exit(0 if report['passed'] else 1)
