-- | Evaluate one JSON case. Each case is an object with "fn" and inputs;
-- the result echoes the case and adds outputs (or "error").
--
--   {"fn":"us76", "z_m": 11000}                -> t_k, p_pa, rho_kg_m3, mu_pa_s, a_m_s, h_m
--   {"fn":"us76_geopotential", "h_m": 11000}   -> same
--   {"fn":"nozzle_mach", "gamma":1.4, "mach":2} -> area_ratio, p_ratio, t_ratio
--   {"fn":"nozzle_area", "gamma":1.2, "area_ratio":10, "supersonic":true}
--                                              -> mach, p_ratio
--   {"fn":"nozzle_cf", "gamma":1.2, "pe_pc":0.01, "pa_pc":0.0, "area_ratio":E?}
--                                              -> cf_opt, cf, area_ratio (from pe if absent)
--   {"fn":"nozzle_cstar", "gamma":1.2, "tc_k":3400, "molar_mass_kg_mol":0.022} -> cstar_m_s
--   {"fn":"balloon", "radius_m":r | "volume_m3":V, "balloon_kg", "payload_kg",
--    "rho_air"?, "rho_gas"?, "cd"?, "g"?, "burst_diameter_m"?, "scale_height_m"?}
--                                              -> gross_lift_kg, neck_lift_kg, free_lift_n, ascent_m_s, burst_altitude_m?
--   {"fn":"balloon_fill", "ascent_m_s":5, "balloon_kg", "payload_kg", ...}
--                                              -> radius_m, volume_m3, neck_lift_kg
--   {"fn":"tube_bending", "od_mm", "id_mm", "moment_nm"}   -> stress_pa (M c / I)
--   {"fn":"tube_buckling", "od_mm", "id_mm", "modulus_pa"} -> stress_pa, knockdown, r_over_t
--                                              (SP-8007 bending, nu 0.3, mid-wall R)
--   {"fn":"lame", "p_pa", "ri_m", "ro_m"}      -> hoop_pa, axial_pa, radial_pa, von_mises_pa
--   {"fn":"flutter", "a_m_s", "p_pa", "g_pa", "span_m", "root_m", "tip_m", "t_m"} -> vf_m_s
--   {"fn":"bulkhead", "radius_m", "p_pa", "sf", "allow_pa", "nu"?}  -> t_m (nu 0.33)
--   {"fn":"fspl", "d_m", "f_mhz"}              -> loss_db
--   {"fn":"radio_geometry", "ground_m", "station_alt_m", "payload_alt_m",
--    "earth_r_m"?, "k_factor"?}                -> slant_m, elevation_rad, within_horizon (0/1)
--   {"fn":"dipole_gain", "peak_dbi", "el_rad", "floor_dbi"?} -> gain_dbi
--   {"fn":"patch_rejection", "f_mhz", "f0_mhz"?, "bw_mhz"?}  -> rejection_db (L1 1575.42, BW 20)
--   {"fn":"gnss_limits", "p1db_dbm"?, "backoff_db"?, "bw_mhz"?, "nf_db"?, "signal_dbm"?, "reacq_dbm"?}
--                                              -> blocking_dbm, inband_dbm (MAX-M10S / BGA524N6 defaults)
--   {"fn":"gnss_harmonics", "f_mhz", "f0_mhz"?, "bw_mhz"?} -> count, lowest (0 when none)
--   {"fn":"gnss_coexist", "tx_dbm", "gain_dbi", "sep_m", "f_mhz", "harmonic_db"?}
--                                              -> coupled_dbm, at_gnss_dbm (fundamental through the
--                                                 patch), harmonic_dbm (coupled - 60)
--   {"fn":"warmer_power", "rated_h", "packet_g"?, "contents"?, "iron"?, "conversion"?} -> watts
--   {"fn":"capacity_fraction", "chemistry": "lipo"|"li_ion"|"lithium_primary", "t_c"} -> fraction
--   {"fn":"box_thermal", "heat_cap_j_k", "r_th_k_w", "p_w", "amb_c", "dt_s", "steps", "t0_c"}
--                                              -> t_c (stepped), t_closed_c, t_steady_c (oracle only)
--   {"fn":"von_karman", "x_m", "l_m", "r_m"}   -> radius_m
--   {"fn":"stagnation_cd", "mach"}             -> cd
--   {"fn":"nose_cd", "mach", "fineness"}       -> cd
--   {"fn":"descent_rate", "mass_kg", "diameter_in", "cd", "rho"} -> v_m_s (g 9.80665)
--   {"fn":"closure_capacity", "wall_m"}        -> shear_n, bearing_n, tearout_n
--   {"fn":"closure_screws", "inner_d_m", "wall_m", "meop_pa"} -> n
--   {"fn":"pcb_pads" | "pcb_courtyard" | "pcb_connectivity" | "pcb_clearance"
--         | "pcb_ampacity" | "pcb_regulator", "board": [netlist, board, route],
--    "drc_unconnected"?, "rise_c"?, "net_current_a"?}  -> see Oracle.Pcb
module Oracle.Cases (evalCase) where

