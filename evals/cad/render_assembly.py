"""Render actual assembly B-reps for static visual inspection (not a geometry check)."""
import argparse
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from raster import render
from build123d import import_step


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory',type=Path)
    parser.add_argument('--title')
    parser.add_argument('--subtitle')
    parser.add_argument('--highlight-clashes',action='store_true')
    args=parser.parse_args()
    root=args.directory
    plan=json.loads((root/'plan.json').read_text())
    report=json.loads((root/'assembly.json').read_text())
    fig=plt.figure(figsize=(12,7),facecolor='#f3f1ec')
    ax=fig.add_axes([.05,.17,.9,.62])
    ax.set_facecolor('#f3f1ec')
    # Render the saved interchange artifact, not regenerated current source.
    # A cache-key change must not make an existing design impossible to inspect.
    assembly=import_step(root/'assembly.step')
    clashes={name for pair in report['clashes'] for name in (pair['a'],pair['b'])}
    def leaves(shape):
        if shape.children:
            for child in shape.children:
                yield from leaves(child)
        else:
            yield shape
    all_faces, all_colors = [], []
    for shape in leaves(assembly):
        vertices, triangles=shape.tessellate(.15)
        points=[[v.X,v.Y,v.Z] for v in vertices]
        faces=[[points[i] for i in tri] for tri in triangles]
        color=tuple(shape.color) if shape.color else '#9aa0a6'
        if args.highlight_clashes and shape.label in clashes:
            color='#d36a4d'
        all_faces.extend(faces)
        all_colors.extend([color]*len(faces))
    ax.imshow(render(all_faces,all_colors))
    ax.set_axis_off()
    fixture=report['name']=='wing_structure_fixture'
    title=args.title or ('Wing structure · CAD geometry fixture' if fixture else 'Aircraft CAD · assembly audit')
    subtitle=args.subtitle or ('5 SD7062 ribs  /  2 spars  /  600 mm span' if fixture else
        f"{report['n_parts']} solids  /  {len(report['clashes'])} overlaps  /  {len(report['floating'])} disconnected contacts")
    fig.text(.06,.92,title,fontsize=22,color='#263245',weight='bold')
    fig.text(.06,.865,subtitle,fontsize=13,color='#536173')
    fig.text(.06,.08,f"Saved STEP geometry • {report['pairs_total']} part pairs considered • named parts",fontsize=11,color='#536173')
    scope=plan.get('scope','Geometry inspection only')
    if len(scope)>115:scope=scope[:112]+'...'
    fig.text(.06,.045,scope,fontsize=10,color='#677384')
    fig.savefig(root/'preview.png',dpi=160,facecolor=fig.get_facecolor())
    plt.close(fig)

if __name__=='__main__':main()
