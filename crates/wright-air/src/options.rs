//! What each catalog-backed design field can take (`Domain::options`), so
//! an agent picks a real row instead of guessing an id.

use crate::catalog::{catalog, Row, Stock};
use crate::spec::Design;
use std::collections::BTreeMap;
use wright_core::CatalogOption;

/// Hardware roles the BOM buys, each a `hardware.<role>` design path.
pub const HARDWARE_ROLES: &[&str] =
    &["ca_glue", "epoxy", "heat_set_inserts", "screws_m3", "servo_extensions", "control_horns", "pushrods", "nickel_strip", "heat_shrink_pack"];

pub fn paths() -> Vec<(&'static str, &'static str)> {
    vec![
        ("wing.airfoil", "wing section (polar table data/air/airfoils.json); specs at Re 200k"),
        ("propulsion.lift_motor", "lift motor (x4)"),
        ("propulsion.lift_prop", "lift propeller, CW/CCW pair"),
        ("propulsion.lift_esc", "lift ESC (4-in-1 or single; spec per_motor_current_limit says if its firmware can limit each motor)"),
        ("propulsion.lift_current_limit_a", "per-motor current limit set in each lift ESC's AM32 firmware, A (even numbers 2-200, or null for none); needs a lift ESC with per_motor_current_limit"),
        ("propulsion.cruise_motor", "pusher motor"),
        ("propulsion.cruise_prop", "pusher propeller"),
        ("propulsion.cruise_esc", "pusher ESC"),
        ("battery", "battery: a pack product {kind: pack, id, count 1-3} or hand-built cells {kind: cells, id, s, p 1-8}"),
        ("avionics.fc", "flight controller"),
        ("avionics.gps", "GPS / compass"),
        ("avionics.receiver", "RC receiver"),
        ("avionics.airspeed", "airspeed sensor"),
        ("avionics.servo", "control-surface servo (x5)"),
        ("structure.main_spar", "carbon main spar tube"),
        ("structure.rear_spar", "carbon rear spar tube or rod"),
        ("structure.joiner", "carbon spar joiner"),
        ("structure.boom_tube", "carbon boom tube"),
        ("structure.shell_filament", "skin filament (vase mode)"),
        ("structure.structural_filament", "fittings filament"),
        ("hardware.ca_glue", "CA glue"),
        ("hardware.epoxy", "epoxy"),
        ("hardware.heat_set_inserts", "heat-set inserts"),
        ("hardware.screws_m3", "M3 screws"),
        ("hardware.servo_extensions", "servo extension leads"),
        ("hardware.control_horns", "control horns"),
        ("hardware.pushrods", "pushrods"),
        ("hardware.nickel_strip", "nickel strip (hand-built pack)"),
        ("hardware.heat_shrink_pack", "pack heat shrink (hand-built pack)"),
    ]
}

fn opt(r: &Row, specs: Vec<(&str, String)>) -> CatalogOption {
    let mut m: BTreeMap<String, String> = specs.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    m.insert("stock".into(), r.stock.label().into());
    CatalogOption {
        id: r.id.clone(),
        label: r.name.clone(),
        specs: m,
        price_usd: r.price.map(|p| p.as_usd()),
        in_stock: match r.stock {
            Stock::InStock => Some(true),
            Stock::Unstated => None,
            _ => Some(false),
        },
        url: r.provenance.url.clone(),
    }
}

fn n(x: f64) -> String {
    format!("{}", (x * 100.0).round() / 100.0)
}