import Data.Maybe (fromMaybe)
import qualified Oracle.Aero as A
import Oracle.Atmosphere
import qualified Oracle.Balloon as B
import qualified Oracle.Beam as S
import Oracle.Json
import qualified Oracle.Nozzle as N
import qualified Oracle.Pcb as Pcb
import qualified Oracle.Radio as R
import qualified Oracle.Thermal as T

evalCase :: J -> J
evalCase c@(JObj kvs) = JObj (kvs ++ out)
  where
    out = case str "fn" c of
      Nothing -> err "missing \"fn\""
      Just f -> fromMaybe (err ("bad inputs for " ++ f)) (run f c)
evalCase _ = JObj (err "case is not an object")

err :: String -> [(String, J)]
err m = [("error", JStr m)]

nums :: [(String, Double)] -> [(String, J)]
nums = map (fmap JNum)

air :: Air -> [(String, J)]
air a = nums [ ("h_m", airH a), ("t_k", airT a), ("p_pa", airP a), ("rho_kg_m3", airRho a)
             , ("mu_pa_s", airMu a), ("a_m_s", airA a) ]

run :: String -> J -> Maybe [(String, J)]
run "us76" c = air . atGeometric <$> num "z_m" c
run "us76_geopotential" c = air . atGeopotential <$> num "h_m" c
run "nozzle_mach" c = do
  g <- num "gamma" c
  m <- num "mach" c
  pure (nums [("area_ratio", N.areaRatio g m), ("p_ratio", N.pressureRatio g m), ("t_ratio", N.temperatureRatio g m)])
run "nozzle_area" c = do
  g <- num "gamma" c
  e <- num "area_ratio" c
  let sup = fromMaybe 1 (num "supersonic" c) /= 0
      m = N.machFromArea g e sup
  pure (nums [("mach", m), ("p_ratio", N.pressureRatio g m)])
run "nozzle_cf" c = do
  g <- num "gamma" c
  pe <- num "pe_pc" c
  let pa = fromMaybe 0 (num "pa_pc" c)
      e = fromMaybe (N.areaRatio g (N.machFromPressure g pe)) (num "area_ratio" c)
  pure (nums [("cf_opt", N.cfOpt g pe), ("cf", N.cf g pe pa e), ("area_ratio_used", e)])
run "nozzle_cstar" c = do
  g <- num "gamma" c
  t0 <- num "tc_k" c
  mm <- num "molar_mass_kg_mol" c
  -- R = 8.314462618 J/(mol K): the exact SI (2019) value, k_B N_A.
  pure (nums [("cstar_m_s", N.cStar g (8.314462618 / mm) t0)])
run "balloon" c = do
  (rhoA, rhoG, g, cd, mb, mp) <- balloonInputs c
  r <- case (num "radius_m" c, num "volume_m3" c) of
    (Just r, _) -> Just r
    (_, Just v) -> Just ((3 * v / (4 * pi)) ** (1 / 3))
    _ -> Nothing
  let l = B.lift rhoA rhoG g cd mb mp r
      vol = 4 / 3 * pi * r ** 3
      burst = case num "burst_diameter_m" c of
        Just d -> nums [("burst_altitude_m", B.burstAltitude (fromMaybe 7238.3 (num "scale_height_m" c)) vol d)]
        Nothing -> []
  pure (nums [ ("radius_m", r), ("volume_m3", vol), ("gross_lift_kg", B.grossKg l)
             , ("neck_lift_kg", B.neckKg l), ("free_lift_n", B.freeN l), ("ascent_m_s", B.ascentMs l) ] ++ burst)
