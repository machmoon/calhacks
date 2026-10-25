"""Extract current native spline sections for a reduced aerodynamic wing model."""
import hashlib
import math
import FreeCAD as App


def extract(doc, object_name, samples_per_branch=65, symmetric=True):
    if type(samples_per_branch) is not int or not 33 <= samples_per_branch <= 129:
        raise ValueError('samples_per_branch must be an integer in33..129')
    if not isinstance(symmetric, bool):
        raise ValueError('symmetric must be boolean')
    obj = doc.getObject(object_name) if isinstance(object_name,str) else None
    if obj is None or obj.TypeId != 'Part::Loft' or not obj.Solid:
        raise ValueError('select a native solid spline loft, not its skin Boolean')
    if not 2 <= len(obj.Sections) <= 16 or not obj.Shape.isValid():
        raise ValueError('requires a valid loft with2..16 source sections')
    if any(abs(a-b)>1e-9 for a,b in zip(obj.getGlobalPlacement().toMatrix().A,obj.Placement.toMatrix().A)):
        raise ValueError('nested assembly coordinate frames are not yet supported')
    rows=[]
    for section in obj.Sections:
        if not getattr(section,'WrightSplineProfile',False) or len(section.Shape.Edges)!=2:
            raise ValueError('each section must have two tagged native spline branches')
        edges=section.Shape.Edges
        curves=[[obj.Placement.multVec(p) for p in edge.discretize(Number=samples_per_branch)] for edge in edges]
        first=curves[0]
        le,te=sorted([first[0],first[-1]],key=lambda p:p.x)
        if max(min((p-le).Length,(p-te).Length) for p in [curves[1][0],curves[1][-1]])>1e-6:
            raise ValueError('branches do not share leading/trailing endpoints')
        chord_vector=te-le;chord=chord_vector.Length
        if chord_vector.x<=0 or not .1<=chord<=10000 or abs(chord_vector.y)>1e-6:
            raise ValueError('requires chordwise+X, spanwiseY sections with nonzero chord')
        points=curves[0]+curves[1]
        if max(abs(p.y-le.y) for p in points)>1e-6:
            raise ValueError('requires sections parallel to the XZ plane')
        xaxis=chord_vector/chord;up=App.Vector(-xaxis.z,0,xaxis.x)
        # Explicitly order upper TE->LE then lower LE->TE for both solvers.
        for curve in curves:
            if (curve[0]-te).Length>(curve[-1]-te).Length:curve.reverse()
        curves.sort(key=lambda curve:sum((p-le).dot(up) for p in curve)/len(curve),reverse=True)
        boundary=curves[0]+list(reversed(curves[1]))[1:]
        coordinates=[[(p-le).dot(xaxis)/chord,(p-le).dot(up)/chord] for p in boundary]
        if max(abs(p[1]) for p in coordinates)>.5:
            raise ValueError('section is not a bounded airfoil-like profile')
        rows.append(dict(native_section=section.Name,xyz_le_m=[v/1000 for v in (le.x,le.y,le.z)],
            chord_m=chord/1000,twist_deg=-math.degrees(math.atan2(chord_vector.z,chord_vector.x)),
            airfoil_coordinates=coordinates,world_samples_m=[[v/1000 for v in (p.x,p.y,p.z)] for p in boundary]))
    ys=[s['xyz_le_m'][1] for s in rows]
    if any(b-a<=1e-6 for a,b in zip(ys,ys[1:])):
        raise ValueError('sections must be ordered at strictly increasing spanwiseY')
    if symmetric and (abs(ys[0])>1e-6 or ys[-1]<=0):
        raise ValueError('symmetric model requires a right half-wing rooted atY=0')
    return dict(schema_version=1,object_name=obj.Name,sections=rows,symmetric=symmetric,
        native_loft_ruled=bool(obj.Ruled),source_geometry_sha256=hashlib.sha256(obj.Shape.cleaned().exportBrepToString().encode()).hexdigest(),
        axes='X aft, Y right, Z up; metres/degrees',
        scope='Reduced wing model from sampled current native source-section curves. '
              'Solvers interpolate linearly between sections; a smooth CAD loft between them is approximated. '
              'Symmetry is an explicit modeling assumption, not verification of the opposite CAD wing. '
              'No fuselage, tail, rotor wash, control deflections, structural or flight validation.')
