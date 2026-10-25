"""Wright VTOL: 6-DOF flight simulation in JSBSim, aerodynamics from AeroSandbox.

    python -I vtol_sim.py INPUTS.json OUT_DIR DATA_AIR_DIR

Nothing here is a new flight-dynamics model:

* JSBSim (JSBSim-Team/jsbsim, pip `jsbsim`) integrates the 6-DOF rigid body,
  the props (FGPropeller: CT/CP tables over advance ratio, reaction torque,
  gyroscopics), the electric motors (FGElectric: shaft power = max x
  throttle), the ground contacts and the atmosphere. The aircraft file
  follows the multirotor model ArduPilot ships for JSBSim SITL
  (Tools/autotest/aircraft/arducopter/arducopter.xml: electric engines,
  propeller thrusters pitched 90 deg for lift).
* The aerodynamic tables are AeroSandbox's AeroBuildup (peterdsharpe/
  AeroSandbox, aerosandbox/aerodynamics/aero_3D/aero_buildup.py, NeuralFoil
  section polars) run on the design's geometry, with its own finite-
  difference stability derivatives (`run_with_stability_derivatives`) and
  control-surface deflections.
* The flight logic follows ArduPlane's QuadPlane: SLT_Transition::update()
  in ArduPlane/quadplane.cpp (AIRSPEED_WAIT: hold the hover until airspeed
  exceeds AIRSPEED_MIN; TIMER: lift throttle scaled linearly to zero over
  Q_TRANSITION_MS; DONE: fixed wing), Q_TRANS_DECEL for the back
  transition, Q_LAND_SPEED for the final descent. The loops themselves are
  plain cascaded PIDs (angle P -> rate PID, AC_AttitudeControl's shape),
  tuned here by hand, not ArduPilot's code.

What it does NOT model: motor/ESC dynamics beyond JSBSim's power lag,
battery sag (the analytic model owns energy), prop-wing interference,
ground effect, wind. Battery power is shaft power over the analytic model's
motor x ESC efficiency at hover and at cruise.
"""

import json
import math
import os
import sys
import time

import numpy as np

IN, OUT, DATA = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(OUT, exist_ok=True)
X = json.load(open(IN))
AIRFOILS = json.load(open(os.path.join(DATA, "airfoils.json")))["airfoils"]

import aerosandbox as asb  # noqa: E402
import jsbsim  # noqa: E402

FT = 0.3048
LBF = 4.4482216
HP = 745.699872
G = 9.80665
RHO = X["air"]["rho"]

# --------------------------------------------------------------- geometry


def airfoil(id_):
    a = AIRFOILS.get(id_)
    if a is None:
        return asb.Airfoil(id_)  # NACA ids resolve inside AeroSandbox
    return asb.Airfoil(id_, coordinates=np.array(a["coordinates"]))


def airplane(de=0.0, da=0.0, dr=0.0):
    """The design as an AeroSandbox Airplane; deflections in degrees
    (elevator +TE down, aileron +right TE down, rudder +TE left)."""
    w, h, v = X["wing"], X["htail"], X["vtails"]
    af = airfoil(w["airfoil"])
    semi = w["span"] / 2
    a0, a1 = w["aileron_y"]

    def chord(y):
        return w["root_chord"] + (w["tip_chord"] - w["root_chord"]) * y / semi

    def twist(y):
        return w["incidence_deg"] - w["washout_deg"] * y / semi

    ail = [asb.ControlSurface("aileron", symmetric=False, deflection=da, hinge_point=1 - w["aileron_chord_frac"])]
    ys = [0.0, a0, a1, semi]
    xsecs = []
    for i, y in enumerate(ys):
        cs = ail if i == 1 else []
        xsecs.append(asb.WingXSec(xyz_le=[0.0, y, 0.0], chord=chord(y), twist=twist(y), airfoil=af, control_surfaces=cs))
    wing = asb.Wing("wing", xsecs=xsecs, symmetric=True)
    taf = airfoil(h["airfoil"])
    elev = [asb.ControlSurface("elevator", symmetric=True, deflection=de, hinge_point=1 - h["elevator_chord_frac"])]
    htail = asb.Wing("htail", symmetric=True, xsecs=[
        asb.WingXSec(xyz_le=[h["x_le"], 0, h["z"]], chord=h["chord"], twist=0, airfoil=taf, control_surfaces=elev),
        asb.WingXSec(xyz_le=[h["x_le"], h["span"] / 2, h["z"]], chord=h["chord"], twist=0, airfoil=taf),
    ])
    vts = []
    for side in (1, -1):
        rud = [asb.ControlSurface("rudder", symmetric=True, deflection=dr, hinge_point=1 - v["rudder_chord_frac"])]
        vts.append(asb.Wing(f"vtail_{side}", symmetric=False, xsecs=[
            asb.WingXSec(xyz_le=[v["x_le"], side * v["y"], v["z_root"]], chord=v["chord"], twist=0, airfoil=taf, control_surfaces=rud),
            asb.WingXSec(xyz_le=[v["x_le"], side * v["y"], v["z_root"] + v["height"]], chord=v["chord"], twist=0, airfoil=taf),
        ]))
    p = X["pod"]
    L = p["x1"] - p["x0"]
    prof = [(0.0, 0.05), (0.08, 0.6), (0.2, 0.92), (0.35, 1.0), (0.65, 1.0), (0.85, 0.7), (1.0, 0.25)]
    pod = asb.Fuselage("pod", xsecs=[asb.FuselageXSec(xyz_c=[p["x0"] + f * L, 0, p["zc"]], width=p["width"] * r, height=p["height"] * r, shape=3.0) for f, r in prof])
    b = X["booms"]
    booms = [asb.Fuselage(f"boom_{s}", xsecs=[asb.FuselageXSec(xyz_c=[x, s * b["y"], b["z"]], radius=b["od"] / 2) for x in (b["x0"], b["x1"])]) for s in (1, -1)]
    return asb.Airplane("wright", xyz_ref=X["mass"]["cg"], wings=[wing, htail] + vts, fuselages=[pod] + booms,
                        s_ref=w["area"], c_ref=w["mac"], b_ref=w["span"])


