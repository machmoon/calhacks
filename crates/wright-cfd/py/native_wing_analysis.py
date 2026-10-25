"""AeroSandbox/NeuralFoil + OptVL analysis of Wright native section exports.

Run with the analysis Python, not FreeCAD's embedded Python:
  python -I native_wing_analysis.py input.json output-directory
"""
import csv
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path
import sys
import numpy as np
import aerosandbox as asb
from optvl import OVLSolver


def finite(value, name, low, high):
    if isinstance(value,bool) or not isinstance(value,(int,float)) or not math.isfinite(value) or not low<=value<=high:
        raise ValueError(f'{name} must be finite in{low}..{high}')
    return float(value)


def validate(case):
    g=case['geometry']
    if g.get('schema_version')!=1 or not isinstance(g.get('symmetric'),bool):
        raise ValueError('unsupported geometry schema')
    sections=g['sections']
    if not 2<=len(sections)<=16:raise ValueError('requires2..16 sections')
    previous=None
    for s in sections:
        if len(s['xyz_le_m'])!=3:raise ValueError('invalid section origin')
        for p in s['xyz_le_m']:finite(p,'coordinate',-1000,1000)
        y=s['xyz_le_m'][1]
        if previous is not None and y-previous<=1e-6:raise ValueError('unordered/duplicate span stations')
        previous=y
        finite(s['chord_m'],'chord',.0001,10);finite(s['twist_deg'],'twist',-30,30)
        coordinates=s['airfoil_coordinates']
        if not 32<=len(coordinates)<=258:raise ValueError('airfoil sampling budget exceeded')
        for p in coordinates:
            if len(p)!=2:raise ValueError('invalid airfoil coordinates')
            finite(p[0],'airfoil x',-.05,1.05);finite(p[1],'airfoil z',-.5,.5)
    if g['symmetric'] and (abs(sections[0]['xyz_le_m'][1])>1e-6 or previous<=0):
        raise ValueError('symmetric wing must begin on the center plane')
    velocity=finite(case.get('velocity_m_s',20),'velocity',1,100)
    altitude=finite(case.get('altitude_m',0),'altitude',0,20000)
    alphas=case.get('alphas_deg',[0,4,8])
    if not isinstance(alphas,list) or not 1<=len(alphas)<=5:raise ValueError('requires1..5 angles')
    alphas=[finite(a,'alpha',-10,12) for a in alphas]
    if len(set(alphas))!=len(alphas):raise ValueError('angles must be unique')
    return g,velocity,altitude,alphas


def airplane(g):
    sections=[asb.WingXSec(xyz_le=s['xyz_le_m'],chord=s['chord_m'],twist=s['twist_deg'],
        airfoil=asb.Airfoil(name='NativeSection'+str(i),coordinates=np.asarray(s['airfoil_coordinates'])))
        for i,s in enumerate(g['sections'])]
    wing=asb.Wing(name='NativeWing',xsecs=sections,symmetric=g['symmetric'])
    reference=np.asarray(sections[0].xyz_le)+[sections[0].chord/4,0,0]
    plane=asb.Airplane(name='Wright native wing',xyz_ref=reference.tolist(),wings=[wing])
    return plane,wing


def avl_file(plane,g,path):
    lines=['Wright native wing','0','0 0 0',f'{float(plane.s_ref):.12g} {float(plane.c_ref):.12g} {float(plane.b_ref):.12g}',
        ' '.join(str(float(v)) for v in plane.xyz_ref),'0','SURFACE','NativeWing','8 1 64 1']
    if g['symmetric']:lines+=['YDUPLICATE','0']
    for s in g['sections']:
        lines+=['SECTION',' '.join(str(v) for v in s['xyz_le_m']+[s['chord_m'],s['twist_deg']]),'AIRFOIL']
        lines+=[' '.join(f'{v:.12g}' for v in p) for p in s['airfoil_coordinates']]
    path.write_text('\n'.join(lines)+'\n')


