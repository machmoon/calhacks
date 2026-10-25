-- | QuickCheck properties and published-table checks for the oracle.
module Main (main) where

import Control.Monad (unless)
import qualified Oracle.Aero as A
import Oracle.Atmosphere
import qualified Oracle.Thermal as T
import qualified Oracle.Balloon as B
import qualified Oracle.Beam as S
import qualified Oracle.Radio as R
import Oracle.Cases (evalCase)
import Oracle.Json
import qualified Oracle.Nozzle as N
import qualified Oracle.Pcb as Pcb
import System.Exit (exitFailure)
import Test.QuickCheck

relErr :: Double -> Double -> Double
relErr a b = abs (a - b) / abs b

-- | US76 Table I rows (geometric z m; T K; P Pa; rho kg/m^3; mu Pa s), as
-- transcribed in ambiance (github.com/airinnova/ambiance,
-- tests/table_data.py), 0..50.4 km.
us76Table :: [(Double, Double, Double, Double, Double)]
us76Table =
  [ (0,     288.150, 1.01325e5, 1.225,       1.7894e-5)
  , (1000,  281.651, 8.98763e4, 1.11166,     1.7579e-5)
  , (2000,  275.154, 7.95014e4, 1.00655,     1.7260e-5)
  , (11000, 216.774, 2.26999e4, 3.64801e-1,  1.4223e-5)
  , (11019, 216.650, 2.26320e4, 3.63918e-1,  1.4216e-5)
  , (15000, 216.650, 1.21118e4, 1.94755e-1,  1.4216e-5)
  , (20000, 216.650, 5.52929e3, 8.89097e-2,  1.4216e-5)
  , (20063, 216.650, 5.47487e3, 8.80345e-2,  1.4216e-5)
  , (25000, 221.552, 2.54921e3, 4.00837e-2,  1.4484e-5)
  , (32162, 228.650, 8.68014e2, 1.32249e-2,  1.4868e-5)
  , (41266, 253.850, 2.42394e2, 3.32646e-3,  1.6189e-5)
  , (47350, 270.650, 1.10906e2, 1.42752e-3,  1.7037e-5)
  , (50396, 270.650, 7.59443e1, 9.77519e-4,  1.7037e-5)
  ]

-- | NACA Report 1135, Table I (gamma = 1.4): M, p/p0, A/A*.
nacaTable :: [(Double, Double, Double)]
nacaTable =
  [ (0.50, 0.8430,  1.3398)
  , (1.00, 0.5283,  1.0000)
  , (1.50, 0.2724,  1.1762)
  , (2.00, 0.1278,  1.6875)
  , (3.00, 0.02722, 4.2346)
  ]

-- | The closed-form column of the FEA validation table in
-- docs/verification.md (6061-T6: E 68.9 GPa, nu 0.33, 2700 kg/m^3), as
-- (name, oracle value, published value). wright-fea computed that column
-- in Rust from the same primary sources; the oracle must reproduce it
-- from the formulas alone. "R 10 mm, t 1 mm" is the mid-wall radius
-- (od 21, id 19 mm), which is what reproduces 19.202 mm.
structureTable :: [(String, Double, Double)]
structureTable =
  let e = 68.9e9
      nu = 0.33
      rho = 2700
      od = 0.021
      idm = 0.019
      i = S.tubeI od idm
      mPerL = rho * S.tubeArea od idm
      cyl = S.lame 1e6 0.099 0.101
      (yc, sc, _) = S.plateClamped e nu 0.1e6 0.1 0.005
      (ys, ss) = S.plateSimple e nu 0.1e6 0.1 0.005
  in [ ("cantilever tube tip, mm (P L^3 / 3EI)", 1e3 * S.cantileverTip 100 0.5 e i, 19.202)
     , ("cantilever tube mid-span stress, MPa (M c / I)", 1e-6 * S.bendingStress 25 (od / 2) i, 83.35)
     , ("cantilever tube first mode, Hz (1.8751)", S.cantileverFirstMode e i mPerL 0.5, 80.05)
     , ("Lame hoop at the bore, MPa", 1e-6 * S.hoop cyl, 50.01)
     , ("Lame axial, MPa", 1e-6 * S.axial cyl, 24.50)
     , ("clamped plate centre deflection, mm (Roark 10b)", 1e3 * yc, 0.1940)
     , ("clamped plate centre stress, MPa (Roark 10b)", 1e-6 * sc, 19.95)
     , ("simply supported plate centre deflection, mm (Roark 10a)", 1e3 * ys, 0.7775)
     , ("simply supported plate centre stress, MPa (Roark 10a)", 1e-6 * ss, 49.95)
     , ("axial buckling load, kN (Timoshenko & Gere 11-13)", 1e-3 * S.shellBucklingClassical e nu 0.05 0.001 * 2 * pi * 0.05 * 0.001, 264.8)
     ]

-- | Textbook free-space loss: 92.45 dB at 1 km and 1 GHz (32.45 + 20 log10
-- d_km + 20 log10 f_MHz, e.g. ITU-R P.525-4 eq. 4).
radioTable :: [(String, Double, Double)]
radioTable =
  [ ("FSPL 1 km 1000 MHz, dB", R.fsplDb 1000 1000, 92.45)
  , ("FSPL 100 km 433 MHz, dB", R.fsplDb 100e3 433, 125.18)
  , ("FSPL 1 m 2400 MHz, dB", R.fsplDb 1 2400, 40.05)
  ]

