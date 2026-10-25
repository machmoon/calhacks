"""Independent physical invariants for the native-wing solver adapter."""
import importlib.util
import json
import math
from pathlib import Path
import tempfile
import aerosandbox as asb
import numpy as np

spec=importlib.util.spec_from_file_location('native_analysis',Path(__file__).with_name('native_wing_analysis.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
coords=asb.Airfoil('naca0012').repanel(n_points_per_side=33).coordinates.tolist()
g={'schema_version':1,'symmetric':True,'sections':[dict(xyz_le_m=[0,y,0],chord_m=.2,twist_deg=0,airfoil_coordinates=coords) for y in [0,1]]}
plane,wing=m.airplane(g)
assert abs(float(plane.s_ref)-.4)<1e-12 and abs(float(plane.b_ref)-2)<1e-12

def solve(speed,alpha):
    a=asb.VortexLatticeMethod(airplane=plane,op_point=asb.OperatingPoint(velocity=speed,alpha=alpha),spanwise_resolution=24,chordwise_resolution=8)
    r=a.run();f=np.asarray(a.forces_geometry);p=np.asarray(a.vortex_centers)
    assert abs(f[p[:,1]>0,2].sum()-f[p[:,1]<0,2].sum())<1e-8
    return {k:float(r[k]) for k in ['CL','L','CD']}
zero=solve(20,0);a=solve(20,2);b=solve(40,2);neg=solve(20,-2)
assert abs(zero['CL'])<1e-8
assert abs(a['CL']+neg['CL'])<1e-8
assert abs(b['CL']-a['CL'])<1e-8 and abs(b['L']/a['L']-4)<1e-8
# Classical finite-wing estimate; rectangular wing differs from ideal elliptic loading.
slope=a['CL']/math.radians(2);reference=2*math.pi*10/(10+2)
assert abs(slope/reference-1)<.12,(slope,reference)
assert a['CD']>0
rejected=0
for patch in [{'velocity_m_s':True},{'velocity_m_s':float('nan')},{'alphas_deg':[0,0]},{'altitude_m':-1}]:
    try:m.validate({'geometry':g,**patch})
    except ValueError:rejected+=1
assert rejected==4
out=Path('out/native-wing-analysis/regression.json');out.parent.mkdir(parents=True,exist_ok=True)
out.write_text(json.dumps(dict(passed=True,zero_lift=zero['CL'],lift_slope_per_rad=slope,classical_estimate_per_rad=reference,slope_tolerance_fraction=.12,speed_doubling_force_ratio=b['L']/a['L'],negative_controls=rejected,scope='Symmetry, dimensional scaling and classical finite-wing sanity check; not experimental validation.'),indent=2))
print(out.read_text())
