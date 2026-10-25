"""Resolve a native shape into document coordinates without changing its object."""


def world_shape(obj):
    shape = obj.Shape.copy()
    # A feature's Shape already carries its own Placement. getGlobalPlacement
    # additionally includes enclosing App::Part transforms. Apply only the
    # missing parent transform; otherwise a placed feature moves twice.
    if hasattr(obj, 'getGlobalPlacement') and hasattr(obj, 'Placement'):
        shape.Placement = obj.getGlobalPlacement().multiply(obj.Placement.inverse()).multiply(shape.Placement)
    return shape


def document_operand(doc, obj, name):
    """Native trace binder for Boolean operands inside transformed assemblies."""
    parent=obj.getGlobalPlacement().multiply(obj.Placement.inverse())
    container=obj.getParentGeoFeatureGroup()
    if container is None and parent.Base.Length<1e-9 and abs(parent.Rotation.Angle)<1e-9:
        return obj
    binder=doc.addObject('PartDesign::ShapeBinder',name)
    binder.Support=[(obj,[''])]
    binder.TraceSupport=True
    binder.addProperty('App::PropertyLink','WrightFrameSource','Wright')
    binder.WrightFrameSource=obj;binder.setEditorMode('WrightFrameSource',1)
    doc.recompute()
    expected=world_shape(obj)
    if binder.Shape.isNull() or expected.cut(binder.Shape).Solids or binder.Shape.cut(expected).Solids:
        raise ValueError('Native trace binder did not preserve the assembly placement')
    return binder