EXTRA_CD = sum(X["extra_drag_area_m2"][k] for k in ("motors", "stopped_props", "legs", "excrescence")) / X["wing"]["area"]


def buildup(ap, V, alpha, beta=0.0, deriv=False):
    op = asb.OperatingPoint(atmosphere=asb.Atmosphere(altitude=X["air"]["altitude_m"]), velocity=V, alpha=alpha, beta=beta)
    ab = asb.AeroBuildup(ap, op, xyz_ref=X["mass"]["cg"])
    return ab.run_with_stability_derivatives() if deriv else ab.run()


# ------------------------------------------------------------ aero tables


def f1(v):
    return float(np.ravel(np.asarray(v, dtype=float))[0])


V_CR = X["analytic"]["best_range_ms"] or 18.0
t0 = time.time()
ALPHA = np.concatenate([np.arange(-180, -30, 15), np.arange(-30, 30, 2), np.arange(30, 181, 15)]).astype(float)
a_tr = X["analytic"]["cruise_alpha_deg"] or 3.0
step = 5.0
import hashlib  # noqa: E402

KEY = hashlib.sha256(open(IN, "rb").read() + open(__file__, "rb").read().split(b"# ---- end aero")[0]).hexdigest()
CACHE = os.path.join(OUT, "aero_tables.json")
cached = json.load(open(CACHE)) if os.path.exists(CACHE) else {}
if cached.get("key") == KEY:
    tab = {k: np.array(v) for k, v in cached["tab"].items()}
    D, CTL = cached["D"], cached["CTL"]
else:
    AP0 = airplane()
    base = buildup(AP0, V_CR, ALPHA)
    tab = {k: np.asarray(base[k], dtype=float) + (EXTRA_CD if k == "CD" else 0.0) for k in ("CL", "CD", "Cm")}
    # the small-angle derivatives at the cruise trim alpha
    der = buildup(AP0, V_CR, a_tr, deriv=True)
    D = {k: f1(der[k]) for k in ("CYb", "Clb", "Cnb", "Clp", "Cnp", "CYp", "Cmq", "CLq", "Clr", "Cnr", "CYr")}

    def dctl(**kw):
        up = buildup(airplane(**{k: step for k in kw}), V_CR, a_tr)
        dn = buildup(airplane(**{k: -step for k in kw}), V_CR, a_tr)
        return {c: (f1(up[c]) - f1(dn[c])) / math.radians(2 * step) for c in ("CL", "CD", "CY", "Cl", "Cm", "Cn")}

    CTL = {"de": dctl(de=1), "da": dctl(da=1), "dr": dctl(dr=1)}
    json.dump({"key": KEY, "alpha": ALPHA.tolist(), "tab": {k: v.tolist() for k, v in tab.items()}, "D": D, "CTL": CTL, "V_ms": V_CR, "alpha_trim_deg": a_tr}, open(CACHE, "w"))
# ---- end aero
aero_s = time.time() - t0
print(f"aero tables {aero_s:.0f} s; controls per rad {CTL}", file=sys.stderr)

# ------------------------------------------------------------- JSBSim XML


def table1(prop, rows):
    body = "\n".join(f"          {a:9.3f} {b:10.5f}" for a, b in rows)
    return f"""<table><independentVar lookup="row">{prop}</independentVar><tableData>
{body}
        </tableData></table>"""


def prop_xml(name, p, blades, ixx):
    rows = p["j_ct_cp"]
    ct = "\n".join(f"      {j:6.3f} {c:9.5f}" for j, c, _ in rows) + f"\n      {4.0:6.3f} {rows[-1][1] - 0.1:9.5f}"
    cp = "\n".join(f"      {j:6.3f} {c:9.5f}" for j, _, c in rows) + f"\n      {4.0:6.3f} {rows[-1][2] - 0.05:9.5f}"
    return f"""<?xml version="1.0"?>
<propeller name="{name}">
  <ixx unit="KG*M2"> {ixx:.3e} </ixx>
  <diameter unit="M"> {p["diameter_m"]:.4f} </diameter>
  <numblades> {blades} </numblades>
  <gearratio> 1.0 </gearratio>
  <!-- measured CT/CP over J at {p["rpm"]:.0f} rpm: Wright prop data {p["data_id"]} (UIUC / maker) -->
  <table name="C_THRUST" type="internal"><tableData>
{ct}
  </tableData></table>
  <table name="C_POWER" type="internal"><tableData>
{cp}
  </tableData></table>
</propeller>
"""


def loc(x, y, z):
    return f"<location unit=\"M\"><x>{x:.4f}</x><y>{y:.4f}</y><z>{z:.4f}</z></location>"


