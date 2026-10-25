"""Render exported native CAD triangles with measured assembly audit counts."""
import json,sys
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from raster import render
root=Path(sys.argv[1]).resolve()
m=json.loads((root/'preview-mesh.json').read_text());r=json.loads((root/'result.json').read_text())
fit=r['fit'];graph=json.loads((root/'connectivity.json').read_text())
fig=plt.figure(figsize=(12,7),facecolor='#f3f1ec');ax=fig.add_axes([.03,.16,.94,.66])
ax.imshow(render(m['faces'],m['colors']));ax.set_axis_off()
fig.text(.05,.94,'Aircraft · native structural iteration',fontsize=22,weight='bold',color='#263245')
fig.text(.05,.885,f"{len(r['physical_parts'])} solids · {sum(p['clash'] for p in fit['pairs'])} overlaps · {sum(p['floating'] for p in fit['pairs'])} unmet direct contacts · {len(graph['components'])} contact groups",fontsize=12,color='#a14f3f')
fig.text(.05,.095,'Both wings use hollow skins and linked ribs. Upper wing skin removed in this view only.',fontsize=11,color='#536173')
fig.text(.05,.05,'Assembly fit FAILS. Root interfaces, clamps, servo mounts and other joints remain incomplete.',fontsize=11,color='#536173')
fig.savefig(root/'preview.png',dpi=160,facecolor=fig.get_facecolor());plt.close(fig)
