-- | The PCB oracle: an independent second reader of a routed board
-- (docs/electronics-plan.md section 4, Lane F). It reads the design's board
-- JSON, which is @serde_json::to_string(&(netlist, board, route))@ of
-- @wright_eda::{Netlist, Board, RouteReport}@, and recomputes from the raw
-- geometry, with its own transform and distance code:
--
--   1. absolute pad positions and extents (own rotation and flip);
--   2. courtyard overlap over all pairs;
--   3. copper connectivity per net: components over the net's pads, tracks
--      and vias, layer-aware; a net is connected when all its pads are in
--      one component. A pour joins its net's copper on its layer only when
--      the case carries @"drc_unconnected": 0@ (KiCad's DRC filled the zone
--      and found nothing unconnected); otherwise the pour is unverified;
--   4. the minimum distance between different-net copper on a shared layer
--      (segment-segment, segment-rectangle, rectangle-rectangle and circle
--      distances, minus half-widths), and the pairs closer than the larger
--      of the two nets' class clearances and the board minimum;
--   5. IPC-2221 ampacity of each net's narrowest track;
--   6. the TPS5450 output from its feedback divider.
--
-- Nothing is taken from the Rust. The design follows the Python baseline's second reader,
-- @engine/silkscreen/audit/rules.py@ (@_net_connectivity@: union of pads,
-- tracks and vias touching on a shared layer, a via joining every layer;
-- @_DisjointSet@), and its ampacity curve, @aircraft/electronics.py@
-- (@trace_capacity_a@: IPC-2221A sect. 6.2, I = k dT^0.44 A^0.725,
-- k = 0.048 external, A in square mils, 1 oz = 1.378 mil). The rotation
-- is KiCad's @RotatePoint@ convention (Y down, positive angle
-- counter-clockwise on screen): x' = x cos + y sin, y' = -x sin + y cos,
-- written here from that formula rather than from @wright_eda::Pt::rot90@.
-- The setpoint is TI SLVS757 sect. 8.2.2.2: Vout = 1.221 (1 + R1 / R2),
-- R1 from VSENSE to the output, R2 from VSENSE to ground.
--
-- Copper shapes: a circle pad is a disc of the pad width; an oval pad is a
-- stadium along the long side of its board-frame extent; every other pad
-- (rect, roundrect, custom) is its board-frame bounding rectangle, which
-- is conservative for roundrect corners. Tracks are stadiums (segment plus
-- half-width), vias discs on both copper layers. A distance below zero
-- (overlap) is reported as 0.
--
-- What this does not do: verify the Rust code. It recomputes the same
-- quantities a second way, and haskell/crosscheck.sh compares the two
-- under stated tolerances; agreement means two readings agree, not that
-- either is proved correct.
module Oracle.Pcb
  ( P
  , Layer (..)
  , Side (..)
  , Shape (..)
  , Pad (..)
  , Footprint (..)
  , Placed (..)
  , Track (..)
  , Via (..)
  , Zone (..)
  , NetClass (..)
  , Board (..)
  , PinI (..)
  , PartI (..)
  , NetI (..)
  , Route (..)
  , Design (..)
  , RectI (..)
  , decodeDesign
  , rotQ
  , toBoard
  , padCentre
  , padRect
  , courtyardAbs
  , overlapPairs
  , Geo (..)
  , Item (..)
  , copperItems
  , geoDist
  , netIslands
  , minGap
  , clearanceViolations
  , traceCapacityA
  , parseOhms
  , tps5450Vout
  , regulatorVout
  , translateDesign
  , rotateDesign
  , pcbCase
  ) where

import Data.Char (isDigit, isSpace)
import Data.List (nub, sort)
import Data.Maybe (fromMaybe, isJust)
import Oracle.Json

-- * Board model (field names follow the serde JSON of wright_eda)

type P = (Integer, Integer)

data RectI = RectI {rMin :: P, rMax :: P} deriving (Eq, Show)

data Layer = FCu | BCu deriving (Eq, Ord, Show)

data Side = Top | Bottom deriving (Eq, Show)

data Shape = SRect | SRoundRect Double | SCircle | SOval | SOther deriving (Eq, Show)

