"""Orthographic views from native reconstruction triangulations."""
import json,sys
from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'evals/cad'));from raster import render
out=ROOT/'out/tsung-vtol-reference';data=json.loads((out/'preview-mesh.json').read_text())
xyz=np.asarray(data['faces']);az,el=np.radians([-52,24]);camera=np.array([np.cos(el)*np.cos(az),np.cos(el)*np.sin(az),np.sin(el)]);right=np.array([-np.sin(az),np.cos(az),0]);base=np.stack([right,np.cross(camera,right),camera],axis=1)
def view(right,up,camera,width,height):
 target=np.stack([right,up,camera],axis=1)
 return render(xyz@target@base.T,data['colors'],width,height)
fig,axes=plt.subplots(1,2,figsize=(15,8),dpi=130)
axes[0].imshow(view([0,1,0],[-1,0,0],[0,0,1],900,800));axes[0].set_title('Top view · 1200 mm span · 770 mm length')
axes[1].imshow(render(xyz,data['colors'],900,800));axes[1].set_title('Inverted-U tail · tapered pod · S3024 wing')
for ax in axes:ax.axis('off')
fig.suptitle('Tsung Xu VTOL | approximate native reference reconstruction',fontsize=18)
fig.text(.5,.035,'Published span, length and main airfoil; remaining station dimensions inferred from uncalibrated views\nBlue disks are swept envelopes, not propeller blades. Wing/tail are solid envelopes; internal assembly and manufacturing fit incomplete.',ha='center',fontsize=11)
fig.savefig(out/'reference-preview.png',bbox_inches='tight',facecolor='#f3f1ec')
