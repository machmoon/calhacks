import sys;sys.path.insert(0,'cad/projects/vtol-demonstrator')
import FreeCAD as A,Part,vtol_features,json
D=A.newDocument();u=D.addObject('Part::FeaturePython','Upper');vtol_features.LiftMount(u,'Upper');l=D.addObject('Part::FeaturePython','Lower');vtol_features.LiftMount(l,'Lower');D.recompute()
m=Part.read('cad/components/vendor/tmotor/mn2806evo.step');m.Placement=A.Placement(A.Vector(0,0,150),A.Rotation(A.Vector(1,0,0),A.Vector(0,0,-1)))
b=Part.makeCylinder(10,50,A.Vector(-25,0,40),A.Vector(1,0,0)).cut(Part.makeCylinder(8,50,A.Vector(-25,0,40),A.Vector(1,0,0)))
r={}
for n,a,c in [('motor_plate',m,u.Shape),('upper_boom',u.Shape,b),('lower_boom',l.Shape,b),('split',u.Shape,l.Shape)]:r[n]={'common_mm3':a.common(c).Volume,'distance_mm':a.distToShape(c)[0]}
open('out/vtol-demonstrator/mount-local-check.json','w').write(json.dumps(r,indent=2));print(r,flush=True)
c=m.common(u.Shape)
for sol in c.Solids:print('penetration',sol.Volume,sol.BoundBox,flush=True)
p=Part.read('cad/components/vendor/tmotor/p12x4.step');p.Placement=A.Placement(A.Vector(0,0,159.2130499798656),A.Rotation(A.Vector(0,0,1),45));print('prop_motor',p.common(m).Volume,p.distToShape(m)[0],flush=True)
