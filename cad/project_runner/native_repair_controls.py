"""Real FreeCAD controls for source-derived, parametric tube envelopes."""
import FreeCAD as A
from tube_envelope import make_outer, outer_source
d=A.newDocument('TubeEnvelopeControls')
outer=d.addObject('Part::Cylinder','Outer');outer.Radius=5;outer.Height=30;outer.Placement.Base=A.Vector(10,20,0)
inner=d.addObject('Part::Cylinder','Inner');inner.Radius=4;inner.Height=32;inner.Placement.Base=A.Vector(10,20,-1)
tube=d.addObject('Part::Cut','Tube');tube.Base=outer;tube.Tool=inner
mirror=d.addObject('Part::Mirroring','Mirror');mirror.Source=tube;mirror.Normal=A.Vector(0,1,0)
d.recompute();envelope=make_outer(d,mirror,'Envelope')
assert not mirror.Shape.cut(envelope.Shape).Solids
assert envelope.Shape.Volume>mirror.Shape.Volume
outer.Height=40;inner.Height=42;d.recompute()
assert abs(envelope.Shape.BoundBox.ZLength-40)<1e-7
mirror.Base=A.Vector(0,5,0);d.recompute()
assert not mirror.Shape.cut(envelope.Shape).Solids
inner.Placement.Base.x+=.1;d.recompute()
try:outer_source(tube)
except ValueError:pass
else:raise AssertionError('Eccentric tube must not be accepted')
inner.Placement.Base.x-=.1;inner.Height=10;d.recompute()
try:outer_source(tube)
except ValueError:pass
else:raise AssertionError('Blind bore must not be accepted')
A.closeDocument(d.Name)
print('Outer-envelope controls: mirrored/parametric containment passes; eccentric/blind bores rejected')
