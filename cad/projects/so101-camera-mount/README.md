# SO101 existing-project camera-mount edit

Wright uses its existing subscription model transport to generate a FreeCAD Python
module that adds a Raspberry Pi Camera Module 3 Standard mount to the existing
SO101 assembly. It does not regenerate the robot or replace FreeCAD.

The reproducible prompt, module boundary, model attempts, source-preservation
checks, and independent geometric evaluation live in
`evals/cad/so101_camera_mount/`. `camera-spec.json` distinguishes manufacturer
measurements from provisional component/cable envelopes. The manufacturer PDF
is retained under `sources/` with its URL and SHA in that specification.

`live_capture.py` is a presentation/execution host, not a geometry generator.
It reads `out/so101-camera-mount/live-config.json` with absolute `input_path`,
`candidate_path`, and a fresh `output_dir`, imports the submitted module, and
calls `build(input_path, output_dir, checkpoint)`. Run inside FreeCAD's Python
console or a FreeCAD macro. Each checkpoint records the real native viewport,
object counts, solid volumes, and time. The host temporarily changes visibility
for presentation and restores it before returning to the generated script.
The native module and input hashes are retained in `live-result.json`.

The saved viewport frames can be encoded with ffmpeg at 5 fps. They are
checkpoint captures during native execution, not a continuous desktop recording;
model computation between checkpoints is not represented in real time. Failed
builds and capture failures remain recorded separately. Do not describe a repaired
run as a one-shot success.

Prototype limitations: no load/strength qualification, complete robot motion
sweep, cable flex-life analysis, or manufacturer-exact camera component CAD.
Nominal screw access checks do not establish assembly access with the servo
installed. The original robot uses upstream mesh references; the added mount is
an editable native solid with expression-driven dimensions.