-- | Hand-checkable values for the new rows. kT at 290 K from the SI
-- Boltzmann constant; the von Karman mid-length radius is R / sqrt 2
-- exactly (theta = pi/2, sin 2theta = 0); the TR-R-100 points at fineness
-- 3 come back unchanged; the loss on the radian sphere is 20 log10 2; the
-- 6.9 cm 1/4-20 screw figures are the published root area times A574.
miscTable :: [(String, Double, Double)]
miscTable =
  [ ("kT at 290 K, dBm/Hz", 10 * logBase 10 (1.380649e-23 * 290 * 1e3), -173.98)
  , ("von Karman r(L/2) / R", A.vonKarmanRadius 0.5 1 1, 1 / sqrt 2)
  , ("TR-R-100 FR3 at M 1.4", A.noseCd 1.4 3, 0.095)
  , ("TR-R-100 FR3 at M 2.0", A.noseCd 2.0 3, 0.091)
  , ("FSPL on the radian sphere, dB", R.fsplNearDb 0 433, 20 * logBase 10 2)
  , ("1/4-20 A574 single shear, kN", 1e-3 * A.screwShear (A.closureCapacity 0.003), 1e-3 * 0.6 * 180 * 6.894757e6 * 0.0269 * 0.0254 ** 2)
  , ("Fe -> Fe2O3, kJ/g", 1e-3 * T.feOxidationJPerG, 7.379)
  , ("HotHands 10 h, W (rules.rs derivation)", T.warmerPowerW 29.7 0.9 0.5 0.5 10, 1.37)
  ]

tableChecks :: [(String, Bool)]
tableChecks =
  [ ("US76 z=" ++ show z, ok)
  | (z, t, p, rho, mu) <- us76Table
  , let a = atGeometric z
        ok = abs (airT a - t) < 2.0e-3 && relErr (airP a) p < 1.0e-4
             && relErr (airRho a) rho < 1.0e-4 && relErr (airMu a) mu < 1.0e-4
  ] ++
  [ ("NACA1135 M=" ++ show m, relErr (N.pressureRatio 1.4 m) pr < 5.0e-4 && relErr (N.areaRatio 1.4 m) ar < 1.0e-4)
  | (m, pr, ar) <- nacaTable ] ++
  [ (n ++ " = " ++ show got ++ " vs " ++ show want, relErr got want < 1.0e-3) | (n, got, want) <- structureTable ] ++
  [ (n ++ " = " ++ show got ++ " vs " ++ show want, relErr got want < 2.0e-4) | (n, got, want) <- radioTable ] ++
  [ (n ++ " = " ++ show got ++ " vs " ++ show want, relErr got want < 2.0e-3) | (n, got, want) <- miscTable ]

-- Generators -------------------------------------------------------------

newtype Alt = Alt Double deriving Show
instance Arbitrary Alt where arbitrary = Alt <$> choose (0, 84000)

newtype Gamma = Gamma Double deriving Show
instance Arbitrary Gamma where arbitrary = Gamma <$> choose (1.1, 1.67)

newtype Mach = Mach Double deriving Show
instance Arbitrary Mach where arbitrary = Mach <$> choose (1.01, 6)

newtype PRatio = PRatio Double deriving Show
instance Arbitrary PRatio where arbitrary = PRatio <$> choose (1.0e-4, 0.5)

-- Atmosphere -------------------------------------------------------------

-- Each published base pressure equals the layer below integrated to its
-- top: the table and the law agree.
prop_layersContinuous :: Bool
prop_layersContinuous = and
  [ relErr (airP (atGeopotential (lH hi - 1.0e-6))) (lP hi) < 2.0e-5
    && abs (airT (atGeopotential (lH hi - 1.0e-6)) - lT hi) < 1.0e-6
  | (_, hi) <- zip layers (drop 1 layers) ]

-- Hydrostatic equilibrium dP/dH = -rho g0 (US76 eq. 31) everywhere.
prop_hydrostatic :: Alt -> Property
prop_hydrostatic (Alt h) =
  not (any (\l -> abs (h - lH l) < 2) layers) ==>
    let d = 0.5
        dpdh = (airP (atGeopotential (h + d)) - airP (atGeopotential (h - d))) / (2 * d)
        a = atGeopotential h
    in counterexample (show (dpdh, -(airRho a) * g0)) $ relErr dpdh (-(airRho a) * g0) < 1.0e-5

prop_pressureFalls :: Alt -> Alt -> Property
prop_pressureFalls (Alt a) (Alt b) = a < b ==> airP (atGeopotential a) > airP (atGeopotential b)

prop_idealGas :: Alt -> Bool
prop_idealGas (Alt h) = let a = atGeopotential h in relErr (airP a) (airRho a * rAir * airT a) < 1.0e-12

-- Nozzle -----------------------------------------------------------------

prop_areaRoundTrip :: Gamma -> Mach -> Bool
prop_areaRoundTrip (Gamma g) (Mach m) = relErr (N.machFromArea g (N.areaRatio g m) True) m < 1.0e-9

prop_subsonicRoundTrip :: Gamma -> Property
prop_subsonicRoundTrip (Gamma g) = forAll (choose (0.05, 0.95)) $ \m ->
  relErr (N.machFromArea g (N.areaRatio g m) False) m < 1.0e-9

prop_throatIsMinimum :: Gamma -> Mach -> Bool
prop_throatIsMinimum (Gamma g) (Mach m) = N.areaRatio g m >= 1 && abs (N.areaRatio g 1 - 1) < 1.0e-12

-- Sutton eq. 3-30 at pe = pa equals u_e / c* from eqs. 3-16 and 3-32,
-- two separately derived formulas.
prop_cfIsUeOverCstar :: Gamma -> PRatio -> Bool
prop_cfIsUeOverCstar (Gamma g) (PRatio pr) =
  let r = 350; t0 = 3200
  in relErr (N.cfOpt g pr) (N.exhaustVelocity g r t0 pr / N.cStar g r t0) < 1.0e-12

-- Thrust coefficient is maximal at optimum expansion (pe = pa).
prop_cfMaxAtMatched :: Gamma -> PRatio -> Property
prop_cfMaxAtMatched (Gamma g) (PRatio pa) = forAll (choose (0.2, 5)) $ \k ->
  let peOf e = N.pressureRatio g (N.machFromArea g e True)
      epsOpt = N.areaRatio g (N.machFromPressure g pa)
      eps = epsOpt * k
  in N.cf g (peOf eps) pa eps <= N.cf g pa pa epsOpt + 1.0e-9