data Pad = Pad
  { padNumber :: String
  , padAt :: P
  , padRotDeg :: Double
  , padSize :: (Integer, Integer)
  , padShape :: Shape
  , padDrill :: Maybe Integer
  , padPlated :: Bool
    -- | Copper layers the library names for the pad, footprint frame
    -- (@wright_eda::Pad.layers@, serde default empty). Empty = the old
    -- rule: SMD on the part's side, plated hole on both, NPTH none.
  , padLayerList :: [Layer]
  } deriving (Eq, Show)

data Footprint = Footprint {fpLib :: String, fpPads :: [Pad], fpCourtyard :: RectI} deriving (Eq, Show)

data Placed = Placed
  { plPart :: Int
  , plFp :: Footprint
  , plAt :: P
  , plRot :: Int
  , plSide :: Side
  } deriving (Eq, Show)

data Track = Track {trA :: P, trB :: P, trWidth :: Integer, trLayer :: Layer, trNet :: Int} deriving (Eq, Show)

data Via = Via {viaAt :: P, viaDiameter :: Integer, viaNet :: Int} deriving (Eq, Show)

data Zone = Zone {zNet :: Int, zLayer :: Layer, zOutline :: [P]} deriving (Eq, Show)

data NetClass = NetClass {ncName :: String, ncWidth :: Integer, ncClearance :: Integer, ncNets :: [Int]} deriving (Eq, Show)

data Board = Board
  { bOutline :: RectI
  , bParts :: [Placed]
  , bTracks :: [Track]
  , bVias :: [Via]
  , bZones :: [Zone]
  , bClasses :: [NetClass]
  , bMinClearance :: Integer
  , bCopperOz :: Double
  } deriving (Eq, Show)

data PinI = PinI {pinName :: String, pinPad :: String, pinNet :: Maybe Int, pinNC :: Bool} deriving (Eq, Show)

data PartI = PartI {ptRef :: String, ptName :: String, ptKind :: String, ptValue :: String, ptPins :: [PinI]} deriving (Eq, Show)

data NetI = NetI {netName :: String, netRole :: String, netPins :: [(Int, String)]} deriving (Eq, Show)

data Route = Route
  { rtAttempted :: [Int]
  , rtRouted :: [Int]
  , rtUnrouted :: [Int]
  , rtPoured :: [Int]
  } deriving (Eq, Show)

data Design = Design {dParts :: [PartI], dNets :: [NetI], dBoard :: Board, dRoute :: Route} deriving (Eq, Show)

-- * JSON decoding

arr :: String -> J -> Maybe [J]
arr k o = case field k o of
  Just (JArr xs) -> Just xs
  _ -> Nothing

int :: J -> Maybe Integer
int (JNum x) | x == fromIntegral (round x :: Integer) = Just (round x)
int _ = Nothing

intF :: String -> J -> Maybe Integer
intF k o = field k o >>= int

idx :: String -> J -> Maybe Int
idx k o = fromIntegral <$> intF k o

pt :: J -> Maybe P
pt o = (,) <$> intF "x" o <*> intF "y" o

rect :: J -> Maybe RectI
rect o = RectI <$> (field "min" o >>= pt) <*> (field "max" o >>= pt)

layer :: J -> Maybe Layer
layer (JStr "F.Cu") = Just FCu
layer (JStr "B.Cu") = Just BCu
layer _ = Nothing

shape :: J -> Maybe Shape
shape (JStr "rect") = Just SRect
shape (JStr "circle") = Just SCircle
shape (JStr "oval") = Just SOval
shape (JStr "other") = Just SOther
shape (JObj [("round_rect", JNum r)]) = Just (SRoundRect r)
shape _ = Nothing

pad :: J -> Maybe Pad
pad o = do
  n <- str "number" o
  at <- field "at" o >>= pt
  r <- num "rot_deg" o
  sz <- case field "size" o of
    Just (JArr [a, b]) -> (,) <$> int a <*> int b
    _ -> Nothing
  sh <- field "shape" o >>= shape
  dr <- case field "drill" o of
    Just JNull -> Just Nothing
    Nothing -> Just Nothing
    Just v -> Just <$> int v
  let plated = field "plated" o == Just (JBool True)
  ls <- case field "layers" o of
    Nothing -> Just []
    Just JNull -> Just []
    Just (JArr xs) -> mapM layer xs
    Just _ -> Nothing
  pure (Pad n at r sz sh dr plated ls)

