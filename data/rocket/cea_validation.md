# NASA CEA validation data for the Rust Gibbs solver

Generated 2026-10-24. Machine-readable copy: `cea_validation.json` (same folder).

## Sources

- NASA CEA is open source: https://github.com/nasa/cea (Apache-2.0). PyPI package `cea` 3.3.4
  (https://pypi.org/project/cea/3.3.4/, author Mark Leader, NASA GRC).
- Published rocket outputs: RP-1311 examples 8 to 13, rendered in
  https://github.com/nasa/cea/tree/main/docs/source/examples/rocket (example8.rst = H2(L)/O2(L)).
  Original report: McBride & Gordon, NASA RP-1311 Part II, https://ntrs.nasa.gov/citations/19960044559.
  Input deck: https://github.com/nasa/cea/blob/main/samples/rp1311_examples.inp
- thermo.inp: https://raw.githubusercontent.com/nasa/cea/main/data/thermo.inp, saved raw as
  `thermo.inp`. Its header line is `200.00 1000.00 6000.00 20000. 9/8/2021`.
- Alternative 7-coefficient source: GRI-Mech 3.0,
  http://combustion.berkeley.edu/gri-mech/version30/files30/thermo30.dat (HTTP 200 on 2026-10-24).
- Loose cross-check: http://www.braeunig.us/space/propel.htm, a table at Pc = 68 atm and pe = 1 atm
  that gives only sea-level Isp.

## Method

`cea_reference.py` runs the official CEA 3.3.4 with the API copied from example8.rst
(`RocketSolver`, `iac=True`, chamber enthalpy `hc` from the reactant weights and temperatures).
All runs use shifting equilibrium and an infinite-area combustor (IAC). The exit is at
pc/pe = Pc / 1.01325 bar.

**Check on the harness:** re-running Example 8 this way reproduces the published example8.rst
block to every printed digit.

## Validation table

All values are from CEA 3.3.4. γ is gamma_s in the chamber. Isp_SL and Isp_vac are at the exit,
where pe = 1 atm.

| Case | Inputs | Tc K | M | γ | c* m/s | Cf | Isp_SL m/s | Isp_vac m/s | Te K |
|---|---|---|---|---|---|---|---|---|---|
| H2/O2 RP-1311 Ex.8 (**published**) | H2(L) 20.27 K, O2(L) 90.17 K, O/F 5.55157, Pc 53.3172 bar | 3383.845 | 12.716 | 1.145 | 2332.34 | 1.2723 (pc/pe=10) | 2967.386 (pc/pe=10) | 3515.549 (pc/pe=10) | 2567.340 |
| LOX/ethanol | C2H5OH(L) 298.15, O2(L) 90.17, Pc 20, O/F 1.3 | 2984.98 | 20.888 | 1.1618 | 1697.56 | 1.4051 | 2385.2 | 2693.7 | 1875.1 |
| LOX/ethanol | Pc 20, O/F 1.5 | 3189.36 | 22.170 | 1.1346 | 1720.43 | 1.4181 | 2439.8 | 2770.0 | 2227.8 |
| LOX/ethanol | Pc 30, O/F 1.3 | 3005.24 | 20.928 | 1.1661 | 1699.43 | 1.4737 | 2504.5 | 2777.5 | 1742.5 |
| LOX/ethanol | Pc 30, O/F 1.5 | 3226.84 | 22.250 | 1.1376 | 1725.54 | 1.4897 | 2570.5 | 2863.8 | 2087.0 |
| LOX/RP-1 | RP-1 298.15, O2(L) 90.17, Pc 68.9, O/F 2.4 | 3616.47 | 22.736 | 1.1436 | 1804.89 | 1.6235 | 2930.3 | 3181.6 | 2129.2 |
| LOX/CH4 | CH4(L) 111.643, O2(L) 90.17, Pc 30, O/F 3.0 | 3373.82 | 20.265 | 1.1345 | 1851.62 | 1.4972 | 2772.2 | 3096.2 | 2303.6 |
| N2O/IPA (gas IPA) | C3H8O,2propanol(g) 298.15, N2O(g) 298.15, Pc 30, O/F 3.0 | 2559.77 | 21.629 | 1.2435 | 1507.29 | 1.4532 | 2190.4 | 2411.0 | 1259.2 |
| N2O/ethanol | C2H5OH(L) 298.15, N2O(g) 298.15, Pc 30, O/F 4.0 | 3066.91 | 24.689 | 1.1679 | 1579.01 | 1.4664 | 2315.5 | 2561.4 | 1716.9 |

### Comparison with the Braeunig table (Pc = 68 atm = 68.9 bar, pe = 1 atm)

| Pair | O/F | Braeunig Isp_SL s | CEA 3.3.4 Isp_SL s | CEA Tc K | CEA c* m/s |
|---|---|---|---|---|---|
| LOX/LH2 | 5.00 | 381 | 387.1 | 3282.57 | 2371.67 |
| LOX/CH4(L) | 2.77 | 299 | 305.9 | 3362.00 | 1867.53 |
| LOX/RP-1 | 2.29 | 289 | 297.4 | 3567.39 | 1807.29 |
| LOX/75% ethanol + 25% water | 1.29 | 269 | 275.2 | 3153.89 | 1666.37 |

Braeunig's Isp is 2 to 3% lower than CEA's in every row. His page does not say how the numbers were
computed, so use this table only as a loose check. The CEA rows are the reference.

## Reactant records from thermo.inp (assigned enthalpy at the given temperature)

| Species | T K | H, J/mol | Note |
|---|---|---|---|
| O2(L) | 90.17 | -12979.0 | |
| H2(L) | 20.27 | -9012.0 | |
| CH4(L) | 111.643 | -89233.0 | |
| RP-1 | 298.15 | -24717.7 | Formula CH1.95, MW 13.976183 |
| C2H5OH(L) | 298.15 | -277510.0 | Hf298; also has a 159 to 390 K fit |
| H2O(L) | 298.15 | -285830.0 | Hf298 |
| N2O (gas) | 298.15 | 81600.0 | Hf298; no N2O(L) in the file |
| C3H8O,2propanol | 298.15 | -272700.0 | **Gas** only; no liquid IPA in the file |
| Jet-A(L) | 298.15 | -303403.0 | |

## Product records

The JSON holds the parsed 9-coefficient data for each of these species: a1 to a7, b1, b2, the
temperature ranges and the raw lines. Hf298 is in J/mol.

| Species | Hf298 | Species | Hf298 | Species | Hf298 |
|---|---|---|---|---|---|
| H2O | -241826 | O2 | 0 | N2 | 0 |
| CO2 | -393510 | OH | 37278.206 | NO | 91271.31 |
| CO | -110535.196 | H | 217998.828 | N | 472680 |
| H2 | 0 | O | 249175.003 | HO2 | 12020 |

N2O, NO2, CH4 and Ar are included as well.

## Caveats

- **No liquid IPA in thermo.inp.** The N2O/IPA row uses gas IPA, which skips the heat of
  vaporization, so it overstates Tc and Isp. For the liquid case, use N2O/ethanol(L), or pass a
  user-assigned liquid enthalpy.
- RP-1311 Example 12 (MMH/N2O4, frozen at the throat) is a second published output if frozen flow
  needs checking. See example12.rst.