def run(case,output):
    g,velocity,altitude,alphas=validate(case)
    output=Path(output).resolve();output.mkdir(parents=True,exist_ok=True)
    plane,wing=airplane(g);atmosphere=asb.Atmosphere(altitude=altitude)
    # Independently record the coordinate change introduced by the solver's
    # section frames; source sections lie in XZ planes, solver frames can tilt.
    errors=[]
    for i,s in enumerate(g['sections']):
        if 'world_samples_m' in s:
            expected=np.asarray(s['world_samples_m'],dtype=float)
            actual=np.array([wing._compute_xyz_of_WingXSec(i,p[0],p[1]) for p in s['airfoil_coordinates']])
            if expected.shape!=actual.shape:raise ValueError('source sample count mismatch')
            errors.extend(np.linalg.norm(expected-actual,axis=1).tolist())
    frame_error=max(errors,default=0)*1000
    if frame_error>.25:raise ValueError(f'section-frame approximation exceeds0.25mm: {frame_error}')
    rows=[];panel_rows=[];convergence=[]
    for alpha in alphas:
        meshes=[]
        for label,span,chord in [('coarse',3,4),('fine',5,6)]:
            analysis=asb.VortexLatticeMethod(airplane=plane,
                op_point=asb.OperatingPoint(atmosphere=atmosphere,velocity=velocity,alpha=alpha),
                spanwise_resolution=span,chordwise_resolution=chord)
            result=analysis.run();forces=np.asarray(analysis.forces_geometry,dtype=float)
            centers=np.asarray(analysis.vortex_centers,dtype=float)
            if len(forces)>1800 or not np.isfinite(forces).all():raise ValueError('invalid/unbounded panel solution')
            values={key:float(result[key]) for key in ['CL','CD','Cm','L','D']}
            if not all(math.isfinite(v) for v in values.values()):raise ValueError('nonfinite aerodynamic coefficients')
            right=centers[:,1]>0
            values.update(mesh=label,panels=len(forces),right_vertical_force_n=float(forces[right,2].sum()),
                right_root_vertical_bending_nm=float((forces[right,2]*centers[right,1]).sum()))
            meshes.append(values)
            if label=='fine':
                for center,force in zip(centers,forces):panel_rows.append(dict(alpha_deg=alpha,x_m=float(center[0]),y_m=float(center[1]),z_m=float(center[2]),fx_n=float(force[0]),fy_n=float(force[1]),fz_n=float(force[2])))
        coarse,fine=meshes
        delta_cl=abs(fine['CL']-coarse['CL']);delta_cd=abs(fine['CD']-coarse['CD'])
        convergence.append(dict(alpha_deg=alpha,delta_CL=delta_cl,delta_CDi=delta_cd,
            passed=delta_cl<=.01+.05*abs(fine['CL']) and delta_cd<=.0005+.1*abs(fine['CD']),
            criterion='CL absolute0.01+5%; induced CD absolute0.0005+10%; two meshes only, not proof of asymptotic convergence'))
        rows.append(dict(alpha_deg=alpha,aerosandbox=fine,coarse=coarse))
    avl_path=output/'native-wing.avl';avl_file(plane,g,avl_path)
    solver=OVLSolver(geo_file=str(avl_path),debug=False);solver.set_parameter('Mach',0)
    comparisons=[]
    for row in rows:
        solver.set_variable('alpha',row['alpha_deg']);solver.execute_run();total=solver.get_total_forces()
        row['avl']={k:float(total[k]) for k in ['CL','CDi','CDff','Cm']}
        if not all(math.isfinite(v) for v in row['avl'].values()):raise ValueError('nonfinite AVL result')
        a=row['aerosandbox'];b=row['avl']
        comparisons.append(dict(alpha_deg=row['alpha_deg'],delta_CL=abs(a['CL']-b['CL']),
            delta_induced_CD=abs(a['CD']-b['CDi']),
            passed=abs(a['CL']-b['CL'])<=.02+.05*abs(b['CL']) and abs(a['CD']-b['CDi'])<=.002+.2*abs(b['CDi']),
            criterion='CL absolute0.02+5%; induced CD absolute0.002+20%; agreement does not validate shared modeling assumptions'))
    # 2D section polars are separate from 3D VLM; do not sum them into total drag.
    polars=[]
    for i in sorted({0,len(wing.xsecs)//2,len(wing.xsecs)-1}):
        section=wing.xsecs[i];re=float(velocity*section.chord/atmosphere.kinematic_viscosity())
        polar=section.airfoil.get_aero_from_neuralfoil(alpha=np.asarray(alphas)+section.twist,Re=re,mach=0)
        polars.append(dict(section_index=i,Re=re,scope='2D isolated section; no finite-wing downwash correction',
            values={k:np.asarray(polar[k]).reshape(-1).tolist() for k in ['CL','CD','CM','analysis_confidence']}))
    with (output/'panel-loads.csv').open('w') as f:
        writer=csv.DictWriter(f,fieldnames=list(panel_rows[0]));writer.writeheader();writer.writerows(panel_rows)
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    fig,axes=plt.subplots(2,2,figsize=(11,8),layout='constrained')
    le=np.array([s.xyz_le for s in wing.xsecs]);te=np.array([wing._compute_xyz_te_of_WingXSec(i) for i in range(len(wing.xsecs))])
    for sign in ([1,-1] if g['symmetric'] else [1]):
        axes[0,0].plot(sign*le[:,1],le[:,0],color='#216e9b');axes[0,0].plot(sign*te[:,1],te[:,0],color='#216e9b')
    axes[0,0].set(title='CAD-derived analysis planform',xlabel='Spanwise y [m]',ylabel='Aft x [m]');axes[0,0].invert_yaxis();axes[0,0].set_aspect('equal')
    for key,other,ax,title in [('CL','CL',axes[0,1],'Lift coefficient'),('CD','CDi',axes[1,0],'Inviscid induced drag coefficient')]:
        ax.plot(alphas,[r['aerosandbox'][key] for r in rows],'o-',label='AeroSandbox');ax.plot(alphas,[r['avl'][other] for r in rows],'s--',label='AVL / OptVL')
        ax.set(title=title,xlabel='Angle of attack [deg]',ylabel=title);ax.legend()
    selected=alphas[len(alphas)//2];panels=[p for p in panel_rows if p['alpha_deg']==selected and p['y_m']>0]
    ys=np.linspace(0,float(le[-1,1]),100)
    shear=[sum(p['fz_n'] for p in panels if p['y_m']>=y) for y in ys]
    axes[1,1].plot(ys,shear);axes[1,1].set(title=f'Right-wing vertical shear, alpha={selected:g}°',xlabel='Spanwise y [m]',ylabel='Outboard vertical force [N]')
    for ax in axes.flat:ax.grid(alpha=.2)
    fig.suptitle('Wright native-wing analysis — reduced, inviscid wing model\nNo fuselage/tail/rotor wash; not a flight or structural approval',fontsize=12)
    fig.savefig(output/'wing-analysis.png',dpi=150);plt.close(fig)
    report=dict(status='analysis_complete',solvers={name:importlib.metadata.version(name) for name in ['aerosandbox','neuralfoil','optvl','numpy']},
        runner_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),geometry=g,
        references=dict(area_m2=float(plane.s_ref),chord_m=float(plane.c_ref),span_m=float(plane.b_ref),moment_reference_m=[float(x) for x in plane.xyz_ref]),
        operating_point=dict(velocity_m_s=velocity,altitude_m=altitude,density_kg_m3=float(atmosphere.density()),alphas_deg=alphas,solver_mach=0),
        sampled_section_frame_error_mm=frame_error,rows=rows,mesh_convergence=convergence,cross_solver=comparisons,section_polars=polars,
        scope='Steady incompressible inviscid wing-only VLM. CD is induced drag, not total aircraft drag. '
              '2D NeuralFoil polars are separate estimates with reported confidence. No stall, propwash, '
              'aircraft stability, structural strength or flight approval. CAD smooth spanwise surfaces are '
              'approximated between source sections. Panel loads omit weight/inertia and attachment flexibility.',
        artifacts=[dict(label='Native-wing analysis report',path=str(output/'analysis.json')),
                   dict(label='Wing analysis plots',path=str(output/'wing-analysis.png')),
                   dict(label='Panel loads for structural analysis',path=str(output/'panel-loads.csv'))])
    (output/'analysis.json').write_text(json.dumps(report,indent=2,allow_nan=False))
    return report


if __name__=='__main__':
    run(json.loads(Path(sys.argv[1]).read_text()),sys.argv[2])