def aircraft_xml():
    m, w = X["mass"], X["wing"]
    cg = m["cg"]
    k = m["kg"] * G / (4 * 0.01)  # 1 cm static squash per leg
    contacts = "\n".join(f"""    <contact type="BOGEY" name="leg{i}">{loc(l['x'], l['y'], l['z'])}
      <static_friction>0.8</static_friction><dynamic_friction>0.5</dynamic_friction><rolling_friction>0.5</rolling_friction>
      <spring_coeff unit="N/M">{k:.0f}</spring_coeff><damping_coeff unit="N/M/SEC">{2 * math.sqrt(k * m['kg'] / 4):.1f}</damping_coeff>
      <max_steer unit="DEG">0</max_steer><brake_group>NONE</brake_group><retractable>0</retractable>
    </contact>""" for i, l in enumerate(X["legs"]))
    engines = []
    for i, r in enumerate(X["lift"]["rotors"]):
        engines.append(f"""    <engine file="lift_motor" name="lift{i}">{loc(r['x'], r['y'], r['z'])}
      <orient unit="DEG"><pitch>90</pitch><roll>0</roll><yaw>0</yaw></orient><feed>0</feed>
      <thruster file="lift_prop">{loc(r['x'], r['y'], r['z'])}<orient unit="DEG"><pitch>90</pitch><roll>0</roll><yaw>0</yaw></orient><sense>{r['sense']}</sense></thruster>
    </engine>""")
    p = X["pusher"]
    engines.append(f"""    <engine file="pusher_motor" name="pusher">{loc(p['x'], p['y'], p['z'])}
      <orient unit="DEG"><pitch>0</pitch><roll>0</roll><yaw>0</yaw></orient><feed>0</feed>
      <thruster file="pusher_prop">{loc(p['x'], p['y'], p['z'])}<orient unit="DEG"><pitch>0</pitch><roll>0</roll><yaw>0</yaw></orient><sense>1</sense></thruster>
    </engine>""")
    a = ALPHA
    cl = table1("aero/alpha-deg", zip(a, tab["CL"]))
    cd = table1("aero/alpha-deg", zip(a, tab["CD"]))
    cm = table1("aero/alpha-deg", zip(a, tab["Cm"]))

    def lin(name, coef, *props):
        ps = "".join(f"<property>{q}</property>" for q in props)
        return f"""      <function name="aero/coefficient/{name}"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property>{ps}<value>{coef:.6f}</value></product></function>"""

    # the linear terms fade out of the attached-flow range (|alpha| > 25 deg)
    fade = "aero/attached"
    b, c = "metrics/bw-ft", "metrics/cbarw-ft"
    return f"""<?xml version="1.0"?>
<fdm_config name="wright_vtol" version="2.0" release="ALPHA">
  <fileheader><author>Wright (generated)</author><description>{X['name']}: geometry, mass and props from Wright; aerodynamics from AeroSandbox AeroBuildup</description></fileheader>
  <metrics>
    <wingarea unit="M2">{w['area']:.5f}</wingarea><wingspan unit="M">{w['span']:.4f}</wingspan><chord unit="M">{w['mac']:.4f}</chord>
    <htailarea unit="M2">0</htailarea><htailarm unit="M">0</htailarm><vtailarea unit="M2">0</vtailarea><vtailarm unit="M">0</vtailarm>
    <location name="AERORP" unit="M"><x>{cg[0]:.4f}</x><y>0</y><z>{cg[2]:.4f}</z></location>
    <location name="EYEPOINT" unit="M"><x>0</x><y>0</y><z>0</z></location>
    <location name="VRP" unit="M"><x>0</x><y>0</y><z>0</z></location>
  </metrics>
  <mass_balance>
    <ixx unit="KG*M2">{m['ixx']:.5f}</ixx><iyy unit="KG*M2">{m['iyy']:.5f}</iyy><izz unit="KG*M2">{m['izz']:.5f}</izz>
    <emptywt unit="KG">{m['kg']:.4f}</emptywt>
    <location name="CG" unit="M"><x>{cg[0]:.4f}</x><y>0</y><z>{cg[2]:.4f}</z></location>
  </mass_balance>
  <ground_reactions>
{contacts}
  </ground_reactions>
  <propulsion>
{chr(10).join(engines)}
    <tank type="FUEL" number="0">{loc(cg[0], 0, cg[2])}<capacity unit="KG">0.0000001</capacity><contents unit="KG">0</contents></tank>
  </propulsion>
  <aerodynamics>
    <function name="aero/attached"><table><independentVar lookup="row">aero/alpha-deg</independentVar><tableData>
      -40 0
      -25 1
       25 1
       40 0
    </tableData></table></function>
    <axis name="LIFT">
      <function name="aero/coefficient/CLalpha"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property>{cl}</product></function>
{lin('CLde', CTL['de']['CL'], 'fcs/elevator-pos-rad', fade)}
      <function name="aero/coefficient/CLq"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>aero/ci2vel</property><property>velocities/q-aero-rad_sec</property><property>{fade}</property><value>{D['CLq']:.6f}</value></product></function>
    </axis>
    <axis name="DRAG">
      <function name="aero/coefficient/CDalpha"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property>{cd}</product></function>
      <function name="aero/coefficient/CDbeta"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><abs><property>aero/beta-rad</property></abs><value>0.3</value></product></function>
    </axis>
    <axis name="SIDE">
{lin('CYb', D['CYb'], 'aero/beta-rad')}
{lin('CYdr', CTL['dr']['CY'], 'fcs/rudder-pos-rad', fade)}
    </axis>
    <axis name="ROLL">
      <function name="aero/coefficient/Clb"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{b}</property><property>aero/beta-rad</property><value>{D['Clb']:.6f}</value></product></function>
      <function name="aero/coefficient/Clp"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{b}</property><property>aero/bi2vel</property><property>velocities/p-aero-rad_sec</property><value>{D['Clp']:.6f}</value></product></function>
      <function name="aero/coefficient/Clr"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{b}</property><property>aero/bi2vel</property><property>velocities/r-aero-rad_sec</property><property>{fade}</property><value>{D['Clr']:.6f}</value></product></function>
      <function name="aero/coefficient/Clda"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{b}</property><property>fcs/left-aileron-pos-rad</property><property>{fade}</property><value>{CTL['da']['Cl']:.6f}</value></product></function>
    </axis>
    <axis name="PITCH">
      <function name="aero/coefficient/Cmalpha"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{c}</property>{cm}</product></function>
      <function name="aero/coefficient/Cmq"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{c}</property><property>aero/ci2vel</property><property>velocities/q-aero-rad_sec</property><value>{D['Cmq']:.6f}</value></product></function>
      <function name="aero/coefficient/Cmde"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{c}</property><property>fcs/elevator-pos-rad</property><property>{fade}</property><value>{CTL['de']['Cm']:.6f}</value></product></function>
    </axis>
    <axis name="YAW">
      <function name="aero/coefficient/Cnb"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{b}</property><property>aero/beta-rad</property><value>{D['Cnb']:.6f}</value></product></function>
      <function name="aero/coefficient/Cnr"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{b}</property><property>aero/bi2vel</property><property>velocities/r-aero-rad_sec</property><value>{D['Cnr']:.6f}</value></product></function>
      <function name="aero/coefficient/Cnp"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{b}</property><property>aero/bi2vel</property><property>velocities/p-aero-rad_sec</property><property>{fade}</property><value>{D['Cnp']:.6f}</value></product></function>
      <function name="aero/coefficient/Cndr"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{b}</property><property>fcs/rudder-pos-rad</property><property>{fade}</property><value>{CTL['dr']['Cn']:.6f}</value></product></function>
      <function name="aero/coefficient/Cnda"><product><property>aero/qbar-psf</property><property>metrics/Sw-sqft</property><property>{b}</property><property>fcs/left-aileron-pos-rad</property><property>{fade}</property><value>{CTL['da']['Cn']:.6f}</value></product></function>
    </axis>
  </aerodynamics>
</fdm_config>
"""