footprint :: J -> Maybe Footprint
footprint o = Footprint <$> str "lib_id" o <*> (arr "pads" o >>= mapM pad) <*> (field "courtyard" o >>= rect)

placed :: J -> Maybe Placed
placed o = do
  p <- idx "part" o
  fp <- field "footprint" o >>= footprint
  at <- field "at" o >>= pt
  r <- idx "rot" o
  s <- case str "side" o of
    Just "top" -> Just Top
    Just "bottom" -> Just Bottom
    _ -> Nothing
  pure (Placed p fp at r s)

track :: J -> Maybe Track
track o = Track <$> (field "a" o >>= pt) <*> (field "b" o >>= pt) <*> intF "width" o <*> (field "layer" o >>= layer) <*> idx "net" o

via :: J -> Maybe Via
via o = Via <$> (field "at" o >>= pt) <*> intF "diameter" o <*> idx "net" o

zone :: J -> Maybe Zone
zone o = Zone <$> idx "net" o <*> (field "layer" o >>= layer) <*> (arr "outline" o >>= mapM pt)

netClass :: J -> Maybe NetClass
netClass o = NetClass <$> str "name" o <*> intF "track_width" o <*> intF "clearance" o
  <*> (arr "nets" o >>= mapM (fmap fromIntegral . int))

board :: J -> Maybe Board
board o = do
  out <- field "outline" o >>= rect
  ps <- arr "parts" o >>= mapM placed
  ts <- arr "tracks" o >>= mapM track
  vs <- arr "vias" o >>= mapM via
  zs <- arr "zones" o >>= mapM zone
  cs <- arr "classes" o >>= mapM netClass
  rules <- field "rules" o
  mc <- intF "min_clearance" rules
  oz <- num "copper_oz" rules
  pure (Board out ps ts vs zs cs mc oz)

pinI :: J -> Maybe PinI
pinI o = do
  n <- str "name" o
  p <- str "pad" o
  net <- case field "net" o of
    Just JNull -> Just Nothing
    Just v -> Just . fromIntegral <$> int v
    Nothing -> Just Nothing
  pure (PinI n p net (field "no_connect" o == Just (JBool True)))

partI :: J -> Maybe PartI
partI o = PartI <$> str "refdes" o <*> str "name" o <*> str "kind" o <*> str "value" o <*> (arr "pins" o >>= mapM pinI)

netI :: J -> Maybe NetI
netI o = NetI <$> str "name" o <*> str "role" o <*> (arr "pins" o >>= mapM (\p -> (,) <$> idx "part" p <*> str "pad" p))

route :: J -> Maybe Route
route o = do
  let ints k = arr k o >>= mapM (fmap fromIntegral . int)
  a <- ints "attempted"
  r <- ints "routed"
  u <- arr "unrouted" o >>= mapM (idx "net")
  p <- ints "poured"
  pure (Route a r u p)

-- | @[netlist, board, route]@ as wright_eda serialises the tuple.
decodeDesign :: J -> Maybe Design
decodeDesign (JArr [nl, b, r]) = Design <$> (arr "parts" nl >>= mapM partI) <*> (arr "nets" nl >>= mapM netI) <*> board b <*> route r
decodeDesign _ = Nothing

-- * Frames

-- | (cos, sin) of q quarter turns.
cosSin :: Int -> (Integer, Integer)
cosSin q = case q `mod` 4 of
  0 -> (1, 0)
  1 -> (0, 1)
  2 -> (-1, 0)
  _ -> (0, -1)

-- | KiCad RotatePoint in the Y-down frame: x' = x c + y s, y' = -x s + y c.
rotQ :: Int -> P -> P
rotQ q (x, y) = let (c, s) = cosSin q in (x * c + y * s, negate x * s + y * c)

