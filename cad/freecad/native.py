"""Typed native FreeCAD features. No generated Python or eval is accepted.

Objects retain native FreeCAD dependencies and recompute after parameter edits.
The typed layer builds primitives, lofts, hollow skins and linked rib profiles.
"""
import math
import re
import hashlib
from collections import OrderedDict
import FreeCAD as App
import profiles


def positive(value, name):
    if isinstance(value, bool) or not isinstance(value, (float, int)) or not math.isfinite(value) or value <= 0:
        raise ValueError(f'{name} must be a finite positive number')
    return value


def vector(value):
    if not isinstance(value, (list, tuple)) or len(value) != 3:
        raise ValueError('expected XYZ vector')
    for v in value:
        if isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v):
            raise ValueError('vector coordinates must be finite numbers')
    return App.Vector(*value)


def name(value):
    if not isinstance(value, str) or not re.fullmatch(r'[A-Za-z][A-Za-z0-9_]{0,95}', value):
        raise ValueError('invalid native object name')
    return value


TUBE_PROXIES={'Rocket.FeatureBodyTube.FeatureBodyTube','Rocket.FeatureInnerTube.FeatureInnerTube','Rocket.FeatureTubeCoupler.FeatureTubeCoupler'}
NATIVE_PARAMETERS={'Part::Box':{'Length','Width','Height'},'Part::Cylinder':{'Radius','Height'},'Part::Extrusion':{'LengthFwd'},'Part::Offset2D':{'Value'}}

def proxy_type(obj):
    proxy=getattr(obj,'Proxy',None)
    return type(proxy).__module__+'.'+type(proxy).__name__ if proxy is not None else None

def raw_editable_parameters(obj):
    import layout
    if layout.is_parameters(obj):return set(layout.limits(obj))
    if proxy_type(obj) in TUBE_PROXIES:
        # Diameter may be automatic, so it is not exposed as a writable override.
        if abs(obj.Proxy.getScale()-1)>=1e-9:return set()
        return {'Length','AxialOffset'} if getattr(obj,'Filled',False) else {'Length','Thickness','AxialOffset'}
    return NATIVE_PARAMETERS.get(obj.TypeId,set())

def editable_parameters(obj):
    driven={p.split('.')[0] for p,_ in getattr(obj,'ExpressionEngine',[])}
    return raw_editable_parameters(obj)-driven

def geometric_sources(obj):
    properties={'Part::Cut':['Base','Tool'],'Part::Fuse':['Base','Tool'],'Part::Common':['Base','Tool'],'Part::MultiFuse':['Shapes'],'Part::Extrusion':['Base'],'Part::Loft':['Sections'],'Part::Offset2D':['Source']}
    sources=[]
    if obj.TypeId=='PartDesign::ShapeBinder':
        sources.extend(link[0] for link in obj.Support)
    for prop in properties.get(obj.TypeId,[]):
        value=getattr(obj,prop)
        sources.extend(value if isinstance(value,(list,tuple)) else [value])
    return [v for v in sources if v is not None]

def parameter_bindings(obj):
    pending=[obj];seen=set();bindings=[]
    while pending:
        source=pending.pop()
        if source.Name in seen:continue
        seen.add(source.Name)
        for parameter in sorted(editable_parameters(source)):
            bindings.append(dict(label=parameter if source==obj else source.Name+'.'+parameter,
                                 object_name=source.Name,parameter=parameter,
                                 value=float(getattr(source,parameter)),unit='mm'))
        pending.extend(reversed(geometric_sources(source)))
        # A mirror is another physical instance, not a consumed construction
        # operand. Its shared edit controls still come from its source geometry.
        if source.TypeId=='Part::Mirroring' and source.Source is not None:
            pending.append(source.Source)
        import layout
        pending.extend(o for o in source.OutList if layout.is_parameters(o))
    return bindings