run "balloon_fill" c = do
  (rhoA, rhoG, g, cd, mb, mp) <- balloonInputs c
  v <- num "ascent_m_s" c
  let r = B.radiusForAscent rhoA rhoG g cd mb mp v
      l = B.lift rhoA rhoG g cd mb mp r
  pure (nums [("radius_m", r), ("volume_m3", 4 / 3 * pi * r ** 3), ("neck_lift_kg", B.neckKg l)])
-- Structures (Oracle.Beam) --------------------------------------------------
run "tube_bending" c = do
  od <- (* 1e-3) <$> num "od_mm" c
  idm <- (* 1e-3) <$> num "id_mm" c
  m <- num "moment_nm" c
  pure (nums [("stress_pa", S.bendingStress m (od / 2) (S.tubeI od idm))])
run "tube_buckling" c = do
  od <- (* 1e-3) <$> num "od_mm" c
  idm <- (* 1e-3) <$> num "id_mm" c
  e <- num "modulus_pa" c
  let t = (od - idm) / 2
      r = (od + idm) / 4
      gamma = S.sp8007Knockdown r t
  pure (nums [("stress_pa", gamma * S.shellBucklingClassical e 0.3 r t), ("knockdown", gamma), ("r_over_t", r / t)])
run "lame" c = do
  p <- num "p_pa" c
  ri <- num "ri_m" c
  ro <- num "ro_m" c
  let l = S.lame p ri ro
  pure (nums [("hoop_pa", S.hoop l), ("axial_pa", S.axial l), ("radial_pa", S.radial l), ("von_mises_pa", S.vonMises l)])
run "flutter" c = do
  a <- num "a_m_s" c
  p <- num "p_pa" c
  g <- num "g_pa" c
  sp <- num "span_m" c
  rt <- num "root_m" c
  tp <- num "tip_m" c
  t <- num "t_m" c
  pure (nums [("vf_m_s", S.flutterSpeed a p g sp rt tp t)])
run "bulkhead" c = do
  a <- num "radius_m" c
  p <- num "p_pa" c
  sf <- num "sf" c
  allow <- num "allow_pa" c
  pure (nums [("t_m", S.bulkheadThickness (fromMaybe 0.33 (num "nu" c)) a p sf allow)])
-- Radio (Oracle.Radio) -------------------------------------------------------
run "fspl" c = do
  d <- num "d_m" c
  f <- num "f_mhz" c
  pure (nums [("loss_db", R.fsplDb d f)])
run "radio_geometry" c = do
  g <- num "ground_m" c
  h1 <- num "station_alt_m" c
  h2 <- num "payload_alt_m" c
  -- the design's Earth: mean radius and the 4/3 standard-refraction factor
  let geo = R.geometry (fromMaybe 6371000 (num "earth_r_m" c)) (fromMaybe (4 / 3) (num "k_factor" c)) g h1 h2
  pure (nums [ ("slant_m", R.slantM geo), ("elevation_rad", R.elevationRad geo)
             , ("within_horizon", if R.withinHorizon geo then 1 else 0) ])
run "dipole_gain" c = do
  pk <- num "peak_dbi" c
  el <- num "el_rad" c
  pure (nums [("gain_dbi", R.dipoleGainDbi (fromMaybe (-10) (num "floor_dbi" c)) pk el)])
-- GNSS coexistence (Oracle.Radio). Defaults are the design's constants
-- (wright-balloon rules.rs): L1 1575.42 MHz (IS-GPS-200), patch -10 dB
-- bandwidth 20 MHz, C/A main lobe 2.046 MHz, MAX-M10S NF 1.5 dB, signal
-- -130 dBm, reacquisition -160 dBm, BGA524N6 P1dB -12 dBm, 10 dB back-off.
run "patch_rejection" c = do
  f <- num "f_mhz" c
  pure (nums [("rejection_db", R.patchRejectionDb (gnssF0 c) (gnssBw c) f)])
run "gnss_limits" c = do
  let p1 = fromMaybe (-12) (num "p1db_dbm" c)
      back = fromMaybe 10 (num "backoff_db" c)
      bw = fromMaybe 2.046 (num "bw_mhz" c)
      nf = fromMaybe 1.5 (num "nf_db" c)
      sig = fromMaybe (-130) (num "signal_dbm" c)
      re = fromMaybe (-160) (num "reacq_dbm" c)
  pure (nums [("blocking_dbm", R.blockingLimitDbm p1 back), ("inband_dbm", R.inbandLimitDbm bw nf sig re)])
