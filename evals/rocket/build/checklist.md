# Before launch

- Proof-test each tank alone with water to 94.6 bar (N2O) and 54.9 bar (fuel), 1.5 x MEOP, before any propellant: ports plugged, relief valve and main valve removed (they are rated for MEOP, 915 / 531 psi, not for proof); 22 + 12 radial closure screws per bulkhead
- N2O: load 28.3 kg into the 44.0 L run tank and hold it between 5 and 30 C with a heater blanket (vapour pressure 35.4-63.1 bar; the checks fly both ends). Set the relief at 915 psi and the fuel regulator to 33.3 bar. Confirm the nitrous grade: speed-shop fills can carry a sulfur dioxide additive
- Cold-flow the feed system: command the N2O main valve open 1.00 s after igniter light and the fuel main 1.25 s after (N2O lead; propellant reaches the injector later by the 0.30 s valve stroke plus line fill, which the start_sequence check reports)
- Static-fire before flight: confirm Pc 20.0 bar and O/F 7.70 against the engine table; the igniter must burn 2.6 s
- Weigh the vehicle and enter the measured dry mass and CG in the spec (measured_dry); re-run the checks before flying
- Ground-test both altimeters (featherweight_blue_raven, eggtimer_quasar) and the GPS tracker (featherweight_gps_tracker); dual-deploy charges sized and ground-tested for the drogue (fc_classic_elliptical_48in) and main (fc_iris_ultra_compact_168in)
- Rail: the design leaves the rail at 109 ft/s; use the rail length it was checked for and confirm the rail guides slide freely
- Predicted apogee 19369 ft (dispersion band 17110-21816 ft): file it as the contract altitude
