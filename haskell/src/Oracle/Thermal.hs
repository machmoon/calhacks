-- | The payload box's thermal and power relations behind the balloon's
-- cold checks (crates/wright-balloon/src/analysis.rs `ThermalModel`,
-- rules.rs), restated from the primary relations with nothing taken from
-- the Rust:
--
--   lumped capacitance   C dT/dt = P - (T - T_amb) / R_th, stepped forward
--                        Euler: T' = T + (P - (T - T_amb) / R_th) dt / C
--                        (Incropera & DeWitt, Fundamentals of Heat and Mass
--                        Transfer, ch. 5.1-5.2, the lumped capacitance method)
--   closed form          T_n = T_ss + (T_0 - T_ss) (1 - dt / (R_th C))^n for
--                        constant P and T_amb, T_ss = T_amb + P R_th: the
--                        exact solution of the Euler recurrence, which the
--                        oracle keeps separately from the stepped one
--   thermal resistance   R_th = t / (k A_cond) + 1 / (h A_out): the wall's
--                        conduction and the outer film in series (Incropera
--                        3.1, eq. 3.19), A_cond = sqrt (A_out A_in) the
--                        geometric-mean area of a wall whose two faces differ
--   film coefficient     h = h_sl sqrt (rho / rho_sl): laminar forced
--                        convection, Nu ~ Re^(1/2) (Incropera 7.2, eq. 7.30),
--                        at one ascent speed so Re scales with rho alone
--   iron hand warmer     P = m_packet f_contents f_Fe eta dH_Fe / t_rated,
--                        dH_Fe = 824.2 kJ / (2 x 55.845 g) for Fe -> 1/2 Fe2O3
--                        (NIST-JANAF / CRC dHf(Fe2O3, s) = -824.2 kJ/mol;
--                        Fe 55.845 g/mol). The packet mass and the fractions
--                        are the design's assumptions (rules.rs, from the
--                        Red Rocks CC 2023 HotHands paper and the Kobayashi
--                        SDS), passed in here, so that row is a
--                        transcription check of the arithmetic
--   cell capacity vs T   linear between data-sheet points, flat beyond the
--                        ends (rules.rs LIPO_CAPACITY from the LP503759 and
--                        HTCNR18650 data sheets; L91_CAPACITY from the
--                        Energizer L91 data sheet)
--
-- The Rust runs the lumped model inside `ThermalModel::case` over a whole
-- flight, driven by a Design and a Catalog; that function is not callable
-- on bare numbers, so the stepped model here is checked by its own
-- properties (test/Props.hs) and only the warmer power and the capacity
-- table are cross-checked against the crate (docs/verification.md, gaps).
module Oracle.Thermal
  ( filmCoefficient
  , thermalResistance
  , boxStep
  , boxSteady
  , boxAfter
  , boxClosedForm
  , feOxidationJPerG
  , warmerPowerW
  , capacityFraction
  , lipoCapacity
  , l91Capacity
  ) where

-- | Outside film coefficient, W/(m^2 K): sea-level value, sea-level density,
-- local density.
filmCoefficient :: Double -> Double -> Double -> Double
filmCoefficient hSl rhoSl rho = hSl * sqrt (rho / rhoSl)

-- | Wall conduction plus outer film, K/W: wall thickness (m), conductivity
-- (W/(m K)), outer area, inner area (m^2), film coefficient.
thermalResistance :: Double -> Double -> Double -> Double -> Double -> Double
thermalResistance t k aOut aIn h = t / (k * sqrt (aOut * aIn)) + 1 / (h * aOut)

-- | One forward-Euler step: heat capacity (J/K), R_th (K/W), heating (W),
-- ambient (C), step (s), temperature (C).
boxStep :: Double -> Double -> Double -> Double -> Double -> Double -> Double
boxStep c rth p tAmb dt t = t + (p - (t - tAmb) / rth) * dt / c

-- | Steady temperature under constant heating.
boxSteady :: Double -> Double -> Double -> Double
boxSteady rth p tAmb = tAmb + p * rth

-- | n steps of `boxStep` from t0.
boxAfter :: Double -> Double -> Double -> Double -> Double -> Int -> Double -> Double
boxAfter c rth p tAmb dt n t0 = iterate (boxStep c rth p tAmb dt) t0 !! n

-- | The Euler recurrence solved: T_n = T_ss + (T_0 - T_ss) (1 - dt / (R C))^n.
boxClosedForm :: Double -> Double -> Double -> Double -> Double -> Int -> Double -> Double
boxClosedForm c rth p tAmb dt n t0 = tss + (t0 - tss) * (1 - dt / (rth * c)) ^ n
  where tss = boxSteady rth p tAmb

-- | Heat of oxidising iron to Fe2O3, J per gram of Fe.
feOxidationJPerG :: Double
feOxidationJPerG = 824.2e3 / (2 * 55.845)

-- | Average warmer output, W: packet mass (g), contents fraction, iron
-- fraction of the contents, fraction of the iron oxidised within the
-- rated duration, rated duration (h).
warmerPowerW :: Double -> Double -> Double -> Double -> Double -> Double
warmerPowerW packetG contents iron conversion ratedH =
  packetG * contents * iron * conversion * feOxidationJPerG / (ratedH * 3600)

-- | Capacity fraction at a cell temperature from a (T, fraction) table
-- sorted by T: flat below the first and above the last point, linear
-- between.
capacityFraction :: [(Double, Double)] -> Double -> Double
capacityFraction [] _ = 1
capacityFraction tab@((t0, f0) : _) t
  | t <= t0 = f0
  | otherwise = go tab
  where
    go ((ta, fa) : rest@((tb, fb) : _))
      | t <= tb = fa + (t - ta) / (tb - ta) * (fb - fa)
      | otherwise = go rest
    go [(_, f)] = f
    go [] = 1

-- | LiPo / Li-ion: -20 C 60 % (HTCNR18650), -10 C 70 % and 0 C 80 %
-- (LP503759), nothing below the rated -20 C.
lipoCapacity :: [(Double, Double)]
lipoCapacity = [(-20.01, 0), (-20, 0.60), (-10, 0.70), (0, 0.80), (25, 1)]

-- | Energizer L91 at 250 mA: ~1600 mAh at -40 C vs ~3550 mAh at 21 C.
l91Capacity :: [(Double, Double)]
l91Capacity = [(-40, 0.45), (21, 1)]