-- | Footprint-local point to board frame: a bottom-side part is mirrored
-- in X (KiCad flips footprints left-right), then rotated, then moved.
toBoard :: Placed -> P -> P
toBoard pl (x, y) =
  let x' = if plSide pl == Bottom then negate x else x
      (rx, ry) = rotQ (plRot pl) (x', y)
      (ax, ay) = plAt pl
  in (rx + ax, ry + ay)

padCentre :: Placed -> Pad -> P
padCentre pl p = toBoard pl (padAt p)

-- | Round half away from zero (Rust's f64::round; Haskell's round is
-- banker's rounding).
roundAway :: Double -> Integer
roundAway x = if x >= 0 then floor (x + 0.5) else negate (floor (negate x + 0.5))

-- | Half extents of a pad in its footprint, its own rotation applied
-- (the bounding box of the rotated rectangle for a non-90-degree angle).
padHalf :: Pad -> (Integer, Integer)
padHalf p =
  let (w, h) = padSize p
      d = padRotDeg p
      q = round (d / 90) :: Integer
  in if d == fromIntegral q * 90
       then (if even q then (roundAway (fromIntegral w / 2), roundAway (fromIntegral h / 2)) else (roundAway (fromIntegral h / 2), roundAway (fromIntegral w / 2)))
       else
         let a = d * pi / 180
             c = abs (cos a)
             s = abs (sin a)
             wd = fromIntegral w
             hd = fromIntegral h
         in (roundAway ((wd * c + hd * s) / 2), roundAway ((wd * s + hd * c) / 2))

-- | A pad's extent on the board.
padRect :: Placed -> Pad -> RectI
padRect pl p =
  let (cx, cy) = padCentre pl p
      (hx0, hy0) = padHalf p
      (hx, hy) = if odd (plRot pl `mod` 4) then (hy0, hx0) else (hx0, hy0)
  in RectI (cx - hx, cy - hy) (cx + hx, cy + hy)

norm :: P -> P -> RectI
norm (ax, ay) (bx, by) = RectI (min ax bx, min ay by) (max ax bx, max ay by)

courtyardAbs :: Placed -> RectI
courtyardAbs pl = let RectI a b = fpCourtyard (plFp pl) in norm (toBoard pl a) (toBoard pl b)

-- | Interiors overlap; touching edges do not.
rectsOverlap :: RectI -> RectI -> Bool
rectsOverlap (RectI (ax0, ay0) (ax1, ay1)) (RectI (bx0, by0) (bx1, by1)) =
  ax0 < bx1 && bx0 < ax1 && ay0 < by1 && by0 < ay1

overlapPairs :: Board -> [(Int, Int)]
overlapPairs b =
  let cs = zip [0 ..] (map courtyardAbs (bParts b))
  in [(i, j) | (i, ci) <- cs, (j, cj) <- cs, i < j, rectsOverlap ci cj]

-- * Copper geometry

-- | A segment with a radius (a disc when both ends coincide), or a box.
data Geo = Seg P P Double | Box RectI deriving (Eq, Show)

data Item = Item {itNet :: Int, itLayers :: [Layer], itGeo :: Geo, itIsPad :: Bool} deriving (Eq, Show)

sideLayer :: Side -> Layer
sideLayer Top = FCu
sideLayer Bottom = BCu

padGeo :: Placed -> Pad -> Geo
padGeo pl p =
  let r@(RectI (x0, y0) (x1, y1)) = padRect pl p
      c@(cx, cy) = padCentre pl p
      hx = (x1 - x0) `div` 2
      hy = (y1 - y0) `div` 2
  in case padShape p of
       SCircle -> Seg c c (fromIntegral (fst (padSize p)) / 2)
       SOval
         | hx >= hy -> Seg (cx - (hx - hy), cy) (cx + (hx - hy), cy) (fromIntegral hy)
         | otherwise -> Seg (cx, cy - (hy - hx)) (cx, cy + (hy - hx)) (fromIntegral hx)
       _ -> Box r

-- | A pad's copper layers on the board. A pad that names its layers (the
-- back-side PowerPAD of @TI_SO-PowerPAD-8_ThermalVias@ is B.Cu on a
-- top-side part) uses them, flipped for a bottom-side part as KiCad's
-- @FOOTPRINT::Flip@ flips every layer; otherwise SMD is on the part's side
-- and a plated hole on both. An NPTH hole has no copper either way.
padLayers :: Placed -> Pad -> [Layer]
padLayers pl p = case padDrill p of
  Just _ | not (padPlated p) -> []
  _ | not (null (padLayerList p)) -> nub (sort (map flipL (padLayerList p)))
  Nothing -> [sideLayer (plSide pl)]
  Just _ -> [FCu, BCu]
  where
    flipL l
      | plSide pl == Bottom = if l == FCu then BCu else FCu
      | otherwise = l

-- | (part index, pad number) -> netlist net.
padNetMap :: Design -> [((Int, String), Int)]
padNetMap d = [((pa, pn), i) | (i, n) <- zip [0 ..] (dNets d), (pa, pn) <- netPins n]

-- | Every copper item. Pads on no netlist net get a net of their own
-- (negative), so they are a different net from everything else.
copperItems :: Design -> [Item]
copperItems d = padItems ++ trackItems ++ viaItems
  where
    b = dBoard d
    m = padNetMap d
    padItems =
      [ Item (fromMaybe (negate k) (lookup (plPart pl, padNumber p) m)) ls (padGeo pl p) True
      | (k, (pl, p)) <- zip [1 ..] [(pl, p) | pl <- bParts b, p <- fpPads (plFp pl)]
      , let ls = padLayers pl p
      , not (null ls) ]
    trackItems = [Item (trNet t) [trLayer t] (Seg (trA t) (trB t) (fromIntegral (trWidth t) / 2)) False | t <- bTracks b]
    viaItems = [Item (viaNet v) [FCu, BCu] (Seg (viaAt v) (viaAt v) (fromIntegral (viaDiameter v) / 2)) False | v <- bVias b]

dbl :: Integer -> Double
dbl = fromIntegral

pdist :: P -> P -> Double
pdist (ax, ay) (bx, by) = sqrt (dbl (ax - bx) ^ (2 :: Int) + dbl (ay - by) ^ (2 :: Int))

pointSeg :: P -> P -> P -> Double
pointSeg p@(px, py) a@(ax, ay) b@(bx, by)
  | a == b = pdist p a
  | otherwise =
      let (dx, dy) = (bx - ax, by - ay)
          l2 = dx * dx + dy * dy
          t = dbl ((px - ax) * dx + (py - ay) * dy) / dbl l2
      in if t <= 0 then pdist p a
         else if t >= 1 then pdist p b
         else let qx = dbl ax + t * dbl dx
                  qy = dbl ay + t * dbl dy
              in sqrt ((dbl px - qx) ^ (2 :: Int) + (dbl py - qy) ^ (2 :: Int))

orient :: P -> P -> P -> Integer
orient (ax, ay) (bx, by) (cx, cy) = signum ((bx - ax) * (cy - ay) - (by - ay) * (cx - ax))

onSeg :: P -> P -> P -> Bool
onSeg (ax, ay) (bx, by) (cx, cy) = min ax bx <= cx && cx <= max ax bx && min ay by <= cy && cy <= max ay by

-- | Closed segments intersect (exact, integer orientation tests).
segsMeet :: P -> P -> P -> P -> Bool
segsMeet p1 q1 p2 q2 =
  let o1 = orient p1 q1 p2
      o2 = orient p1 q1 q2
      o3 = orient p2 q2 p1
      o4 = orient p2 q2 q1
  in (o1 /= o2 && o3 /= o4)
     || (o1 == 0 && onSeg p1 q1 p2) || (o2 == 0 && onSeg p1 q1 q2)
     || (o3 == 0 && onSeg p2 q2 p1) || (o4 == 0 && onSeg p2 q2 q1)

segSeg :: P -> P -> P -> P -> Double
segSeg a1 b1 a2 b2
  | segsMeet a1 b1 a2 b2 = 0
  | otherwise = minimum [pointSeg a1 a2 b2, pointSeg b1 a2 b2, pointSeg a2 a1 b1, pointSeg b2 a1 b1]

inside :: RectI -> P -> Bool
inside (RectI (x0, y0) (x1, y1)) (x, y) = x0 <= x && x <= x1 && y0 <= y && y <= y1

segBox :: P -> P -> RectI -> Double
segBox a b r@(RectI (x0, y0) (x1, y1))
  | inside r a || inside r b = 0
  | otherwise = minimum [segSeg a b p q | (p, q) <- [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))]]