run "gnss_harmonics" c = do
  f <- num "f_mhz" c
  let hs = R.harmonicsIn (gnssF0 c) (fromMaybe 2.046 (num "bw_mhz" c)) f
  pure (nums [("count", fromIntegral (length hs)), ("lowest", if null hs then 0 else fromIntegral (minimum hs))])
run "gnss_coexist" c = do
  tx <- num "tx_dbm" c
  g <- num "gain_dbi" c
  sep <- num "sep_m" c
  f <- num "f_mhz" c
  let coupled = R.coupledDbm tx g sep f
  pure (nums [ ("coupled_dbm", coupled), ("at_gnss_dbm", coupled - R.patchRejectionDb (gnssF0 c) (gnssBw c) f)
             , ("harmonic_dbm", coupled - fromMaybe 60 (num "harmonic_db" c)) ])
-- Thermal (Oracle.Thermal). Warmer defaults: HotHands 29.7 g packet, 90 %
-- contents, 50 % iron, half oxidised within the rated time (rules.rs).
run "warmer_power" c = do
  h <- num "rated_h" c
  pure (nums [("watts", T.warmerPowerW (fromMaybe 29.7 (num "packet_g" c)) (fromMaybe 0.9 (num "contents" c))
                                       (fromMaybe 0.5 (num "iron" c)) (fromMaybe 0.5 (num "conversion" c)) h)])
run "capacity_fraction" c = do
  chem <- str "chemistry" c
  t <- num "t_c" c
  tab <- case chem of
    "lipo" -> Just T.lipoCapacity
    "li_ion" -> Just T.lipoCapacity
    "lithium_primary" -> Just T.l91Capacity
    _ -> Nothing
  pure (nums [("fraction", T.capacityFraction tab t)])
run "box_thermal" c = do
  cap <- num "heat_cap_j_k" c
  rth <- num "r_th_k_w" c
  p <- num "p_w" c
  amb <- num "amb_c" c
  dt <- num "dt_s" c
  n <- round <$> num "steps" c
  t0 <- num "t0_c" c
  pure (nums [ ("t_c", T.boxAfter cap rth p amb dt n t0), ("t_closed_c", T.boxClosedForm cap rth p amb dt n t0)
             , ("t_steady_c", T.boxSteady rth p amb) ])
-- Rocket aero, recovery and closures (Oracle.Aero) ---------------------------
run "von_karman" c = do
  x <- num "x_m" c
  l <- num "l_m" c
  r <- num "r_m" c
  pure (nums [("radius_m", A.vonKarmanRadius x l r)])
run "stagnation_cd" c = do
  m <- num "mach" c
  pure (nums [("cd", A.stagnationCd m)])
run "nose_cd" c = do
  m <- num "mach" c
  f <- num "fineness" c
  pure (nums [("cd", A.noseCd m f)])
run "descent_rate" c = do
  m <- num "mass_kg" c
  d <- (* 0.0254) <$> num "diameter_in" c
  cd <- num "cd" c
  rho <- num "rho" c
  pure (nums [("v_m_s", A.descentRate m 9.80665 d cd rho)])
run "closure_capacity" c = do
  w <- num "wall_m" c
  let cl = A.closureCapacity w
  pure (nums [("shear_n", A.screwShear cl), ("bearing_n", A.wallBearing cl), ("tearout_n", A.wallTearout cl)])
run "closure_screws" c = do
  d <- num "inner_d_m" c
  w <- num "wall_m" c
  p <- num "meop_pa" c
  pure (nums [("n", fromIntegral (A.closureScrews d w p))])
run f c | take 4 f == "pcb_" = Pcb.pcbCase f c
run _ _ = Nothing

gnssF0, gnssBw :: J -> Double
gnssF0 c = fromMaybe 1575.42 (num "f0_mhz" c)
gnssBw c = fromMaybe 20 (num "bw_mhz" c)

-- Defaults are the CUSF burst calculator's (index.html): air 1.2050,
-- helium 0.1786 kg/m^3, g 9.80665, cd 0.3.
balloonInputs :: J -> Maybe (Double, Double, Double, Double, Double, Double)
balloonInputs c = do
  mb <- num "balloon_kg" c
  mp <- num "payload_kg" c
  pure ( fromMaybe 1.2050 (num "rho_air" c), fromMaybe 0.1786 (num "rho_gas" c)
       , fromMaybe 9.80665 (num "g" c), fromMaybe 0.3 (num "cd" c), mb, mp )