ROOT = os.path.join(OUT, "jsbsim")
os.makedirs(os.path.join(ROOT, "aircraft", "wright_vtol"), exist_ok=True)
os.makedirs(os.path.join(ROOT, "engine"), exist_ok=True)
open(os.path.join(ROOT, "aircraft", "wright_vtol", "wright_vtol.xml"), "w").write(aircraft_xml())
LIFT_W = X["lift"]["max_shaft_w"]
PUSH_W = X["pusher"]["max_shaft_w"]
for name, wmax in (("lift_motor", LIFT_W), ("pusher_motor", PUSH_W)):
    open(os.path.join(ROOT, "engine", name + ".xml"), "w").write(f'<?xml version="1.0"?>\n<electric_engine name="{name}"><power unit="WATTS">{wmax:.1f}</power></electric_engine>\n')
for name, part in (("lift_prop", X["lift"]), ("pusher_prop", X["pusher"])):
    d = part["prop"]["diameter_m"]
    ixx = 0.02 * d * d / 12 * 2  # ~20 g blade pair as a rod (ASSUMPTION)
    open(os.path.join(ROOT, "engine", name + ".xml"), "w").write(prop_xml(name, part["prop"], part["blades"], ixx))

# ------------------------------------------------------------- simulation

DT = 1.0 / 400.0
fdm = jsbsim.FGFDMExec(ROOT, None)
fdm.set_debug_level(0)
fdm.load_model("wright_vtol")
fdm.set_dt(DT)
# resting on the legs: h-agl is the CG height; legs sit this far below it, 1 cm squash
fdm["ic/h-agl-ft"] = (cg0 := X["mass"]["cg"][2] - min(l["z"] for l in X["legs"]) - 0.01) / FT
fdm["ic/terrain-elevation-ft"] = X["air"]["altitude_m"] / FT
fdm["ic/u-fps"] = fdm["ic/v-fps"] = fdm["ic/w-fps"] = 0.0
fdm["ic/theta-deg"] = fdm["ic/phi-deg"] = 0.0
fdm["ic/psi-true-deg"] = 0.0
fdm.run_ic()
for i in range(5):
    fdm[f"propulsion/engine[{i}]/set-running"] = 1


class PID:
    def __init__(s, kp, ki=0.0, kd=0.0, lim=1e9, ilim=None):
        s.kp, s.ki, s.kd, s.lim = kp, ki, kd, lim
        s.ilim = ilim if ilim is not None else lim
        s.i = 0.0
        s.prev = None

    def __call__(s, e, dt, de=None):
        s.i = max(-s.ilim, min(s.ilim, s.i + e * dt * s.ki))
        if de is None:
            de = 0.0 if s.prev is None else (e - s.prev) / dt
        s.prev = e
        return max(-s.lim, min(s.lim, s.kp * e + s.i + s.kd * de))

    def reset(s):
        s.i, s.prev = 0.0, None


M = X["mass"]["kg"]
W = M * G
ar = X["ardupilot"]
V_MIN = ar["AIRSPEED_MIN"] or 1.2 * X["analytic"]["stall_eas_ms"]
V_CRUISE = V_CR
T_TRANS = ar["Q_TRANSITION_MS"] / 1000.0
DECEL = ar["Q_TRANS_DECEL"]
LAND_V = ar["Q_LAND_SPEED_CMS"] / 100.0
LAND_FINAL_ALT = 6.0  # Q_LAND_FINAL_ALT default (quadplane.cpp)
ALT = 30.0
# lift: static thrust per unit at full throttle, and P ~ T^1.5 (momentum and CT/CP at J=0)
lp = X["lift"]["prop"]
ct0, cp0 = lp["j_ct_cp"][0][1], lp["j_ct_cp"][0][2]
dl = lp["diameter_m"]
n_max = (LIFT_W / (cp0 * RHO * dl ** 5)) ** (1 / 3)
T_MAX = ct0 * RHO * n_max ** 2 * dl ** 4
rot = X["lift"]["rotors"]
cg = X["mass"]["cg"]
arm_r = [r["y"] for r in rot]  # +y right: a right rotor's thrust rolls left
arm_p = [cg[0] - r["x"] for r in rot]  # x aft: a front rotor (x < cg) pitches up
sense = [r["sense"] for r in rot]

