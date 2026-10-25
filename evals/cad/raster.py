"""Small orthographic depth-buffer renderer for saved CAD triangulations.

Depth is evaluated per pixel; large planar triangles must not paint over smaller
parts merely because their centroids happen to sort in front of those parts.
"""
import numpy as np
from matplotlib.colors import to_rgb


def render(faces,colors,width=1200,height=620):
    xyz=np.asarray(faces,dtype=float)
    az,el=np.radians([-52,24])
    camera=np.array([np.cos(el)*np.cos(az),np.cos(el)*np.sin(az),np.sin(el)])
    right=np.array([-np.sin(az),np.cos(az),0])
    up=np.cross(camera,right)
    center=(xyz.min(axis=(0,1))+xyz.max(axis=(0,1)))/2
    q=(xyz-center)@np.stack([right,up,camera],axis=1)
    lo=q[:,:,:2].min(axis=(0,1));hi=q[:,:,:2].max(axis=(0,1))
    scale=min((width-60)/(hi[0]-lo[0]),(height-40)/(hi[1]-lo[1]))
    q[:,:,0]=(q[:,:,0]-(lo[0]+hi[0])/2)*scale+width/2
    q[:,:,1]=height/2-(q[:,:,1]-(lo[1]+hi[1])/2)*scale
    rgb=np.full((height,width,3),[243,241,236],dtype=np.uint8)
    depth=np.full((height,width),-np.inf)
    light=np.array([-.3,-.4,.85]);light/=np.linalg.norm(light)
    for source,tri,color in zip(xyz,q,colors):
        xmin=max(0,int(np.floor(tri[:,0].min())));xmax=min(width-1,int(np.ceil(tri[:,0].max())))
        ymin=max(0,int(np.floor(tri[:,1].min())));ymax=min(height-1,int(np.ceil(tri[:,1].max())))
        if xmin>xmax or ymin>ymax:continue
        a,b,c=tri
        den=(b[1]-c[1])*(a[0]-c[0])+(c[0]-b[0])*(a[1]-c[1])
        if abs(den)<1e-10:continue
        yy,xx=np.mgrid[ymin:ymax+1,xmin:xmax+1];xx=xx+.5;yy=yy+.5
        u=((b[1]-c[1])*(xx-c[0])+(c[0]-b[0])*(yy-c[1]))/den
        v=((c[1]-a[1])*(xx-c[0])+(a[0]-c[0])*(yy-c[1]))/den
        w=1-u-v
        z=u*a[2]+v*b[2]+w*c[2]
        region=depth[ymin:ymax+1,xmin:xmax+1]
        mask=(u>=-1e-9)&(v>=-1e-9)&(w>=-1e-9)&(z>region)
        if not mask.any():continue
        normal=np.cross(source[1]-source[0],source[2]-source[0]);length=np.linalg.norm(normal)
        shade=.55+.45*abs(normal@light/length) if length else .7
        region[mask]=z[mask]
        rgb[ymin:ymax+1,xmin:xmax+1][mask]=np.clip(np.array(to_rgb(color))*shade*255,0,255).astype(np.uint8)
    return rgb