def validate_live_feature(obj):
    if obj.TypeId.endswith('Python') and hasattr(obj,'Shape'):
        if not callable(getattr(getattr(obj,'Proxy',None),'execute',None)):
            raise ValueError(f'{obj.Name}: Python feature has no live recompute handler; required workbench may be missing')
    if proxy_type(obj) in TUBE_PROXIES and abs(obj.Proxy.getScale()-1)<1e-9:
        length=positive(obj.Length.Value,'Length');diameter=positive(obj.Diameter.Value,'Diameter')
        wall=diameter/2 if getattr(obj,'Filled',False) else positive(obj.Thickness.Value,'Thickness')
        if wall>diameter/2:raise ValueError(f'{obj.Name}: tube wall exceeds radius')
        if not obj.Shape.isNull():
            expected=math.pi*((diameter/2)**2-(diameter/2-wall)**2)*length*len(obj.Shape.Solids)
            if abs(obj.Shape.Volume-expected)>max(1e-5,expected*1e-7):
                raise ValueError(f'{obj.Name}: tube geometry disagrees with its live parameters')


def shape_bounds(shape, conservative=False):
    # BoundBox can retain an untrimmed supporting surface after Common/Cut.
    # Avoid cached triangulation; use native trimmed geometry. Collision broad
    # phases additionally include the kernel's shape tolerances.
    if shape.isNull():return shape.optimalBoundingBox(False, conservative)
    key=(hashlib.sha256(shape.cleaned().exportBrepToString().encode()).digest(),bool(conservative))
    if key in _bounds_cache:
        _bounds_cache.move_to_end(key)
        values=_bounds_cache[key]
    else:
        box=shape.optimalBoundingBox(False, conservative)
        values=(box.XMin,box.YMin,box.ZMin,box.XMax,box.YMax,box.ZMax)
        if not all(math.isfinite(v) for v in values):return box
        _bounds_cache[key]=values
        if len(_bounds_cache)>_BOUNDS_CACHE_LIMIT:_bounds_cache.popitem(last=False)
    # Return a fresh mutable box: callers cannot corrupt the cached tuple.
    return App.BoundBox(*values)


_bounds_cache=OrderedDict()
_BOUNDS_CACHE_LIMIT=4096


_validity_cache=OrderedDict()
# Complex aircraft documents include hundreds of construction shapes. Keep a
# full working set of hashes without retaining geometry or BREP strings.
_VALIDITY_CACHE_LIMIT=4096


def validate_aggregate_leaves(shape):
    """Validate actual compound once, then cache exact checked leaf geometry.

    OCCT 7.8.1 BRepCheck_Analyzer::IsValid recursively checks subshapes.
    Leaves are derived here, never accepted from callers as alleged children.
    Placement is included in each serialized key; no rigid-transform shortcut.
    """
    if shape.isNull() or not shape.isValid():
        raise ValueError('aggregate contains invalid native geometry')
    leaves=[]
    def walk(current):
        if current.ShapeType in ('Compound','CompSolid'):
            for child in current.childShapes(True,True):walk(child)
        else:leaves.append(current)
    walk(shape)
    if len(leaves)>4096:raise ValueError('aggregate exceeds 4096 leaf validation bound')
    # Compute all keys before changing the cache. No metadata is proof of validity.
    keys=[hashlib.sha256(leaf.cleaned().exportBrepToString().encode()).digest() for leaf in leaves]
    for key in keys:
        _validity_cache[key]=True
        _validity_cache.move_to_end(key)
        if len(_validity_cache)>_VALIDITY_CACHE_LIMIT:_validity_cache.popitem(last=False)
    return leaves


def shape_is_valid(shape):
    """Memoize only kernel validity of exactly serialized native geometry.

    No object identity, action counter or saved metadata is trusted. Topology,
    geometry, tolerances and placements are in the BREP; mesh caches are removed.
    Live feature state, expressions and assembly constraints remain separate.
    """
    if shape.isNull():return False
    key=hashlib.sha256(shape.cleaned().exportBrepToString().encode()).digest()
    if key in _validity_cache:
        _validity_cache.move_to_end(key)
        return _validity_cache[key]
    valid=bool(shape.isValid())
    _validity_cache[key]=valid
    if len(_validity_cache)>_VALIDITY_CACHE_LIMIT:_validity_cache.popitem(last=False)
    return valid