# hover loops (angle P -> rate PID), per unit thrust fractions
roll_rate = PID(0.25, 0.05, 0.004, lim=0.3, ilim=0.1)
pitch_rate = PID(0.25, 0.05, 0.008, lim=0.3, ilim=0.1)
yaw_rate = PID(0.6, 0.1, 0.0, lim=0.25, ilim=0.1)
alt_pid = PID(1.2, 0.3, 0.0, lim=6.0, ilim=3.0)
vz_pid = PID(1.5, 0.6, 0.0, lim=8.0, ilim=4.0)
vx_pid = PID(0.08, 0.02, 0.0, lim=math.radians(15), ilim=math.radians(5))
vy_pid = PID(0.08, 0.02, 0.0, lim=math.radians(15), ilim=math.radians(5))
# fixed-wing loops
pitch_fw = PID(1.2, 0.4, 0.05, lim=math.radians(20), ilim=math.radians(8))
roll_fw = PID(0.8, 0.1, 0.03, lim=math.radians(20), ilim=math.radians(5))
yaw_damp = PID(0.4, 0.0, 0.0, lim=math.radians(15))
alt_fw = PID(0.04, 0.004, 0.0, lim=math.radians(12), ilim=math.radians(6))
spd_fw = PID(0.08, 0.04, 0.0, lim=1.0, ilim=0.6)


def st():
    return {
        "t": fdm["simulation/sim-time-sec"],
        "h": fdm["position/h-agl-ft"] * FT,
        "phi": fdm["attitude/phi-rad"], "theta": fdm["attitude/theta-rad"], "psi": fdm["attitude/psi-rad"],
        "p": fdm["velocities/p-rad_sec"], "q": fdm["velocities/q-rad_sec"], "r": fdm["velocities/r-rad_sec"],
        "vn": fdm["velocities/v-north-fps"] * FT, "ve": fdm["velocities/v-east-fps"] * FT, "vd": fdm["velocities/v-down-fps"] * FT,
        "u": fdm["velocities/u-fps"] * FT,
        "eas": fdm["velocities/ve-fps"] * FT,
        "alpha": fdm["aero/alpha-deg"],
        "x": fdm["position/distance-from-start-lon-mt"] if False else None,
    }


def wrap(a):
    return (a + math.pi) % (2 * math.pi) - math.pi


def set_lift(coll, tr, tp, ty):
    """coll: per-unit thrust fraction; tr/tp/ty: roll/pitch/yaw demands."""
    out = []
    for i in range(4):
        f = coll - tr * (1 if arm_r[i] > 0 else -1) + tp * (1 if arm_p[i] > 0 else -1) + ty * sense[i]
        f = max(0.0, min(1.0, f)) * SPOOL[0]
        out.append(f)
        fdm[f"fcs/throttle-cmd-norm[{i}]"] = f ** 1.5  # power ~ thrust^1.5
    return out


def hover_ctl(s, dt, vx_t, vy_t, h_t=None, vz_t=None, coll_scale=1.0, tilt_lim=math.radians(15)):
    if vz_t is None:
        vz_t = alt_pid(h_t - s["h"], dt)
    vz = -s["vd"]
    az = vz_pid(vz_t - vz, dt)
    tilt = math.cos(s["phi"]) * math.cos(s["theta"])
    coll = M * (G + az) / max(0.5, tilt) / (4 * T_MAX)
    # velocity -> attitude (heading 0: north is forward)
    th_t = max(-tilt_lim, min(tilt_lim, -vx_pid(vx_t - s["vn"], dt)))
    ph_t = max(-tilt_lim, min(tilt_lim, vy_pid(vy_t - s["ve"], dt)))
    return attitude(s, dt, coll * coll_scale, ph_t, th_t, 0.0)


def attitude(s, dt, coll, ph_t, th_t, psi_t):
    tr = roll_rate(4.5 * (ph_t - s["phi"]) - s["p"], dt)
    tp = pitch_rate(4.5 * (th_t - s["theta"]) - s["q"], dt)
    ty = yaw_rate(2.0 * wrap(psi_t - s["psi"]) - s["r"], dt)
    return set_lift(coll, tr, tp, ty)


PUSH = [0.0]
# JSBSim's FGPropeller applies the motor's reaction torque as power / max(0.01,
# omega): a stopped prop given power kicks the airframe with a huge torque.
# The pusher therefore idles at 2 % (ArduPlane stops it) and slews at
# THR_SLEWRATE's default 100 %/s.
PUSH_IDLE = 0.02


def set_pusher(thr):
    thr = max(PUSH_IDLE, min(1.0, thr))
    PUSH[0] += max(-DT, min(DT, thr - PUSH[0]))  # 100 %/s
    fdm["fcs/throttle-cmd-norm[4]"] = PUSH[0]


def fixed_wing(s, dt, h_t, v_t, thr=None):
    th_t = alt_fw(h_t - s["h"], dt) + math.radians(2)
    # demands are "pitch up / roll right / yaw right"; each surface's sign
    # comes from its AeroBuildup derivative, not from a convention guessed here
    de = SGN_DE * pitch_fw(th_t - s["theta"], dt, de=-s["q"])
    da = SGN_DA * roll_fw(0.0 - s["phi"], dt, de=-s["p"])
    dr = SGN_DR * yaw_damp(-s["r"], dt)
    fdm["fcs/elevator-pos-rad"] = de
    fdm["fcs/left-aileron-pos-rad"] = da
    fdm["fcs/rudder-pos-rad"] = dr
    if thr is None:
        thr = max(0.0, min(1.0, 0.4 + spd_fw(v_t - s["eas"], dt)))
    set_pusher(thr)
    return thr