/// The options at `path`, filtered by `query` (case-insensitive substring of
/// id or label), in catalog (id) order.
pub fn options(_design: &Design, path: &str, query: Option<&str>) -> Vec<CatalogOption> {
    let c = catalog();
    let mut v: Vec<CatalogOption> = match path {
        "wing.airfoil" => crate::airfoil::names()
            .into_iter()
            .filter_map(crate::airfoil::get)
            .map(|af| {
                let re = 2e5;
                let over = crate::rules::clmax_over(&af.name);
                let (clmax, _) = af.cl_max(re);
                let ld = (0..=40).map(|k| -2.0 + 0.5 * k as f64).map(|a| af.cl(a, re) / af.cd(a, re).max(1e-6)).fold(f64::NEG_INFINITY, f64::max);
                let cd0 = (0..=40).map(|k| -4.0 + 0.5 * k as f64).map(|a| af.cd(a, re)).fold(f64::INFINITY, f64::min);
                let mut m = BTreeMap::new();
                m.insert("clmax_corrected".to_string(), n(clmax / (1.0 + over)));
                m.insert("clmax_table".to_string(), n(clmax));
                m.insert("clmax_over_prediction_pct".to_string(), n(100.0 * over));
                m.insert("cd_min".to_string(), format!("{cd0:.4}"));
                m.insert("ld_max".to_string(), n(ld));
                m.insert("thickness_pct".to_string(), n(100.0 * af.t_max));
                m.insert("camber_pct".to_string(), n(100.0 * af.camber));
                CatalogOption { id: af.name.clone(), label: af.name.to_uppercase(), specs: m, price_usd: None, in_stock: None, url: "https://m-selig.ae.illinois.edu/ads/coord_database.html".into() }
            })
            .collect(),
        "propulsion.lift_current_limit_a" => (1..=100)
            .map(|k| {
                let a = 2.0 * k as f64;
                let mut m = BTreeMap::new();
                m.insert("am32_limits_current".to_string(), k.to_string());
                m.insert("needs".to_string(), "a lift ESC whose row has per_motor_current_limit (e.g. holybro_tekko32_f4_45a_single_am32, four of them)".to_string());
                CatalogOption { id: format!("{a}"), label: format!("{a:.0} A per motor"), specs: m, price_usd: None, in_stock: None, url: "https://github.com/am32-firmware/AM32/blob/main/Src/main.c".into() }
            })
            .collect(),
        "propulsion.lift_motor" | "propulsion.cruise_motor" => c
            .motors
            .values()
            .map(|m| {
                let mut s = vec![("kv", n(m.kv)), ("max_current_a", n(m.max_current.as_a())), ("mass_g", n(m.mass.as_g())), ("resistance_ohm", n(m.resistance_ohm))];
                if let (Some(lo), Some(hi)) = (m.cells_min, m.cells_max) {
                    s.push(("cells", format!("{lo}-{hi}S")));
                }
                if let Some(i) = &m.prop_interface {
                    s.push(("prop_mount", format!("bore {:?} mm, bolts {:?}, adapters {:?}", i.bore_mm, i.bolts, i.adapters_mm)));
                }
                opt(&m.row, s)
            })
            .collect(),
        "propulsion.lift_prop" | "propulsion.cruise_prop" => c
            .props
            .values()
            .filter(|p| path != "propulsion.lift_prop" || p.rotation.as_deref() == Some("pair"))
            .map(|p| {
                let mut s = vec![("diameter_in", n(p.diameter_in)), ("pitch_in", n(p.pitch_in)), ("blades", p.blades.to_string()), ("folding", p.folding.to_string()), ("mass_g", n(p.mass.as_g()))];
                if let Some(r) = &p.rotation {
                    s.push(("rotation", r.clone()));
                }
                if let Some(h) = &p.hub {
                    s.push(("hub", format!("bore {:?} mm, bolts {:?}", h.bore_mm, h.bolts)));
                }
                opt(&p.row, s)
            })
            .collect(),
        "propulsion.lift_esc" | "propulsion.cruise_esc" => c
            .escs
            .values()
            .map(|e| {
                let mut s = vec![("channels", e.channels.to_string()), ("cont_current_a", n(e.cont_current.as_a())), ("mass_g", n(e.mass.as_g()))];
                s.push(("per_motor_current_limit", e.current_limit_source.is_some().to_string()));
                if let Some(b) = e.burst_current {
                    s.push(("burst_current_a", n(b.as_a())));
                }
                opt(&e.row, s)
            })
            .collect(),
        "battery" => c
            .packs
            .values()
            .map(|p| opt(&p.row, vec![("kind", "pack".into()), ("cells", format!("{}S{}P", p.s, p.p)), ("capacity_mah", n(p.capacity.as_mah())), ("mass_g", n(p.mass.as_g())), ("max_cont_a", n(p.max_cont.as_a()))]))
            .chain(c.cells.values().map(|x| opt(&x.row, vec![("kind", "cells".into()), ("format", x.format.clone()), ("capacity_mah", n(x.capacity.as_mah())), ("mass_g", n(x.mass.as_g())), ("max_cont_a", n(x.max_cont.as_a()))])))
            .collect(),
        "avionics.fc" => c.fcs.values().map(|f| opt(&f.row, vec![("pwm_outputs", f.pwm_outputs.to_string()), ("servo_bec_a", n(f.servo_bec.as_a())), ("mcu", f.mcu.clone()), ("mass_g", n(f.mass.as_g()))])).collect(),
        "avionics.gps" => c.gps.values().map(|m| opt(&m.row, vec![("spec", m.key_spec.trim().into()), ("mass_g", n(m.mass.as_g()))])).collect(),
        "avionics.receiver" => c.receivers.values().map(|m| opt(&m.row, vec![("spec", m.key_spec.trim().into()), ("mass_g", n(m.mass.as_g()))])).collect(),
        "avionics.airspeed" => c.airspeed.values().map(|m| opt(&m.row, vec![("spec", m.key_spec.trim().into()), ("mass_g", n(m.mass.as_g()))])).collect(),
        "avionics.servo" => c.servos.values().map(|x| opt(&x.row, vec![("dims_mm", format!("{} x {} x {}", n(x.dims[0].as_mm()), n(x.dims[1].as_mm()), n(x.dims[2].as_mm()))), ("mass_g", n(x.mass.as_g()))])).collect(),
        "structure.main_spar" | "structure.rear_spar" | "structure.joiner" | "structure.boom_tube" => c
            .tubes
            .values()
            .map(|t| {
                let (od, id) = (t.od.as_m(), t.id.map(|x| x.as_m()).unwrap_or(0.0));
                let i_m4 = std::f64::consts::PI / 64.0 * (od.powi(4) - id.powi(4));
                let e_pa = match t.kind {
                    crate::catalog::TubeKind::Pultruded => crate::rules::CF_PULTRUDED.modulus_pa,
                    crate::catalog::TubeKind::RollWrapped => crate::rules::CF_ROLLED.modulus_pa,
                };
                opt(
                    &t.row,
                    vec![
                        ("od_mm", n(od * 1e3)),
                        ("id_mm", n(id * 1e3)),
                        ("wall_mm", n((od - id) / 2.0 * 1e3)),
                        ("kind", format!("{:?}", t.kind)),
                        ("ei_n_m2", n(e_pa * i_m4)),
                        ("kg_per_m", format!("{:.4}", t.kg_per_m)),
                        ("stock_length_mm", n(t.stock_length.as_mm())),
                    ],
                )
            })
            .collect(),
        "structure.shell_filament" | "structure.structural_filament" => c.filaments.values().map(|f| opt(&f.row, vec![("density_kg_m3", n(f.density_kg_m3)), ("spool_g", n(f.spool.as_g()))])).collect(),
        p if p.starts_with("hardware.") => {
            let role = &p["hardware.".len()..];
            c.hardware.values().filter(|h| h.role.as_deref() == Some(role)).map(|h| opt(&h.row, vec![("role", role.into())])).collect()
        }
        _ => vec![],
    };
    if let Some(q) = query.map(str::to_lowercase).filter(|q| !q.is_empty()) {
        v.retain(|o| o.id.to_lowercase().contains(&q) || o.label.to_lowercase().contains(&q));
    }
    v
}