def describe(obj):
    import layout
    if layout.is_parameters(obj):return layout.describe(obj)
    if obj.TypeId in ('App::Line','App::Plane','App::Point'):
        return dict(name=obj.Name,type=obj.TypeId,volume_mm3=None,solids=0,
                    valid='Invalid' not in obj.State,datum=True,reference_geometry=False,
                    assembly_container=False,profile=False,bbox_mm=None,
                    dependencies=[o.Name for o in obj.OutList],editable_parameters={})
    validate_live_feature(obj)
    if 'Invalid' in obj.State:raise ValueError(f'{obj.Name}: native feature recompute failed')
    from assembly_frames import world_shape
    shape = world_shape(obj)
    import import_geometry
    reference=import_geometry.is_reference(obj)
    if reference:import_geometry.validate_reference(obj)
    profile = obj.TypeId in ('Sketcher::SketchObject','Part::Offset2D') and getattr(obj,'WrightProfile',False)
    if not shape_is_valid(shape) or (not profile and not reference and (not shape.Solids or shape.Volume <= 0)):
        raise ValueError(f'{obj.Name}: invalid or empty geometry')
    if profile and (not shape.isClosed() or (obj.TypeId=='Sketcher::SketchObject' and obj.FullyConstrained is not True)):
        raise ValueError(f'{obj.Name}: profile must be closed and constrained')
    b = shape_bounds(shape)
    return dict(name=obj.Name, type=obj.TypeId, volume_mm3=None if reference else shape.Volume,
                reference_geometry=reference,surface_area_mm2=shape.Area if reference else None,
                loft_interpolation=dict(ruled=bool(obj.Ruled),solid=bool(obj.Solid),section_count=len(obj.Sections)) if obj.TypeId=='Part::Loft' else None,
                solids=len(shape.Solids), valid=True,
                bbox_mm=[[b.XMin,b.YMin,b.ZMin],[b.XMax,b.YMax,b.ZMax]],
                dependencies=[o.Name for o in obj.OutList],
                profile=bool(profile),proxy_type=proxy_type(obj),
                assembly_container=proxy_type(obj) in {'Rocket.FeatureRocket.FeatureRocket','Rocket.FeatureStage.FeatureStage'},
                editable_parameters={p:float(getattr(obj,p)) for p in sorted(editable_parameters(obj))})


