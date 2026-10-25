//! Student liquid rockets, down to the engine: equilibrium thermochemistry
//! (CEA method), engine sizing, vehicle, stability, trajectory, checks.

pub mod aero;
pub mod atmosphere;
pub mod catalog;
pub mod domain;
pub mod engine;
pub mod eval;
pub mod equilibrium;
pub mod geometry;
pub mod ideal;
pub mod nitrous;
pub mod options;
pub mod performance;
pub mod propellant;
pub mod reference;
pub mod rules;
pub mod sim;
pub mod thermo;
pub mod trajectory;
pub mod vehicle;

pub use domain::{Rocket, RocketCheck, RocketDesign, RocketSpec};
