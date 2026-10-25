//! System prompts, specialist rosters and tool definitions.
//!
//! Everything here is a pure function of (role, domain), so the `tools` +
//! `system` prefix is byte-identical across calls and caches. Tools are
//! `strict: true` with `additionalProperties: false` and every property
//! required (the strict-mode contract, claude-api skill "Strict tool use").
//! Free-form edits travel as a JSON-string `patch_json` (RFC 7396 merge
//! patch) because a strict schema cannot hold an open object; the harness
//! parses and validates it, and a patch that does not parse is a malformed
//! call that counts toward the requery limit.

use crate::tools::DomainTools;
use serde_json::{json, Value};

/// Specialist roles per domain, `(role, what it owns)`.
pub fn roles(domain: &str) -> &'static [(&'static str, &'static str)] {
    match domain {
        "aircraft" => &[
            ("cad", "editable native FreeCAD parts, placements, explicit contacts, interference checks and interchange artifacts; reconcile CAD dimensions with the resolved design"),
            ("aero", "wing and tail sizing, airfoil, drag buildup, cruise and stall speeds, stability margins"),
            ("structures", "spars, booms, load paths from every load to the CG, fastener and joint margins"),
            ("electronics", "motors, ESCs, pack, wiring gauge, voltage and current ratings, flight controller and radio"),
            ("sourcing", "every BOM line priced and in stock from a named vendor, total against the budget"),
            ("print", "3D-printed parts: orientation, bed fit, slicer time and grams, wall thickness"),
        ],
        "rocket" => &[
            ("cad", "editable airframe and avionics CAD, component fit, interference checks and native/STEP/GLB artifacts; track unsupported geometry explicitly"),
            ("propulsion", "motor or engine selection, thrust curve, total impulse class, propellant mass"),
            ("engine", "chamber pressure, injector, nozzle expansion, cooling (regen or ablative), wall stress"),
            ("structures", "airframe tube, fins, bulkheads, couplers, flutter and buckling margins"),
            ("recovery", "drogue and main chutes, deployment altitudes, descent rates, ejection charge sizing"),
        ],
        "balloon" => &[
            ("cad", "payload enclosure and board mounts, component/cable keepouts, suspension interfaces and native CAD fit checks"),
            ("envelope", "balloon size, lift gas fill, free lift, ascent rate and burst altitude"),
            ("payload", "payload mass, thermal and power budget, parachute and descent rate, flight-train rules"),
            ("tracking", "APRS / LoRa / ELRS radios, antennas, link budget to the chase car, GPS altitude limits"),
        ],
        "electronics" => &[
            ("circuit", "the circuit: template or full netlist, regulator divider and setpoint, part values and voltage/current ratings, pin functions"),
            ("layout", "placement intent as groups.json (near/apart/edge with reasons, decap distance limits, keep-outs) and the seed judge's pick; the router lays copper"),
            ("verification", "re-measure every layout claim on the routed board and read kicad-cli ERC / DRC / parity; report what does not hold, never edit the design"),
            ("sourcing", "every BOM line priced from a live listing (Creao Build Desk), total against the budget"),
        ],
        _ => &[("generalist", "any part of the design the lead assigns")],
    }
}

pub(crate) fn tool(name: &str, description: &str, props: Value) -> Value {
    let required: Vec<String> = props.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    json!({
        "name": name,
        "description": description,
        "strict": true,
        "input_schema": {
            "type": "object",
            "properties": props,
            "required": required,
            "additionalProperties": false
        }
    })
}