def make_part(doc, spec):
    """Build atomically; failed requests remove all their new native objects."""
    allowed = {'box': {'x','y','z'}, 'cylinder': {'radius','height'},
               'mount': {'x','y','z','bores'}, 'tube': {'radius','bore_radius','height'},
               'loft': {'rings'}, 'loft_shell': {'rings','inner'},
               'spline_loft': {'rings','split_index','sampled_tolerance_mm'},
               'extruded_skin': {'profile','direction','wall'},
               'profile_plate': {'profile_object','direction','thickness','bores'}}
    kind = spec['kind']
    if kind not in allowed:
        raise ValueError(f'unsupported native part kind {kind}')
    part_name = name(spec['name'])
    params = spec['params']
    optional={'ruled'} if kind=='spline_loft' else set()
    if not isinstance(params, dict) or not allowed[kind]<=set(params) or set(params)-allowed[kind]-optional:
        suffix='; optional ruled:boolean' if optional else ''
        raise ValueError(f'{kind} requires {sorted(allowed[kind])}{suffix}')
    if doc.getObject(part_name):
        raise ValueError('object already exists')
    reserved=[part_name]
    if kind=='tube': reserved += [part_name+'_Outer',part_name+'_Bore']
    if kind=='loft_shell': reserved += [part_name+'_Outer',part_name+'_Inner']
    if kind=='extruded_skin':reserved += [part_name+s for s in ['_Profile','_InnerProfile','_Outer','_Inner']]
    if kind in ('mount','profile_plate'):
        bores=params['bores']
        if not isinstance(bores,list) or not bores: raise ValueError('mount/plate requires explicit bores')
        reserved += [part_name+'_Blank']+[part_name+f'_Seat{i}' for i in range(len(bores))]+[part_name+f'_Cut{i}' for i in range(len(bores)-1)]
    if any(doc.getObject(n) for n in reserved):
        raise ValueError('native feature name already exists')
    doc.openTransaction('Create '+part_name)
    try:
        if kind=='profile_plate':
            base=doc.getObject(name(params['profile_object']))
            if base is None or not describe(base)['profile']:raise ValueError('plate needs a native profile')
            axis=vector(params['direction'])
            if axis.Length<=1e-8:raise ValueError('plate direction cannot be zero')
            obj=doc.addObject('Part::Extrusion',part_name+'_Blank')
            obj.Base=base;obj.DirMode='Custom';obj.Dir=axis
            obj.LengthFwd=positive(params['thickness'],'thickness');obj.Solid=True
            doc.recompute();describe(obj)
            for i,bore in enumerate(params['bores']):
                start,end,radius=bore;start,end=vector(start),vector(end);delta=end-start
                tool=doc.addObject('Part::Cylinder',part_name+f'_Seat{i}')
                tool.Radius=positive(radius,'radius');tool.Height=positive(delta.Length,'bore length')
                tool.Placement=App.Placement(start,App.Rotation(App.Vector(0,0,1),delta))
                cut=doc.addObject('Part::Cut',part_name if i==len(params['bores'])-1 else part_name+f'_Cut{i}')
                cut.Base,cut.Tool=obj,tool;doc.recompute();describe(cut)
                if len(cut.Shape.Solids)!=1 or obj.Shape.Volume-cut.Shape.Volume<1e-6:
                    raise ValueError('plate bore must remove material and leave one solid')
                obj=cut
        elif kind=='extruded_skin':
            obj=profiles.extruded_skin(doc,part_name,params['profile'],params['direction'],positive(params['wall'],'wall'),vector)
        elif kind=='spline_loft':
            obj=profiles.spline_loft(doc,part_name,params['rings'],params['split_index'],vector,
                                    sampled_tolerance_mm=params['sampled_tolerance_mm'],ruled=params.get('ruled',True))
            describe(obj)
        elif kind in ('loft','loft_shell'):
            obj=profiles.loft(doc,part_name if kind=='loft' else part_name+'_Outer',params['rings'],vector)
            describe(obj)
            if kind=='loft_shell':
                inner=profiles.loft(doc,part_name+'_Inner',params['inner'],vector)
                describe(inner)
                cut=doc.addObject('Part::Cut',part_name)
                cut.Base,cut.Tool=obj,inner
                doc.recompute()
                if obj.Shape.Volume-cut.Shape.Volume<=1e-6:raise ValueError('inner loft does not remove shell material')
                obj=cut
        elif kind in ('cylinder','tube'):
            obj = doc.addObject('Part::Cylinder', part_name if kind=='cylinder' else part_name+'_Outer')
            obj.Radius = positive(params['radius'], 'radius')
            obj.Height = positive(params['height'], 'height')
            if kind == 'tube':
                radius = positive(params['bore_radius'],'bore_radius')
                if radius >= params['radius']: raise ValueError('bore must be smaller than outer radius')
                tool = doc.addObject('Part::Cylinder',part_name+'_Bore')
                tool.Radius, tool.Height = radius, params['height']
                tool.setExpression('Height',obj.Name+'.Height')
                cut = doc.addObject('Part::Cut',part_name)
                cut.Base, cut.Tool = obj, tool
                obj = cut
        else:
            obj = doc.addObject('Part::Box', part_name if kind == 'box' else part_name+'_Blank')
            for prop, key in [('Length','x'),('Width','y'),('Height','z')]:
                setattr(obj, prop, positive(params[key], key))
            doc.recompute()
            if kind == 'mount':
                if not isinstance(params['bores'], list) or not params['bores']:
                    raise ValueError('mount requires explicit bores')
                for i, bore in enumerate(params['bores']):
                    start, end, radius = bore
                    start, end = vector(start), vector(end)
                    delta = end-start
                    positive(delta.Length,'seat length')
                    tool = doc.addObject('Part::Cylinder',part_name+f'_Seat{i}')
                    tool.Radius = positive(radius, 'radius')
                    tool.Height = delta.Length
                    tool.Placement = App.Placement(start,App.Rotation(App.Vector(0,0,1),delta))
                    cut = doc.addObject('Part::Cut',part_name if i==len(params['bores'])-1 else part_name+f'_Cut{i}')
                    cut.Base, cut.Tool = obj, tool
                    doc.recompute()
                    result = describe(cut)
                    if result['solids'] != 1 or obj.Shape.Volume-cut.Shape.Volume < 1e-6:
                        raise ValueError('seat must remove material and leave one solid')
                    obj = cut
        doc.recompute()
        result = describe(obj)
        if result['solids'] != 1:raise ValueError('a physical part must be one connected solid')
        doc.commitTransaction()
        return result
    except Exception:
        doc.abortTransaction()
        doc.recompute()
        raise