-- Pressure ratio and Mach invert each other.
prop_pressureMach :: Gamma -> Mach -> Bool
prop_pressureMach (Gamma g) (Mach m) = relErr (N.machFromPressure g (N.pressureRatio g m)) m < 1.0e-9

-- Balloon ----------------------------------------------------------------

newtype Rig = Rig (Double, Double, Double, Double) deriving Show
instance Arbitrary Rig where
  arbitrary = do
    mb <- choose (0.2, 3)
    mp <- choose (0.1, 5)
    cd <- choose (0.2, 0.5)
    v <- choose (1, 10)
    pure (Rig (mb, mp, cd, v))

prop_fillRoundTrip :: Rig -> Bool
prop_fillRoundTrip (Rig (mb, mp, cd, v)) =
  let r = B.radiusForAscent 1.205 0.1786 9.80665 cd mb mp v
  in relErr (B.ascentMs (B.lift 1.205 0.1786 9.80665 cd mb mp r)) v < 1.0e-9

-- At the steady ascent rate, free lift equals drag.
prop_freeLiftIsDrag :: Rig -> Bool
prop_freeLiftIsDrag (Rig (mb, mp, cd, v)) =
  let r = B.radiusForAscent 1.205 0.1786 9.80665 cd mb mp v
      l = B.lift 1.205 0.1786 9.80665 cd mb mp r
  in relErr (B.freeN l) (0.5 * 1.205 * v * v * cd * pi * r * r) < 1.0e-9

prop_burstAtBurstVolume :: Property
prop_burstAtBurstVolume = forAll (choose (2, 13)) $ \d ->
  abs (B.burstAltitude 7238.3 (4 / 3 * pi * (d / 2) ** 3) d) < 1.0e-9

-- Structures -------------------------------------------------------------

-- p, inner radius, wall: a pressurised cylinder.
newtype Cyl = Cyl (Double, Double, Double) deriving Show
instance Arbitrary Cyl where
  arbitrary = do
    p <- choose (1.0e5, 1.0e7)
    ri <- choose (0.02, 0.5)
    t <- choose (5.0e-4, 0.05)
    pure (Cyl (p, ri, t))

-- Lame's principals are ordered hoop > axial > 0 > radial.
prop_lameOrdering :: Cyl -> Bool
prop_lameOrdering (Cyl (p, ri, t)) =
  let l = S.lame p ri (ri + t) in S.hoop l > S.axial l && S.axial l > 0 && S.radial l < 0

-- Von Mises lies between (sqrt 3 / 2) and 1 times the largest principal
-- difference, for any three principals.
prop_lameVonMisesBounds :: Cyl -> Bool
prop_lameVonMisesBounds (Cyl (p, ri, t)) =
  let l = S.lame p ri (ri + t)
      d = S.hoop l - S.radial l
  in sqrt 3 / 2 * d <= S.vonMises l * (1 + 1.0e-12) && S.vonMises l <= d * (1 + 1.0e-12)

-- Lame at the bore against the thin-wall p R_m / t: the ratio is exactly
-- 1 + (t / 2 R_m)^2, so the thin-wall formula is low by that much.
prop_lameThinWall :: Cyl -> Bool
prop_lameThinWall (Cyl (p, ri, t)) =
  let ro = ri + t
      rm = (ri + ro) / 2
      l = S.lame p ri ro
  in abs (S.hoop l / (p * rm / t) - 1 - (t / (2 * rm)) ** 2) < 1.0e-9

-- Tip deflection goes as L^3; stress is linear in M.
prop_cantileverCubic :: Property
prop_cantileverCubic = forAll (choose (0.1, 3)) $ \l ->
  let i = S.tubeI 0.02 0.018
  in relErr (S.cantileverTip 100 (2 * l) 70e9 i) (8 * S.cantileverTip 100 l 70e9 i) < 1.0e-12
     && relErr (S.bendingStress 50 0.01 i) (2 * S.bendingStress 25 0.01 i) < 1.0e-12

-- Simply supported over clamped centre deflection is (5 + nu) / (1 + nu).
prop_plateRatio :: Property
prop_plateRatio = forAll (choose (0.2, 0.4)) $ \nu ->
  let (yc, _, _) = S.plateClamped 70e9 nu 1.0e5 0.1 0.004
      (ys, _) = S.plateSimple 70e9 nu 1.0e5 0.1 0.004
  in relErr (ys / yc) ((5 + nu) / (1 + nu)) < 1.0e-12

-- The bulkhead thickness puts the Roark 10a centre stress at sf x p exactly
-- on the allowable.
prop_bulkheadInverts :: Property
prop_bulkheadInverts = forAll (choose (0.02, 0.2)) $ \a -> forAll (choose (1.0e6, 8.0e6)) $ \p ->
  let t = S.bulkheadThickness 0.33 a p 1.5 240e6
  in relErr (snd (S.plateSimple 70e9 0.33 (1.5 * p) a t)) 240e6 < 1.0e-9

-- The SP-8007 knock-down is between 0.269 (R/t -> infinity) and 1, and
-- falls as the shell gets thinner.
prop_knockdown :: Property
prop_knockdown = forAll (choose (5, 2000)) $ \rt ->
  let g = S.sp8007Knockdown rt 1
  in g > 0.269 && g <= 1 && S.sp8007Knockdown (2 * rt) 1 <= g

-- Radio ------------------------------------------------------------------

newtype Link = Link (Double, Double) deriving Show
instance Arbitrary Link where
  arbitrary = do
    d <- choose (1, 1.0e6)
    f <- choose (100, 6000)
    pure (Link (d, f))

-- Friis in the engineering form 32.45 + 20 log10 d_km + 20 log10 f_MHz.
prop_fsplFriis :: Link -> Bool
prop_fsplFriis (Link (d, f)) = abs (R.fsplDb d f - (32.4478 + 20 * logBase 10 (d / 1000) + 20 * logBase 10 f)) < 1.0e-3

