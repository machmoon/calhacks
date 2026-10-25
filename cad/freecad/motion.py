"""Conservative straight-translation screening; no claim of general path planning."""
import math
import FreeCAD as App
import Part
import native


def check_translation(moving,obstacles,translation,samples=11):
    vector=native.vector(translation)
    if vector.Length<=0:raise ValueError('translation must have nonzero length')
    if type(samples) is not int or not 2<=samples<=101:raise ValueError('samples must be an integer from 2 to 101')
    if {o.Name for o in moving}&{o.Name for o in obstacles}:raise ValueError('moving and obstacle selections overlap')
    candidates=[]
    # A box enclosing both endpoint AABBs encloses every pose under straight
    # translation. No intersection here certifies no positive-volume collision
    # for this selected geometry along the whole path, not just sampled poses.
    for obj in moving:
        b=native.shape_bounds(obj.Shape,conservative=True)
        lows=[getattr(b,a+'Min')+min(0,d) for a,d in zip('XYZ',translation)]
        highs=[getattr(b,a+'Max')+max(0,d) for a,d in zip('XYZ',translation)]
        envelope=Part.makeBox(*(hi-lo for hi,lo in zip(highs,lows)),App.Vector(*lows))
        for obstacle in obstacles:
            volume=envelope.common(obstacle.Shape).Volume
            if not math.isfinite(volume):raise ValueError('nonfinite swept-envelope evidence')
            if volume>1e-6:candidates.append((obj,obstacle))
    collisions=[]
    for step in range(samples):
        fraction=step/(samples-1)
        for obj,obstacle in candidates:
            shape=obj.Shape.copy();shape.translate(vector*fraction)
            overlap=shape.common(obstacle.Shape).Volume
            if not math.isfinite(overlap):raise ValueError('nonfinite motion collision evidence')
            if overlap>1e-6:
                collisions.append(dict(moving=obj.Name,obstacle=obstacle.Name,fraction=fraction,overlap_mm3=overlap))
    status='blocked' if collisions else 'inconclusive' if candidates else 'clear'
    return dict(status=status,continuous_envelope_clear=not candidates,overlap_tolerance_mm3=1e-6,
                moving=[o.Name for o in moving],obstacles=[o.Name for o in obstacles],translation_mm=list(translation),samples=samples,
                candidate_pairs=[[a.Name,b.Name] for a,b in candidates],collisions=collisions,
                scope='Straight rigid translation against selected obstacles only. Clear means conservative swept boxes have no penetration. Inconclusive means sampled poses passed but continuous clearance was not proved. Touching is allowed; no tolerance, cable, flex or tool-access certification.')