SGN_DE = 1.0 if CTL["de"]["Cm"] > 0 else -1.0
SGN_DA = 1.0 if CTL["da"]["Cl"] > 0 else -1.0
SGN_DR = 1.0 if CTL["dr"]["Cn"] > 0 else -1.0
LOG = []
PHASES = []
phase = None


def enter(name):
    global phase
    phase = name
    PHASES.append((fdm["simulation/sim-time-sec"], name))


def lift_shaft():
    return [fdm[f"propulsion/engine[{i}]/power-hp"] * HP for i in range(4)]


def log(s, lift):
    ps = lift_shaft()
    LOG.append({
        "t": s["t"], "phase": phase, "x_m": XPOS[0], "y_m": XPOS[1], "h_m": s["h"],
        "eas_ms": s["eas"], "u_ms": s["u"], "vz_ms": -s["vd"], "alpha_deg": s["alpha"],
        "roll_deg": math.degrees(s["phi"]), "pitch_deg": math.degrees(s["theta"]), "yaw_deg": math.degrees(s["psi"]),
        "lift_throttle": sum(lift) / 4 if lift else 0.0,
        "pusher_throttle": fdm["fcs/throttle-cmd-norm[4]"],
        "lift_shaft_w": sum(ps), "pusher_shaft_w": fdm["propulsion/engine[4]/power-hp"] * HP,
        "pusher_thrust_n": fdm["propulsion/engine[4]/thrust-lbs"] * LBF,
        "lift_thrust_n": sum(fdm[f"propulsion/engine[{i}]/thrust-lbs"] for i in range(4)) * LBF,
        "elevator_deg": math.degrees(fdm["fcs/elevator-pos-rad"]),
    })


XPOS = [0.0, 0.0]
# AP_MotorsMulticopter spool-up (MOT_SPOOL_TIME default 0.5 s): throttle ceiling ramps from 0
SPOOL = [0.0]
lift = [0, 0, 0, 0]
t_end = None
events = {}
last_lift = 0.0
trans_t0 = None
decel_v = None
k = 0
enter("takeoff")
while True:
    fdm.run()
    s = st()
    t = s["t"]
    XPOS[0] += s["vn"] * DT
    XPOS[1] += s["ve"] * DT
    SPOOL[0] = min(1.0, t / 0.5)
    if phase == "takeoff":
        lift = hover_ctl(s, DT, 0, 0, vz_t=min(2.5, 1.2 * (ALT - s["h"]) + 0.3))
        set_pusher(0)
        if s["h"] > ALT - 0.5:
            events["hover_alt_s"] = t
            enter("hover")
    elif phase == "hover":
        lift = hover_ctl(s, DT, 0, 0, h_t=ALT)
        if t - events["hover_alt_s"] > 8:
            enter("transition: airspeed wait")
    elif phase == "transition: airspeed wait":
        # SLT_Transition AIRSPEED_WAIT: hover holds height and level attitude, pusher on
        lift = attitude(s, DT, M * (G + vz_pid(alt_pid(ALT - s["h"], DT) + s["vd"], DT)) / (4 * T_MAX), 0.0, 0.0, 0.0)
        fixed_wing(s, DT, ALT, V_CRUISE, thr=1.0)
        if s["eas"] > V_MIN:
            last_lift = sum(lift) / 4
            trans_t0 = t
            events["airspeed_min_s"] = t
            enter("transition: timer")
    elif phase == "transition: timer":
        scale = max(0.0, (T_TRANS - (t - trans_t0)) / T_TRANS)
        # hold_stabilize(last_throttle x scale): attitude held, collective ramps down
        lift = attitude(s, DT, last_lift * scale, 0.0, s["theta"] if scale < 0.5 else 0.0, 0.0)
        fixed_wing(s, DT, ALT, V_CRUISE)
        if t - trans_t0 > T_TRANS:
            for i in range(4):
                fdm[f"fcs/throttle-cmd-norm[{i}]"] = 0
            lift = [0, 0, 0, 0]
            events["transition_done_s"] = t
            enter("cruise")
    elif phase == "cruise":
        fixed_wing(s, DT, ALT, V_CRUISE)
        if t - events["transition_done_s"] > 120:
            enter("back-transition")
            events["back_s"] = t
            decel_v = s["vn"]
            for p_ in (roll_rate, pitch_rate, yaw_rate, alt_pid, vz_pid, vx_pid, vy_pid):
                p_.reset()
    elif phase == "back-transition":
        set_pusher(0)
        fdm["fcs/elevator-pos-rad"] = fdm["fcs/left-aileron-pos-rad"] = fdm["fcs/rudder-pos-rad"] = 0.0
        decel_v = max(0.0, decel_v - DECEL * DT)
        ramp = min(1.0, (t - events["back_s"]) / (ar["Q_BACKTRANS_MS"] / 1000.0))
        lift = hover_ctl(s, DT, decel_v, 0, h_t=ALT, tilt_lim=math.radians(15) * ramp + 1e-3)
        if decel_v == 0.0 and abs(s["vn"]) < 0.5:
            events["stopped_s"] = t
            enter("landing")
    elif phase == "landing":
        lift = hover_ctl(s, DT, 0, 0, vz_t=-(LAND_V if s["h"] < LAND_FINAL_ALT else 1.5))
        if any(fdm[f"gear/unit[{i}]/WOW"] for i in range(4)) and abs(s["vd"]) < 0.3:
            events["touchdown_s"] = t
            for i in range(4):
                fdm[f"fcs/throttle-cmd-norm[{i}]"] = 0
            enter("landed")
            t_end = t + 2
    if t_end is not None and t > t_end:
        break
    if t > 900 or s["h"] < -1 or abs(s["phi"]) > math.radians(80) and phase not in ("landed",):
        events["aborted"] = f"{phase} at t={t:.1f}s: h {s['h']:.1f} m, roll {math.degrees(s['phi']):.0f} deg"
        break
    if k % 20 == 0:
        log(s, lift)
    k += 1

