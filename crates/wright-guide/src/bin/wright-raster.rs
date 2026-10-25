//! Bounded display-only raster worker. Input: u32 LE width,height,count followed
//! by 21 f64 LE values per triangle (screen xyz, vertex normals, RGB colour).
//! Output: packed RGB. Geometry projection/tessellation remains in FreeCAD.
use std::io::{self, Read, Write};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    io::stdin().take(84_000_013).read_to_end(&mut bytes)?;
    if bytes.len() < 12 { return Err("short header".into()); }
    let num = |i| u32::from_le_bytes(bytes[i..i+4].try_into().unwrap()) as usize;
    let (w,h,count) = (num(0),num(4),num(8));
    if w == 0 || h == 0 || w > 2048 || h > 2048 || w*h > 4_194_304 || count > 500_000 || bytes.len() != 12+count*168 {
        return Err("raster budget or payload mismatch".into());
    }
    let mut rgb = vec![0u8;w*h*3];
    for pixel in rgb.chunks_exact_mut(3) { pixel.copy_from_slice(&[239,242,245]); }
    let mut depth = vec![f64::NEG_INFINITY;w*h];
    let light_norm = (0.35f64.powi(2)+0.45f64.powi(2)+0.82f64.powi(2)).sqrt();
    let light = [-0.35/light_norm,-0.45/light_norm,0.82/light_norm];
    let mut work = 0usize;
    for chunk in bytes[12..].chunks_exact(168) {
        let mut t = [0f64;21];
        for (i,v) in t.iter_mut().enumerate() { *v = f64::from_le_bytes(chunk[i*8..i*8+8].try_into().unwrap()); }
        if t.iter().any(|v| !v.is_finite()) { return Err("nonfinite triangle".into()); }
        let xmin=t[0].min(t[3]).min(t[6]).floor().max(0.) as usize;
        let xmax=t[0].max(t[3]).max(t[6]).ceil().min((w-1) as f64);
        let ymin=t[1].min(t[4]).min(t[7]).floor().max(0.) as usize;
        let ymax=t[1].max(t[4]).max(t[7]).ceil().min((h-1) as f64);
        if xmax < xmin as f64 || ymax < ymin as f64 {continue;}
        let (xmax,ymax)=(xmax as usize,ymax as usize);
        work += (xmax-xmin+1)*(ymax-ymin+1);
        if work > 300_000_000 {return Err("projected pixel budget".into());}
        let den=(t[4]-t[7])*(t[0]-t[6])+(t[6]-t[3])*(t[1]-t[7]);
        if den.abs()<1e-10 {continue;}
        for y in ymin..=ymax { for x in xmin..=xmax {
            let (px,py)=(x as f64+0.5,y as f64+0.5);
            let u=((t[4]-t[7])*(px-t[6])+(t[6]-t[3])*(py-t[7]))/den;
            let v=((t[7]-t[1])*(px-t[6])+(t[0]-t[6])*(py-t[7]))/den;
            let b=1.-u-v;
            let z=u*t[2]+v*t[5]+b*t[8]; let idx=y*w+x;
            if u < -1e-9 || v < -1e-9 || b < -1e-9 || z <= depth[idx] {continue;}
            let n=[u*t[9]+v*t[12]+b*t[15],u*t[10]+v*t[13]+b*t[16],u*t[11]+v*t[14]+b*t[17]];
            let len=(n[0]*n[0]+n[1]*n[1]+n[2]*n[2]).sqrt().max(1e-15);
            let shade=0.43+0.57*((n[0]*light[0]+n[1]*light[1]+n[2]*light[2])/len).abs();
            depth[idx]=z;
            for c in 0..3 {rgb[idx*3+c]=(t[18+c]*shade).clamp(0.,255.) as u8;}
        }}
    }
    io::stdout().write_all(&rgb)?; Ok(())
}
fn main() {if let Err(error)=run() {eprintln!("{error}");std::process::exit(1);}}
