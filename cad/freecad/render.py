"""Bounded orthographic native-shape renderer. NumPy + standard-library PNG only.

The per-pixel depth buffer follows evals/cad/raster.py; no source geometry or
visibility changes, GUI, display tessellation cache, or generated imagery.
"""
import base64
import hashlib
import math
import struct
import zlib
import numpy as np

BACKGROUND=(243,241,236)
PALETTE=((76,139,153),(214,158,71),(98,157,126),(145,116,170),
         (188,113,100),(108,132,179),(168,155,101),(122,145,145))
MAX_PIXELS=1024*1024
MAX_TRIANGLES=100000
MAX_PIXEL_WORK=80000000


def integer(value,name,low,high):
    if isinstance(value,bool) or not isinstance(value,int) or not low<=value<=high:
        raise ValueError(f'{name} must be an integer in [{low}, {high}]')
    return value


def number(value,name,low,high):
    if isinstance(value,bool) or not isinstance(value,(int,float)) or not math.isfinite(value) or not low<=value<=high:
        raise ValueError(f'{name} must be finite and in [{low}, {high}]')
    return float(value)


def part_color(name):
    return PALETTE[int.from_bytes(hashlib.sha256(name.encode()).digest()[:4],'big')%len(PALETTE)]


def png_rgb(rgb):
    height,width,channels=rgb.shape
    if channels!=3 or rgb.dtype!=np.uint8:raise ValueError('PNG encoder requires uint8 RGB')
    def chunk(kind,data):
        return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data)&0xffffffff)
    raw=b''.join(b'\0'+row.tobytes() for row in rgb)
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',width,height,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(raw,6))+chunk(b'IEND',b'')


def rasterize(faces,colors,width,height,azimuth_degrees,elevation_degrees):
    xyz=np.asarray(faces,dtype=np.float64)
    if xyz.ndim!=3 or xyz.shape[1:]!=(3,3) or not np.isfinite(xyz).all():raise ValueError('invalid display triangulation')
    az,el=np.radians([azimuth_degrees,elevation_degrees])
    camera=np.array([math.cos(el)*math.cos(az),math.cos(el)*math.sin(az),math.sin(el)])
    right=np.array([-math.sin(az),math.cos(az),0.0]);up=np.cross(camera,right)
    low=xyz.min(axis=(0,1));high=xyz.max(axis=(0,1));center=low+(high-low)/2
    q=(xyz-center)@np.stack([right,up,camera],axis=1)
    lo=q[:,:,:2].min(axis=(0,1));hi=q[:,:,:2].max(axis=(0,1));extent=hi-lo
    if max(extent)<=1e-12:raise ValueError('geometry has no projected extent')
    margin=12
    scale=min((width-2*margin)/max(extent[0],1e-12),(height-2*margin)/max(extent[1],1e-12))
    q[:,:,0]=(q[:,:,0]-(lo[0]+hi[0])/2)*scale+width/2
    q[:,:,1]=height/2-(q[:,:,1]-(lo[1]+hi[1])/2)*scale
    boxes=[];pixel_work=0
    for tri in q:
        xmin=max(0,int(np.floor(tri[:,0].min())));xmax=min(width-1,int(np.ceil(tri[:,0].max())))
        ymin=max(0,int(np.floor(tri[:,1].min())));ymax=min(height-1,int(np.ceil(tri[:,1].max())))
        boxes.append((xmin,xmax,ymin,ymax))
        pixel_work+=max(0,xmax-xmin+1)*max(0,ymax-ymin+1)
        if pixel_work>MAX_PIXEL_WORK:raise ValueError('render projected-pixel work budget exceeded; reduce dimensions or select fewer objects')
    rgb=np.full((height,width,3),BACKGROUND,dtype=np.uint8);depth=np.full((height,width),-np.inf)
    light=np.array([-.3,-.4,.85]);light/=np.linalg.norm(light)
    for source,tri,color,bbox in zip(xyz,q,colors,boxes):
        xmin,xmax,ymin,ymax=bbox
        if xmin>xmax or ymin>ymax:continue
        a,b,c=tri;den=(b[1]-c[1])*(a[0]-c[0])+(c[0]-b[0])*(a[1]-c[1])
        if abs(den)<1e-10:continue
        yy,xx=np.mgrid[ymin:ymax+1,xmin:xmax+1];xx=xx+.5;yy=yy+.5
        u=((b[1]-c[1])*(xx-c[0])+(c[0]-b[0])*(yy-c[1]))/den
        v=((c[1]-a[1])*(xx-c[0])+(a[0]-c[0])*(yy-c[1]))/den;w=1-u-v
        z=u*a[2]+v*b[2]+w*c[2];region=depth[ymin:ymax+1,xmin:xmax+1]
        mask=(u>=-1e-9)&(v>=-1e-9)&(w>=-1e-9)&(z>region)
        if not mask.any():continue
        normal=np.cross(source[1]-source[0],source[2]-source[0]);length=np.linalg.norm(normal)
        shade=.55+.45*abs(normal@light/length) if length else .7
        region[mask]=z[mask]
        rgb[ymin:ymax+1,xmin:xmax+1][mask]=np.clip(np.asarray(color)*shade,0,255).astype(np.uint8)
    covered=int(np.isfinite(depth).sum())
    if not covered:raise ValueError('render produced no visible geometry')
    return rgb,dict(bounds_mm=[low.tolist(),high.tolist()],visible_pixels=covered,projected_pixel_work=pixel_work)


