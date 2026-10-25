"""Read-only obstruction checks for imported reference surfaces, not mass solids."""
import math
import Part


def obstruction(reference,gauge,tolerance_mm=1e-6):
    if reference.ShapeType not in ('Shell','Face') or not reference.isValid():
        raise ValueError('reference must be a valid shell or face')
    if len(gauge.Solids)!=1 or not gauge.isValid():raise ValueError('gauge must be one valid solid')
    if isinstance(tolerance_mm,bool) or not isinstance(tolerance_mm,(int,float)) or not math.isfinite(tolerance_mm) or not 0<=tolerance_mm<=1:
        raise ValueError('invalid reference clearance tolerance')
    distance=reference.distToShape(gauge)[0]
    if not math.isfinite(distance):raise ValueError('nonfinite reference distance')
    if distance<=tolerance_mm:
        return dict(obstructed=True,reason='reference_surface_contact',surface_distance_mm=distance)
    # Distance to a closed shell is positive for a gauge wholly inside it.
    # Use a temporary solid only to test occupied space; never assign it mass,
    # export it, or change the reference object's physical classification.
    if reference.ShapeType=='Shell' and reference.isClosed():
        envelope=Part.makeSolid(reference)
        if len(envelope.Solids)!=1 or not envelope.isValid() or envelope.Volume<=0:
            raise ValueError('closed reference cannot form a valid temporary enclosure')
        common=envelope.common(gauge)
        if not common.isNull() and not common.isValid():raise ValueError('invalid reference enclosure intersection')
        if not math.isfinite(common.Volume):raise ValueError('nonfinite reference overlap')
        if common.Volume>1e-6:return dict(obstructed=True,reason='inside_closed_reference_enclosure',surface_distance_mm=distance,enclosed_gauge_volume_mm3=common.Volume)
    return dict(obstructed=False,reason='no_surface_or_closed_enclosure_intersection',surface_distance_mm=distance)
