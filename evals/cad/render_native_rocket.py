"""Render measured native rocket reference geometry, with scope kept visible."""
import json,sys
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from raster import render
source=Path(sys.argv[1]).resolve();index=int(sys.argv[2])
r=json.loads(source.read_text())['imports'][index];out=Path(r['native_path']).parent
m=json.loads((out/'preview-mesh.json').read_text())
fig=plt.figure(figsize=(14,6),facecolor='#f3f1ec');ax=fig.add_axes([.02,.16,.96,.64])
ax.imshow(render(m['faces'],m['colors'],width=1500,height=540));ax.set_axis_off()
fig.text(.04,.94,'Two-stage reference · native CAD audit',fontsize=23,weight='bold',color='#263245')
ignored=sum(e['status']=='ignored' for e in r['component_coverage'])
fig.text(.04,.875,f"{r['exchange']['physical_solids']} supported solids · {r['independent_checks_count']} numerical checks pass · {ignored} recovery components omitted",fontsize=13,color='#536173')
fig.text(.04,.095,'Longitudinal cutaway: front halves of tubes and nose removed for display only.',fontsize=11,color='#536173')
fig.text(.04,.05,'Full CAD coverage FAILS. Source geometry is not a completed manufacturing or flight design.',fontsize=11,color='#a14f3f')
fig.savefig(out/'preview.png',dpi=150,facecolor=fig.get_facecolor());plt.close(fig)