def validate_document(doc):
    import layout
    import booleans
    import mirrors
    import cut_proofs
    results=[describe(o) for o in doc.Objects if hasattr(o,'Shape') or layout.is_parameters(o)]
    for obj in doc.Objects:
        deferred = obj.TypeId=='Part::Cut' and cut_proofs.should_defer(obj)
        booleans.validate(obj, defer_material=deferred)
        mirrors.validate(obj)
        if obj.TypeId=='Part::Cut':
            if len(obj.Shape.Solids)!=1:
                raise ValueError('cut requires a single-solid result')
            if not deferred:
                booleans.validate_cut_feature(obj)
                cut_proofs.record_verified(obj)
    return results


def set_parameter(doc, object_name, parameter, value):
    obj = doc.getObject(name(object_name))
    if obj is None:
        raise ValueError('unknown object')
    if parameter not in editable_parameters(obj):
        raise ValueError('parameter is not editable through the typed API')
    if proxy_type(obj) in TUBE_PROXIES and parameter=='AxialOffset':
        if isinstance(value,bool) or not isinstance(value,(int,float)) or not math.isfinite(value):raise ValueError('AxialOffset must be a finite number')
    else:positive(-value if obj.TypeId=='Part::Offset2D' else value,parameter)
    doc.openTransaction('Set '+object_name+'.'+parameter)
    try:
        if proxy_type(obj) in TUBE_PROXIES:
            if parameter=='Thickness' and value>obj.Diameter.Value/2:raise ValueError('tube wall exceeds radius')
            # Property assignment alone rebuilds the shape but does not notify
            # the Rocket assembly to update dependent axial positions.
            getattr(obj.Proxy,'set'+parameter)(value)
            if abs(float(getattr(obj,parameter))-value)>1e-9:raise ValueError('native component changed the requested parameter value')
        else:setattr(obj,parameter,value)
        doc.recompute()
        results=validate_document(doc)
        doc.commitTransaction()
        return results
    except Exception:
        doc.abortTransaction()
        doc.recompute()
        raise


def set_profile(doc, object_name, points):
    sketch=doc.getObject(name(object_name))
    if sketch is None or sketch.TypeId!='Sketcher::SketchObject' or not getattr(sketch,'WrightProfile',False):raise ValueError('not a native Wright profile')
    if getattr(sketch,'WrightSplineProfile',False):
        # Spline sections keep two native curves, not one edge per input point.
        # assign_spline validates the bounded source data inside the transaction.
        geometry_count=2
    else:
        geometry,_=profiles.prepare(points,vector)
        geometry_count=len(geometry)
    for parent in sketch.InList:
        if parent.TypeId=='Part::Loft' and any(s!=sketch and s.GeometryCount!=geometry_count for s in parent.Sections):
            raise ValueError('profile edit must preserve corresponding loft vertices')
    doc.openTransaction('Edit '+object_name)
    try:
        profiles.assign(sketch,points,vector)
        doc.recompute()
        results=validate_document(doc)
        doc.commitTransaction()
        return results
    except Exception:
        doc.abortTransaction();doc.recompute();raise