boxBox :: RectI -> RectI -> Double
boxBox (RectI (ax0, ay0) (ax1, ay1)) (RectI (bx0, by0) (bx1, by1)) =
  let dx = max 0 (max (bx0 - ax1) (ax0 - bx1))
      dy = max 0 (max (by0 - ay1) (ay0 - by1))
  in sqrt (dbl dx ^ (2 :: Int) + dbl dy ^ (2 :: Int))

-- | Edge-to-edge distance; negative when the copper overlaps by more than
-- touching (only segment shapes can report a depth).
geoDist :: Geo -> Geo -> Double
geoDist (Seg a1 b1 r1) (Seg a2 b2 r2) = segSeg a1 b1 a2 b2 - (r1 + r2)
geoDist (Seg a b r) (Box x) = segBox a b x - r
geoDist (Box x) (Seg a b r) = segBox a b x - r
geoDist (Box x) (Box y) = boxBox x y

shareLayer :: Item -> Item -> Bool
shareLayer a b = any (`elem` itLayers b) (itLayers a)

touches :: Item -> Item -> Bool
touches a b = shareLayer a b && geoDist (itGeo a) (itGeo b) <= 0

-- | Connected components of [0 .. n-1] under an adjacency test.
components :: Int -> (Int -> Int -> Bool) -> [[Int]]
components n adj = go [0 .. n - 1] []
  where
    go [] acc = reverse acc
    go (s : rest) acc =
      let comp = grow [s] [s]
      in go (filter (`notElem` comp) rest) (sort comp : acc)
    grow [] seen = seen
    grow (x : frontier) seen =
      let new = [y | y <- [0 .. n - 1], y `notElem` seen, adj x y]
      in grow (frontier ++ new) (seen ++ new)

