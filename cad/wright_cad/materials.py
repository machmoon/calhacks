"""Bulk material properties for mass, CG and inertia from the B-rep, plus the
visual colour and the SimReady non-visual material class used by the USD
export. An assembly may override density per instance (a catalog row's own
value wins). Every row names its source; `confidence` "assumed" marks a
typical value with no single datasheet behind it.

`nonvisual` is one of NVIDIA SimReady's `omni:simready:nonvisual:base`
tokens (simready-foundation tier_core
capabilities/nonvisual_sensors/nonvisual_materials/validation.py,
`allowed_values`).
"""

MATERIALS = {
    "al6061_t6": dict(density=2700.0, color="#b8bcc2", nonvisual="aluminum", friction=(1.05, 1.4), restitution=0.3,
                      source="ASM Aerospace Specification Metals, Aluminum 6061-T6 datasheet: 2.7 g/cc", confidence="datasheet"),
    "steel_304": dict(density=8000.0, color="#8e9399", nonvisual="steel", friction=(0.6, 0.8), restitution=0.3,
                      source="ASM Aerospace Specification Metals, AISI 304: 8.00 g/cc", confidence="datasheet"),
    "brass_360": dict(density=8500.0, color="#c9a44c", nonvisual="brass", friction=(0.35, 0.5), restitution=0.3,
                      source="Copper Development Assn. C36000 free-cutting brass: 0.307 lb/in3 = 8.50 g/cc", confidence="datasheet"),
    "g10_fr4": dict(density=1850.0, color="#c8d29a", nonvisual="fiberglass", friction=(0.4, 0.5), restitution=0.4,
                    source="NEMA LI-1 G-10 / FR-4 typical 1.80-1.90 g/cc; midpoint", confidence="assumed"),
    "fiberglass_g12": dict(density=1900.0, color="#d9d3b0", nonvisual="fiberglass", friction=(0.4, 0.5), restitution=0.4,
                           source="filament-wound G12 airframe tube, typical 1.85-1.95 g/cc", confidence="assumed"),
    "silica_phenolic": dict(density=1700.0, color="#6b4f3a", nonvisual="plastic", friction=(0.5, 0.6), restitution=0.3,
                            source="silica/glass-phenolic ablative liner, typical 1.6-1.8 g/cc", confidence="assumed"),
    "carbon_fiber": dict(density=1550.0, color="#2b2d30", nonvisual="carbon_fiber", friction=(0.4, 0.5), restitution=0.4,
                         source="pultruded T700/epoxy at ~60 % fibre volume: 1.55 g/cc (rule of mixtures 1.80/1.20)", confidence="assumed"),
    "pla": dict(density=1240.0, color="#e8e4da", nonvisual="plastic", friction=(0.5, 0.6), restitution=0.4,
                source="NatureWorks Ingeo 4043D technical data sheet: 1.24 g/cc (solid; slicer grams govern printed mass)", confidence="datasheet"),
    "pla_aero": dict(density=1240.0, color="#f2f0ea", nonvisual="plastic", friction=(0.5, 0.6), restitution=0.4,
                     source="as solid PLA; foamed LW-PLA prints lighter -- the slicer's grams govern printed mass", confidence="assumed"),
    "petg": dict(density=1270.0, color="#5c7fa3", nonvisual="plastic", friction=(0.5, 0.6), restitution=0.4,
                 source="Eastman Eastar 6763 copolyester: 1.27 g/cc (solid)", confidence="datasheet"),
    "xps_foam": dict(density=24.0, color="#7fb4e0", nonvisual="plastic", friction=(0.7, 0.8), restitution=0.2,
                     source="data/balloon/catalog.json insulation row: 24 kg/m3 (its own ASSUMPTION, XPS 1.3-1.6 lb/ft3)", confidence="assumed"),
    "pcb_fr4": dict(density=1850.0, color="#2f6b3a", nonvisual="fiberglass", friction=(0.4, 0.5), restitution=0.3,
                    source="as G-10/FR-4 (bare board; components counted by the domain's BOM mass)", confidence="assumed"),
}


def material(key: str) -> dict:
    if key not in MATERIALS:
        raise KeyError(f"unknown material {key!r}; known: {sorted(MATERIALS)}")
    return MATERIALS[key]
