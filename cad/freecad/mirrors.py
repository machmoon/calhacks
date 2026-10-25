"""Native linked mirror instances, exposed without arbitrary Python execution."""
import native
import booleans
import math


def validate(obj):
    if not getattr(obj, 'WrightTypedMirror', False):return
    if obj.TypeId != 'Part::Mirroring' or obj.Source is None:
        raise ValueError('typed mirror requires a native source')
    booleans._acyclic([obj])
    for part in (obj.Source, obj):
        info=native.describe(part)
        if info['solids'] != 1 or info['assembly_container'] or getattr(part,'NonphysicalKeepout',False):
            raise ValueError('mirror requires physical single-solid source and result')


def create(doc, name, source, normal, origin=(0,0,0)):
    result_name=native.name(name);source_name=native.name(source)
    axis=native.vector(normal);point=native.vector(origin)
    if not math.isfinite(axis.Length) or axis.Length < 1e-12:raise ValueError('mirror plane normal must have finite nonzero length')
    axis.normalize()
    if doc.getObject(result_name) is not None:raise ValueError('result name already exists')
    original=doc.getObject(source_name)
    if original is None:raise ValueError('unknown mirror source')
    booleans._acyclic([original])
    info=native.describe(original)
    if info['solids']!=1 or info['assembly_container'] or getattr(original,'NonphysicalKeepout',False):
        raise ValueError('mirror source must be a physical single solid')
    doc.openTransaction('Mirror '+result_name)
    try:
        result=doc.addObject('Part::Mirroring',result_name)
        result.Source=original;result.Normal=axis;result.Base=point
        result.addProperty('App::PropertyBool','WrightTypedMirror','Wright')
        result.WrightTypedMirror=True;result.setEditorMode('WrightTypedMirror',1)
        doc.recompute();native.validate_document(doc)
        evidence=native.describe(result)
        doc.commitTransaction()
        return evidence
    except Exception:
        doc.abortTransaction();doc.recompute();raise