# --------------------------------------------------------------- results

import csv  # noqa: E402

csv_path = os.path.join(OUT, "trajectory.csv")
with open(csv_path, "w", newline="") as f:
    wr = csv.DictWriter(f, fieldnames=list(LOG[0].keys()))
    wr.writeheader()
    wr.writerows(LOG)

A = X["analytic"]
avi = A["avionics_w"]
eta_lift = (4 * (A["hover_shaft_each_w"] or 1)) / max(1e-6, A["hover_battery_w"] - avi)
eta_push = (A["cruise_shaft_w"] or 1) / max(1e-6, A["cruise_battery_w"] - avi)


def window(name, t0, t1, key):
    v = [r[key] for r in LOG if r["phase"] == name and t0 <= r["t"] <= t1]
    return float(np.mean(v)) if v else float("nan")


metrics = {"aero_tables_s": round(aero_s, 1), "events_s": events}
ok = "aborted" not in events
if "transition_done_s" in events:
    c1 = events["transition_done_s"] + 120
    c0 = c1 - 40
    cr = {k_: window("cruise", c0, c1, k_) for k_ in ("eas_ms", "u_ms", "pusher_shaft_w", "pusher_thrust_n", "alpha_deg", "pitch_deg", "h_m", "elevator_deg", "pusher_throttle")}
    p_batt = cr["pusher_shaft_w"] / eta_push + avi
    # the analytic range and endurance scaled by the sim's cruise power and
    # speed (same energy budget, reserve and VTOL/climb energy as the model)
    ratio = A["cruise_battery_w"] / p_batt
    range_km = A["range_km"] * ratio * cr["u_ms"] / A["best_range_ms"]
    t_h = (A["range_km"] / 3.6 / A["best_range_ms"]) * ratio
    metrics["cruise"] = {
        "eas_ms": cr["eas_ms"], "alpha_deg": cr["alpha_deg"], "elevator_deg": cr["elevator_deg"],
        "thrust_n": cr["pusher_thrust_n"], "analytic_drag_n": A["cruise_drag_n"],
        "pusher_shaft_w": cr["pusher_shaft_w"], "analytic_shaft_w": A["cruise_shaft_w"],
        "battery_w": p_batt, "analytic_battery_w": A["cruise_battery_w"],
        "battery_vs_analytic": p_batt / A["cruise_battery_w"] - 1,
        "range_km_at_sim_power": range_km, "analytic_range_km": A["range_km"],
        "cruise_min_at_sim_power": t_h * 60, "analytic_cruise_min": A["range_km"] / 3.6 / A["best_range_ms"] * 60,
        "speed_held_ms": V_CRUISE,
    }
hov = [r for r in LOG if r["phase"] == "hover"]
if hov:
    sh = float(np.mean([r["lift_shaft_w"] for r in hov]))
    metrics["hover"] = {"lift_shaft_w": sh, "analytic_lift_shaft_w": 4 * (A["hover_shaft_each_w"] or 0), "battery_w": sh / eta_lift + avi,
                        "analytic_battery_w": A["hover_battery_w"], "throttle_mean": float(np.mean([r["lift_throttle"] for r in hov]))}
metrics["max_roll_deg"] = float(max(abs(r["roll_deg"]) for r in LOG))
metrics["min_height_after_transition_m"] = float(min((r["h_m"] for r in LOG if r["phase"].startswith("transition") or r["phase"] == "cruise"), default=float("nan")))
metrics["completed"] = ok and "touchdown_s" in events
metrics["aero"] = {"V_ms": V_CR, "derivatives": D, "controls_per_rad": CTL, "extra_CD": EXTRA_CD}

# plots
import matplotlib  # noqa: E402

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402

T = np.array([r["t"] for r in LOG])
col = {k_: np.array([r[k_] for r in LOG]) for k_ in LOG[0] if k_ != "phase"}
fig, ax = plt.subplots(5, 1, figsize=(10, 12), sharex=True)
ax[0].plot(T, col["h_m"]); ax[0].set_ylabel("height AGL, m")
ax[1].plot(T, col["eas_ms"], label="EAS"); ax[1].axhline(V_MIN, ls="--", c="r", lw=0.8, label="AIRSPEED_MIN"); ax[1].axhline(V_CRUISE, ls=":", c="g", lw=0.8, label="best-range speed"); ax[1].set_ylabel("m/s"); ax[1].legend(loc="upper right", fontsize=8)
ax[2].plot(T, col["pitch_deg"], label="pitch"); ax[2].plot(T, col["roll_deg"], label="roll"); ax[2].plot(T, col["alpha_deg"], label="alpha", alpha=0.6); ax[2].set_ylabel("deg"); ax[2].set_ylim(-30, 30); ax[2].legend(loc="upper right", fontsize=8)
ax[3].plot(T, col["lift_throttle"], label="lift (mean)"); ax[3].plot(T, col["pusher_throttle"], label="pusher"); ax[3].set_ylabel("throttle"); ax[3].legend(loc="upper right", fontsize=8)
ax[4].plot(T, col["lift_shaft_w"], label="lift shaft"); ax[4].plot(T, col["pusher_shaft_w"], label="pusher shaft"); ax[4].set_ylabel("W"); ax[4].set_xlabel("time, s"); ax[4].legend(loc="upper right", fontsize=8)
for a_ in ax:
    for tp_, name in PHASES:
        a_.axvline(tp_, c="k", lw=0.4, alpha=0.4)
for tp_, name in PHASES:
    ax[0].text(tp_, ax[0].get_ylim()[1] * 0.95, " " + name, fontsize=7, rotation=90, va="top")
