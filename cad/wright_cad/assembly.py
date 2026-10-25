"""Declarative assemblies: what to build and where it goes, not the solids.

A domain's `cad/parts/<domain>/assembly.py` defines

    def assembly(design: dict) -> Assembly

reading the design JSON the domain wrote (`<out_dir>/design.json`). It only
*declares* instances (part id + parameters + placement); the Rust side
(crates/wright-cad) hashes each instance, builds the ones not in the cache on
a pool of workers in parallel, then one worker places them and checks the
whole assembly. That split is what makes a one-parameter edit rebuild one part.

Placement follows build123d (`joints.py`: `RigidJoint.connect_to` moves the
other part so its joint coincides with this one's) and CadQuery
(`cadquery/assembly.py`: `Assembly.add(obj, loc=Location, name=, color=)`,
names as the tree path):

    a = Assembly("rocket")
    a.add("chamber", "rocket.chamber", at=Loc(0, 0, 120), group="engine", bore=76.0)
    a.add("injector", "rocket.injector_plate", at=Mate("chamber", "chamber_fwd", "chamber"),
          group="engine", od=96.0)

`Loc(x, y, z, rx, ry, rz)` is a build123d `Location((x, y, z), (rx, ry, rz))`
(mm, degrees, intrinsic X-Y-Z). `Mate(other, their_joint, my_joint)` puts
this part where `my_joint` coincides with `other`'s `their_joint` (the
other instance must be declared first). `group` is the part tree path
("engine/feed"). `explode` is this part's exploded-view offset in mm; when
absent the worker pushes the part away from the assembly's centre.
`touches` names instances this one is bonded or bolted to without a mate:
they must contact within tolerance; solid overlap is still a clash.
"""
from __future__ import annotations

from dataclasses import dataclass, field


@dataclass(frozen=True)
class Loc:
    x: float = 0.0
    y: float = 0.0
    z: float = 0.0
    rx: float = 0.0
    ry: float = 0.0
    rz: float = 0.0

    def to_json(self):
        return {"loc": {"pos": [self.x, self.y, self.z], "rot": [self.rx, self.ry, self.rz]}}


@dataclass(frozen=True)
class Mate:
    other: str
    their_joint: str
    my_joint: str

    def to_json(self):
        return {"mate": {"to": self.other, "theirs": self.their_joint, "mine": self.my_joint}}


@dataclass
class Assembly:
    name: str
    instances: list = field(default_factory=list)

    def add(self, name: str, part_id: str, *, at: Loc | Mate = Loc(), group: str = "",
            explode: tuple | None = None, material: str | None = None, density: float | None = None,
            color: str | None = None, touches: tuple = (), **params) -> str:
        if any(i["name"] == name for i in self.instances):
            raise ValueError(f"duplicate instance name {name!r}")
        if isinstance(at, Mate) and not any(i["name"] == at.other for i in self.instances):
            raise ValueError(f"{name}: mates to {at.other!r}, which is not declared before it")
        self.instances.append(dict(
            name=name, part=part_id, params=params, placement=at.to_json(), group=group,
            explode=list(explode) if explode is not None else None, material=material,
            density=density, color=color, touches=list(touches)))
        return name

    def to_json(self):
        from .validation import validate_instances
        validate_instances(self.instances)
        return {"name": self.name, "instances": self.instances}
