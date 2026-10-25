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
   ry=72*radius-(2 if o.Inner else 0);rz=p.PodHalfHeight.Value*radius-(2 if o.Inner else 0)
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
    rad=5.6/math.sqrt(3);points=[A.Vector(rad*math.cos(i*math.pi/3),sign*(o.Parameters.ClampBoltOffset.Value if o.Parameters else 25)+rad*math.sin(i*math.pi/3),27) for i in range(6)]
    pocket=Part.Face(Part.makePolygon(points+points[:1])).extrude(A.Vector(0,0,3.5));body=body.cut(pocket)
  offset=o.Parameters.ClampBoltOffset.Value if o.Parameters else 25
  for sign in [-1,1]:body=body.cut(Part.makeCylinder(1.6,35,A.Vector(0,sign*offset,24)))
  shape=body.removeSplitter()
  if not shape.isValid() or len(shape.Solids)!=1:raise ValueError('Invalid split motor mount '+o.MountMode)
  placement=o.Placement;o.Shape=shape;o.Placement=placement
 def dumps(self):return None
 def loads(self,state):pass


class TailSaddle(LiftMount):
 """Split boom-bearing fin root; inherited clamp geometry and original fin web."""
 def execute(self,o):
  LiftMount.execute(self,o)
  if o.MountMode=='Upper':
   placement=o.Placement;shape=o.Shape.copy();shape.Placement=A.Placement()
   lower=shape.common(Part.makeBox(100,100,35,A.Vector(-50,-50,20)))
   fin=Part.makeBox(130,5,162,A.Vector(-40,-2.5,53))
   shape=lower.fuse(fin).removeSplitter()
   if not shape.isValid() or len(shape.Solids)!=1:raise ValueError('Invalid clamped fin root')
   o.Shape=shape;o.Placement=placement

class WingBridge:
 """Original retained boom/spar or pod/spar joint, geometrically checked only."""
 def __init__(self,o,mode):
  o.addProperty('App::PropertyString','BridgeMode');o.BridgeMode=mode
  o.addProperty('App::PropertyLink','Parameters');o.Proxy=self
 def execute(self,o):
  def box(x,y,z,dx,dy,dz):return Part.makeBox(dx,dy,dz,A.Vector(x,y,z))
  def hexhole(x,y,z,depth):
   r=5.6/math.sqrt(3);pts=[A.Vector(x+r*math.cos(i*math.pi/3),y+r*math.sin(i*math.pi/3),z) for i in range(6)]
   return Part.Face(Part.makePolygon(pts+pts[:1])).extrude(A.Vector(0,0,depth))
  ring=Part.makeCylinder(8,12,A.Vector(0,-6,85),A.Vector(0,1,0)).cut(Part.makeCylinder(5,14,A.Vector(0,-7,85),A.Vector(0,1,0)))
  upper=o.BridgeMode=='Cap'
  body=ring.common(box(-25,-10,85.5 if upper else 75,50,20,15 if upper else 9.5))
  for sign in [-1,1]:
   body=body.fuse(box(sign*12-5,-6,85.5 if upper else 77,10,12,5.5 if upper else 7.5))
   body=body.cut(Part.makeCylinder(1.6,25,A.Vector(sign*12,0,74)))
   if not upper:body=body.cut(hexhole(sign*12,0,76,3.5))
  if o.BridgeMode=='BoomBody':
   root=Part.makeCylinder(15,40,A.Vector(-20,0,40),A.Vector(1,0,0)).cut(Part.makeCylinder(10,42,A.Vector(-21,0,40),A.Vector(1,0,0)))
   root=root.common(box(-25,-35,40.5,50,70,15))
   for sign in [-1,1]:
    root=root.fuse(box(-10,sign*20-9,40.5,20,18,12.5))
    root=root.cut(Part.makeCylinder(1.6,35,A.Vector(0,sign*25,24)))
    root=root.cut(hexhole(0,sign*25,50.6,3.4))
   body=body.fuse(box(-5,-6,50,10,12,35)).fuse(root)
  elif o.BridgeMode=='PodBody':
   body=body.fuse(box(-5,-6,47,10,12,38)).fuse(box(-8,-20,47,16,40,3))
  # Bore after ALL web/foot fusions so no support material fills the shaft path.
  body=body.cut(Part.makeCylinder(5,14,A.Vector(0,-7,85),A.Vector(0,1,0))).removeSplitter()
  if not body.isValid() or len(body.Solids)!=1:raise ValueError('Invalid retained wing bridge '+o.BridgeMode)
  placement=o.Placement;o.Shape=body;o.Placement=placement
 def dumps(self):return None
 def loads(self,state):pass