fn s(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

/// The engineering tools every designing agent has, generic over the domain.
pub fn workbench_tools() -> Vec<Value> {
    vec![
        tool(
            "edit_spec",
            "Change the requirements by RFC 7396 merge patch (objects merge, null deletes). The result must fit the spec schema in the system prompt or the edit is rejected with the reason.",
            json!({
                "patch_json": s("merge patch as a JSON object string, e.g. {\"payload_kg\": 1.2}"),
                "why": s("one line: why"),
                "assumptions": {"type": "array", "items": {"type": "string"}, "description": "dotted spec paths in this patch whose values YOU chose because the request did not state them (e.g. [\"ground_temperature_c\"]). On the first spec write every other value is locked as the person's request; later writes are all recorded as assumptions. The reviewer challenges assumptions. [] when every value comes from the request"}
            }),
        ),
        tool(
            "design",
            "Resolve every open choice in the current spec deterministically (parts, dimensions) and replace the current design; then runs every check and returns the report.",
            json!({}),
        ),
        tool(
            "revert_best",
            "Make the best design verified so far (fewest failing checks including regression cases, then the best objective) the current design again, and run every check. Use it when your changes made things worse.",
            json!({}),
        ),
        tool(
            "edit_design",
            "Change the resolved design by RFC 7396 merge patch, e.g. swap a part id or a dimension. Must stay a valid design. Run check afterwards.",
            json!({"patch_json": s("merge patch as a JSON object string"), "why": s("one line: why")}),
        ),
        tool(
            "catalog",
            "Read-only lookup of the catalog rows a design choice can take (id, label, key specs, price, stock, url). Call it before putting any part id into edit_design: never guess an id. With path \"\" it lists which design paths are catalog-backed. Changes nothing.",
            json!({
                "path": s("dotted design path of the choice, e.g. \"structure.boom_tube\"; \"\" to list the catalog-backed paths"),
                "query": s("text to match in the id or label, e.g. \"carbon 16mm\"; \"\" for no filter")
            }),
        ),
        // lane 8 sourcing tools (src/sourcing.rs): ranked search, alternates, BOM diff
        tool(
            "catalog_search",
            "Ranked, read-only search of the catalog rows at one design path, filtered by text, numeric spec ranges and stock, each row with why it matched. Rows you can buy today rank first, then the closest text match, then price. Use it to find a part by attribute (a lighter motor, a stiffer tube) instead of scanning the catalog listing.",
            json!({
                "path": s("dotted design path of the choice, e.g. \"propulsion.lift_motor\"; \"\" to list the catalog-backed paths"),
                "query": s("text to match in id, label or any spec; \"\" for none"),
                "ranges": s("numeric spec ranges, comma separated, `attr:min..max` with either bound optional, e.g. \"kv:800..1200, mass_g:..60\"; \"\" for none; the spec names are the keys shown by catalog"),
                "stock": {"type": "string", "enum": ["in_stock", "listed", "lead_time", "restock"], "description": "which stock states to return: in_stock only; listed (in stock or priced with no stock line; the default and the spec's usual stock policy); lead_time adds made-to-order/pre-order/backorder; restock adds out of stock. Discontinued rows never return"},
                "sort": s("lead with one spec, e.g. \"mass_g\" ascending, \"-kv\" descending, \"price\"; \"\" for the default ranking")
            }),
        ),
        tool(
            "alternates",
            "Read-only: every other catalog row that can take the same role as the current (or a named) part at a design path, with a delta table of every spec that differs (from -> to, percent) and the price change, best first (stock, then fewest differences, then smallest price change). Use it before swapping a part to see what you would give up.",
            json!({
                "path": s("dotted design path of the choice, e.g. \"propulsion.lift_motor\""),
                "id": s("the incumbent id to compare against; \"\" for the current design's choice at that path")
            }),
        ),
        tool(
            "bom_diff",
            "Read-only: the bill-of-materials diff between the best verified design (the incumbent) and the current design: lines added, removed and changed with each line's cost delta, the total before and after, unpriced lines, and any part the current design brings in that is not in stock. Call it after edit_design to see what a swap costs before you check.",
            json!({}),
        ),
        tool(
            "check",
            "Run every declared check (plus every confirmed regression case) on the current spec and design. Returns each margin with its unit and source; missing evidence fails.",
            json!({}),
        ),
        tool("bom", "The bill of materials: every line with price, vendor URL and the total. Unpriced lines are errors.", json!({})),
        tool(
            "cad_action",
            "Use the shared persistent native FreeCAD document. Units mm/degrees. For coupled dimensions prefer bind_parameters:{bindings:[{object_name,parameter,source_object,source_parameter,scale:1,offset_mm:0},...]}: 1..128 unique typed targets are applied atomically with one recompute/validation and full rollback on failure. No arbitrary expression strings. Start complex CAD discovery with inspect:{summary:true,offset:0,limit:100}; this returns a lightweight paginated object/dependency list and does not verify geometry. Then inspect:{objects:[names]} for current geometry and dimensions of only the features you need, including construction features and shared dimensions. Follow dependency names to inspect deeper as needed. Avoid full inspect:{} unless the complete geometry tree is necessary. Results include editable_parameters and assembly containers. For motors, propellers, PCBs/flight controllers and ESCs, prefer suitable off-the-shelf parts and query component_catalog first. component_catalog:{query} lists exact local manufacturer CAD identities and availability; query may be an explicitly mapped BOM catalog ID. Before inserting a model as selected real hardware, its exact identity and source provenance must match the resolved BOM. If exact CAD is unavailable, report the gap; any envelope or proposed alternative must be separately labeled and must not silently replace the selected component. import_component:{component_id,name,position:[x,y,z],axis:[x,y,z],angle_degrees:0} verifies the catalog hash and imports that exact model with provenance. It does not select a replacement in the aircraft BOM; unavailable exact models remain unavailable. Preserve the supplier coordinate frame and use explicit placements: mounting_frame_verified:false means no mounting datum or automatic alignment is established, so do not infer a mount from bounding-box centers. Do not resize or relabel a different motor, propeller or flight controller to impersonate the selected part. import_step: {path,name,position:[x,y,z],axis:[x,y,z],angle_degrees:0} imports actual local STEP solids as separately placeable native BREP snapshots, preserves source hash and matching labels, and uses file-derived units normalized by OCC: SI mm/cm/m and audited inch/foot conversion graphs, including mixed per-representation contexts. No caller unit override; unknown, cyclic or inconsistent unit graphs are rejected. Import defaults remain 50 MiB and 512 entities; explicit integer max_source_mib:1..200 and max_entities:1..4096 may raise those requested budgets, while the 100000-face cap remains. Both import actions support preserve_labels:false to skip the optional second supplier STEP read for matching labels; default true preserves existing behavior. Source entity indices, source hash and native geometry remain retained; labels_status:not_requested explicitly reports absent recovered labels. Prefer summary:true for large imports: full validation still runs, but detailed object records are omitted; physical/reference names, counts, unit/source provenance and requested budgets remain, with a 512 KiB summary limit. Use paginated/scoped inspect afterward. Non-solid geometry is rejected by default. import_component/import_step may explicitly set preserve_reference_geometry:true to retain supported shells/faces as reference-only objects alongside solids; object_names lists physical solids and reference_object_names lists preserved surfaces. Reference surfaces have no mass/collision-solid claim; check geometry_coverage for omitted same-import surfaces. Native checkpoints preserve all geometry, but checked STEP save rejects any selection from a mixed import instance. render may explicitly include reference_objects:[names]; omission from rendering or physical checks must not be described as complete source coverage. Source history and PCB electrical connectivity are not imported; inspect returned object_names, place/check/save them. open_document: {path} opens an existing FCStd into an empty session; later checkpoints go to this run, preserving the source file. make_parameters: {name,parameters:{Gap:{value:12,min:1,max:30}}} creates shared dimensions in mm. Native Part::Mirroring also permits binding Base.x, Base.y, or Base.z plane-origin coordinates (not other object types); for example bind_parameter:{object_name:LiftRRMotor,parameter:Base.x,source_object:RearRotorLayout,source_parameter:RearMotorX,scale:0.5,offset_mm:52.5}. bind_parameter: {object_name,parameter,source_object,source_parameter,scale:1,offset_mm:0} binds a native editable dimension or Placement.Base.x/y/z to a shared dimension using a linear expression; then edit the shared source with set_parameter. boolean: {name,operation:cut|fuse|common,base,tool} creates a new native editable boolean from two distinct existing physical single solids; use a new result name. Empty/invalid results, no-effect cuts/fuses and disconnected fuses fail transactionally. Export final result only, not its construction operands. For a cut using an actual mating component, explicitly pass retain_tool_as_part:true; that mating tool may then be exported with the result only when their material does not overlap. mirror: {name,source,normal:[0,1,0],origin:[0,0,0]} creates a linked physical instance reflected across a plane; edit source dimensions to update both sides. Source and mirror may both be checked/exported; connectivity and collisions still require check. convert_spline_loft: {source,name,split_index,sampled_tolerance_mm} creates a separately named native spline loft from an existing solid ruled polygon-section Part::Loft without retyping coordinates. Source is preserved. Supports line-only Wright sections and copies supported shared-dimension section translation expressions; source coordinate edits are NOT associative. Records source geometry hash and snapshot provenance. Use explicit split and sampled tolerance, then inspect/check new outputs; not exact polygon equivalence or a certified continuous deviation bound. make_part: {spec:{name,kind,params}}; kinds box{x,y,z}, cylinder{radius,height}, tube{radius,bore_radius,height}, mount{x,y,z,bores:[[startXYZ,endXYZ,radius],...]}, loft{rings:[closed XYZ polygons]}, spline_loft{rings,split_index,sampled_tolerance_mm,ruled?:boolean} uses optional strict boolean ruled defaults true; false requests smooth native spanwise interpolation and requires renewed wall/interference checks (section sampled tolerance is not a 3D wall guarantee). Two interpolated native B-splines per closed section: 2-16 corresponding sections, 8-256 points per section, total <=2048, split leaves >=4 points on each branch, 0<tolerance<=1 mm. Tolerance measures sampled curve-to-source-polyline deviation, not a certified continuous bound. loft_shell{rings,inner}, extruded_skin{profile:[XYZ vertices],direction:XYZ,wall}, profile_plate{profile_object,direction:XYZ,thickness,bores} (links to an existing skin cavity profile). set_profile: {object_name,points:[XYZ vertices],summary:true} edits a constrained native sketch section, preserving spline type and provenance; inspect names first. set_profile and set_parameter summary:true keep full native validation/rollback but return compact edit metadata and dependent-name candidates; these are not contact checks. Scoped-inspect final physical outputs and run check after edits. Omit summary for legacy full document descriptions. set_parameter: {object_name,parameter,value,summary:true}; native properties Length/Width/Height or Radius/Height, extrusion LengthFwd, inward profile offset Value (negative), and supported Rocket tube Length/Thickness/AxialOffset (signed); only use names listed in editable_parameters. place: {object_name,position:[x,y,z],axis:[x,y,z],angle_degrees}; direct place is unavailable for assembly-controlled Rocket components. analyze_wing: {object_name,velocity_m_s:20,alphas_deg:[0,4,8],altitude_m:0,samples_per_branch:65,symmetric:true} runs the fixed open-source AeroSandbox/OptVL cross-check and separate NeuralFoil section polar analysis on actual current native two-branch spline-loft sections. Select a supported solid spline loft, often the outer construction loft, not a skin Boolean. Limits: velocity 1..100 m/s; 1..5 unique finite alphas in -10..12 degrees; altitude 0..20000 m; integer samples 33..129; strict boolean symmetric (right half-wing root must be at Y=0). Output directory is managed. Read-only: returns bounded numeric summary plus JSON/PNG/CSV artifacts, source geometry/runner hashes, mesh convergence and cross-solver comparison. Solvers approximate smooth spanwise CAD between sampled sections; symmetry is an assumption, and section polar confidence is not whole-wing stall proof. Do not claim full-aircraft stability, rotor-wash effects, structural integrity or flight readiness from this wing-only result. check_containment: {container:name,contents:[names],minimum_clearance_mm:0} measures actual solid containment and distance to all container boundary shells, allowing valid one-solid construction objects. Boundary distance is not clearance when contained:false; inspect per-object outside_volume/contained/clearance_passed and coverage. For wall clearance use the outer solid as container and cavity solid as content; for component fit use the cavity solid as container. It is read-only and does not prove load rating or manufacturing readiness. check_translation: {moving:[names],obstacles:[names],translation:[dx,dy,dz],samples:11} screens a straight removal/insertion path: clear only if conservative swept bounds prove no penetration, blocked if an actual sampled collision exists, otherwise inconclusive. It does not solve assembly joints or cable/tool access. check: {objects:[names],contacts:[[a,b],...],tolerance_mm:0.1,require_connected:true,summary:true}; summary computes all pairs but returns only clash and declared-contact pair details plus total counts/components; omitted pairs are not untested, and full evidence remains available in saved geometry-check.json. Omit summary or use false when every pair detail is needed. Prefer summary for large assemblies. Use require_connected for a single assembly; geometric connectivity is not a load-bearing joint verification. Declared contacts never exempt penetration. render: {objects:[final physical names],width:640,height:480,azimuth_degrees:35,elevation_degrees:25,deflection_mm:0.5,max_triangles:100000,angular_deflection_radians:0.35} uses optional angular_deflection_radians in 0.05..1.0. Render defaults remain max_objects:128 and max_source_faces:20000; explicit budgets may raise these to 4096 and 100000, but existing 100000-triangle and pixel/work caps still apply, so use scoped selections when full rendering fails. It returns a bounded native B-rep raster image with geometry fingerprint metadata; inspect the image together with native measurements, not as proof of fit or hidden geometry. save_preview: {objects:[physical names],reference_objects:[reference names],max_objects:512} writes an editable native document and GLB preview with explicit physical/reference counts and visible geometry_role metadata; it does not produce STEP or claim roundtrip_valid. The preview object budget defaults to 512 and may explicitly be set to an integer up to 4096. Include all intended reference objects and inspect preview_coverage for omissions. Use it to display mixed manufacturer geometry in the project without weakening checked export. save: {objects:[final physical output names]}; directory managed by the run. Native helper objects are not extra assembly parts. CAD edits do not automatically change the resolved domain design, flight checks or BOM; reconcile them explicitly. A passing CAD fit check is not manufacturing or flight approval.",
            json!({"action":{"type":"string","enum":["component_catalog","analyze_wing","inspect","open_document","import_component","import_step","make_part","convert_spline_loft","mirror","boolean","make_parameters","bind_parameter","bind_parameters","set_parameter","set_profile","place","check","check_containment","check_translation","render","save","save_preview"]},"args_json":s("Action parameters as a JSON object string; no Python code")}),
        ),
        crate::sweep::tool_def(),
    ]
}

pub fn orchestrator_tools(domain: &dyn DomainTools) -> Vec<Value> {
    orchestrator_tools_with(domain, &[])
}

/// [`orchestrator_tools`], plus `run_stage` when post-design stages are
/// registered for the domain.
pub fn orchestrator_tools_with(domain: &dyn DomainTools, stages: &[&str]) -> Vec<Value> {
    let mut t = orchestrator_base_tools(domain);
    if !stages.is_empty() {
        t.push(tool(
            "run_stage",
            "Run one post-design analysis on the current design (it is built first): CFD, FEA, a flight sim, fabrication, CAD. Returns the stage's own checks and numbers; a failing stage check is evidence to act on (e.g. an FEA hot spot: delegate a structures fix, then check). Slow: run it when the declared checks pass, not every turn.",
            json!({"stage": {"type": "string", "enum": stages}}),
        ));
    }
    t
}

/// Does this domain have a `cad` specialist? Then only that role carries
/// the ~12K-character `cad_action` manual (cost + latency, 10/24 night).
pub fn has_cad_role(domain: &str) -> bool {
    roles(domain).iter().any(|r| r.0 == "cad")
}

fn orchestrator_base_tools(domain: &dyn DomainTools) -> Vec<Value> {
    let names: Vec<&str> = roles(domain.name()).iter().map(|r| r.0).collect();
    let mut t = workbench_tools();
    if has_cad_role(domain.name()) {
        // CAD work is delegated to the cad specialist
        t.retain(|x| x["name"] != "cad_action");
    }
    t.push(tool(
        "delegate",
        "Hand one focused task to a specialist. It works on the shared DesignState with the same engineering tools and returns a short summary of what it changed and the numbers.",
        json!({"role": {"type": "string", "enum": names}, "task": s("the task, with the failing check names and margins it should fix"), "plan_task": s("id of the plan task this delegation does (required once a plan is set; \"\" before one)")}),
    ));
    t.extend(crate::plan::tools(domain));
    t.push(tool(
        "submit",
        "End this round and send the design to the hostile reviewer. Call it when every check passes, or when you cannot make more progress this round.",
        json!({"summary": s("what the design is and what changed this round, with key numbers")}),
    ));
    t
}

pub fn specialist_tools() -> Vec<Value> {
    // the design is theirs; the spec (what the builder faces) is not
    let mut t: Vec<Value> = workbench_tools().into_iter().filter(|x| x["name"] != "edit_spec").collect();
    t.push(tool(
        "report",
        "Finish: report to the lead in at most 300 tokens -- what you changed, the margins before and after, what is still failing.",
        json!({"summary": s("at most 300 tokens")}),
    ));
    t
}

/// A specialist's tools for one role: `cad_action` only for the `cad`
/// role (or every role when the domain has no cad specialist), plus
/// `full_report`, since the task message carries only the failing and
/// nearly failing checks (`crate::slim`).
pub fn specialist_tools_for(domain: &str, role: &str) -> Vec<Value> {
    let mut t = specialist_tools();
    if role != "cad" && has_cad_role(domain) {
        t.retain(|x| x["name"] != "cad_action");
    }
    t.push(tool(
        "full_report",
        "The whole DesignState with every check and its margin (your task message shows only the failing and nearly failing checks).",
        json!({}),
    ));
    t
}

pub fn reviewer_tools() -> Vec<Value> {
    // + the read-only `catalog` lookup (lane 4): stock/sourcing attacks cite catalog rows as evidence
    let catalog = workbench_tools().into_iter().filter(|t| t["name"] == "catalog");
    let mut t = vec![
        tool(
            "cite_failing_check",
            "Block on a check the report shows failing. Rejected if that check passes.",
            json!({"check": s("check name exactly as in the report"), "argument": s("why this failure matters to the builder")}),
        ),
        tool(
            "attack",
            "A concrete, testable attack: a plausible change to the spec the builder will face (hot day, gust, heavier payload, a part out of stock, a price rise) under which you predict a named check fails on the current design. The host runs it; only a confirmed attack counts, and it becomes a permanent regression case.",
            // + category and evidence (review lane, crate::review)
            crate::review::attack_properties(),
        ),
        tool(
            "verdict",
            "Finish the review. Only confirmed citations and attacks block; opinions do not. List the taxonomy categories you walked and found clear.",
            crate::review::verdict_properties(),
        ),
    ];
    t.extend(catalog);
    t
}

fn domain_block(domain: &dyn DomainTools) -> String {
    let checks: String = domain.checks().iter().map(|(n, d)| format!("- {n}: {d}\n")).collect();
    format!(
        "Domain: {}.\n\nSpec JSON schema (what a person asks for; choices may be left open):\n{}\n\nChecks (every one must pass; a check that could not be measured fails, NaN fails, and confirmed regression cases appear as name@id):\n{}",
        domain.name(),
        serde_json::to_string_pretty(&domain.spec_schema()).unwrap_or_default(),
        checks
    )
}

const RULES: &str = "Rules that are not negotiable:\n\
- You act only through tools. Every number you state must come from a tool result in this conversation; never from memory.\n\
- Never say a check passes unless the latest check report shows it passing.\n\
- Change the smallest thing that fixes the failure, then run check to verify it.\n\
- A design is something a person will build and fly or launch: prefer parts that exist, are priced, and are in stock.\n\
- If your changes made the design worse (more failing checks, or a lower objective with the same failures), call revert_best rather than undoing them by hand.\n\
- Never guess a part id. Call catalog (path \"\" first if you do not know where the choice lives in the design) and use an id it returned.";

pub fn orchestrator_system(domain: &dyn DomainTools) -> String {
    orchestrator_system_with(domain, &[])
}

/// [`orchestrator_system`], plus how to use `run_stage` when stages exist.
pub fn orchestrator_system_with(domain: &dyn DomainTools, stages: &[&str]) -> String {
    let mut s = orchestrator_base_system(domain);
    if !stages.is_empty() {
        s.push_str(&format!(
            "\nPost-design analyses you can run with run_stage: {}. They are slower and higher-fidelity than the declared checks. Once the checks pass, run the ones that bear on what changed; if one reports a failing check (a stress hot spot, an unstable sim), treat it like a confirmed issue: delegate the fix, check, and run the stage again. Their results land in notes for everyone.",
            stages.join(", ")
        ));
    }
    s
}

fn orchestrator_base_system(domain: &dyn DomainTools) -> String {
    let team: String = roles(domain.name()).iter().map(|(r, d)| format!("- {r}: {d}\n")).collect();
    format!(
        "You lead a small engineering team designing a {name} that a real person will build. \
Your team shares one DesignState (domain, spec, current design, last check report, open feedback). \
The user message each round shows it.\n\n{RULES}\n\n{block}\nYour specialists (use delegate, one focused task per call; independent roles such as structures and electronics may be delegated in the same turn with several delegate calls -- they then run at the same time on the current design and the harness merges their edits, reporting any edit to the same field):\n{team}\n\
How to work: make sure the spec is valid (edit_spec), run design, read the check report, delegate fixes for failing checks to the right specialist, and check again. \
When every check passes, or you cannot make more progress this round, call submit. A hostile reviewer then attacks the design; \
its confirmed issues and any feedback a person draws on a render come back to you next round. Treat a person's sketch as a requirement to satisfy unless a check proves it unsafe, and say so.\n\
After the reviewer confirms a failure case (a new regression case under open_feedback), FIRST call design again: the search then designs against every regression case's conditions. Only then hand-tune with edit_design and delegate. If hand-tuning makes the design worse, call revert_best; the harness also keeps the best verified design and submits it instead of a worse one.\n\
If the run starts with no spec, your first edit_spec writes it from the person's request and LOCKS every value in it, except the paths you list in `assumptions`: list there every value the request did not state (a temperature, a wind, a target you picked). Locked values cannot be changed later by anyone; assumptions can be revised and the reviewer will challenge them. The design's own copy of any spec field (e.g. `requirements`) cannot be edited with edit_design.\n\n{plan}",
        name = domain.name(),
        block = domain_block(domain),
        plan = crate::plan::GUIDE,
    )
}

pub fn specialist_system(domain: &dyn DomainTools, role: &str) -> String {
    let owns = roles(domain.name()).iter().find(|r| r.0 == role).map(|r| r.1).unwrap_or("the task you are given");
    format!(
        "You are the {role} specialist on a team designing a {name}. You own: {owns}.\n\n{RULES}\n\n{block}\n\
Do the task the lead gives you with the engineering tools, verify with check, then call report (at most 300 tokens: what you changed, margins before and after, what still fails). \
Stay in your lane; if the fix belongs to another specialist, say so in the report.",
        name = domain.name(),
        block = domain_block(domain),
    )
}

pub fn reviewer_system(domain: &dyn DomainTools) -> String {
    format!(
        "You are a hostile design reviewer for a {name} that a real person is about to build with their own money and safety. \
You see only the DesignState and its check report. Find what would break.\n\n{block}\n\
You may block the design in exactly two ways: cite_failing_check (a check the report shows failing), or attack (a concrete spec change the builder will plausibly face, with the check you predict fails; the host runs it). \
Attacks the host cannot confirm are discarded. An attack perturbs conditions the builder does not control (weather, altitude, gusts, part tolerances, supplier stock and prices, a heavier as-built mass); never rewrite what the person asked for (their budget, their payload, their target), because a design cannot be blamed for missing a requirement nobody gave it; the host rejects any patch that changes a field of the person's own spec. Do not block on taste or on things no check measures; mention those in the verdict instead. \
Conditions fixed by a competition's or regulator's rules (the provided launch rail, the field elevation, a legal limit) are not attacks: the builder cannot face anything else there. \
locked_from_request lists the values locked as the person's request. You may challenge a locked value only if it contradicts the request text; in that case report it as a FINDING in your verdict (quote the request and the locked value), not as an attack: the host rejects attacks on locked values. \
Attack values must be physically plausible: numbers within the schema's range (or, without one, within a few times today's value), and text or true/false values only where the schema enumerates them. \
Read assumptions_added_by_agents first: any spec value the agents chose themselves (a longer launch rail, a milder temperature) that makes a check easier to pass is the first thing to attack, by patching it back to what a builder would actually face. Make at most three attacks, the strongest first, then call verdict.",
        name = domain.name(),
        block = domain_block(domain),
    ) + &envelope_block(domain)
        + &crate::review::taxonomy_block()
}

/// The declared envelope, from the spec schema's bounds: the reviewer
/// attacks inside it (the host rejects patches outside it, and patched
/// specs the domain's validation refuses).
fn envelope_block(domain: &dyn DomainTools) -> String {
    let lines = crate::harness::envelope_lines(&domain.spec_schema());
    if lines.is_empty() {
        return "\n\nDeclared envelope: the schema states no field bounds; the host bounds each value by its unit (a few times today's value, within physical limits) and rejects patched specs the domain's validation refuses.".into();
    }
    format!(
        "\n\nDeclared envelope (attack inside it: the host rejects any value outside these bounds, and any patched spec the domain's validation refuses, e.g. a combination of conditions that cannot occur together):\n- {}",
        lines.join("\n- ")
    )
}
