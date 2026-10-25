"""Build an airfoil rib/spar CAD fixture using the existing aircraft demo's section.
This tests assembly geometry; it is not the complete wing or a structural check.
"""
import json
from pathlib import Path
from wright_cad import Assembly, Loc
from wright_cad.build import build_plan

ROOT=Path(__file__).resolve().parents[2]

def main():
    design=json.loads((ROOT/'demo/aircraft/design.json').read_text())
    chord=design['wing']['root_chord_mm']
    foil=json.loads((ROOT/'data/air/airfoils.json').read_text())['airfoils'][design['wing']['airfoil']]
    coordinates=[list(p) for p in foil['coordinates']]
    # The source rounds the two nominal trailing-edge endpoints separately;
    # SD7062's 0.0042 mm gap makes the closing segments cross. Explicitly
    # merge these nearly coincident endpoints, and record the displacement.
    import math
    endpoint_gap=math.dist(coordinates[0],coordinates[-1])*chord
    if endpoint_gap > .01:
        raise ValueError('trailing edge is open; this fixture only supports a closed section')
    midpoint=[(a+b)/2 for a,b in zip(coordinates[0],coordinates[-1])]
    coordinates[0]=midpoint
    coordinates[-1]=midpoint
    le=min(range(len(coordinates)),key=lambda i:coordinates[i][0])
    def ordinate(branch,x):
        branch=sorted(branch)
        for (a,ya),(b,yb) in zip(branch,branch[1:]):
            if a <= x <= b and b>a:
                return ya+(yb-ya)*(x-a)/(b-a)
        raise ValueError('bore station outside section')
    bores=[]
    for fraction,radius in ((.28,6.35),(.65,2.)):
        mid=(ordinate(coordinates[:le+1],fraction)+ordinate(coordinates[le:],fraction))/2
        bores.append([fraction*chord,mid*chord,radius])
    assembly=Assembly('wing_structure_fixture')
    for index,(x,z,r) in enumerate(bores):
        assembly.add(f'spar_{index}','common.axial_tube',start=[x,-2,z],end=[x,600,z],
                     radius=r,bore_radius=max(0,r-1),group='spars')
    for y in (0,150,300,450,600):
        assembly.add(f'rib_{y}','aircraft.rib',at=Loc(0,y,0,90,0,0),
                     profile=coordinates,chord=chord,thickness=2,bores=bores,
                     touches=('spar_0','spar_1'),group='ribs')
    plan=assembly.to_json()
    plan['scope']='Geometry fixture only: no skin, control surfaces, root attachment or structural verification'
    plan['source_airfoil']=dict(id=design['wing']['airfoil'],source=foil['source'],
                               normalization='merged nominal trailing-edge endpoints',
                               maximum_endpoint_displacement_mm=endpoint_gap/2)
    out=ROOT/'out/cad-wing-fixture'
    report=build_plan(plan,out)
    print(json.dumps({k:report[k] for k in ('passed','n_parts','cache','clashes','floating','total_ms')},indent=2))
    if not report['passed']:
        raise SystemExit(1)

if __name__=='__main__':main()