class BentTail:
 def __init__(self,o,elevator=False):
  o.addProperty('App::PropertyLink','Parameters');o.addProperty('App::PropertyBool','Elevator');o.Elevator=elevator;o.Proxy=self
 def execute(self,o):
  half=o.Parameters.BoomHalfSpacing.Value+2.5; bend=math.radians(o.Parameters.TailBendAngle.Value); wires=[]
  for y in [-half,-half+12,-35,35,half-12,half]:
   rise=max(0,half-12-max(35,abs(y)))*math.tan(bend); fraction=rise/max(1,(half-47)*math.tan(bend));x=900+65*fraction+(132 if o.Elevator else 0);z=215+rise;chord=38 if o.Elevator else 130
   pts=[A.Vector(x,y,z),A.Vector(x+chord,y,z),A.Vector(x+chord,y,z+6),A.Vector(x,y,z+6)]
   wires.append(Part.makePolygon(pts+pts[:1]))
  o.Shape=Part.makeLoft(wires,True,True)
 def dumps(self):return None
 def loads(self,state):pass

class Winglet:
 def __init__(self,o):
  o.addProperty('App::PropertyLink','Parameters');o.addProperty('App::PropertyString','Coordinates');o.Proxy=self
 def execute(self,o):
  p=o.Parameters;theta=math.radians(90-p.WingletCantAngle.Value);radius=30;h=p.WingletHeight.Value;wires=[]
  stations=[(radius*math.sin(a),radius*(1-math.cos(a)),a) for a in [theta*i/5 for i in range(6)]]
  y,z,a=stations[-1]
  for f in [.35,.7,1]:stations.append((y+(h-z)*f/math.tan(theta),z+(h-z)*f,theta))
  for y,z,a in stations:
   chord=.22*p.RootChord.Value*(1-.55*z/h);le=400+p.WingletSweepOffset.Value*z/h
   pts=[A.Vector(le+x*chord,p.Span.Value/2+y-math.sin(a)*t*chord,80+z+math.cos(a)*t*chord) for x,t in json.loads(o.Coordinates)];pts[-1]=pts[0];split=min(range(len(pts)),key=lambda i:pts[i].x);edges=[]
   for row in [pts[:split+1],pts[split:]]:
    curve=Part.BSplineCurve();curve.interpolate(row);edges.append(curve.toShape())
   wires.append(Part.Wire(edges))
  o.Shape=Part.makeLoft(wires,True,False)
 def dumps(self):return None
 def loads(self,state):pass

class CruiseProp:
 def __init__(self,o):
  o.addProperty('App::PropertyString','StationData');o.Proxy=self
 def execute(self,o):
  wires=[]
  for s in json.loads(o.StationData):
   if s['radius_mm']<5:continue
   r=s['radius_mm'];c=s['chord_mm'];t=s['max_thickness_mm'];tw=math.radians(s['twist_deg']);pts=[]
   for i in range(32):
    a=2*math.pi*i/32;u=c*(1-math.cos(a))/2;v=t*math.sin(a)/2
    pts.append(A.Vector(692+s['rake_le_mm']+u*math.sin(tw)+v*math.cos(tw),r,24+s['sweep_le_mm']-u*math.cos(tw)+v*math.sin(tw)))
   curve=Part.BSplineCurve();curve.interpolate(pts,PeriodicFlag=True);wires.append(Part.Wire(curve.toShape()))
  blade=Part.makeLoft(wires,True,False);other=blade.copy();other.rotate(A.Vector(0,0,24),A.Vector(1,0,0),180)
  hub=Part.makeCylinder(10.16,10.414,A.Vector(687,0,24),A.Vector(1,0,0))
  o.Shape=hub.fuse(blade).fuse(other).cut(Part.makeCylinder(3.175,50,A.Vector(675,0,24),A.Vector(1,0,0))).removeSplitter()
 def dumps(self):return None
 def loads(self,state):pass