-- Doubling the distance adds exactly 20 log10 2 dB.
prop_fsplDoubling :: Link -> Bool
prop_fsplDoubling (Link (d, f)) = abs (R.fsplDb (2 * d) f - R.fsplDb d f - 20 * logBase 10 2) < 1.0e-9

-- ground distance, station altitude, payload altitude
newtype Sight = Sight (Double, Double, Double) deriving Show
instance Arbitrary Sight where
  arbitrary = do
    g <- choose (1000, 500e3)
    h1 <- choose (0, 3000)
    h2 <- choose (0, 40000)
    pure (Sight (g, h1, h2))

earthR :: Double
earthR = 6371000

-- The slant range from the law of cosines equals the hypotenuse of the
-- ray's vertical and horizontal components at the station, and the
-- elevation is that triangle's angle (sin el x slant = vertical component).
prop_geometryTriangle :: Sight -> Bool
prop_geometryTriangle (Sight (g, h1, h2)) =
  let geo = R.geometry earthR (4 / 3) g h1 h2
      th = g / earthR
      a = earthR + h1
      b = earthR + h2
      vert = b * cos th - a
      horiz = b * sin th
  in relErr (R.slantM geo) (sqrt (vert * vert + horiz * horiz)) < 1.0e-9
     && abs (sin (R.elevationRad geo) * R.slantM geo - vert) < 1.0e-3

-- Swapping the two ends keeps the range and the horizon test.
prop_geometrySymmetric :: Sight -> Bool
prop_geometrySymmetric (Sight (g, h1, h2)) =
  let x = R.geometry earthR (4 / 3) g h1 h2
      y = R.geometry earthR (4 / 3) g h2 h1
  in relErr (R.slantM x) (R.slantM y) < 1.0e-9 && R.withinHorizon x == R.withinHorizon y

-- A payload overhead within the horizon distance: the horizon is monotone
-- in both heights.
prop_horizonMonotone :: Sight -> Bool
prop_horizonMonotone (Sight (g, h1, h2)) =
  not (R.withinHorizon (R.geometry earthR (4 / 3) g h1 h2)) || R.withinHorizon (R.geometry earthR (4 / 3) g h1 (h2 + 1000))

-- The dipole peaks broadside, never exceeds its peak, is symmetric in
-- elevation and sits on the floor along its axis.
prop_dipolePattern :: Property
prop_dipolePattern = forAll (choose (-pi / 2, pi / 2)) $ \el ->
  let g = R.dipoleGainDbi (-10) 2.15 el
  in g <= 2.15 + 1.0e-9 && abs (g - R.dipoleGainDbi (-10) 2.15 (-el)) < 1.0e-9
     && abs (R.dipoleGainDbi (-10) 2.15 0 - 2.15) < 1.0e-9
     && R.dipoleGainDbi (-10) 2.15 (pi / 2) == -10

-- GNSS coexistence ------------------------------------------------------

-- The resonator rejection is zero at the centre and the same at f0 k and
-- f0 / k (symmetric on a log axis), and grows with the detuning.
prop_rejectionSymmetric :: Property
prop_rejectionSymmetric = forAll (choose (1.001, 20)) $ \k ->
  let f0 = 1575.42
      r = R.patchRejectionDb f0 20
  in abs (r f0) < 1.0e-9 && abs (r (f0 * k) - r (f0 / k)) < 1.0e-6 && r (f0 * k) > r (f0 * sqrt k)

-- Every harmonic order found lands inside the band, and none outside it was missed.
prop_harmonicsInBand :: Property
prop_harmonicsInBand = forAll (choose (70, 1000)) $ \f ->
  let f0 = 1575.42; bw = 2.046
      hs = R.harmonicsIn f0 bw f
  in all (\n -> abs (fromIntegral n * f - f0) <= bw / 2) hs
     && all (\n -> (n `elem` hs) == (abs (fromIntegral n * f - f0) <= bw / 2)) [2 .. 20 :: Int]

-- Coupled power never exceeds P_tx + G - 6.02 dB (the radian-sphere clamp)
-- and falls with separation beyond it.
prop_coupledClamp :: Link -> Property
prop_coupledClamp (Link (d, f)) = forAll (choose (0, 2)) $ \sep ->
  let c = R.coupledDbm 30 2 sep f
      edge = 299792458 / (f * 1e6) / (2 * pi)
  in c <= 30 + 2 - 20 * logBase 10 2 + 1.0e-9
     && (sep <= edge || R.coupledDbm 30 2 (sep + d) f <= c + 1.0e-9)

-- The in-band limit sits exactly (signal - reacq) above the floor and the
-- floor is kTB at 290 K plus NF to within the -174 rounding (0.03 dB).
prop_inbandLimit :: Property
prop_inbandLimit = forAll (choose (0.1, 50)) $ \bw -> forAll (choose (0, 10)) $ \nf ->
  let kt = 10 * logBase 10 (1.380649e-23 * 290 * 1e3)
  in abs (R.inbandLimitDbm bw nf (-130) (-160) - R.thermalFloorDbm bw nf - 30) < 1.0e-9
     && abs (R.thermalFloorDbm bw nf - (kt + 10 * logBase 10 (bw * 1e6) + nf)) < 0.03

-- Thermal ----------------------------------------------------------------

-- heat capacity J/K, R_th K/W, heating W, ambient C, step s
newtype Box = Box (Double, Double, Double, Double, Double) deriving Show
instance Arbitrary Box where
  arbitrary = do
    c <- choose (200, 3000)
    rth <- choose (2, 60)
    p <- choose (0, 10)
    amb <- choose (-70, 40)
    dt <- choose (0.1, 5)
    pure (Box (c, rth, p, amb, dt))

-- The stepped Euler model equals its closed form after any number of steps.
prop_boxClosedForm :: Box -> Property
prop_boxClosedForm (Box (c, rth, p, amb, dt)) = forAll (choose (0, 400)) $ \n ->
  let a = T.boxAfter c rth p amb dt n 20
      b = T.boxClosedForm c rth p amb dt n 20
  in abs (a - b) < 1.0e-6 * max 1 (abs b)

