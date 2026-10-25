//! Where everything is: the one geometric model. Mass properties, drag,
//! structure, the CAD meshes and every clearance check read it; nothing else
//! defines a position or a size (the prototype's layout and CAD each defined
//! prop discs and the servo pocket, and drifted).
//!
//! Frame: origin at the wing root's leading edge, x aft, y starboard, z up.
//! The leading edge is straight and the taper is on the trailing edge, so a
//! panel prints flat on its leading edge. The 4+1 quadplane arrangement (the
//! reference build's): two booms at +/- `boom_y` carrying a front and a rear
//! lift motor, the four discs symmetric about the CG (equal hover throttle,
//! what ArduPilot's QuadPlane assumes) and clear of the wing in plan view; an
//! H-tail across the boom ends behind the rear discs; the pod under the wing
//! round the battery with the avionics bay ahead of it; the pusher at the
//! pod's tail on the CG's height. The battery slides along the pod to put the
//! CG at `rules::STATIC_MARGIN_TARGET` -- how a builder trims a quadplane --
//! and [`solve`] iterates that, the motor stations and the tail together.

use crate::airfoil::{self, Airfoil};
use crate::battery::{self, Battery};
use crate::catalog::{catalog, Esc, Filament, FlightController, Module, Motor, Prop, Servo, Tube};
use crate::rules::*;
use crate::spec::Design;
use crate::stability;
use wright_core::units::{Area, Length, Mass, G0};

/// Every catalog part a design names, resolved once.
#[derive(Debug, Clone)]
pub struct Parts {
    pub lift_motor: &'static Motor,
    pub lift_prop: &'static Prop,
    pub lift_esc: &'static Esc,
    pub cruise_motor: &'static Motor,
    pub cruise_prop: &'static Prop,
    pub cruise_esc: &'static Esc,
    pub fc: &'static FlightController,
    pub gps: &'static Module,
    pub receiver: &'static Module,
    pub airspeed: Option<&'static Module>,
    pub servo: &'static Servo,
    pub main_spar: &'static Tube,
    pub rear_spar: &'static Tube,
    pub joiner: &'static Tube,
    pub boom: &'static Tube,
    pub shell: &'static Filament,
    pub structural: &'static Filament,
    pub wing_airfoil: &'static Airfoil,
    pub tail_airfoil: &'static Airfoil,
}

