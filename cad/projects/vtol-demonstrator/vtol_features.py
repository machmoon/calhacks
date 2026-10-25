"""Persistent native FreeCAD scripted section features, no external CAD service."""
import json,math
import FreeCAD as A
import Part
class Section:
 def __init__(self,obj,kind):
  obj.addProperty('App::PropertyString','SectionKind');obj.SectionKind=kind
  obj.addProperty('App::PropertyLink','Parameters');obj.addProperty('App::PropertyFloat','Eta')
  obj.addProperty('App::PropertyString','Coordinates');obj.addProperty('App::PropertyBool','Inner')
  obj.addProperty('App::PropertyBool','WrightSplineProfile');obj.WrightSplineProfile=False
  obj.Proxy=self
 def execute(self,o):
  p=o.Parameters;eta=o.Eta
  if o.SectionKind=='wing':
   chord=p.RootChord.Value*(1-.78*eta**2);le=250+150*eta**2
   coords=json.loads(o.Coordinates); inset=3 if o.Inner else 0
   chord-=2*inset;scale=.72 if o.Inner else 1
   pts=[A.Vector(le+inset+x*chord,eta*p.Span.Value/2,80+z*chord*scale+(.8 if o.Inner else 0)) for x,z in coords]
   pts[-1]=pts[0] # UIUC trailing-edge x differs by0.00001 chord; close consistently.
   split=min(range(len(pts)),key=lambda i:pts[i].x)
   curves=[]
   for row in [pts[:split+1],pts[split:]]:
    c=Part.BSplineCurve();c.interpolate(row);curves.append(c.toShape())
   if (pts[-1]-pts[0]).Length>1e-7:curves.append(Part.makeLine(pts[-1],pts[0]))
   o.Shape=Part.Wire(curves)
   if len(o.Shape.Edges)!=2 or not Part.Face(o.Shape).isValid():raise ValueError("Invalid two-spline section")
   o.WrightSplineProfile=True
  elif o.SectionKind=='pod':
   x=120+eta*520;radius=math.sin(math.pi*(.05+.9*eta))**.65
   ry=72*radius-(2 if o.Inner else 0);rz=42*radius-(2 if o.Inner else 0)
   center=A.Vector(x,0,24)
   e=Part.Ellipse(center+A.Vector(0,ry,0),center+A.Vector(0,0,rz),center)
   o.Shape=Part.Wire(e.toShape())
 def dumps(self):return None
 def loads(self,state):pass

class LiftMount:
 """Original split saddle/pylon with drawing-confirmed motor hole patterns.
 Nominal boom fit and fastener envelopes do not certify clamp load or threads.
 """
 def __init__(self,o,mode):
  o.addProperty('App::PropertyString','MountMode');o.MountMode=mode
  o.addProperty('App::PropertyLink','Parameters');o.Proxy=self
 def execute(self,o):
  def box(x,y,z,dx,dy,dz):return Part.makeBox(dx,dy,dz,A.Vector(x,y,z))
  ring=Part.makeCylinder(15,40,A.Vector(-20,0,40),A.Vector(1,0,0)).cut(Part.makeCylinder(10,42,A.Vector(-21,0,40),A.Vector(1,0,0)))
  if o.MountMode=='Upper':
   body=ring.common(box(-25,-30,40.5,50,60,15))
   for sign in [-1,1]:
    body=body.fuse(box(-10,sign*20-9,40.5,20,18,12.5))
    body=body.fuse(box(sign*17.5-2.5,-10,50,5,20,82.3))
   body=body.fuse(box(-20,-20,132.3,40,40,4))
   # Source shaft protrudes below its base; this central aperture preserves it.
   body=body.cut(Part.makeCylinder(4.5,6,A.Vector(0,0,131.3)))
   for x,y in [(8.8388369737,8.8388320263),(-8.8388369737,-8.8388320263),(-6.7175125,6.7175155),(6.7175125,-6.7175165)]:
    body=body.cut(Part.makeCylinder(1.6,6,A.Vector(x,y,131.3)))
  else:
   body=ring.common(box(-25,-30,20,50,60,19.5))
   for sign in [-1,1]:
    body=body.fuse(box(-10,sign*20-9,28,20,18,11.5))
    rad=5.6/math.sqrt(3);points=[A.Vector(rad*math.cos(i*math.pi/3),sign*20+rad*math.sin(i*math.pi/3),27) for i in range(6)]
    pocket=Part.Face(Part.makePolygon(points+points[:1])).extrude(A.Vector(0,0,3.5));body=body.cut(pocket)
  for sign in [-1,1]:body=body.cut(Part.makeCylinder(1.6,35,A.Vector(0,sign*20,24)))
  shape=body.removeSplitter()
  if not shape.isValid() or len(shape.Solids)!=1:raise ValueError('Invalid split motor mount '+o.MountMode)
  placement=o.Placement;o.Shape=shape;o.Placement=placement
 def dumps(self):return None
 def loads(self,state):pass