-- The steady temperature is a fixed point, and a step from either side
-- moves toward it without crossing when dt < R C.
prop_boxSteady :: Box -> Property
prop_boxSteady (Box (c, rth, p, amb, dt)) = dt < rth * c ==>
  let tss = T.boxSteady rth p amb
      step = T.boxStep c rth p amb dt
  in abs (step tss - tss) < 1.0e-9 && step (tss + 10) <= tss + 10 && step (tss + 10) >= tss
     && step (tss - 10) >= tss - 10 && step (tss - 10) <= tss

-- The film coefficient scales as sqrt rho and the two resistances add.
prop_resistanceAdds :: Box -> Bool
prop_resistanceAdds (Box (_, _, _, _, _)) =
  let h = T.filmCoefficient 10 1.225 0.30625
      r = T.thermalResistance 0.02 0.035 0.3 0.2 h
  in abs (h - 5) < 1.0e-12 && abs (r - (0.02 / (0.035 * sqrt 0.06) + 1 / (5 * 0.3))) < 1.0e-12

-- Capacity fractions lie in [0, 1], never fall as the cell warms, and hit
-- the table points exactly.
prop_capacityMonotone :: Property
prop_capacityMonotone = forAll (choose (-60, 60)) $ \t -> forAll (choose (0, 20)) $ \d ->
  and [ f t >= 0 && f t <= 1 && f (t + d) >= f t && all (\(x, y) -> abs (f x - y) < 1.0e-12) tab
      | tab <- [T.lipoCapacity, T.l91Capacity], let f = T.capacityFraction tab ]

-- Warmer output is inversely proportional to the rated duration.
prop_warmerInverse :: Property
prop_warmerInverse = forAll (choose (1, 24)) $ \h ->
  relErr (T.warmerPowerW 29.7 0.9 0.5 0.5 (2 * h)) (T.warmerPowerW 29.7 0.9 0.5 0.5 h / 2) < 1.0e-12

-- Rocket aero, recovery, closures -----------------------------------------

-- The von Karman profile starts at 0, ends at R, never exceeds R, and rises
-- along the nose.
prop_vonKarmanShape :: Property
prop_vonKarmanShape = forAll (choose (0, 1)) $ \x -> forAll (choose (0, 1)) $ \d ->
  let r = A.vonKarmanRadius x 1 0.05
  in abs (A.vonKarmanRadius 0 1 0.05) < 1.0e-12 && relErr (A.vonKarmanRadius 1 1 0.05) 0.05 < 1.0e-12
     && r <= 0.05 + 1.0e-12 && A.vonKarmanRadius (min 1 (x + d)) 1 0.05 >= r - 1.0e-12

-- The two stagnation brackets meet at M 1 to within 1 % (1.275 vs 1.281:
-- the series and the Rayleigh fit are different approximations), and the
-- supersonic branch rises toward its limit 0.85 x 1.84.
prop_stagnation :: Property
prop_stagnation = forAll (choose (1.0001, 10)) $ \m ->
  abs (A.stagnationCd 1 - A.stagnationCd 1.0000001) < 0.01 * A.stagnationCd 1
  && A.stagnationCd m < 0.85 * 1.84 && A.stagnationCd (m + 0.1) >= A.stagnationCd m

-- Fineness 3 returns the TR-R-100 points; a slimmer nose has less drag and
-- none below M 0.9.
prop_noseCd :: Property
prop_noseCd = forAll (choose (0.5, 3)) $ \m -> forAll (choose (3, 8)) $ \f ->
  and [relErr (A.noseCd mi 3) cd < 1.0e-12 | (mi, cd) <- A.trR100Fr3, cd > 0]
  && A.noseCd m f <= A.noseCd m 3 + 1.0e-12 && (m > 0.9 || A.noseCd m f == 0)

-- At the terminal rate, drag equals weight.
prop_descentDrag :: Property
prop_descentDrag = forAll (choose (0.5, 30)) $ \m -> forAll (choose (12, 120)) $ \dIn ->
  let d = dIn * 0.0254
      v = A.descentRate m 9.80665 d 1.5 1.1
  in relErr (0.5 * 1.1 * v * v * 1.5 * pi * d * d / 4) (m * 9.80665) < 1.0e-9

-- The screw count is even, at least 6, and enough: bearing at 1.5 MEOP on
-- the bore under F_bry.
prop_closureScrews :: Property
prop_closureScrews = forAll (choose (0.05, 0.2)) $ \d -> forAll (choose (0.002, 0.01)) $ \w -> forAll (choose (1.0e6, 8.0e6)) $ \p ->
  let n = A.closureScrews d w p
  in even n && n >= 6 && fromIntegral n * (0.25 * 0.0254) * w * 386e6 >= 1.5 * p * pi * d * d / 4

-- Same numbers through the JSON CLI path.
prop_cliMatches :: Alt -> Bool
prop_cliMatches (Alt h) = case evalCase (JObj [("fn", JStr "us76_geopotential"), ("h_m", JNum h)]) of
  o -> num "p_pa" o == Just (airP (atGeopotential h))

-- * PCB oracle (Oracle.Pcb, docs/electronics-plan.md Lane F)

-- | A random small board: 2..6 two-pad parts, net 0 on every pad "1",
-- net 1 on every pad "2", random poses, and random tracks and vias between
-- same-net pad centres. Coordinates in nm on a 1 um grid.
newtype PcbBoard = PcbBoard Pcb.Design deriving Show