fig.suptitle(f"{X['name']}: JSBSim 6-DOF mission")
fig.tight_layout()
states_png = os.path.join(OUT, "states.png")
fig.savefig(states_png, dpi=110)
plt.close(fig)

fig, ax = plt.subplots(figsize=(11, 4))
sc = ax.scatter(col["x_m"], col["h_m"], c=T, s=2, cmap="viridis")
ax.set_xlabel("distance north, m"); ax.set_ylabel("height AGL, m"); ax.set_title("trajectory (colour: time)")
fig.colorbar(sc, label="s")
fig.tight_layout()
traj_png = os.path.join(OUT, "trajectory.png")
fig.savefig(traj_png, dpi=110)
plt.close(fig)

# animation: side view following the aircraft, ~25 s of video
mp4 = os.path.join(OUT, "flight.mp4")
anim_ok = False
try:
    from matplotlib.animation import FFMpegWriter

    fig, (a1, a2) = plt.subplots(1, 2, figsize=(12, 4.5), gridspec_kw={"width_ratios": [2, 1]})
    n = len(T)
    frames = np.linspace(0, n - 1, min(n, 25 * 20)).astype(int)
    wr_ = FFMpegWriter(fps=20, bitrate=1800)
    c = X["wing"]["root_chord"]
    xs_body = np.array([X["pod"]["x0"], X["htail"]["x_le"] + X["htail"]["chord"]]) - cg[0]
    with wr_.saving(fig, mp4, dpi=90):
        for i in frames:
            a1.cla(); a2.cla()
            a1.plot(col["x_m"][: i + 1], col["h_m"][: i + 1], c="0.6", lw=1)
            th = math.radians(col["pitch_deg"][i])
            sc_ = 6.0
            bx = -xs_body * sc_ * math.cos(th)
            bz = -xs_body * sc_ * -math.sin(th)
            a1.plot(col["x_m"][i] + bx, col["h_m"][i] + bz, c="k", lw=3)
            a1.set_xlim(col["x_m"][i] - 60, col["x_m"][i] + 30); a1.set_ylim(-2, ALT + 15)
            a1.set_title(f"t {T[i]:5.1f} s  {LOG[i]['phase']}  (aircraft drawn 6x)")
            a1.set_xlabel("north, m"); a1.set_ylabel("height, m")
            a2.bar(["lift", "pusher"], [col["lift_throttle"][i], col["pusher_throttle"][i]], color=["tab:blue", "tab:orange"])
            a2.set_ylim(0, 1); a2.set_title(f"EAS {col['eas_ms'][i]:4.1f} m/s  pitch {col['pitch_deg'][i]:4.1f} deg")
            wr_.grab_frame()
    plt.close(fig)
    anim_ok = True
except Exception as ex:  # noqa: BLE001
    metrics["animation_error"] = str(ex)

summary_path = os.path.join(OUT, "sim_summary.json")
json.dump(metrics, open(summary_path, "w"), indent=2, default=float)


def fmt(v, n=1):
    return "-" if v is None or (isinstance(v, float) and math.isnan(v)) else f"{v:.{n}f}"


cr = metrics.get("cruise", {})
hv = metrics.get("hover", {})
items = []
if anim_ok:
    items.append({"label": "flight animation", "path": "flight.mp4", "media": "mp4", "summary": "side view of the whole mission, phases labelled", "metrics": {"completed": metrics["completed"]}})
items += [
    {"label": "mission states", "path": "states.png", "media": "png",
     "summary": "height, airspeed, attitude, throttles and shaft power through takeoff, hover, transition, cruise, back-transition and landing",
     "metrics": {"max_roll_deg": metrics["max_roll_deg"], "min_height_in_transition_m": metrics["min_height_after_transition_m"]}},
    {"label": "trajectory", "path": "trajectory.png", "media": "png", "summary": "distance vs height", "metrics": {}},
    {"label": "cruise vs analytic model", "path": "sim_summary.json", "media": "json",
     "summary": (f"cruise at {fmt(cr.get('eas_ms'))} m/s EAS: pusher {fmt(cr.get('pusher_shaft_w'), 0)} W shaft vs {fmt(cr.get('analytic_shaft_w'), 0)} W analytic; "
                 f"battery {fmt(cr.get('battery_w'), 0)} W vs {fmt(cr.get('analytic_battery_w'), 0)} W ({fmt(100 * cr.get('battery_vs_analytic', float('nan')), 1)} %); "
                 f"range at the sim's power {fmt(cr.get('range_km_at_sim_power'))} km vs {fmt(cr.get('analytic_range_km'))} km analytic") if cr else "cruise not reached",
     "metrics": cr},
    {"label": "hover vs analytic model", "path": "sim_summary.json", "media": "json",
     "summary": f"hover shaft {fmt(hv.get('lift_shaft_w'), 0)} W vs {fmt(hv.get('analytic_lift_shaft_w'), 0)} W analytic, mean throttle {fmt(hv.get('throttle_mean'), 2)}" if hv else "no hover",
     "metrics": hv},
    {"label": "trajectory log", "path": "trajectory.csv", "media": "csv", "summary": f"{len(LOG)} rows at 20 Hz", "metrics": {}},
    {"label": "JSBSim aircraft", "path": "jsbsim/aircraft/wright_vtol/wright_vtol.xml", "media": "json",
     "summary": "the generated JSBSim model (XML): re-run with any JSBSim", "metrics": {}},
]
title = "6-DOF flight simulation (JSBSim + AeroSandbox AeroBuildup)" + ("" if metrics["completed"] else f" -- DID NOT COMPLETE: {events.get('aborted', 'no touchdown')}")
json.dump({"kind": "sim", "title": title, "items": items}, open(os.path.join(OUT, "manifest.json"), "w"), indent=2, default=float)
print(json.dumps({"completed": metrics["completed"], "events": events, "cruise": cr, "hover": hv}, default=float))
