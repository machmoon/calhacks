"""Expose required bending strength of the actual provisional spar.

Assumes one circular tube carries all half-wing bending as a cantilever, with
elliptical lift. Does not assume an allowable for unknown CFRP or credit skins.
Run with cad/freecad/run.sh; no FEA/structural approval claim.
"""
import hashlib,json,math,sys
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'cad/freecad'))
import native
source=ROOT/'out/tsung-vtol-wing-structure/unverified-build.FCStd'
before=hashlib.sha256(source.read_bytes()).hexdigest()
doc=App.openDocument(str(source));spar=doc.RightSpar
radius=spar.Base.Radius.Value;bore=spar.Tool.Radius.Value
assert spar.Base.TypeId=='Part::Cylinder' and spar.Tool.TypeId=='Part::Cylinder'
length=native.shape_bounds(spar.Shape).YLength
assert abs(spar.Shape.Volume-math.pi*(radius**2-bore**2)*length)<1e-6
second_moment=math.pi*(radius**4-bore**4)/4
# For an axial Y tube, its polar second moment is the Y mass inertia divided
# by length at FreeCAD's unit density; I_x=I_z=J_y/2.
assert len(spar.Shape.Solids)==1
native_I=spar.Shape.Solids[0].MatrixOfInertia.A22/(2*length)
assert abs(native_I-second_moment)<1e-6
half_span=doc.WingControls.HalfSpan.Value
audit=ROOT/'cad/projects/tsung_vtol/reference-audit.json'
mass=json.loads(audit.read_text())['published_constraints']['reported_weight_kg'];gravity=9.80665
# Integral y*sqrt(1-(y/b)^2) / integral sqrt(1-(y/b)^2) = 4b/(3pi).
lift_centroid=4*half_span/(3*math.pi)
# Independent midpoint quadrature catches an incorrect closed-form centroid.
weights=[math.sqrt(1-((i+.5)/10000)**2) for i in range(10000)]
numerical_centroid=sum(w*(i+.5)*half_span/10000 for i,w in enumerate(weights))/sum(weights)
assert abs(lift_centroid-numerical_centroid)<.001
cases=[]
for load_factor in [1,2,3]:
    lift=load_factor*mass*gravity/2
    moment=lift*lift_centroid
    cases.append(dict(assumed_load_factor=load_factor,half_wing_lift_N=lift,
        root_bending_moment_Nmm=moment,
        required_extreme_fiber_stress_MPa=moment*radius/second_moment))
out=ROOT/'out/tsung-vtol-spar-load-screen';out.mkdir(parents=True,exist_ok=True)
report=dict(status='Required stress calculated; strength acceptance undetermined without load-sharing and material evidence',
    source=str(source),source_sha256=before,outer_diameter_mm=2*radius,inner_diameter_mm=2*bore,
    section_second_moment_mm4=second_moment,native_inertia_crosscheck_mm4=native_I,
    aircraft_mass_kg=mass,mass_basis=str(audit),mass_source_sha256=hashlib.sha256(audit.read_bytes()).hexdigest(),
    assumed_lift_distribution='Elliptical, symmetric, all vertical lift carried by two half-wings',
    half_span_mm=half_span,lift_centroid_from_root_mm=lift_centroid,
    numerical_centroid_crosscheck_mm=numerical_centroid,cases=cases,
    allowable_stress_MPa=None,structural_pass=None,
    limitations=['All bending assigned to one tube; skin load sharing is unknown and not credited.',
        'No wing self-weight relief, local hardpoint loads, torsion, buckling, joints or dynamic load analysis.',
        '1/2/3 load factors are screening scenarios, not a specified flight envelope or design limit.',
        'CFRP layup, directional strength, knockdowns and safety factors are unknown.',
        'Tube ends at span500mm; assumed cantilever root restraint and outboard load transfer are not yet designed.'])
App.closeDocument(doc.Name)
assert hashlib.sha256(source.read_bytes()).hexdigest()==before
(out/'result.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))
