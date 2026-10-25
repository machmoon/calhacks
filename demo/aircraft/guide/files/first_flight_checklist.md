# First-flight checklist: quadplane_130mi

Work top to bottom; a NO-GO line stops the session.

## 1. Before power: CG and balance

- [ ] CG at 74 mm behind the wing root's leading edge (static margin 10 % MAC, neutral point 95 mm). Slide the battery to get it; do not fly tail-heavy.
- [ ] Take-off mass 4529 g (+/- 5 %); heavier means more hover current than predicted below.

## 2. Control surfaces (props OFF), in FBWA

| output | function | stick input | expected deflection |
|---|---|---|---|
| SERVO6 | left aileron | roll right | left aileron DOWN |
| SERVO7 | right aileron | roll right | right aileron UP (set SERVO7_REVERSED if not) |
| SERVO8 | elevator | pull back | elevator UP |
| SERVO9/10 | rudders | yaw right | both rudders' trailing edges RIGHT |
| (stabilisation) | | tilt the nose DOWN by hand | elevator moves UP to correct |
| (stabilisation) | | roll the right wing DOWN by hand | right aileron moves DOWN |

## 3. Motor order and spin (props OFF), ArduPilot Quad X

Mission Planner > Motor Test, one at a time:

| output | position | spin (from above) |
|---|---|---|
| SERVO1 (Motor1) | front right | CCW |
| SERVO2 (Motor2) | rear left | CCW |
| SERVO3 (Motor3) | front left | CW |
| SERVO4 (Motor4) | rear right | CW |
| SERVO5 | pusher | pushes air AFT (thrust forward) |

## 4. Parameters that matter for the first transition

- [ ] Q_ENABLE = 1 (QuadPlane on)
- [ ] Q_FRAME_CLASS = 1 (quad)
- [ ] Q_FRAME_TYPE = 1 (X, matching the motor table above)
- [ ] Q_M_THST_HOVER = 0.357 (predicted hover throttle; Q_M_HOVER_LEARN refines it)
- [ ] Q_ASSIST_SPEED = 15.4 (quad motors assist below this airspeed)
- [ ] AIRSPEED_MIN = 18 (transition completes above this; 1.25 x predicted stall)
- [ ] AIRSPEED_CRUISE = 19.3 (best-range speed)
- [ ] Q_M_BAT_CURR_MAX = 90 (lift current cap: pack and connector burst)
- [ ] TRIM_THROTTLE = 69 (pusher throttle at cruise)

Predicted stall 14.0 m/s; transition airspeed (AIRSPEED_MIN) 18 m/s.

## 5. Hover current: GO / NO-GO (first QHOVER, 1-2 m, calm)

| quantity | predicted | NO-GO if measured above |
|---|---|---|
| per-motor current, full pack | 18.4 A | 21.2 A (+15 %) |
| pack current, full pack | 28.0 A | 32.2 A (+15 %) |
| hover throttle (Q_M_THST_HOVER learnt) | 0.36 | 0.65 |
| per-motor current, hot day, low pack (landing) | 18.4 A | rated 46 A |

Measure on the bench with props on, the airframe held down, and a clamp meter on one motor's phase or the pack lead.

## 6. First transition

- [ ] QLOITER to 20 m, into wind; switch to FBWA; expect the pusher to spool and the quad motors to hand over above 15 m/s.
- [ ] If it pitches up as the pusher spools, abort to QHOVER: the thrust line is +0 mm from the CG's height by design.