impl Parts {
    pub fn resolve(d: &Design) -> Result<Parts, String> {
        let c = catalog();
        fn get<'a, T>(m: &'a std::collections::BTreeMap<String, T>, id: &str, what: &str) -> Result<&'a T, String> {
            m.get(id).ok_or_else(|| {
                let valid: Vec<&str> = m.keys().map(String::as_str).take(40).collect();
                format!("{what} {id:?} is not a valid catalog row; valid ids include {} (see `catalog` for this path)", valid.join(", "))
            })
        }
        let p = &d.propulsion;
        let s = &d.structure;
        Ok(Parts {
            lift_motor: get(&c.motors, &p.lift_motor, "lift motor")?,
            lift_prop: get(&c.props, &p.lift_prop, "lift prop")?,
            lift_esc: get(&c.escs, &p.lift_esc, "lift ESC")?,
            cruise_motor: get(&c.motors, &p.cruise_motor, "cruise motor")?,
            cruise_prop: get(&c.props, &p.cruise_prop, "cruise prop")?,
            cruise_esc: get(&c.escs, &p.cruise_esc, "cruise ESC")?,
            fc: get(&c.fcs, &d.avionics.fc, "flight controller")?,
            gps: get(&c.gps, &d.avionics.gps, "GPS")?,
            receiver: get(&c.receivers, &d.avionics.receiver, "receiver")?,
            airspeed: match &d.avionics.airspeed {
                Some(a) => Some(get(&c.airspeed, a, "airspeed sensor")?),
                None => None,
            },
            servo: get(&c.servos, &d.avionics.servo, "servo")?,
            main_spar: get(&c.tubes, &s.main_spar, "main spar")?,
            rear_spar: get(&c.tubes, &s.rear_spar, "rear spar")?,
            joiner: get(&c.tubes, &s.joiner, "spar joiner")?,
            boom: get(&c.tubes, &s.boom_tube, "boom tube")?,
            shell: get(&c.filaments, &s.shell_filament, "shell filament")?,
            structural: get(&c.filaments, &s.structural_filament, "structural filament")?,
            wing_airfoil: airfoil::get(&d.wing.airfoil).ok_or_else(|| crate::spec::ids::check("wing.airfoil", "wing.airfoil", &d.wing.airfoil).unwrap_err())?,
            tail_airfoil: airfoil::get(&d.tail.airfoil).ok_or_else(|| crate::spec::ids::check("tail.airfoil", "tail.airfoil", &d.tail.airfoil).unwrap_err())?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct MassItem {
    pub name: &'static str,
    pub group: &'static str,
    pub mass: Mass,
    pub x: Length,
    pub z: Length,
}

/// A box in the pod (battery, avionics): centre and size.
#[derive(Debug, Clone)]
pub struct Boxed {
    pub name: &'static str,
    pub center: [Length; 3],
    pub size: [Length; 3],
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub design: Design,
    pub parts: Parts,
    pub battery: Battery,
    // wing
    pub span: Length,
    pub root_chord: Length,
    pub tip_chord: Length,
    pub area: Area,
    pub mac: Length,
    pub aspect_ratio: f64,
    pub taper: f64,
    pub x_ac_wing: Length,
    // booms, lift motors
    pub boom_y: Length,
    pub boom_z: Length,
    pub boom_x0: Length,
    pub boom_x1: Length,
    pub x_front: Length,
    pub x_rear: Length,
    pub lift_radius: Length,
    /// Lift motor mount top, motor top, and the lift discs' lower face.
    pub mount_top_z: Length,
    pub motor_top_z: Length,
    pub lift_disc_z: Length,
    // tail
    pub x_tail_le: Length,
    pub h_area: Area,
    pub h_span: Length,
    pub h_chord: Length,
    pub v_area_each: Area,
    pub v_height: Length,
    pub v_chord: Length,
    pub x_ac_tail: Length,
    // pod, battery, bay, pusher
    pub pod_x0: Length,
    pub pod_x1: Length,
    pub pod_width: Length,
    pub pod_height: Length,
    pub pod_zc: Length,
    /// Where the full section starts and ends (nose and tail cone outside).
    pub pod_full_x0: Length,
    pub pod_full_x1: Length,
    pub bay: Vec<Boxed>,
    pub battery_x: Length,
    pub pusher_x: Length,
    pub pusher_radius: Length,
    pub pusher_z: Length,
    // spars (straight: constant x and z)
    pub main_spar_x: Length,
    pub main_spar_z: Length,
    pub rear_spar_x: Length,
    pub rear_spar_z: Length,
    /// Whether the joiner slides over the spar ("sleeve") or into it.
    pub joiner_kind: Option<&'static str>,
    // servos: an aileron servo's centre (starboard; port mirrored)
    pub aileron_servo: [Length; 3],
    pub servo_size: [Length; 3],
    // legs
    pub leg_height: Length,
    pub leg_thickness: Length,
    pub leg_chord: Length,
    pub ground_z: Length,
    // balance
    pub x_cg: Length,
    pub z_cg: Length,
    pub x_np: Length,
    pub static_margin: f64,
    pub items: Vec<MassItem>,
}

fn m(v: f64) -> Length {
    Length::m(v)
}

impl Layout {
    pub fn mass(&self) -> Mass {
        self.items.iter().map(|i| i.mass).sum()
    }
    pub fn weight_n(&self) -> f64 {
        self.mass().as_kg() * G0
    }
    pub fn chord_at(&self, y: Length) -> Length {
        let eta = (y.as_m().abs() / (self.span.as_m() / 2.0)).min(1.0);
        self.root_chord + (self.tip_chord - self.root_chord) * eta
    }
    pub fn semi_span(&self) -> Length {
        self.span / 2.0
    }
    pub fn mass_of(&self, group: &str) -> Mass {
        self.items.iter().filter(|i| i.group == group).map(|i| i.mass).sum()
    }
    /// The aileron's spanwise extent (starboard), m: 40 mm outboard of the
    /// boom or 45 % of the semi-span, to 15 mm inside the tip.
    pub fn aileron_span(&self) -> (Length, Length) {
        let half = self.semi_span();
        ((self.boom_y + m(0.04)).max(half * 0.45), half - m(0.015))
    }
    /// The section's (upper, lower) z at chordwise x and span station y
    /// (washout ignored in geometry: under 2 deg it changes depth by cos).
    pub fn wing_surface(&self, x: Length, y: Length) -> Option<(Length, Length)> {
        let c = self.chord_at(y);
        let xc = x / c;
        if !(0.0..=1.0).contains(&xc) {
            return None;
        }
        let (u, l) = self.parts.wing_airfoil.surfaces_at(xc);
        let dz = y.as_m().abs() * self.design.wing.dihedral_deg.to_radians().tan();
        Some((c * u + m(dz), c * l + m(dz)))
    }
    pub fn folding_pusher(&self) -> bool {
        self.parts.cruise_prop.folding
    }
}

/// Bay stacks: the 4-in-1 lift ESC under the flight controller (the
/// 30.5/20 mm stack), the GPS on the receiver, then the airspeed sensor; each
/// stack centred on the pod's axis, the stacks in a row along x.
/// (name, size, x in the bay, z off the pod axis) per item; bay length, width, height.
type Stacks = (Vec<(&'static str, [f64; 3], f64, f64)>, f64, f64, f64);

fn bay_stacks(p: &Parts) -> Stacks {
    let dims = |d: &[Length; 3]| {
        let mut v = [d[0].as_m(), d[1].as_m(), d[2].as_m()];
        // longest side along x, then across, thinnest up
        v.sort_by(|a, b| b.total_cmp(a));
        v
    };
    let mut stacks: Vec<Vec<(&'static str, [f64; 3])>> = vec![
        vec![("lift_esc", dims(&p.lift_esc.dims)), ("flight_controller", dims(&p.fc.dims))],
        vec![("receiver", dims(&p.receiver.dims)), ("gps", dims(&p.gps.dims))],
    ];
    if let Some(a) = p.airspeed {
        stacks.push(vec![("airspeed_sensor", dims(&a.dims))]);
    }
    let mut out = vec![];
    let mut x = BAY_GAP_M;
    let (mut wmax, mut hmax) = (0.0f64, 0.0f64);
    for st in stacks {
        let len = st.iter().map(|(_, d)| d[0]).fold(0.0, f64::max);
        let h: f64 = st.iter().map(|(_, d)| d[2]).sum::<f64>() + BAY_GAP_M * (st.len() as f64 - 1.0);
        let mut z = -h / 2.0;
        for (name, d) in st {
            out.push((name, d, x + len / 2.0, z + d[2] / 2.0));
            wmax = wmax.max(d[1]);
            z += d[2] + BAY_GAP_M;
        }
        hmax = hmax.max(h);
        x += len + BAY_GAP_M;
    }
    (out, x.max(0.04), wmax, hmax)
}

/// The full layout of a resolved design.
pub fn solve(d: &Design) -> Result<Layout, String> {
    d.requirements.validate()?;
    let parts = Parts::resolve(d)?;
    let batt = battery::resolve(&d.battery)?;
    let w = &d.wing;
    let span = w.span_mm / 1000.0;
    let (cr, ct) = (w.root_chord_mm / 1000.0, w.tip_chord_mm / 1000.0);
    if !(span > 0.0 && cr > 0.0 && ct > 0.0 && ct <= cr) {
        return Err(format!("wing {span} m span, chords {cr}/{ct} m is not a wing"));
    }
    let lam = ct / cr;
    let area = span * (cr + ct) / 2.0;
    let mac = 2.0 / 3.0 * cr * (1.0 + lam + lam * lam) / (1.0 + lam);
    let lift_r = parts.lift_prop.data.diameter.as_m() / 2.0;
    let push_r = parts.cruise_prop.data.diameter.as_m() / 2.0;
    let boom_od = parts.boom.od.as_m();
    let bsize = [batt.size[0].as_m(), batt.size[1].as_m(), batt.size[2].as_m()];
    let (stacks, bay_len, bay_w, bay_h) = bay_stacks(&parts);
    let k = 2f64.powf(1.0 / POD_EXPONENT);
    let pod_w = (bsize[1].max(bay_w) + 2.0 * BATTERY_CLEARANCE_M) * k + 2.0 * POD_WALL_M;
    let pod_h = ((bsize[2].max(bay_h) + 2.0 * BATTERY_CLEARANCE_M) * k + 2.0 * POD_WALL_M).max(0.05);
    let boom_y = (lift_r + pod_w / 2.0 + LAYOUT_GAP_M).max(push_r + boom_od / 2.0 + LAYOUT_GAP_M);
    let mut lay = Layout {
        design: d.clone(),
        parts,
        battery: batt,
        span: m(span),
        root_chord: m(cr),
        tip_chord: m(ct),
        area: Area::m2(area),
        mac: m(mac),
        aspect_ratio: span * span / area,
        taper: lam,
        x_ac_wing: m(0.25 * mac),
        boom_y: m(boom_y),
        boom_z: m(0.0),
        boom_x0: m(0.0),
        boom_x1: m(0.0),
        x_front: m(0.0),
        x_rear: m(0.0),
        lift_radius: m(lift_r),
        mount_top_z: m(0.0),
        motor_top_z: m(0.0),
        lift_disc_z: m(0.0),
        x_tail_le: m(0.0),
        h_area: Area::m2(0.0),
        h_span: m(2.0 * boom_y),
        h_chord: m(0.0),
        v_area_each: Area::m2(0.0),
        v_height: m(0.0),
        v_chord: m(0.0),
        x_ac_tail: m(0.0),
        pod_x0: m(-0.12),
        pod_x1: m(0.3),
        pod_width: m(pod_w),
        pod_height: m(pod_h),
        pod_zc: m(0.0),
        pod_full_x0: m(0.0),
        pod_full_x1: m(0.0),
        bay: vec![],
        battery_x: m(0.1),
        pusher_x: m(0.0),
        pusher_radius: m(push_r),
        pusher_z: m(0.0),
        main_spar_x: m(MAIN_SPAR_TIP_FRAC * ct),
        main_spar_z: m(0.0),
        rear_spar_x: m(REAR_SPAR_TIP_FRAC * ct),
        rear_spar_z: m(0.0),
        joiner_kind: None,
        aileron_servo: [m(0.0); 3],
        servo_size: [m(0.0); 3],
        leg_height: m(0.02),
        leg_thickness: Length::mm(d.structure.leg_thickness_mm),
        leg_chord: Length::mm(d.structure.leg_thickness_mm * LEG_CHORD_OVER_THICKNESS),
        ground_z: m(0.0),
        x_cg: m(0.3 * mac),
        z_cg: m(0.0),
        x_np: m(0.0),
        static_margin: 0.0,
        items: vec![],
    };
    lay.pod_zc = m(-pod_h / 2.0 - 0.001);
    spars_and_servo(&mut lay);
    lay.boom_z = boom_z(&lay);
    let motor_len = lay.parts.lift_motor.bell_length().as_m();
    lay.mount_top_z = lay.boom_z + m(boom_od / 2.0 + MOUNT_ABOVE_BOOM_M);
    lay.motor_top_z = lay.mount_top_z + m(motor_len);
    lay.lift_disc_z = lay.motor_top_z + m(PROP_ABOVE_MOTOR_M);

    let mut x_cg = 0.3 * mac;
    let mut pusher_x: Option<f64> = None;
    for _ in 0..30 {
        let c_boom = lay.chord_at(m(boom_y)).as_m();
        let mut half = (x_cg + lift_r + LAYOUT_GAP_M).max(c_boom - x_cg + lift_r + LAYOUT_GAP_M);
        if let Some(px) = pusher_x {
            half = rear_clear_of_pusher(&lay, px, x_cg + half) - x_cg;
        }
        lay.x_front = m(x_cg - half);
        lay.x_rear = m(x_cg + half);
        lay.x_tail_le = lay.x_rear + m(lift_r + TAIL_GAP_M);
        // H-tail between the booms, sized by its volume coefficient
        let mut chord = 0.06;
        let mut h_area = 0.0;
        for _ in 0..30 {
            let arm = lay.x_tail_le.as_m() + 0.25 * chord - lay.x_ac_wing.as_m();
            h_area = d.tail.vh * area * mac / arm;
            chord = h_area / (2.0 * boom_y);
        }
        lay.h_area = Area::m2(h_area);
        lay.h_chord = m(chord);
        lay.x_ac_tail = lay.x_tail_le + m(0.25 * chord);
        let arm = (lay.x_ac_tail - lay.x_ac_wing).as_m();
        let v_total = d.tail.vv * area * span / arm;
        lay.v_area_each = Area::m2(v_total / 2.0);
        lay.v_height = m((d.tail.aspect_ratio_v * v_total / 2.0).sqrt());
        lay.v_chord = m(v_total / 2.0 / lay.v_height.as_m());
        lay.boom_x0 = lay.x_front - m(0.02);
        lay.boom_x1 = lay.x_tail_le + lay.h_chord.max(lay.v_chord);
        // pod round the battery: nose, bay, battery, tail cone
        let place_pod = |lay: &mut Layout, bx: f64| {
            lay.battery_x = m(bx);
            lay.pod_x0 = m(bx - bsize[0] / 2.0 - bay_len - NOSE_LENGTH_M);
            lay.pod_full_x0 = m(bx - bsize[0] / 2.0 - bay_len);
            lay.pod_full_x1 = m(bx + bsize[0] / 2.0);
            // the pusher disc sits behind the wing's trailing edge
            let end = (bx + bsize[0] / 2.0 + TAIL_CONE_M).max(cr + 0.02);
            lay.pod_x1 = m(end);
            let body = lay.parts.cruise_motor.bell_length().as_m();
            lay.pusher_x = m(end + body + 0.012);
        };
        let bx0 = lay.battery_x.as_m();
        place_pod(&mut lay, bx0);
        vertical(&mut lay);
        items(&mut lay, &stacks);
        lay.x_np = stability::neutral_point(&lay);
        let target = lay.x_np.as_m() - STATIC_MARGIN_TARGET * mac;
        let total = lay.mass().as_kg();
        let bm = lay.battery.pack.mass.as_kg();
        let others: f64 = lay.items.iter().filter(|i| i.name != "battery").map(|i| i.mass.as_kg() * i.x.as_m()).sum();
        let bx = (target * total - others) / bm;
        place_pod(&mut lay, bx);
        pusher_x = Some(lay.pusher_x.as_m());
        vertical(&mut lay);
        items(&mut lay, &stacks);
        let new_cg = lay.items.iter().map(|i| i.mass.as_kg() * i.x.as_m()).sum::<f64>() / lay.mass().as_kg();
        let done = (new_cg - x_cg).abs() < 1e-6;
        x_cg = new_cg;
        if done {
            break;
        }
    }
    lay.x_np = stability::neutral_point(&lay);
    lay.x_cg = m(x_cg);
    let mt = lay.mass().as_kg();
    lay.z_cg = m(lay.items.iter().map(|i| i.mass.as_kg() * i.z.as_m()).sum::<f64>() / mt);
    lay.static_margin = (lay.x_np - lay.x_cg) / lay.mac;
    // the bay's boxes, placed
    let bay_x0 = lay.pod_full_x0.as_m();
    lay.bay = stacks
        .iter()
        .map(|(name, d, x, z)| Boxed {
            name,
            center: [m(bay_x0 + x), m(0.0), lay.pod_zc + m(*z)],
            size: [m(d[0]), m(d[1]), m(d[2])],
        })
        .collect();
    let mut esc = [
        lay.parts.cruise_esc.dims[0].as_m(),
        lay.parts.cruise_esc.dims[1].as_m(),
        lay.parts.cruise_esc.dims[2].as_m(),
    ];
    esc.sort_by(|a, b| b.total_cmp(a));
    lay.bay.push(Boxed {
        name: "pusher_esc",
        center: [lay.pod_full_x1 + m(BAY_GAP_M + esc[0] / 2.0), m(0.0), lay.pod_zc],
        size: esc.map(m),
    });
    lay.bay.push(Boxed {
        name: "battery",
        center: [lay.battery_x, m(0.0), lay.pod_zc],
        size: lay.battery.size,
    });
    if !lay.mass().as_kg().is_finite() || lay.mass().as_kg() <= 0.0 {
        return Err(format!("the layout has no finite mass ({} kg): the bay cannot be laid out for this payload", lay.mass().as_kg()));
    }
    Ok(lay)
}

/// Spar stations, the joiner's fit, and the aileron servo's pocket.
fn spars_and_servo(lay: &mut Layout) {
    let semi = lay.semi_span().as_m();
    let mid_at = |lay: &Layout, x: f64, y: f64| {
        lay.wing_surface(m(x), m(y)).map(|(u, l)| (u.as_m() + l.as_m()) / 2.0).unwrap_or(0.0)
    };
    let ms_x = lay.main_spar_x.as_m();
    lay.main_spar_z = m(mid_at(lay, ms_x, MAIN_SPAR_REACH * semi));
    let rs_x = lay.rear_spar_x.as_m();
    lay.rear_spar_z = m(mid_at(lay, rs_x, REAR_SPAR_REACH * semi));
    let (spar, joiner) = (lay.parts.main_spar, lay.parts.joiner);
    lay.joiner_kind = joiner_fit(spar, joiner);
    // servo: lying flat, its length along the span 20 mm in from the
    // aileron's inboard end, chordwise at the deepest point between the
    // spars, 2 mm clear of each
    let mut d = [lay.parts.servo.dims[0].as_m(), lay.parts.servo.dims[1].as_m(), lay.parts.servo.dims[2].as_m()];
    d.sort_by(|a, b| b.total_cmp(a));
    let (len, wid, thick) = (d[0], d[1], d[2]);
    let boom_y = lay.boom_y.as_m();
    let y0 = (boom_y + 0.04).max(0.45 * semi);
    let y = y0 + 0.020 + len / 2.0;
    let chord = lay.chord_at(m(y)).as_m();
    let lo = ms_x + spar.od.as_m() / 2.0 + 0.002 + wid / 2.0;
    let mut hi = rs_x - lay.parts.rear_spar.od.as_m() / 2.0 - 0.002 - wid / 2.0;
    let (lo, hi) = if hi < lo {
        hi = (1.0 - lay.design.wing.aileron_chord_frac) * chord - 0.008 - wid / 2.0;
        (hi, hi)
    } else {
        (lo, hi)
    };
    let mut best = (f64::NEG_INFINITY, lo, 0.0);
    for k in 0..=8 {
        let x = lo + (hi - lo) * k as f64 / 8.0;
        // the depth over the servo's whole footprint (both edges, both ends)
        let mut depth = f64::INFINITY;
        let mut zmid = 0.0;
        for (dx, dy) in [(-wid / 2.0, -len / 2.0), (wid / 2.0, -len / 2.0), (-wid / 2.0, len / 2.0), (wid / 2.0, len / 2.0)] {
            if let Some((u, l)) = lay.wing_surface(m(x + dx), m(y + dy)) {
                let dd = u.as_m() - l.as_m();
                if dd < depth {
                    depth = dd;
                    zmid = (u.as_m() + l.as_m()) / 2.0;
                }
            } else {
                depth = f64::NEG_INFINITY;
            }
        }
        if depth > best.0 {
            best = (depth, x, zmid);
        }
    }
    lay.aileron_servo = [m(best.1), m(y), m(best.2)];
    lay.servo_size = [m(wid), m(len), m(thick)];
}

/// "sleeve" when the joiner slides over the spar, "insert" when inside it,
/// within `JOINER_FIT_M` of diametral clearance.
pub fn joiner_fit(spar: &Tube, joiner: &Tube) -> Option<&'static str> {
    let fit = JOINER_FIT_M;
    if let Some(jid) = joiner.id {
        let gap = (jid - spar.od).as_m();
        if (0.0..=fit).contains(&gap) {
            return Some("sleeve");
        }
    }
    if let Some(sid) = spar.id {
        let gap = (sid - joiner.od).as_m();
        if (0.0..=fit).contains(&gap) {
            return Some("insert");
        }
    }
    None
}

/// The boom axis: under the wing's lowest point at the boom's station.
fn boom_z(lay: &Layout) -> Length {
    let y = lay.boom_y;
    let c = lay.chord_at(y);
    let mut lowest = f64::INFINITY;
    for k in 0..=40 {
        if let Some((_, l)) = lay.wing_surface(c * (k as f64 / 40.0), y) {
            lowest = lowest.min(l.as_m());
        }
    }
    m(lowest - BOOM_BELOW_WING_M - lay.parts.boom.od.as_m() / 2.0)
}

fn vertical(lay: &mut Layout) {
    let z_cg = if lay.items.is_empty() {
        lay.pod_zc.as_m()
    } else {
        lay.items.iter().map(|i| i.mass.as_kg() * i.z.as_m()).sum::<f64>() / lay.mass().as_kg()
    };
    // the pusher on the CG's height, within the pod's own height
    let pzc = lay.pod_zc.as_m();
    let ph = lay.pod_height.as_m();
    lay.pusher_z = m(z_cg.clamp(pzc - ph / 4.0, -0.004));
    // a folding prop folds back for take-off and landing: only a fixed
    // pusher's tip has to clear the ground
    let mut lowest = (pzc - ph / 2.0).min(lay.boom_z.as_m() - lay.parts.boom.od.as_m() / 2.0);
    if !lay.folding_pusher() {
        lowest = lowest.min(lay.pusher_z.as_m() - lay.pusher_radius.as_m());
    }
    let ground = lowest - (GROUND_CLEARANCE_M + 0.005);
    lay.ground_z = m(ground);
    lay.leg_height = m(((lay.boom_z.as_m() - lay.parts.boom.od.as_m() / 2.0) - ground).max(0.02));
}

/// Distance between the pusher's disc and a lift disc centred at `x_lift`
/// on a boom: the pusher a slab PUSHER_DISC_THICK deep in x, a lift disc a
/// slab LIFT_DISC_THICK deep in z. Each horizontal section of the pusher
/// slab is a rectangle and the lift disc a vertical circle extrusion, so the
/// distance from one section is exact; sections every hundredth of a radius.
pub fn pusher_gap(lay: &Layout, x_pusher: f64, x_lift: f64) -> f64 {
    let (rp, rl) = (lay.pusher_radius.as_m(), lay.lift_radius.as_m());
    let (x0, x1) = (x_pusher, x_pusher + PUSHER_DISC_THICK_M);
    let (z0, z1) = (lay.lift_disc_z.as_m(), lay.lift_disc_z.as_m() + LIFT_DISC_THICK_M);
    let dx = (x0 - x_lift).max(x_lift - x1).max(0.0);
    let pz = lay.pusher_z.as_m();
    let by = lay.boom_y.as_m();
    let mut best = f64::INFINITY;
    for i in 0..=200 {
        let z = pz + rp * (i as f64 / 100.0 - 1.0);
        let w = (rp * rp - (z - pz).powi(2)).max(0.0).sqrt();
        let across = ((dx).hypot((by - w).max(0.0)) - rl).max(0.0);
        let up = (z0 - z).max(z - z1).max(0.0);
        best = best.min(across.hypot(up));
    }
    best
}

fn rear_clear_of_pusher(lay: &Layout, x_pusher: f64, x_rear: f64) -> f64 {
    if pusher_gap(lay, x_pusher, x_rear) >= LAYOUT_GAP_M {
        return x_rear;
    }
    let mut lo = x_rear.max(x_pusher + PUSHER_DISC_THICK_M);
    let mut hi = x_pusher + PUSHER_DISC_THICK_M + lay.lift_radius.as_m() + LAYOUT_GAP_M;
    if lo >= hi || pusher_gap(lay, x_pusher, lo) >= LAYOUT_GAP_M {
        return lo;
    }
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if pusher_gap(lay, x_pusher, mid) >= LAYOUT_GAP_M {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// Every mass the aircraft carries, at its station. Printed parts are
/// estimated from the skin model here; the sliced grams check them.
#[allow(clippy::type_complexity)]
fn items(lay: &mut Layout, stacks: &[(&'static str, [f64; 3], f64, f64)]) {
    let p = lay.parts.clone();
    let d = lay.design.clone();
    let rho_shell = p.shell.density_kg_m3;
    let rho_struct = p.structural.density_kg_m3;
    let mut v: Vec<MassItem> = vec![];
    let mut add = |name, group, kg: f64, x: f64, z: f64| v.push(MassItem { name, group, mass: Mass::kg(kg), x: m(x), z: m(z) });
    let mac = lay.mac.as_m();
    let semi = lay.semi_span().as_m();
    let area = lay.area.as_m2();
    let skin = |a: f64, tc: f64| 2.0 * a * (1.0 + 0.2 * tc) * SKIN_WALL_M * rho_shell * WING_INTERNAL_FACTOR;
    let z_wing = 0.03 * mac;
    add("wing (printed)", "structure", skin(area, p.wing_airfoil.t_max), 0.42 * mac, z_wing);
    let msx = lay.main_spar_x.as_m();
    add("main spar", "structure", p.main_spar.kg_per_m * 2.0 * MAIN_SPAR_REACH * semi, msx, lay.main_spar_z.as_m());
    add("spar joiner", "structure", p.joiner.kg_per_m * 2.0 * JOINER_HALF_FRAC * semi, msx, lay.main_spar_z.as_m());
    add("rear spar", "structure", p.rear_spar.kg_per_m * 2.0 * REAR_SPAR_REACH * semi, lay.rear_spar_x.as_m(), lay.rear_spar_z.as_m());
    let tail = skin(lay.h_area.as_m2(), p.tail_airfoil.t_max) + skin(2.0 * lay.v_area_each.as_m2(), p.tail_airfoil.t_max);
    let boom_top = lay.boom_z.as_m() + p.boom.od.as_m() / 2.0;
    add("tail (printed)", "structure", tail, lay.x_tail_le.as_m() + 0.4 * lay.h_chord.as_m(), boom_top + 0.3 * lay.v_height.as_m());
    let boom_len = (lay.boom_x1 - lay.boom_x0).as_m();
    add("booms", "structure", 2.0 * p.boom.kg_per_m * boom_len, (lay.boom_x0 + lay.boom_x1).as_m() / 2.0, lay.boom_z.as_m());
    let pod_len = (lay.pod_x1 - lay.pod_x0).as_m().max(0.25);
    let pod_wet = std::f64::consts::PI * (lay.pod_width + lay.pod_height).as_m() / 2.0 * pod_len * 0.85;
    add("pod (printed)", "structure", pod_wet * POD_WALL_M * rho_shell * POD_INTERNAL_FACTOR, lay.pod_x0.as_m() + 0.45 * pod_len, lay.pod_zc.as_m());
    // printed fittings: their SOLID volume as drawn (the shapes the CAD
    // and the slicer read) times the filament's density -- an upper bound
    // on a 4-wall, 40 % infill print, so the slicer's grams cannot exceed it
    {
        let geo = crate::geometry::parts(lay);
        for (prefix, name) in [("leg_", "landing legs"), ("lift_mount_", "lift motor mounts"), ("boom_clamp_", "boom clamps"), ("tail_joint_", "tail joints"), ("pusher_mount", "pusher mount")] {
            for g in geo.iter().filter(|g| g.name.starts_with(prefix)) {
                let (lo, hi) = shape_x_z(&g.shape);
                add(name, "structure", g.shape.volume_m3() * rho_struct, lo, hi);
            }
        }
    }
    let zm = (lay.mount_top_z.as_m() + lay.motor_top_z.as_m()) / 2.0;
    let hub = |pr: &Prop| if pr.folding { FOLDING_HUB_KG } else { 0.0 };
    for x in [lay.x_front, lay.x_rear] {
        add("lift motors", "propulsion", 2.0 * p.lift_motor.mass.as_kg(), x.as_m(), zm);
        add("lift props", "propulsion", 2.0 * (p.lift_prop.mass.as_kg() + hub(p.lift_prop)), x.as_m(), lay.lift_disc_z.as_m());
    }
    let px = lay.pusher_x.as_m();
    add("cruise motor", "propulsion", p.cruise_motor.mass.as_kg(), px - 0.012 - p.cruise_motor.bell_length().as_m() / 2.0, lay.pusher_z.as_m());
    add("cruise prop", "propulsion", p.cruise_prop.mass.as_kg() + hub(p.cruise_prop), px, lay.pusher_z.as_m());
    let bay_x0 = lay.battery_x.as_m() - lay.battery.size[0].as_m() / 2.0 - stacks.iter().map(|s| s.2 + s.1[0] / 2.0).fold(0.0, f64::max) - BAY_GAP_M;
    for (name, _, x, z) in stacks {
        let kg = match *name {
            "lift_esc" => p.lift_esc.mass.as_kg(),
            "flight_controller" => p.fc.mass.as_kg(),
            "receiver" => p.receiver.mass.as_kg(),
            "gps" => p.gps.mass.as_kg(),
            _ => p.airspeed.map(|a| a.mass.as_kg()).unwrap_or(0.0),
        };
        add(name, "avionics", kg, bay_x0 + x, lay.pod_zc.as_m() + z);
    }
    let bxe = lay.battery_x.as_m() + lay.battery.size[0].as_m() / 2.0;
    add("pusher_esc", "propulsion", p.cruise_esc.mass.as_kg(), bxe + 0.02, lay.pod_zc.as_m());
    add("battery", "energy", lay.battery.pack.mass.as_kg(), lay.battery_x.as_m(), lay.pod_zc.as_m());
    let sv = p.servo.mass.as_kg();
    add("aileron servos", "avionics", 2.0 * sv, lay.aileron_servo[0].as_m(), lay.aileron_servo[2].as_m());
    add("tail servos", "avionics", 3.0 * sv, lay.x_tail_le.as_m() - 0.03, lay.boom_z.as_m());
    // wiring: three phases from the bay to each lift motor along its boom at
    // the gauge rated for the motor's maximum current; the battery lead at
    // the pack's burst; servo leads and the pusher's at 50 g (ASSUMPTION)
    let bay_x = bay_x0 + 0.05;
    let by = lay.boom_y.as_m();
    let reach = by.hypot(bay_x - lay.x_front.as_m()) + by.hypot(lay.x_rear.as_m() - bay_x);
    let gauge_kg_m = |amps: f64| {
        WIRE_AWG.iter().rev().find(|w| w.2 >= amps).map(|w| w.3).unwrap_or(WIRE_AWG[0].3) / 1000.0
    };
    let phase = 2.0 * reach * 3.0 * gauge_kg_m(p.lift_motor.max_current.as_a()) * WIRE_ROUTING;
    let lead = 2.0 * 0.15 * gauge_kg_m(lay.battery.pack.burst.as_a().min(125.0));
    add("wiring", "avionics", phase + lead + 0.05, bay_x, lay.pod_zc.as_m());
    add("hardware and glue", "structure", HARDWARE_KG, 0.3 * lay.root_chord.as_m(), 0.0);
    let pay = lay.design.requirements.payload_g;
    if pay > 0.0 {
        add("payload", "payload", pay / 1000.0, bay_x, lay.pod_zc.as_m());
    }
    let _ = d;
    lay.items = v;
}

/// A shape's centroid (x, z), m: the mean of its defining points.
fn shape_x_z(s: &crate::geometry::Shape) -> (f64, f64) {
    use crate::geometry::Shape;
    match s {
        Shape::Box { min, max } => ((min[0] + max[0]) / 2.0, (min[2] + max[2]) / 2.0),
        Shape::Cyl { p0, p1, .. } | Shape::Tube { p0, p1, .. } => ((p0[0] + p1[0]) / 2.0, (p0[2] + p1[2]) / 2.0),
        Shape::Loft { rings } | Shape::Shell { outer: rings, .. } => {
            let pts: Vec<&[f64; 3]> = rings.iter().flatten().collect();
            let n = pts.len() as f64;
            (pts.iter().map(|p| p[0]).sum::<f64>() / n, pts.iter().map(|p| p[2]).sum::<f64>() / n)
        }
    }
}
