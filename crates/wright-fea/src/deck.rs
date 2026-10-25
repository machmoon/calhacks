//! CalculiX input decks. The card order and the step/output cards follow
//! FreeCAD's CalculiX writer (FreeCAD `src/Mod/Fem/femsolver/calculix/`:
//! `write_mesh.py`, `write_femelement_material.py`,
//! `write_femelement_geometry.py`, `write_constraint_fixed.py`,
//! `write_step_equation.py` -- `*STATIC` / `*FREQUENCY` with the mode count
//! / `*BUCKLE` with the factor count -- and `write_step_output.py` --
//! `*NODE FILE` U, `*EL FILE` S, E, `*NODE PRINT ... TOTALS=ONLY` RF).
//! One analysis per deck, so a frequency step never inherits a preload.

use crate::mesh::{Mesh, V3};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub enum Elastic {
    Iso { e: f64, nu: f64 },
    /// `*ELASTIC, TYPE=ENGINEERING CONSTANTS`: E1 E2 E3 nu12 nu13 nu23 G12 G13 G23
    /// in the global axes (a shell in the x-y plane needs no orientation).
    Ortho { e: [f64; 3], nu: [f64; 3], g: [f64; 3] },
}

#[derive(Debug, Clone)]
pub struct Material {
    pub name: String,
    pub elastic: Elastic,
    pub density: f64,
    /// Allowable (or yield) stress the margin is taken against, Pa.
    pub allowable_pa: f64,
    pub source: String,
}

impl Material {
    pub fn iso(name: &str, e: f64, nu: f64, density: f64, allowable_pa: f64, source: &str) -> Self {
        Material { name: name.into(), elastic: Elastic::Iso { e, nu }, density, allowable_pa, source: source.into() }
    }
}

#[derive(Debug, Clone)]
pub enum Section {
    Shell { set: String, material: String, thickness: f64 },
    Solid { set: String, material: String },
}

#[derive(Debug, Clone)]
pub enum Analysis {
    Static,
    Frequency { modes: usize },
    Buckle { factors: usize },
}

#[derive(Debug, Clone)]
pub struct Model {
    pub mesh: Mesh,
    pub materials: Vec<Material>,
    pub sections: Vec<Section>,
    /// (node, first dof, last dof), held at zero.
    pub fixed: Vec<(usize, u8, u8)>,
    /// Lumped masses (node, kg).
    pub masses: Vec<(usize, f64)>,
    pub analysis: Analysis,
    /// Concentrated forces (node, vector), N. Static and buckle steps.
    pub forces: Vec<(usize, V3)>,
    /// Shell pressure (element set, Pa) and solid face pressure
    /// (element index 0-based, face number, Pa).
    pub shell_pressure: Vec<(String, f64)>,
    pub face_pressure: Vec<(usize, usize, f64)>,
}

impl Model {
    pub fn new(mesh: Mesh, analysis: Analysis) -> Self {
        Model { mesh, materials: vec![], sections: vec![], fixed: vec![], masses: vec![], analysis, forces: vec![], shell_pressure: vec![], face_pressure: vec![] }
    }
    pub fn fix(&mut self, nodes: &[usize], first: u8, last: u8) {
        self.fixed.extend(nodes.iter().map(|&n| (n, first, last)));
    }

