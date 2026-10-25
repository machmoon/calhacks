"""Recognize a native coaxial tube and reuse its editable outer cylinder."""
import math


def outer_source(obj):
    if obj.TypeId=='Part::Mirroring':
        base, mirrors=outer_source(obj.Source)
        return base, mirrors+[obj]
    if obj.TypeId!='Part::Cut' or obj.Base.TypeId!='Part::Cylinder' or obj.Tool.TypeId!='Part::Cylinder':
        raise ValueError('Outer envelope requires a native cylinder-minus-cylinder tube')
    outer,inner=obj.Base,obj.Tool
    if not 0<inner.Radius.Value<outer.Radius.Value or abs(outer.Angle.Value-360)>1e-8 or abs(inner.Angle.Value-360)>1e-8:
        raise ValueError('Expected complete annular tube')
    import FreeCAD as App
    axis=outer.Placement.Rotation.multVec(App.Vector(0,0,1))
    other=inner.Placement.Rotation.multVec(App.Vector(0,0,1))
    delta=inner.Placement.Base-outer.Placement.Base
    if abs(axis.dot(other)-1)>1e-8 or (delta-axis*delta.dot(axis)).Length>1e-7:
        raise ValueError('Tube axes must be coaxial and equally oriented')
    start=delta.dot(axis)
    if start>1e-7 or start+inner.Height.Value<outer.Height.Value-1e-7:
        raise ValueError('Inner bore must pass through the outer cylinder')
    expected=math.pi*(outer.Radius.Value**2-inner.Radius.Value**2)*outer.Height.Value
    if len(obj.Shape.Solids)!=1 or abs(obj.Shape.Volume-expected)>max(1e-5,expected*1e-7):
        raise ValueError('Native tube geometry disagrees with its source cylinders')
    return outer,[]


def make_outer(doc,obj,name):
    outer, mirrors=outer_source(obj)
    result=outer
    for index, source in enumerate(mirrors):
        result=doc.addObject('Part::Mirroring',name+str(index))
        result.Source=outer
        for prop in ('Normal','Base'):
            setattr(result,prop,getattr(source,prop))
            for coordinate in ('x','y','z'):
                result.setExpression(prop+'.'+coordinate,source.Name+'.'+prop+'.'+coordinate)
        outer=result
    doc.recompute()
    if obj.Shape.cut(result.Shape).Solids:
        raise ValueError('Outer envelope does not contain the source tube')
    return result
