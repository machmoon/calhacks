"""Typed shared dimensions and linear native-expression bindings; no source code."""
import json, math
import native


def is_parameters(obj):
    return obj.TypeId=='App::FeaturePython' and getattr(obj,'WrightParameterSet',False) is True


def limits(obj):
    return json.loads(obj.WrightParameterLimits) if is_parameters(obj) else {}


def validate(obj):
    for key,bounds in limits(obj).items():
        value=float(getattr(obj,key))
        if not math.isfinite(value) or not bounds['min']<=value<=bounds['max']:
            raise ValueError('shared dimension outside declared bounds: '+obj.Name+'.'+key)


def describe(obj):
    validate(obj)
    return dict(name=obj.Name,type=obj.TypeId,parameter_set=True,solids=0,
                assembly_container=False,valid=True,units='mm',limits=limits(obj),
                editable_parameters={k:float(getattr(obj,k)) for k in limits(obj)})


def create(doc,name,parameters):
    name=native.name(name)
    if doc.getObject(name) is not None:raise ValueError('object already exists')
    if not isinstance(parameters,dict) or not parameters:raise ValueError('parameters must be a nonempty object')
    for key,spec in parameters.items():
        native.name(key)
        if not isinstance(spec,dict) or set(spec)!={'value','min','max'}:raise ValueError('dimension requires value, min and max')
        for value in spec.values():
            if isinstance(value,bool) or not isinstance(value,(int,float)) or not math.isfinite(value):raise ValueError('dimensions must be finite numbers')
        if not 0<spec['min']<=spec['value']<=spec['max']:raise ValueError('expected 0 < min <= value <= max')
    doc.openTransaction('Create shared dimensions')
    try:
        obj=doc.addObject('App::FeaturePython',name)
        obj.addProperty('App::PropertyBool','WrightParameterSet','Wright');obj.WrightParameterSet=True
        obj.addProperty('App::PropertyString','WrightParameterLimits','Wright')
        for key,spec in parameters.items():
            if key in obj.PropertiesList:raise ValueError('dimension name conflicts with native property')
            obj.addProperty('App::PropertyLength',key,'Layout');setattr(obj,key,spec['value'])
        obj.WrightParameterLimits=json.dumps({key:{k:v for k,v in spec.items() if k!='value'} for key,spec in parameters.items()})
        doc.recompute();result=describe(obj);doc.commitTransaction();return result
    except Exception:
        doc.abortTransaction();doc.recompute();raise


def _binding(doc,object_name,parameter,source_object,source_parameter,scale=1,offset_mm=0):
    obj=doc.getObject(native.name(object_name));source=doc.getObject(native.name(source_object))
    if obj is None or not hasattr(obj,'Shape') or source is None or not is_parameters(source):raise ValueError('binding requires a physical target and shared dimension source')
    if source_parameter not in limits(source):raise ValueError('unknown source dimension')
    if (native.proxy_type(obj) or '').startswith('Rocket.'):raise ValueError('Rocket assembly parameters require native setters')
    placement_target=parameter in {'Placement.Base.x','Placement.Base.y','Placement.Base.z'}
    # Base is a plane-origin vector only on Part::Mirroring. Other features
    # use Base for linked construction objects, so never allow generic Base.*.
    mirror_plane_target=obj.TypeId=='Part::Mirroring' and parameter in {'Base.x','Base.y','Base.z'}
    if parameter not in native.raw_editable_parameters(obj) and not placement_target and not mirror_plane_target:
        raise ValueError('unsupported binding target')
    for value in (scale,offset_mm):
        if isinstance(value,bool) or not isinstance(value,(int,float)) or not math.isfinite(value):raise ValueError('binding coefficients must be finite')
    if scale==0:raise ValueError('binding scale cannot be zero')
    expression=f'{source.Name}.{source_parameter} * {scale!r} + ({offset_mm!r} mm)'
    return obj,parameter,expression


def bind(doc,object_name,parameter,source_object,source_parameter,scale=1,offset_mm=0):
    obj,parameter,expression=_binding(doc,object_name,parameter,source_object,source_parameter,scale,offset_mm)
    doc.openTransaction('Bind shared dimension')
    try:
        obj.setExpression(parameter,expression);doc.recompute();native.validate_document(doc)
        doc.commitTransaction();return dict(object_name=obj.Name,parameter=parameter,expression=expression)
    except Exception:
        doc.abortTransaction();doc.recompute();raise


def bind_many(doc,bindings):
    """Apply related typed bindings atomically; no arbitrary expression strings."""
    if not isinstance(bindings,list) or not 1<=len(bindings)<=128:
        raise ValueError('bindings must contain 1..128 typed binding objects')
    required={'object_name','parameter','source_object','source_parameter'}
    allowed=required|{'scale','offset_mm'}
    prepared=[];seen=set()
    for item in bindings:
        if not isinstance(item,dict) or not required<=set(item) or not set(item)<=allowed:
            raise ValueError('each binding needs target/source names and parameters; only scale/offset_mm optional')
        obj,parameter,expression=_binding(doc,**item)
        key=(obj.Name,parameter)
        if key in seen:raise ValueError('duplicate binding target in batch')
        seen.add(key);prepared.append((obj,parameter,expression))
    doc.openTransaction('Bind shared dimensions atomically')
    try:
        for obj,parameter,expression in prepared:obj.setExpression(parameter,expression)
        doc.recompute();native.validate_document(doc)
        result=dict(bindings=[dict(object_name=o.Name,parameter=p,expression=e) for o,p,e in prepared],
                    binding_count=len(prepared),atomic=True)
        doc.commitTransaction();return result
    except Exception:
        doc.abortTransaction();doc.recompute();raise