def selected(objects,fingerprint,width=640,height=480,azimuth_degrees=-52,elevation_degrees=24,deflection_mm=.5,max_triangles=50000,angular_deflection_radians=.35,max_objects=128,max_source_faces=20000):
    width=integer(width,'width',64,1024);height=integer(height,'height',64,1024)
    if width*height>MAX_PIXELS:raise ValueError('render pixel budget exceeded')
    azimuth_degrees=number(azimuth_degrees,'azimuth_degrees',-360,360)
    elevation_degrees=number(elevation_degrees,'elevation_degrees',-90,90)
    deflection_mm=number(deflection_mm,'deflection_mm',.05,10)
    max_triangles=integer(max_triangles,'max_triangles',1,MAX_TRIANGLES)
    angular_deflection_radians=number(angular_deflection_radians,'angular_deflection_radians',.05,1.0)
    max_objects=integer(max_objects,'max_objects',1,4096)
    max_source_faces=integer(max_source_faces,'max_source_faces',1,100000)
    if not 1<=len(objects)<=max_objects:raise ValueError('render object budget exceeded')
    if sum(len(o.Shape.Faces) for o in objects)>max_source_faces:raise ValueError('render source-face budget exceeded; select fewer objects')
    before=fingerprint(objects);faces=[];colors=[];parts=[]
    for obj in sorted(objects,key=lambda o:o.Name):
        # Mesh detached copies only. Relative floor prevents enormous-coordinate
        # geometry requesting microscopic display tessellation.
        import native
        from assembly_frames import world_shape
        shape=world_shape(obj).cleaned();effective=max(deflection_mm,native.shape_bounds(shape).DiagonalLength/2000)
        import MeshPart
        mesh=MeshPart.meshFromShape(Shape=shape,LinearDeflection=effective,AngularDeflection=angular_deflection_radians,Relative=False)
        vertices,triangles=mesh.Topology
        if len(faces)+len(triangles)>max_triangles:raise ValueError('render triangle budget exceeded; increase deflection or select fewer objects')
        color=part_color(obj.Name)
        for tri in triangles:
            faces.append([[vertices[j].x,vertices[j].y,vertices[j].z] for j in tri]);colors.append(color)
        parts.append(dict(name=obj.Name,label=obj.Label,color_rgb=list(color),triangles=len(triangles),effective_deflection_mm=effective,angular_deflection_radians=angular_deflection_radians))
    rgb,stats=rasterize(faces,colors,width,height,azimuth_degrees,elevation_degrees)
    if fingerprint(objects)!=before:raise RuntimeError('render unexpectedly changed source geometry')
    encoded=base64.b64encode(png_rgb(rgb)).decode('ascii')
    return dict(content_blocks=[dict(type='image',source=dict(type='base64',media_type='image/png',data=encoded))],
                width=width,height=height,objects=[o.Name for o in objects],parts=parts,
                camera=dict(projection='orthographic',azimuth_degrees=azimuth_degrees,elevation_degrees=elevation_degrees),
                resource_budget=dict(max_objects=max_objects,max_source_faces=max_source_faces,max_triangles=max_triangles),
                triangles=len(faces),geometry_fingerprint=before,source_geometry_unchanged=True,
                scope='Depth-buffered display triangulation of selected native shapes; not a geometry-fit or manufacturing approval',**stats)