centreOf :: Geo -> (Double, Double)
centreOf (Seg (ax, ay) (bx, by) _) = ((dbl ax + dbl bx) / 2, (dbl ay + dbl by) / 2)
centreOf (Box (RectI (x0, y0) (x1, y1))) = ((dbl x0 + dbl x1) / 2, (dbl y0 + dbl y1) / 2)

-- | Even-odd ray cast.
inPolygon :: [P] -> (Double, Double) -> Bool
inPolygon ps (x, y) = odd (length (filter crosses (zip ps (drop 1 ps ++ take 1 ps))))
  where
    crosses ((ax, ay), (bx, by)) =
      let (ax', ay', bx', by') = (dbl ax, dbl ay, dbl bx, dbl by)
      in (ay' > y) /= (by' > y) && x < (bx' - ax') * (y - ay') / (by' - ay') + ax'

-- | Islands of a net's pads: the number of copper components holding at
-- least one of its pads, plus one for every netlist pin with no pad on the
-- board (a pin with no copper reaches nothing). @zonesOk@: pours count.
netIslands :: Bool -> Design -> Int -> Int
netIslands zonesOk d n =
  let items = [it | it <- copperItems d, itNet it == n]
      zs = [z | zonesOk, z <- bZones (dBoard d), zNet z == n, n `elem` rtPoured (dRoute d)]
      inZone z it = zLayer z `elem` itLayers it && inPolygon (zOutline z) (centreOf (itGeo it))
      k = length items
      adj i j = let a = items !! i; b = items !! j
                in touches a b || any (\z -> inZone z a && inZone z b) zs
      comps = components k adj
      padComps = length [c | c <- comps, any (\i -> itIsPad (items !! i)) c]
      onBoard (pa, pn) = or [plPart pl == pa && padNumber p == pn && not (null (padLayers pl p)) | pl <- bParts (dBoard d), p <- fpPads (plFp pl)]
      missing = length (filter (not . onBoard) (netPins (dNets d !! n)))
  in padComps + missing

-- | The clearance a pair needs: the larger class clearance, at least the
-- board minimum (KiCad takes the larger of the two items' clearances).
classClearance :: Board -> Int -> Integer
classClearance b n
  | n < 0 = bMinClearance b
  | otherwise = case [c | c <- bClasses b, n `elem` ncNets c] ++ take 1 (bClasses b) of
      (c : _) -> ncClearance c
      [] -> bMinClearance b

-- | (gap, required) for every different-net pair of copper items on a
-- shared layer; the gap is clamped at 0.
gapPairs :: Design -> [(Double, Integer)]
gapPairs d =
  let b = dBoard d
      its = zip [0 :: Int ..] (copperItems d)
  in [ (max 0 (geoDist (itGeo x) (itGeo y)), maximum [classClearance b (itNet x), classClearance b (itNet y), bMinClearance b])
     | (i, x) <- its, (j, y) <- its, i < j, itNet x /= itNet y, shareLayer x y ]

minGap :: Design -> Maybe Double
minGap d = case map fst (gapPairs d) of
  [] -> Nothing
  gs -> Just (minimum gs)

clearanceViolations :: Design -> Int
clearanceViolations d = length [() | (g, req) <- gapPairs d, g < dbl req]

-- * Ampacity and the regulator

-- | IPC-2221 external layer at a rise of @riseC@: width in nm, copper in oz.
traceCapacityA :: Integer -> Double -> Double -> Double
traceCapacityA widthNm riseC oz =
  let areaMil2 = dbl widthNm / 25400 * 1.378 * oz
  in 0.048 * riseC ** 0.44 * areaMil2 ** 0.725

-- | "10k", "2.55k", "4k7", "1M", "100R", "100", "10k 1%" -> ohms.
parseOhms :: String -> Maybe Double
parseOhms s0 =
  let s = takeWhile (not . isSpace) (dropWhile isSpace s0)
      (a, rest) = span (\c -> isDigit c || c == '.') s
      mult c = lookup c [('R', 1), ('r', 1), ('k', 1e3), ('K', 1e3), ('M', 1e6), ('m', 1e-3)]
      readD t = case reads (if take 1 t == "." then '0' : t else t) :: [(Double, String)] of
        [(v, "")] -> Just v
        _ -> Nothing
  in case rest of
       "" -> readD a
       (c : tl) -> do
         m <- mult c
         let tl' = filter (/= '\937') tl -- an Omega sign after the multiplier
         if null tl' || tl' == "ohm"
           then (* m) <$> readD a
           else if all isDigit tl' && '.' `notElem` a
             then (* m) <$> readD (a ++ "." ++ tl')
             else Nothing

tps5450Vout :: Double -> Double -> Double
tps5450Vout r1 r2 = 1.221 * (1 + r1 / r2)

-- | (R1, R2, Vout) of the first TPS5450's VSENSE divider.
regulatorVout :: Design -> Either String (Double, Double, Double)
regulatorVout d = case [p | p <- dParts d, take 7 (ptValue p) == "TPS5450" || take 7 (ptName p) == "TPS5450"] of
  [] -> Left "no TPS5450 in the netlist"
  (u : _) -> case [n | PinI nm _ (Just n) _ <- ptPins u, nm == "VSENSE"] of
    [] -> Left "TPS5450 VSENSE is on no net"
    (fb : _) ->
      let rs = [ (v, other)
               | p <- dParts d, ptKind p == "resistor"
               , any ((== Just fb) . pinNet) (ptPins p)
               , Just v <- [parseOhms (ptValue p)]
               , other <- [n | PinI _ _ (Just n) _ <- ptPins p, n /= fb] ]
          isGnd n = netRole (dNets d !! n) == "ground"
          r2s = [v | (v, o) <- rs, isGnd o]
          r1s = [v | (v, o) <- rs, not (isGnd o)]
      in case (r1s, r2s) of
           ([r1], [r2]) -> Right (r1, r2, tps5450Vout r1 r2)
           _ -> Left ("VSENSE divider is not one resistor to the output and one to ground (" ++ show (length r1s) ++ " up, " ++ show (length r2s) ++ " down)")

-- * Whole-board transforms (for the invariance properties)

mapPts :: (P -> P) -> Int -> Design -> Design
mapPts f q d =
  let b = dBoard d
      RectI a c = bOutline b
      pl' p = p {plAt = f (plAt p), plRot = (plRot p + q) `mod` 4}
  in d { dBoard = b
         { bOutline = norm (f a) (f c)
         , bParts = map pl' (bParts b)
         , bTracks = [t {trA = f (trA t), trB = f (trB t)} | t <- bTracks b]
         , bVias = [v {viaAt = f (viaAt v)} | v <- bVias b]
         , bZones = [z {zOutline = map f (zOutline z)} | z <- bZones b] } }

translateDesign :: P -> Design -> Design
translateDesign (dx, dy) = mapPts (\(x, y) -> (x + dx, y + dy)) 0

-- | Rotate the whole board by q quarter turns about the origin.
rotateDesign :: Int -> Design -> Design
rotateDesign q = mapPts (rotQ q) q

-- * The JSON cases (Oracle.Cases dispatches "pcb_*" here)

nums :: [(String, Double)] -> [(String, J)]
nums = map (fmap JNum)

refOf :: Design -> Placed -> String
refOf d pl = if plPart pl < length (dParts d) then ptRef (dParts d !! plPart pl) else "part" ++ show (plPart pl)

-- | Outputs are named "quantity:item" so the comparison groups them by
-- quantity and reports the item where the worst difference is.
pcbCase :: String -> J -> Maybe [(String, J)]
pcbCase f c = do
  d <- field "board" c >>= decodeDesign
  let b = dBoard d
      netNm n = netName (dNets d !! n)
      zonesOk = num "drc_unconnected" c == Just 0
      tracksOf n = [t | t <- bTracks b, trNet t == n]
  case f of
    "pcb_pads" -> Just $ nums $ concat
      [ [ ("pad_x:" ++ k, dbl x), ("pad_y:" ++ k, dbl y)
        , ("pad_minx:" ++ k, dbl x0), ("pad_miny:" ++ k, dbl y0), ("pad_maxx:" ++ k, dbl x1), ("pad_maxy:" ++ k, dbl y1) ]
      | pl <- bParts b, (i, p) <- zip [0 :: Int ..] (fpPads (plFp pl))
      , let k = refOf d pl ++ "#" ++ show i
            (x, y) = padCentre pl p
            RectI (x0, y0) (x1, y1) = padRect pl p ]
    "pcb_courtyard" -> Just $ nums $
      ("overlap_pairs", fromIntegral (length (overlapPairs b))) : concat
      [ [("crtyd_minx:" ++ k, dbl x0), ("crtyd_miny:" ++ k, dbl y0), ("crtyd_maxx:" ++ k, dbl x1), ("crtyd_maxy:" ++ k, dbl y1)]
      | pl <- bParts b, let k = refOf d pl, let RectI (x0, y0) (x1, y1) = courtyardAbs pl ]
    "pcb_connectivity" ->
      let r = dRoute d
          att = rtAttempted r
          isl = [(n, netIslands zonesOk d n) | n <- att]
          needs = [n | (n, nt) <- zip [0 ..] (dNets d), length (netPins nt) >= 2]
          floating = length [() | p <- dParts d, q <- ptPins p, not (isJust (pinNet q)), not (pinNC q)]
      in Just $ nums $
           [("conn:" ++ netNm n, if k == 1 then 1 else 0) | (n, k) <- isl]
           ++ [("islands:" ++ netNm n, fromIntegral k) | (n, k) <- isl]
           ++ [ ("unrouted_count", fromIntegral (length [() | (_, k) <- isl, k /= 1]))
              , ("claim_mismatches", fromIntegral (length [() | (n, k) <- isl, (n `elem` rtRouted r) /= (k == 1)]))
              , ("uncovered_nets", fromIntegral (length [n | n <- needs, n `notElem` att, n `notElem` rtPoured r]))
              , ("unverified_pours", fromIntegral (if zonesOk then 0 else length (nub (rtPoured r))))
              , ("floating_pins", fromIntegral floating) ]
    "pcb_clearance" -> do
      g <- minGap d
      Just (nums [("min_gap_nm", g), ("violations", fromIntegral (clearanceViolations d))])
    "pcb_ampacity" ->
      let rise = fromMaybe 20 (num "rise_c" c)
          per = [ (n, w, traceCapacityA w rise (bCopperOz b))
                | n <- nub (sort (map trNet (bTracks b))), let w = minimum (map trWidth (tracksOf n)) ]
          currents = case arr "net_current_a" c of
            Just xs -> Just [v | JNum v <- xs]
            Nothing -> Nothing
          short = case currents of
            Just cs -> [("short_nets", fromIntegral (length [() | (n, _, a) <- per, n < length cs, a < cs !! n]))]
            Nothing -> []
      in Just $ nums $ concat [[("minw_nm:" ++ netNm n, dbl w), ("amp_a:" ++ netNm n, a)] | (n, w, a) <- per] ++ short
    "pcb_regulator" -> case regulatorVout d of
      Right (r1, r2, v) -> Just (nums [("vout_v", v), ("r1_ohm", r1), ("r2_ohm", r2)])
      Left e -> Just [("error", JStr e)]
    _ -> Nothing