    pub fn inp(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "** written by wright-fea (CalculiX 2.23 deck)\n*HEADING\nwright-fea");
        let _ = writeln!(s, "*NODE, NSET=NALL");
        for (i, p) in self.mesh.nodes.iter().enumerate() {
            let _ = writeln!(s, "{}, {:.9e}, {:.9e}, {:.9e}", i + 1, p[0], p[1], p[2]);
        }
        // elements grouped by (type, set)
        let mut groups: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
        for (i, e) in self.mesh.elements.iter().enumerate() {
            groups.entry((e.ty, e.set.as_str())).or_default().push(i);
        }
        for ((ty, set), ids) in &groups {
            let _ = writeln!(s, "*ELEMENT, TYPE={ty}, ELSET={set}");
            for &i in ids {
                let e = &self.mesh.elements[i];
                let ns: Vec<String> = e.nodes.iter().map(|n| n.to_string()).collect();
                // ccx reads at most 16 entries per line; continue with a trailing comma
                let mut line = format!("{}", i + 1);
                for (k, n) in ns.iter().enumerate() {
                    if k == 15 {
                        let _ = writeln!(s, "{line},");
                        line = n.clone();
                    } else {
                        line = format!("{line}, {n}");
                    }
                }
                let _ = writeln!(s, "{line}");
            }
        }
        // lumped masses: one MASS element per node, one set per distinct value
        for (k, (n, kg)) in self.masses.iter().enumerate() {
            let id = self.mesh.elements.len() + 1 + k;
            let _ = writeln!(s, "*ELEMENT, TYPE=MASS, ELSET=EM{k}\n{id}, {n}\n*MASS, ELSET=EM{k}\n{kg:.9e}");
        }
        for m in &self.materials {
            let _ = writeln!(s, "*MATERIAL, NAME={}", m.name);
            match &m.elastic {
                Elastic::Iso { e, nu } => {
                    let _ = writeln!(s, "*ELASTIC\n{e:.6e}, {nu}");
                }
                Elastic::Ortho { e, nu, g } => {
                    let _ = writeln!(s, "*ELASTIC, TYPE=ENGINEERING CONSTANTS\n{:.6e}, {:.6e}, {:.6e}, {}, {}, {}, {:.6e}, {:.6e},\n{:.6e}", e[0], e[1], e[2], nu[0], nu[1], nu[2], g[0], g[1], g[2]);
                }
            }
            let _ = writeln!(s, "*DENSITY\n{:.6e}", m.density);
        }
        for sec in &self.sections {
            match sec {
                Section::Shell { set, material, thickness } => {
                    let _ = writeln!(s, "*SHELL SECTION, ELSET={set}, MATERIAL={material}\n{thickness:.9e}");
                }
                Section::Solid { set, material } => {
                    let _ = writeln!(s, "*SOLID SECTION, ELSET={set}, MATERIAL={material}");
                }
            }
        }
        if !self.fixed.is_empty() {
            let _ = writeln!(s, "*BOUNDARY");
            for (n, a, b) in &self.fixed {
                let _ = writeln!(s, "{n}, {a}, {b}");
            }
        }
        let _ = writeln!(s, "*STEP");
        match self.analysis {
            Analysis::Static => {
                let _ = writeln!(s, "*STATIC");
            }
            Analysis::Frequency { modes } => {
                let _ = writeln!(s, "*FREQUENCY\n{modes}");
            }
            Analysis::Buckle { factors } => {
                let _ = writeln!(s, "*BUCKLE\n{factors}");
            }
        }
        if !matches!(self.analysis, Analysis::Frequency { .. }) {
            if !self.forces.is_empty() {
                let _ = writeln!(s, "*CLOAD");
                for (n, f) in &self.forces {
                    for (d, v) in f.iter().enumerate() {
                        if *v != 0.0 {
                            let _ = writeln!(s, "{n}, {}, {v:.9e}", d + 1);
                        }
                    }
                }
            }
            if !self.shell_pressure.is_empty() || !self.face_pressure.is_empty() {
                let _ = writeln!(s, "*DLOAD");
                for (set, p) in &self.shell_pressure {
                    let _ = writeln!(s, "{set}, P, {p:.9e}");
                }
                for (e, f, p) in &self.face_pressure {
                    let _ = writeln!(s, "{}, P{f}, {p:.9e}", e + 1);
                }
            }
        }
        let _ = writeln!(s, "*NODE FILE\nU");
        let _ = writeln!(s, "*EL FILE\nS");
        let _ = writeln!(s, "*END STEP");
        s
    }
}