instance Arbitrary PcbBoard where
  arbitrary = do
    k <- choose (2, 6)
    pls <- mapM genPart [0 .. k - 1]
    let d0 = Pcb.Design (map partI [0 .. k - 1]) [net "A" "1" k, net "B" "2" k] (board0 pls []  []) (Pcb.Route [0, 1] [] [] [])
        centres n = [Pcb.padCentre pl p | pl <- pls, p <- Pcb.fpPads (Pcb.plFp pl), Pcb.padNumber p == (if n == 0 then "1" else "2")]
    nt <- choose (0, 8 :: Int)
    ts <- vectorOf nt $ do
      n <- elements [0, 1]
      a <- elements (centres n)
      b <- elements (centres n)
      w <- (* 1000) <$> choose (150, 800)
      l <- elements [Pcb.FCu, Pcb.BCu]
      pure (Pcb.Track a b w l n)
    nv <- choose (0, 3 :: Int)
    vs <- vectorOf nv $ do
      n <- elements [0, 1]
      a <- elements (centres n)
      pure (Pcb.Via a 600000 n)
    pure (PcbBoard d0 {Pcb.dBoard = board0 pls ts vs})
    where
      partI i = Pcb.PartI ("R" ++ show (i + 1)) ("R" ++ show i) "resistor" "10k" [Pcb.PinI "1" "1" (Just 0) False, Pcb.PinI "2" "2" (Just 1) False]
      net nm pn k = Pcb.NetI nm "signal" [(i, pn) | i <- [0 .. k - 1]]
      board0 pls ts vs = Pcb.Board (Pcb.RectI (-50000000, -50000000) (50000000, 50000000)) pls ts vs [] [Pcb.NetClass "Default" 250000 200000 [0, 1]] 200000 1
      genPart i = do
        sh <- elements [Pcb.SRect, Pcb.SCircle, Pcb.SOval, Pcb.SRoundRect 0.25]
        tht <- elements [False, False, True]
        let drill = if tht then Just 400000 else Nothing
            pad n x = Pcb.Pad n (x, 0) 0 (600000, 900000) sh drill tht []
        x <- (* 1000) <$> choose (-20000, 20000)
        y <- (* 1000) <$> choose (-20000, 20000)
        r <- choose (0, 3)
        s <- elements [Pcb.Top, Pcb.Bottom]
        pure (Pcb.Placed i (Pcb.Footprint "Test:R" [pad "1" (-800000), pad "2" 800000] (Pcb.RectI (-1500000, -750000) (1500000, 750000))) (x, y) r s)

islandsOf :: Pcb.Design -> [Int]
islandsOf d = [Pcb.netIslands False d n | n <- [0, 1]]

near :: Maybe Double -> Maybe Double -> Bool
near (Just a) (Just b) = abs (a - b) <= 1.0e-3
near Nothing Nothing = True
near _ _ = False

-- Connectivity is a property of the copper, not of where the board sits.
prop_connTranslate :: PcbBoard -> Int -> Int -> Bool
prop_connTranslate (PcbBoard d) dx dy =
  islandsOf (Pcb.translateDesign (fromIntegral dx * 1000, fromIntegral dy * 1000) d) == islandsOf d

prop_connRotate :: PcbBoard -> Property
prop_connRotate (PcbBoard d) = forAll (choose (1, 3)) $ \q -> islandsOf (Pcb.rotateDesign q d) == islandsOf d

-- Rotating the whole board rotates every pad centre (own transform, exact).
prop_padsRotate :: PcbBoard -> Property
prop_padsRotate (PcbBoard d) = forAll (choose (1, 3)) $ \q ->
  let cs e = [Pcb.padCentre pl p | pl <- Pcb.bParts (Pcb.dBoard e), p <- Pcb.fpPads (Pcb.plFp pl)]
  in cs (Pcb.rotateDesign q d) == map (Pcb.rotQ q) (cs d)

-- The clearance oracle is symmetric: distance(a, b) = distance(b, a) for
-- every pair of copper items, and the board's minimum gap and violation
-- count do not depend on the order items are listed in.
prop_clearanceSymmetric :: PcbBoard -> Bool
prop_clearanceSymmetric (PcbBoard d) =
  let its = Pcb.copperItems d
      b = Pcb.dBoard d
      rev = d {Pcb.dBoard = b {Pcb.bParts = reverse (Pcb.bParts b), Pcb.bTracks = reverse (Pcb.bTracks b), Pcb.bVias = reverse (Pcb.bVias b)}}
  in and [Pcb.geoDist (Pcb.itGeo x) (Pcb.itGeo y) == Pcb.geoDist (Pcb.itGeo y) (Pcb.itGeo x) | x <- its, y <- its]
     && Pcb.minGap rev == Pcb.minGap d && Pcb.clearanceViolations rev == Pcb.clearanceViolations d

prop_clearanceRigid :: PcbBoard -> Property
prop_clearanceRigid (PcbBoard d) = forAll (choose (1, 3)) $ \q ->
  near (Pcb.minGap (Pcb.rotateDesign q d)) (Pcb.minGap d) && near (Pcb.minGap (Pcb.translateDesign (1234000, -567000) d)) (Pcb.minGap d)

