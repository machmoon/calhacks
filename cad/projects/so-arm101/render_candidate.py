"""Render a saved SO101 candidate's actual native geometry without rebuilding it."""
import argparse
import hashlib
import json
from pathlib import Path
import FreeCAD as App
import render


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('candidate', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    source = args.candidate.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    original_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    doc = App.openDocument(str(source))
    objects = [o for o in doc.Objects if o.TypeId == 'Mesh::Feature' and o.Name.startswith('Visual_')]
    if len(objects) != 17:
        raise ValueError('Expected exactly 17 source mesh visual instances')
    if sum(o.Mesh.CountFacets for o in objects) > 1000000:
        raise ValueError('Preview exceeds explicit one-million-facet budget')
    report = {'source': str(source), 'source_sha256': original_hash,
              'geometry_kind': 'native mesh assembly', 'illustrative_colors': True,
              'independent_evaluation': False, 'previews': []}
    for name, angles in [('saved-pose', None), ('demonstration-pose', {
        'shoulder_pan': 25, 'shoulder_lift': -20, 'elbow_flex': 35,
        'wrist_flex': -25, 'wrist_roll': 30, 'gripper': 35})]:
        if angles:
            for key, value in angles.items():
                if not getattr(doc.JointParameters, key+'_lower').Value <= value <= getattr(doc.JointParameters, key+'_upper').Value:
                    raise ValueError('Demonstration angle outside source limits')
            for key, value in angles.items():
                setattr(doc.JointParameters, key, value)
            doc.recompute()
        triangles, colors = [], []
        for obj in objects:
            mesh = obj.Mesh.copy()
            mesh.Placement = obj.getGlobalPlacement().multiply(obj.Placement.inverse()).multiply(mesh.Placement)
            vertices, facets = mesh.Topology
            identity = obj.Label.lower()
            for prop in ('SourceMesh', 'SourceAsset'):
                if prop in obj.PropertiesList:
                    identity += ' ' + str(getattr(obj, prop)).lower()
            color = (68, 78, 88) if 'sts3215' in identity else (64, 139, 157)
            if 'moving_jaw' in identity:
                color = (215, 153, 59)
            for facet in facets:
                triangles.append([[vertices[j].x, vertices[j].y, vertices[j].z] for j in facet])
                colors.append(color)
        rgb, stats = render.rasterize(triangles, colors, 800, 600, 38, 24)
        path = output / (name+'.png')
        path.write_bytes(render.png_rgb(rgb))
        report['previews'].append({'image': str(path), 'angles_degrees': angles,
                                   'triangle_count': len(triangles), 'stats': stats})
        if angles:
            doc.saveAs(str(output / 'demonstration-pose.FCStd'))
    App.closeDocument(doc.Name)
    report['source_unchanged'] = hashlib.sha256(source.read_bytes()).hexdigest() == original_hash
    if not report['source_unchanged']:
        raise RuntimeError('Rendering unexpectedly modified source')
    (output/'render-report.json').write_text(json.dumps(report, indent=2))
    print(json.dumps({'source_unchanged': True, 'output': str(output)}), flush=True)


if __name__ == '__main__':
    main()
