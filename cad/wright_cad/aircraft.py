"""Convert Wright's metre-valued aircraft geometry export into a CAD plan.

Uses the domain's placements directly. Propeller sweep envelopes are recorded
as exclusions, not silently counted as manufactured solid parts.
"""
import argparse
import json
from pathlib import Path
import re
from . import Assembly, Loc


def plan_geometry(geometry):
    if geometry.get("unit") != "m":
        raise ValueError("aircraft geometry must declare metres")
    assembly = Assembly(geometry["name"])
    names = {p["name"]: "part_" + re.sub(r"[^A-Za-z0-9_]", "_", p["name"]) for p in geometry["parts"]}
    if len(set(names.values())) != len(geometry["parts"]):
        raise ValueError("duplicate or ambiguous part names")
    solid_names = {p["name"] for p in geometry["parts"] if not p["envelope"]}
    excluded = []
    for item in geometry["parts"]:
        if item["envelope"]:
            excluded.append(dict(name=item["name"], reason="swept clearance envelope; not a physical solid"))
            continue
        shape = item["shape"]
        kwargs = dict(touches=tuple(names[n] for n in item["touches"] if n in solid_names))
        if item["filament"]:
            kwargs["material"] = "petg" if "petg" in item["filament"].lower() else "pla_aero"
        kind = shape["type"]
        if kind == "box":
            lo, hi = shape["min"], shape["max"]
            assembly.add(names[item["name"]], "common.block", at=Loc(*(x*1000 for x in lo)),
                         x=(hi[0]-lo[0])*1000,y=(hi[1]-lo[1])*1000,z=(hi[2]-lo[2])*1000, **kwargs)
        elif kind == "tube":
            assembly.add(names[item["name"]], "common.axial_tube",
                         start=[v*1000 for v in shape["p0"]], end=[v*1000 for v in shape["p1"]],
                         radius=shape["r_out"]*1000,bore_radius=shape["r_in"]*1000, **kwargs)
        elif kind in ("loft", "shell"):
            scale = lambda rings: [[[v*1000 for v in point] for point in ring] for ring in rings]
            assembly.add(names[item["name"]], "common.loft", rings=scale(shape["rings"]),
                         inner=scale(shape.get("inner",[])), **kwargs)
        else:
            raise ValueError(f"unsupported aircraft geometry {kind}")
    for inst in assembly.instances:
        source = next(p for p in geometry["parts"] if names[p["name"]] == inst["name"])
        inst["kind"] = "printed" if source["filament"] else "bought"
    plan = assembly.to_json()
    plan["scope"] = "Existing domain geometry converted to B-rep; not a fabrication or flight approval"
    plan["excluded_envelopes"] = excluded
    plan["source_names"] = {v:k for k,v in names.items()}
    return plan


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("geometry",type=Path)
    parser.add_argument("plan",type=Path)
    args = parser.parse_args()
    plan = plan_geometry(json.loads(args.geometry.read_text()))
    args.plan.parent.mkdir(parents=True,exist_ok=True)
    args.plan.write_text(json.dumps(plan,indent=2,allow_nan=False))
    print(f"{len(plan['instances'])} solid instances; {len(plan['excluded_envelopes'])} separately recorded envelopes")


if __name__ == '__main__':
    main()
