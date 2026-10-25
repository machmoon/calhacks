"""Native-face display mesh with face-local smooth normals; geometry unchanged.

Normals average triangle area only within each actual BRep face. Separate faces
retain their own normals, so a real hard edge is not hidden by vertex welding.
"""
import math,hashlib
from collections import OrderedDict
_DISPLAY_CACHE=OrderedDict()
import numpy as np
import render
import native
import MeshPart


def selected(objects, path, width=1000, height=750, azimuth=-52, elevation=24, linear_deflection=.25, angular_deflection=.25, clip_planes=None, translations=None):
    width=render.integer(width,'width',64,1024);height=render.integer(height,'height',64,1024)
    azimuth=render.number(azimuth,'azimuth',-360,360);elevation=render.number(elevation,'elevation',-90,90)
    if width*height>1024*1024 or not 1<=len(objects)<=128:raise ValueError('Preview pixel/object budget exceeded')
    if sum(len(o.Shape.Faces) for o in objects)>20000:raise ValueError('Preview source-face budget exceeded')
    triangles=[]; normals=[]; colors=[]; counts=[]
    for obj in objects:
        from assembly_frames import world_shape
        shape=world_shape(obj).cleaned()
        name=obj.Name
        shift=np.asarray((translations or {}).get(name,(0,0,0)),dtype=float)
        if shift.shape!=(3,) or not np.isfinite(shift).all():raise ValueError('Invalid display translation')
        if name=='MG90SStaticEnvelope':color=(60,135,203)
        elif 'Skin' in name:color=(220,229,232)
        elif 'Rib' in name:color=(221,160,70)
        elif 'Spar' in name:color=(75,91,99)
        elif 'Boom' in name:color=(52,64,72)
        elif name=='Pod':color=(61,117,145)
        elif 'Fin' in name or 'Stabilizer' in name:color=(84,139,157)
        elif 'Motor' in name or 'AOS' in name:color=(68,79,87)
        else:color=(143,156,163)
        key=(hashlib.sha256(shape.exportBrepToString().encode()).digest(),linear_deflection,angular_deflection,tuple((clip_planes or {}).get(name,())))
        if key in _DISPLAY_CACHE:
            cached_xyz,cached_normals=_DISPLAY_CACHE[key];_DISPLAY_CACHE.move_to_end(key)
            if len(triangles)+len(cached_xyz)>500000:raise ValueError('Preview exceeds 500000 native display triangles')
            triangles.extend(cached_xyz+shift);normals.extend(cached_normals);colors.extend([color]*len(cached_xyz))
            counts.append(dict(name=name,faces=len(shape.Faces),triangles=len(cached_xyz)))
            continue
        start_index=len(triangles)
        count=0
        for face in shape.Faces:
            mesh=MeshPart.meshFromShape(Shape=face,LinearDeflection=linear_deflection,AngularDeflection=angular_deflection,Relative=False)
            vertices,indices=mesh.Topology
            if not indices:continue
            vertices=np.array([[p.x,p.y,p.z] for p in vertices]);indices=np.array(indices,dtype=int)
            xyz=vertices[indices]
            cross=np.cross(xyz[:,1]-xyz[:,0],xyz[:,2]-xyz[:,0])
            vn=np.zeros_like(vertices)
            for j in range(3):np.add.at(vn,indices[:,j],cross)
            lengths=np.linalg.norm(vn,axis=1);lengths[lengths<1e-15]=1
            vn/=lengths[:,None]
            face_normals=vn[indices]
            if clip_planes and name in clip_planes:
                axis,limit=clip_planes[name]
                inside=xyz[:,:,axis]<=limit
                mixed=np.any(inside,axis=1)&~np.all(inside,axis=1)
                kept_xyz=list(xyz[np.all(inside,axis=1)]);kept_normals=list(face_normals[np.all(inside,axis=1)])
                for triangle,ns in zip(xyz[mixed],face_normals[mixed]):
                    polygon=list(zip(triangle,ns));clipped=[]
                    for i,(point,normal) in enumerate(polygon):
                        previous,previous_normal=polygon[i-1]
                        a=previous[axis]<=limit;b=point[axis]<=limit
                        if a!=b:
                            fraction=(limit-previous[axis])/(point[axis]-previous[axis])
                            clipped.append((previous+fraction*(point-previous),previous_normal+fraction*(normal-previous_normal)))
                        if b:clipped.append((point,normal))
                    for j in range(1,len(clipped)-1):
                        tri=[clipped[0],clipped[j],clipped[j+1]]
                        kept_xyz.append([v[0] for v in tri]);kept_normals.append([v[1] for v in tri])
                xyz=np.asarray(kept_xyz);face_normals=np.asarray(kept_normals)
            triangles.extend(xyz);normals.extend(face_normals);colors.extend([color]*len(xyz));count+=len(xyz)
            if len(triangles)>500000:raise ValueError('Preview exceeds 500000 native display triangles')
        _DISPLAY_CACHE[key]=(np.asarray(triangles[start_index:]),np.asarray(normals[start_index:]))
        while sum(len(row[0]) for row in _DISPLAY_CACHE.values())>500000:_DISPLAY_CACHE.popitem(last=False)
        if np.any(shift):
            triangles[start_index:]=[np.asarray(tri)+shift for tri in triangles[start_index:]]
        counts.append(dict(name=name,faces=len(shape.Faces),triangles=count))
    xyz=np.asarray(triangles);normals=np.asarray(normals);colors=np.asarray(colors)
    # Fixed 2x supersampling improves edge coverage, not geometric smoothness.
    w,h=width*2,height*2
    az,el=np.radians([azimuth,elevation]);camera=np.array([math.cos(el)*math.cos(az),math.cos(el)*math.sin(az),math.sin(el)])
    right=np.array([-math.sin(az),math.cos(az),0]);up=np.cross(camera,right)
    low=xyz.min(axis=(0,1));high=xyz.max(axis=(0,1));center=(low+high)/2
    q=(xyz-center)@np.stack([right,up,camera],axis=1)
    lo=q[:,:,:2].min(axis=(0,1));hi=q[:,:,:2].max(axis=(0,1));extent=hi-lo
    scale=min((w-100)/max(extent[0],1e-12),(h-100)/max(extent[1],1e-12))
    q[:,:,0]=(q[:,:,0]-(lo[0]+hi[0])/2)*scale+w/2
    q[:,:,1]=h/2-(q[:,:,1]-(lo[1]+hi[1])/2)*scale
    import display_raster
    rgb=display_raster.rasterize(q,normals,colors,w,h)
    raster_backend='rust' if rgb is not None else 'numpy_reference'
    if rgb is None:
        depth=np.full((h,w),-np.inf);rgb=np.full((h,w,3),(239,242,245),dtype=np.uint8)
        light=np.array([-.35,-.45,.82]);light/=np.linalg.norm(light)
        work=0
        for tri,vn,color in zip(q,normals,colors):
            xmin=max(0,int(np.floor(tri[:,0].min())));xmax=min(w-1,int(np.ceil(tri[:,0].max())))
            ymin=max(0,int(np.floor(tri[:,1].min())));ymax=min(h-1,int(np.ceil(tri[:,1].max())))
            if xmin>xmax or ymin>ymax:continue
            work+=(xmax-xmin+1)*(ymax-ymin+1)
            if work>300000000:raise ValueError('Preview exceeds projected pixel budget')
            a,b,c=tri;den=(b[1]-c[1])*(a[0]-c[0])+(c[0]-b[0])*(a[1]-c[1])
            if abs(den)<1e-10:continue
            yy,xx=np.mgrid[ymin:ymax+1,xmin:xmax+1];xx=xx+.5;yy=yy+.5
            u=((b[1]-c[1])*(xx-c[0])+(c[0]-b[0])*(yy-c[1]))/den
            v=((c[1]-a[1])*(xx-c[0])+(a[0]-c[0])*(yy-c[1]))/den;ww=1-u-v
            z=u*a[2]+v*b[2]+ww*c[2];region=depth[ymin:ymax+1,xmin:xmax+1]
            mask=(u>=-1e-9)&(v>=-1e-9)&(ww>=-1e-9)&(z>region)
            if not mask.any():continue
            n=u[mask,None]*vn[0]+v[mask,None]*vn[1]+ww[mask,None]*vn[2]
            length=np.linalg.norm(n,axis=1);length[length<1e-15]=1;n/=length[:,None]
            shade=.43+.57*np.abs(n@light)
            region[mask]=z[mask];rgb[ymin:ymax+1,xmin:xmax+1][mask]=np.clip(color*shade[:,None],0,255).astype(np.uint8)
    rgb=rgb.reshape(height,2,width,2,3).mean(axis=(1,3)).astype(np.uint8)
    path.write_bytes(render.png_rgb(rgb))
    return dict(path=str(path),width=width,height=height,triangles=len(triangles),parts=counts,
        bounds_mm=[low.tolist(),high.tolist()],camera=dict(azimuth=azimuth,elevation=elevation),
        raster_backend=raster_backend,display_method=f'{linear_deflection}mm / {angular_deflection}rad native-face display tessellation; area-weighted face-local interpolated normals; 2x edge supersampling; no cross-face normal welding',
        source_geometry_modified=False,scope='Display only; not fit or strength validation')