-- Taking away copper never joins islands.
prop_removeTrackSplits :: PcbBoard -> Bool
prop_removeTrackSplits (PcbBoard d) =
  let b = Pcb.dBoard d
      d' = d {Pcb.dBoard = b {Pcb.bTracks = drop 1 (Pcb.bTracks b)}}
  in and (zipWith (>=) (islandsOf d') (islandsOf d))

-- | Hand-checked geometry and the published formulas.
pcbTable :: [(String, Bool)]
pcbTable =
  let r0603 = Pcb.Footprint "Resistor_SMD:R_0603_1608Metric"
        [Pcb.Pad "1" (-825000, 0) 0 (800000, 950000) (Pcb.SRoundRect 0.25) Nothing False [], Pcb.Pad "2" (825000, 0) 0 (800000, 950000) (Pcb.SRoundRect 0.25) Nothing False []]
        (Pcb.RectI (-1480000, -730000) (1480000, 730000))
      rotated = Pcb.Placed 0 r0603 (10000000, 10000000) 1 Pcb.Top
      flipped = Pcb.Placed 0 r0603 (0, 0) 0 Pcb.Bottom
      pad1 = Pcb.Pad "1" (-825000, 0) 0 (800000, 950000) (Pcb.SRoundRect 0.25) Nothing False []
      Pcb.RectI (cx0, cy0) (cx1, cy1) = Pcb.courtyardAbs rotated
      seg a b = Pcb.Seg a b 125000
      relWithin tol a b = abs (a - b) <= tol * abs b
  in [ ("pcb: KiCad +90 CCW puts pad 1 of an 0603 at +y (wright-eda board.rs case)", Pcb.padCentre rotated pad1 == (10000000, 10825000))
     , ("pcb: a bottom-side part mirrors X before rotating", Pcb.padCentre flipped pad1 == (825000, 0))
     , ("pcb: rotation swaps courtyard extents (1.46 x 2.96 mm)", (cx1 - cx0, cy1 - cy0) == (1460000, 2960000))
     , ("pcb: parallel 0.25 mm tracks 1 mm apart are 0.75 mm apart", Pcb.geoDist (seg (0, 0) (5000000, 0)) (seg (0, 1000000) (5000000, 1000000)) == 750000)
     , ("pcb: crossing tracks overlap (distance < 0)", Pcb.geoDist (seg (0, 0) (2000000, 2000000)) (seg (0, 2000000) (2000000, 0)) < 0)
     , ("pcb: track end 1 mm from a pad edge", Pcb.geoDist (seg (-3000000, 0) (-2000000, 0)) (Pcb.Box (Pcb.RectI (-1000000, -500000) (1000000, 500000))) == 875000)
     , ("pcb: pad boxes 3-4-5 apart", Pcb.geoDist (Pcb.Box (Pcb.RectI (0, 0) (1, 1))) (Pcb.Box (Pcb.RectI (3000001, 4000001) (3000002, 4000002))) == 5000000)
       -- IPC-2221A 6.2, external, 20 C, 1 oz = 1.378 mil: the same value as
       -- the Python baseline's aircraft/electronics.py trace_capacity_a(0.8) = 2.7598527691900028
     , ("pcb: IPC-2221 0.8 mm 1 oz 20 C = the baseline's trace_capacity_a", relWithin 1e-12 (Pcb.traceCapacityA 800000 20 1) 2.7598527691900028)
     , ("pcb: IPC-2221 1.0 mm 1 oz 20 C = 3.2445 A", relWithin 1e-12 (Pcb.traceCapacityA 1000000 20 1) 3.2444842753531953)
     , ("pcb: IPC-2221 doubling copper scales current by 2^0.725", relWithin 1e-12 (Pcb.traceCapacityA 500000 20 2 / Pcb.traceCapacityA 500000 20 1) (2 ** 0.725))
       -- TI SLVS757 sect. 8.2.2.2 (R1 = 10 k): 3.24 k gives the 5 V example
     , ("pcb: TPS5450 10k / 3.24k = 4.99 V (SLVS757 5 V design)", relWithin 1e-12 (Pcb.tps5450Vout 10000 3240) 4.989518518518518)
     , ("pcb: TPS5450 10k / 2.55k = 6.009 V (servo board)", relWithin 1e-12 (Pcb.tps5450Vout 10000 2550) 6.009235294117648)
     , ("pcb: a B.Cu pad on a top-side part is shorted by a B.Cu track over it", backPadShort)
     , ("pcb: the same pad without its layers reads F.Cu and misses the short", not backPadOldRule)
     , ("pcb: a B.Cu pad on a bottom-side part flips to F.Cu", Pcb.copperItems (backPad Pcb.Bottom [Pcb.BCu] Pcb.FCu) /= [] && backPadShortOn Pcb.Bottom [Pcb.BCu] Pcb.FCu)
     , ("pcb: resistor values parse", map Pcb.parseOhms ["10k", "2.55k", "4k7", "1M", "100R", "100", "10k 1%", "x"] == [Just 1e4, Just 2550, Just 4700, Just 1e6, Just 100, Just 100, Just 1e4, Nothing])
     ]

-- | The first real servo board's DRC finding (2026-10-24): U1's PowerPAD
-- (pad 9) is an SMD pad on B.Cu of a top-side SO-8, and a +6V B.Cu track
-- runs under it. Net 0 = GND (pad 9), net 1 = +6V (the track).
backPad :: Pcb.Side -> [Pcb.Layer] -> Pcb.Layer -> Pcb.Design
backPad side ls trackLayer =
  let pp = Pcb.Pad "9" (0, 0) 0 (2410000, 3100000) Pcb.SRect Nothing False ls
      fp = Pcb.Footprint "Package_SO:TI_SO-PowerPAD-8_ThermalVias" [pp] (Pcb.RectI (-3700000, -2700000) (3700000, 2700000))
      pl = Pcb.Placed 0 fp (10000000, 10000000) 0 side
      tr = Pcb.Track (5000000, 10000000) (15000000, 10000000) 800000 trackLayer 1
      b = Pcb.Board (Pcb.RectI (0, 0) (20000000, 20000000)) [pl] [tr] [] [] [Pcb.NetClass "Default" 250000 200000 [0, 1]] 200000 1
  in Pcb.Design [Pcb.PartI "U1" "TPS5450DDA" "ic" "TPS5450DDA" [Pcb.PinI "PAD" "9" (Just 0) False]]
       [Pcb.NetI "GND" "ground" [(0, "9")], Pcb.NetI "+6V" "power" []] b (Pcb.Route [] [] [] [])

backPadShortOn :: Pcb.Side -> [Pcb.Layer] -> Pcb.Layer -> Bool
backPadShortOn side ls l = let d = backPad side ls l in Pcb.minGap d == Just 0 && Pcb.clearanceViolations d == 1

backPadShort, backPadOldRule :: Bool
backPadShort = backPadShortOn Pcb.Top [Pcb.BCu] Pcb.BCu
backPadOldRule = backPadShortOn Pcb.Top [] Pcb.BCu

-- | The synthetic servo board the Rust probe wrote (wright_eda serde
-- format; haskell/crosscheck-rs `gen-pcb`; made-up copper on the real
-- netlist): the oracle reads it, finds the net the generator left
-- unrouted, agrees with every other routed claim, sees the GND pour as
-- unverified without DRC, and reads the regulator setpoint.
pcbFixtureChecks :: IO [(String, Bool)]
pcbFixtureChecks = do
  s <- readFile "test/fixtures/synthetic_servo_board.json"
  case parseJson s of
    Left e -> pure [("pcb fixture parses: " ++ e, False)]
    Right j -> do
      let run f extra = evalCase (JObj ([("fn", JStr f), ("board", j)] ++ extra))
          conn = run "pcb_connectivity" []
          reg = run "pcb_regulator" []
          dsg = Pcb.decodeDesign j
          gndPoured = case dsg of
            Just d -> Pcb.rtPoured (Pcb.dRoute d) == [2]
            Nothing -> False
          brokeNet = case dsg of
            Just d | (n : _) <- Pcb.rtRouted (Pcb.dRoute d) ->
              let b = Pcb.dBoard d
                  d' = d {Pcb.dBoard = b {Pcb.bTracks = [t | t <- Pcb.bTracks b, Pcb.trNet t /= n]}}
              in Pcb.netIslands False d' n > 1 && Pcb.netIslands False d n == 1
            _ -> False
      pure
        [ ("pcb fixture: decodes as wright_eda (netlist, board, route)", isJustD dsg)
        , ("pcb fixture: oracle agrees with every routed claim", num "claim_mismatches" conn == Just 0)
        , ("pcb fixture: the net left unrouted is in two islands", num "islands:SIG_ELEVATOR" conn == Just 2 && num "conn:SIG_ELEVATOR" conn == Just 0)
        , ("pcb fixture: one unrouted net", num "unrouted_count" conn == Just 1)
        , ("pcb fixture: the GND pour is unverified without DRC", gndPoured && num "unverified_pours" conn == Just 1)
        , ("pcb fixture: every net with >= 2 pads is attempted or poured", num "uncovered_nets" conn == Just 0)
        , ("pcb fixture: removing a routed net's tracks splits it", brokeNet)
        , ("pcb fixture: TPS5450 divider reads 6.009 V", maybe False (\v -> abs (v - 6.009235294117648) < 1e-12) (num "vout_v" reg))
        ]
  where
    isJustD = maybe False (const True)

main :: IO ()
main = do
  fixture <- pcbFixtureChecks
  let checks = tableChecks ++ pcbTable ++ fixture
      bad = [n | (n, ok) <- checks, not ok]
  mapM_ (\(n, ok) -> putStrLn ((if ok then "ok   " else "FAIL ") ++ n)) checks
  rs <- sequence
    [ putStr (n ++ ": ") >> quickCheckResult p
    | (n, p) <-
        [ ("prop_layersContinuous", property prop_layersContinuous)
        , ("prop_hydrostatic", property prop_hydrostatic)
        , ("prop_pressureFalls", property prop_pressureFalls)
        , ("prop_idealGas", property prop_idealGas)
        , ("prop_areaRoundTrip", property prop_areaRoundTrip)
        , ("prop_subsonicRoundTrip", property prop_subsonicRoundTrip)
        , ("prop_throatIsMinimum", property prop_throatIsMinimum)
        , ("prop_cfIsUeOverCstar", property prop_cfIsUeOverCstar)
        , ("prop_cfMaxAtMatched", property prop_cfMaxAtMatched)
        , ("prop_pressureMach", property prop_pressureMach)
        , ("prop_fillRoundTrip", property prop_fillRoundTrip)
        , ("prop_freeLiftIsDrag", property prop_freeLiftIsDrag)
        , ("prop_burstAtBurstVolume", property prop_burstAtBurstVolume)
        , ("prop_cliMatches", property prop_cliMatches)
        , ("prop_lameOrdering", property prop_lameOrdering)
        , ("prop_lameVonMisesBounds", property prop_lameVonMisesBounds)
        , ("prop_lameThinWall", property prop_lameThinWall)
        , ("prop_cantileverCubic", property prop_cantileverCubic)
        , ("prop_plateRatio", property prop_plateRatio)
        , ("prop_bulkheadInverts", property prop_bulkheadInverts)
        , ("prop_knockdown", property prop_knockdown)
        , ("prop_fsplFriis", property prop_fsplFriis)
        , ("prop_fsplDoubling", property prop_fsplDoubling)
        , ("prop_geometryTriangle", property prop_geometryTriangle)
        , ("prop_geometrySymmetric", property prop_geometrySymmetric)
        , ("prop_horizonMonotone", property prop_horizonMonotone)
        , ("prop_dipolePattern", property prop_dipolePattern)
        , ("prop_rejectionSymmetric", property prop_rejectionSymmetric)
        , ("prop_harmonicsInBand", property prop_harmonicsInBand)
        , ("prop_coupledClamp", property prop_coupledClamp)
        , ("prop_inbandLimit", property prop_inbandLimit)
        , ("prop_boxClosedForm", property prop_boxClosedForm)
        , ("prop_boxSteady", property prop_boxSteady)
        , ("prop_resistanceAdds", property prop_resistanceAdds)
        , ("prop_capacityMonotone", property prop_capacityMonotone)
        , ("prop_warmerInverse", property prop_warmerInverse)
        , ("prop_vonKarmanShape", property prop_vonKarmanShape)
        , ("prop_stagnation", property prop_stagnation)
        , ("prop_noseCd", property prop_noseCd)
        , ("prop_descentDrag", property prop_descentDrag)
        , ("prop_closureScrews", property prop_closureScrews)
        , ("prop_connTranslate", property prop_connTranslate)
        , ("prop_connRotate", property prop_connRotate)
        , ("prop_padsRotate", property prop_padsRotate)
        , ("prop_clearanceSymmetric", property prop_clearanceSymmetric)
        , ("prop_clearanceRigid", property prop_clearanceRigid)
        , ("prop_removeTrackSplits", property prop_removeTrackSplits)
        ]
    ]
  unless (null bad && all isSuccess rs) exitFailure
