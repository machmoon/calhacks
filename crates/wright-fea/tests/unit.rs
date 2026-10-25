//! Fast tests (no solver): meshes, decks and the frd reader.

use wright_fea::deck::{Analysis, Model, Section};
use wright_fea::mesh;
use wright_fea::results::{read_frd, von_mises};

#[test]
fn tube_mesh_is_closed_and_ring_loads_sum_to_the_force() {
    let st = mesh::stations(0.0, 1.0, 0.1, &[0.35]);
    assert!(st.iter().any(|s| (s - 0.35).abs() < 1e-12), "station through the mark");
    let tb = mesh::tube([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 0.01, &st, 12, |_| "T".into());
    assert_eq!(tb.mesh.elements.len(), 12 * (st.len() - 1));
    // every node used, all on the radius
    let used = tb.mesh.select(|_| true);
    assert_eq!(used.len(), tb.mesh.nodes.len());
    assert!(tb.mesh.nodes.iter().all(|p| (p[1].hypot(p[2]) - 0.01).abs() < 1e-12));
    let f = tb.ring_force(4, [0.0, 0.0, 7.0]);
    assert!((f.iter().map(|(_, v)| v[2]).sum::<f64>() - 7.0).abs() < 1e-12);
    // a ring moment has no net force and the stated moment about +y
    let m = tb.ring_moment(4, 3.0, [0.0, 1.0, 0.0]);
    let fx: f64 = m.iter().map(|(_, v)| v[0]).sum();
    let my: f64 = m.iter().map(|(n, v)| tb.mesh.node(*n)[2] * v[0]).sum();
    assert!(fx.abs() < 1e-9 && (my - 3.0).abs() < 1e-9, "fx {fx} my {my}");
}

#[test]
fn deck_has_the_cards_ccx_needs() {
    let (m, _) = mesh::quad_plate([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]], 2, 2, "P");
    let mut model = Model::new(m, Analysis::Frequency { modes: 3 });
    model.materials.push(wright_fea::validate::al6061());
    model.sections.push(Section::Shell { set: "P".into(), material: "AL6061".into(), thickness: 0.002 });
    model.fix(&[1, 2], 1, 6);
    model.masses.push((5, 0.1));
    let inp = model.inp();
    for card in ["*NODE, NSET=NALL", "*ELEMENT, TYPE=S8R, ELSET=P", "*ELEMENT, TYPE=MASS", "*MASS, ELSET=EM0", "*SHELL SECTION", "*BOUNDARY", "*FREQUENCY\n3", "*NODE FILE\nU", "*END STEP"] {
        assert!(inp.contains(card), "missing {card}");
    }
}

#[test]
fn frd_reader_reads_ccx_fixed_columns() {
    // two nodes, one block, columns as ccx 2.23 writes them (values touching)
    let frd = "    2C                             2                                     1\n -1         1 0.00000E+00 0.00000E+00-2.50000E-03\n -1         2 1.00000E+00 0.00000E+00 2.50000E-03\n -3\n    3C                             1                                     1\n -1         1    6    0    1\n -2         1         2         1         2         1         2         1         2         1         2\n -3\n  100CL  102 3.040792630          20                     2    2MODAL      1\n -4  DISP        4    1\n -5  D1          1    2    1    0\n -1         1-4.31082E-04 1.48083E-02-4.00117E-01\n -1         2 0.00000E+00 0.00000E+00 0.00000E+00\n -3\n";
    let r = read_frd(frd).unwrap();
    assert_eq!(r.nodes.len(), 2);
    assert_eq!(r.elements[0].0, 6);
    assert_eq!(r.elements[0].1.len(), 10);
    let b = &r.blocks[0];
    assert_eq!(b.name, "DISP");
    assert!((b.value - 3.040792630).abs() < 1e-9);
    assert_eq!(b.data[&1], vec![-4.31082e-4, 1.48083e-2, -4.00117e-1]);
    assert!((von_mises(&[100.0, 0.0, 0.0, 0.0, 0.0, 0.0]) - 100.0).abs() < 1e-12);
}

#[cfg(feature = "air")]
#[test]
fn balloon_petg_matches_wright_air_row() {
    let p = wright_air::rules::PETG;
    assert_eq!(p.modulus_pa, wright_fea::cases::balloon::PETG_E_PA);
    assert_eq!(p.allowable_pa, wright_fea::cases::balloon::PETG_ALLOWABLE_PA);
}

/// The closed-form validation (docs/verification.md); needs ccx and gmsh,
/// about 3 minutes: `cargo test -p wright-fea -- --ignored`.
#[test]
#[ignore]
fn validation_within_three_percent() {
    let dir = std::env::temp_dir().join(format!("wright-fea-validate-{}", std::process::id()));
    let rows = wright_fea::validate::run(&dir).unwrap();
    for r in &rows {
        assert!(r.error_pct.abs() < 3.0, "{} {}: {:+.2} %", r.case, r.quantity, r.error_pct);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
