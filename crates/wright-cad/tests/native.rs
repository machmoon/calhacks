use serde_json::json;
use std::path::Path;
use std::time::{Duration,Instant};
use wright_cad::Worker;

#[test]
#[ignore = "actual 74 MB manufacturer component; bounded native import, no model/API use"]
fn actual_jetson_import_with_explicit_budgets() {
    let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let inventory:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("out/vendor-component-inventory/jetson-orin-nano.json")).unwrap()).unwrap();
    let source=Path::new(inventory["source"].as_str().unwrap());let original=std::fs::read(source).unwrap();
    let output=root.join(format!("out/native-jetson-import/run-{}",std::process::id()));std::fs::create_dir_all(&output).unwrap();
    let args=json!({"path":source,"name":"JetsonStudy","preserve_reference_geometry":true,"max_source_mib":80,"max_entities":2048,"summary":true});
    std::fs::write(output.join("request.json"),serde_json::to_vec_pretty(&args).unwrap()).unwrap();
    let mut session=wright_cad::Session::new(output.join("cad"));let start=Instant::now();
    let result=session.action("import_step",args);let elapsed=start.elapsed().as_secs_f64();
    let unchanged=std::fs::read(source).unwrap()==original;
    let record=match &result {
        Ok(value)=>json!({"status":"native_import_completed","elapsed_seconds":elapsed,"source_file_unchanged":unchanged,"result":value,"independent_preservation_oracle_pending":true,"scope":"Native component study only; no BOM substitution, source feature history, fit or flight claim"}),
        Err(error)=>json!({"status":"native_import_failed","elapsed_seconds":elapsed,"source_file_unchanged":unchanged,"error":format!("{error:#}"),"scope":"No import success claimed; existing 300-second action bound retained"})
    };
    std::fs::write(output.join("attempt.json"),serde_json::to_vec_pretty(&record).unwrap()).unwrap();eprintln!("Actual Jetson evidence: {}",output.display());
    assert!(unchanged);let imported=result.expect("bounded actual import failed; see retained attempt.json");
    assert_eq!(imported["solid_count"],1393);assert_eq!(imported["reference_surface_count"],527);assert_eq!(imported["all_object_count"],1920);
    assert_eq!(imported["provenance"]["source_sha256"],inventory["sha256"]);assert_eq!(imported["provenance"]["source_length_unit"],"mixed");
    assert_eq!(imported["provenance"]["resource_budget"]["max_entities"],2048);assert!(imported.get("objects").is_none());assert!(imported["checkpoint"].is_string());
    let inspection=session.action("inspect",json!({"summary":true,"limit":10})).unwrap();assert_eq!(inspection["total"],1920);
}

#[test]
fn typed_wing_analysis_is_readonly_and_returns_scoped_artifacts() {
    let source=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/freecad-aero-sections/fixture.FCStd");
    if !source.exists(){eprintln!("SKIP run test_aero_sections.py first");return;}
    let dir=std::env::temp_dir().join(format!("wright-wing-analysis-{}",std::process::id()));
    let mut session=wright_cad::Session::new(dir.clone());
    session.action("open_document",json!({"path":source})).unwrap();
    let checkpoint=std::fs::read(dir.join("working.FCStd")).unwrap();
    assert!(session.action("analyze_wing",json!({"object_name":"Wing","directory":"/tmp/not-managed"})).is_err());
    assert!(session.action("analyze_wing",json!({"object_name":"Wing","velocity_m_s":101})).is_err());
    let result=session.action("analyze_wing",json!({"object_name":"Wing","alphas_deg":[4],"samples_per_branch":33})).unwrap();
    assert_eq!(result["status"],"analysis_complete","{result}");
    assert_eq!(result["native_document_changed"],false);assert!(result.get("checkpoint").is_none());
    assert_eq!(result["section_count"],3);assert!(result["solvers"]["optvl"].is_string());
    assert_eq!(result["rows"].as_array().unwrap().len(),1);
    assert_eq!(result["artifacts"].as_array().unwrap().len(),3);
    for item in result["artifacts"].as_array().unwrap(){let path=Path::new(item["path"].as_str().unwrap());assert!(path.starts_with(dir.canonicalize().unwrap()) && path.is_file());}
    assert_eq!(std::fs::read(dir.join("working.FCStd")).unwrap(),checkpoint);
    drop(session);std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mixed_reference_import_is_explicit_and_survives_session_reopen() {
    let source=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/native-reference-contract/mixed.step");
    if !source.exists(){eprintln!("SKIP run cad/freecad/test_reference_contract.py first");return;}
    let dir=std::env::temp_dir().join(format!("wright-mixed-reference-{}",std::process::id()));
    let (solid,reference);
    {
        let mut session=wright_cad::Session::new(dir.clone());
        assert!(session.action("import_step",json!({"path":source,"name":"Strict"})).is_err());
        let imported=session.action("import_step",json!({"path":source,"name":"Mixed","preserve_reference_geometry":true})).unwrap();
        assert_eq!(imported["solid_count"],1);assert_eq!(imported["reference_surface_count"],1);
        solid=imported["object_names"][0].as_str().unwrap().to_owned();reference=imported["reference_object_names"][0].as_str().unwrap().to_owned();
        assert!(imported["checkpoint"].is_string());
    }
    {
        let mut session=wright_cad::Session::new(dir.clone());
        let inspected=session.action("inspect",json!({"objects":[reference]})).unwrap();
        assert_eq!(inspected[0]["reference_geometry"],true);assert!(inspected[0]["volume_mm3"].is_null());
        let checked=session.action("check",json!({"objects":[solid]})).unwrap();
        assert_eq!(checked["geometry_coverage"]["complete_source_geometry_checked"],false);
        assert_eq!(checked["geometry_coverage"]["omitted_reference_objects"],json!([reference]));
        assert!(session.action("save",json!({"objects":[solid]})).is_err());
        let render=session.action("render",json!({"objects":[solid],"reference_objects":[reference],"width":128,"height":128})).unwrap();
        assert_eq!(render["content_blocks"][0]["type"],"image");
        assert_eq!(render["reference_objects"],json!([reference]));
        assert!(session.action("save_preview",json!({"objects":[solid],"reference_objects":[reference],"directory":"/tmp/not-managed"})).is_err());
        let preview=session.action("save_preview",json!({"objects":[solid],"reference_objects":[reference]})).unwrap();
        assert_eq!(preview["preview"]["physical_nodes"],1);assert_eq!(preview["preview"]["reference_nodes"],1);
        assert!(preview.get("step").is_none() && preview.get("roundtrip_valid").is_none());
        assert_eq!(preview["step_export"]["status"],"unavailable_in_preview_contract");
        assert!(Path::new(preview["glb"].as_str().unwrap()).exists());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn typed_containment_reports_boundary_clearance_without_checkpoint() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() { return; }
    let dir=std::env::temp_dir().join(format!("wright-containment-{}",std::process::id()));
    let mut session=wright_cad::Session::new(dir.clone());
    for (name,size,position) in [("Container",10.,0.),("Inside",8.,1.)] {
        session.action("make_part",json!({"spec":{"name":name,"kind":"box","params":{"x":size,"y":size,"z":size}}})).unwrap();
        session.action("place",json!({"object_name":name,"position":[position,position,position]})).unwrap();
    }
    let pass=session.action("check_containment",json!({"container":"Container","contents":["Inside"],"minimum_clearance_mm":1.})).unwrap();
    assert_eq!(pass["passed"],true);assert!(pass.get("checkpoint").is_none());
    assert_eq!(pass["results"][0]["contained"],true);
    assert!((pass["results"][0]["boundary_distance_mm"].as_f64().unwrap()-1.).abs()<1e-7);
    assert_eq!(pass["results"][0]["outside_volume_mm3"],0.);
    assert_eq!(pass["geometry_sha256"]["Inside"].as_str().unwrap().len(),64);
    assert_eq!(pass["geometry_coverage"]["complete_source_geometry_checked"],true);
    let fail=session.action("check_containment",json!({"container":"Container","contents":["Inside"],"minimum_clearance_mm":1.1})).unwrap();
    assert_eq!(fail["passed"],false);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn component_catalog_imports_exact_manufacturer_identity() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() { return; }
    let dir=std::env::temp_dir().join(format!("wright-cad-cots-{}",std::process::id()));
    {
        let mut session=wright_cad::Session::new(dir.clone());
        assert_eq!(session.action("component_catalog",json!({"query":"tmotor"})).unwrap()["components"].as_array().unwrap().len(),2);
        assert!(session.action("component_catalog",json!({"query":"AOS2807"})).unwrap()["components"].as_array().unwrap().is_empty());
        assert!(session.action("import_component",json!({"component_id":"aos2807","name":"Wrong"})).is_err());
        let result=session.action("import_component",json!({"component_id":"tmotor_mn2806_evo","name":"VendorMotor"})).unwrap();
        assert_eq!(result["solid_count"],1);
        assert_eq!(result["provenance"]["component_identity"]["catalog_id"],"tmotor_mn2806_evo");
        assert_eq!(result["provenance"]["component_identity"]["bom_catalog_ids"],json!(["tmotor_mn2806_evo_650kv"]));
        assert_eq!(result["provenance"]["component_identity"]["mounting_frame_verified"],false);
    }
    {
        let mut session=wright_cad::Session::new(dir.clone());
        let result=session.action("inspect",json!({"objects":["VendorMotor"]})).unwrap();
        assert_eq!(result[0]["step_import"]["component_identity"]["catalog_id"],"tmotor_mn2806_evo");
        assert_eq!(result[0]["solids"],1);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn actual_pcb_step_import_survives_checkpoint_and_checked_export() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() { return; }
    let source=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/step-import-fixture/pcb-assembly-board.step");
    if !source.exists(){eprintln!("SKIP run the explicit PCB fixture export first");return;}
    let original=std::fs::read(&source).unwrap();
    let dir=std::env::temp_dir().join(format!("wright-cad-step-import-{}",std::process::id()));
    let names;
    {
        let mut session=wright_cad::Session::new(dir.clone());
        let result=session.action("import_step",json!({"path":std::fs::canonicalize(&source).unwrap(),"name":"Pcb","position":[0,20,0]})).unwrap();
        assert_eq!(result["solid_count"],6);
        assert_eq!(result["provenance"]["source_length_unit"],"mm");
        assert!(result["checkpoint"].is_string());names=result["object_names"].clone();
        assert!(session.action("import_step",json!({"path":source,"name":"Pcb"})).is_err());
    }
    {
        let mut session=wright_cad::Session::new(dir.clone());
        let inspected=session.action("inspect",json!({"objects":names})).unwrap();
        assert_eq!(inspected.as_array().unwrap().len(),6);
        assert!(inspected[0]["step_import"]["source_sha256"].as_str().unwrap().len()==64);
        let saved=session.action("save",json!({"objects":names})).unwrap();
        assert_eq!(saved["roundtrip_valid"],true);
        assert_eq!(saved["preview"]["physical_nodes"],6);
    }
    assert_eq!(std::fs::read(&source).unwrap(),original);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn actual_reference_wing_converts_and_resumes_through_typed_session() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() { return; }
    let source=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/tsung-vtol-reference/assembly.FCStd");
    if !source.exists(){eprintln!("SKIP actual wing fixture is unavailable");return;}
    let original=std::fs::read(&source).unwrap();
    let dir=std::env::temp_dir().join(format!("wright-cad-wing-conversion-{}",std::process::id()));
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("open_document",json!({"path":std::fs::canonicalize(&source).unwrap()})).unwrap();
        let converted=session.action("convert_spline_loft",json!({"source":"WingRight","name":"SmoothWingRight","split_index":31,"sampled_tolerance_mm":0.2})).unwrap();
        assert_eq!(converted["conversion"]["source_point_count"],660);
        assert_eq!(converted["conversion"]["source_point_edits_associative"],false);
        assert_eq!(converted["conversion"]["preserved_layout_expressions"].as_array().unwrap().len(),11);
        assert!(converted["checkpoint"].is_string());
        assert!(session.action("convert_spline_loft",json!({"source":"WingRight","name":"SmoothWingRight","split_index":31,"sampled_tolerance_mm":0.2})).is_err());
    }
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("set_parameter",json!({"object_name":"ReferenceDimensions","parameter":"HalfSpan","value":610,"summary":true})).unwrap();
        let inspected=session.action("inspect",json!({"objects":["WingRight","SmoothWingRight"]})).unwrap();
        for part in inspected.as_array().unwrap(){assert!((part["bbox_mm"][1][1].as_f64().unwrap()-610.).abs()<1e-5);}
        let before=inspected.clone();
        assert!(session.action("convert_spline_loft",json!({"source":"SmoothWingRight","name":"Unsupported","split_index":31,"sampled_tolerance_mm":0.2})).is_err());
        assert_eq!(session.action("inspect",json!({"objects":["WingRight","SmoothWingRight"]})).unwrap(),before);
    }
    assert_eq!(std::fs::read(&source).unwrap(),original,"source fixture must not be overwritten");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn spline_loft_edits_and_resumes_through_typed_session() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() { return; }
    let ring=|z:f64| (0..12).map(|i| {
        let angle=i as f64*std::f64::consts::TAU/12.0;
        json!([10.0*angle.cos(),5.0*angle.sin(),z])
    }).collect::<Vec<_>>();
    let dir=std::env::temp_dir().join(format!("wright-cad-spline-{}",std::process::id()));
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("make_part",json!({"spec":{"name":"Smooth","kind":"spline_loft","params":{
            "rings":[ring(0.0),ring(20.0)],"split_index":6,"sampled_tolerance_mm":1.0}}})).unwrap();
    }
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("set_profile",json!({"object_name":"Smooth_Section1","points":ring(25.0)})).unwrap();
        let inspected=session.action("inspect",json!({"objects":["Smooth"]})).unwrap();
        assert!((inspected[0]["bbox_mm"][1][2].as_f64().unwrap()-25.0).abs()<1e-6);
        assert!(session.action("set_profile",json!({"object_name":"Smooth_Section1","points":[[0,0,0],[1,0,0],[2,0,0]]})).is_err());
        let after=session.action("inspect",json!({"objects":["Smooth"]})).unwrap();
        assert_eq!(after,inspected,"invalid spline edits must roll back");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn typed_mirror_instances_resume_and_follow_source_edits() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() { return; }
    let dir=std::env::temp_dir().join(format!("wright-cad-mirror-{}",std::process::id()));
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("make_part",json!({"spec":{"name":"Right","kind":"box","params":{"x":10,"y":4,"z":3}}})).unwrap();
        session.action("place",json!({"object_name":"Right","position":[0,10,0]})).unwrap();
        session.action("mirror",json!({"name":"Left","source":"Right","normal":[0,1,0]})).unwrap();
    }
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("set_parameter",json!({"object_name":"Right","parameter":"Length","value":15})).unwrap();
        let tree=session.action("inspect",json!({"objects":["Right","Left"]})).unwrap();
        for part in tree.as_array().unwrap() {assert!((part["volume_mm3"].as_f64().unwrap()-180.0).abs()<1e-6);}
        let check=session.action("check",json!({"objects":["Right","Left"]})).unwrap();
        assert_eq!(check["overlap_count"],0);
        assert_eq!(check["connected"],false);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_document_survives_errors_and_reports_interference() {
    let launcher=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cad/freecad/run.sh");
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() {
        eprintln!("SKIP native FreeCAD integration: app is not installed");return;
    }
    let mut worker=Worker::spawn(&launcher,Duration::from_secs(60)).unwrap();
    let pid=worker.call("ping",json!({})).unwrap()["pid"].clone();
    worker.call("document",json!({})).unwrap();
    for name in ["A","B"] {
        worker.call("make_part",json!({"spec":{"name":name,"kind":"box","params":{"x":10,"y":10,"z":10}}})).unwrap();
    }
    let collision=worker.call("check",json!({"objects":["A","B"],"contacts":[["A","B"]]})).unwrap();
    assert_eq!(collision["passed"],false,"contact declarations must not suppress collisions");
    let compact=worker.call("check",json!({"objects":["A","B"],"contacts":[["A","B"]],"summary":true})).unwrap();
    assert_eq!(compact["pairs"],collision["pairs"]);
    assert_eq!(compact["pair_details_scope"],"all_clashes_and_declared_contacts");
    assert_eq!(compact["omitted_pair_count"],0);
    worker.call("place",json!({"object_name":"B","position":[10,0,0]})).unwrap();
    assert_eq!(worker.call("check",json!({"objects":["A","B"],"contacts":[["A","B"]]})).unwrap()["passed"],true);
    worker.call("place",json!({"object_name":"B","position":[11,0,0]})).unwrap();
    assert_eq!(worker.call("check",json!({"objects":["A","B"],"contacts":[["A","B"]]})).unwrap()["pairs"][0]["floating"],true);
    assert!(worker.call("place",json!({"object_name":"B","position":[0,0,0],"axis":[0,0,0]})).is_err());
    assert_eq!(worker.call("check",json!({"objects":["A","B"],"contacts":[["A","B"]]})).unwrap()["pairs"][0]["floating"],true);
    assert!(worker.call("check",json!({"objects":["A","A"]})).is_err());
    let disconnected=worker.call("check",json!({"objects":["A","B"],"require_connected":true})).unwrap();
    assert_eq!(disconnected["passed"],false);
    assert_eq!(disconnected["components"],json!([["A"],["B"]]));
    assert_eq!(worker.call("ping",json!({})).unwrap()["pid"],pid);
}

#[cfg(unix)]
#[test]
fn timeout_stops_worker_without_restarting_document() {
    use std::os::unix::fs::PermissionsExt;
    let path=std::env::temp_dir().join(format!("wright-cad-timeout-{}.sh",std::process::id()));
    // A shell builtin emits readiness without Python import/startup contention.
    // After the first request, exec keeps the sleeping process under the worker
    // PID so timeout cleanup cannot leave an orphan child.
    std::fs::write(&path,"#!/bin/sh\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"method\":\"ready\",\"params\":{\"backend\":\"FreeCAD\"}}'\nread -r request\nexec /bin/sleep 30\n").unwrap();
    std::fs::set_permissions(&path,std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut worker=Worker::spawn(&path,Duration::from_secs(1)).unwrap();
    let start=Instant::now();
    assert!(worker.call("ping",json!({})).is_err());
    assert!(start.elapsed()<Duration::from_secs(5));
    let message=worker.call("ping",json!({})).unwrap_err().to_string();
    assert!(message.contains("no automatic"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_new_session_resumes_its_saved_native_feature_graph() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() { return; }
    let dir=std::env::temp_dir().join(format!("wright-cad-resume-{}",std::process::id()));
    let _=std::fs::remove_dir_all(&dir);
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("make_part",json!({"spec":{"name":"Saved","kind":"tube","params":{"radius":5,"bore_radius":4,"height":30}}})).unwrap();
    }
    assert!(dir.join("working.FCStd").exists());
    {
        let mut resumed=wright_cad::Session::new(dir.clone());
        resumed.action("set_parameter",json!({"object_name":"Saved_Outer","parameter":"Height","value":40})).unwrap();
        let tree=resumed.action("inspect",json!({})).unwrap();
        let tube=tree.as_array().unwrap().iter().find(|v|v["name"]=="Saved").unwrap();
        assert_eq!(tube["type"],"Part::Cut");
        assert_eq!(tube["dependencies"],json!(["Saved_Outer","Saved_Bore"]));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn constrained_profiles_edit_and_resume_through_session() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() { return; }
    let dir=std::env::temp_dir().join(format!("wright-cad-profile-{}",std::process::id()));
    let _=std::fs::remove_dir_all(&dir);
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("make_part",json!({"spec":{"name":"Wing","kind":"loft","params":{"rings":[[[0,0,0],[10,0,0],[10,10,0],[0,10,0]],[[0,0,20],[10,0,20],[10,10,20],[0,10,20]]]}}})).unwrap();
        session.action("set_profile",json!({"object_name":"Wing_Section1","points":[[0,0,30],[10,0,30],[10,10,30],[0,10,30]]})).unwrap();
        assert!(session.action("check",json!({"objects":["Wing_Section1"]})).is_err());
    }
    {
        let mut session=wright_cad::Session::new(dir.clone());
        let tree=session.action("inspect",json!({})).unwrap();
        let wing=tree.as_array().unwrap().iter().find(|v|v["name"]=="Wing").unwrap();
        assert!((wing["volume_mm3"].as_f64().unwrap()-3000.).abs()<1e-5);
        assert_eq!(wing["type"],"Part::Loft");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn imported_rocket_edits_checkpoint_and_resume_as_live_features() {
    let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source=root.join("out/freecad-openrocket-verified/A simple model rocket/imported.FCStd");
    if !source.exists() { eprintln!("SKIP: generate the native OpenRocket benchmark artifacts first");return; }
    let original=std::fs::read(&source).unwrap();
    let dir=std::env::temp_dir().join(format!("wright-cad-imported-rocket-{}",std::process::id()));
    let _=std::fs::remove_dir_all(&dir);
    {
        let mut session=wright_cad::Session::new(dir.clone());
        let opened=session.action("open_document",json!({"path":source})).unwrap();
        assert_eq!(opened["opened"],true);
        assert!(opened["checkpoint"].is_string());
        session.action("set_parameter",json!({"object_name":"InnerTube","parameter":"Length","value":80})).unwrap();
        assert!(session.action("set_parameter",json!({"object_name":"InnerTube","parameter":"Thickness","value":1000})).is_err());
        assert!(session.action("open_document",json!({"path":source})).is_err());
    }
    {
        let mut resumed=wright_cad::Session::new(dir.clone());
        let tree=resumed.action("inspect",json!({})).unwrap();
        let tube=tree.as_array().unwrap().iter().find(|o|o["name"]=="InnerTube").unwrap();
        assert_eq!(tube["editable_parameters"]["Length"],80.0);
        assert_eq!(tube["proxy_type"],"Rocket.FeatureInnerTube.FeatureInnerTube");
        resumed.action("set_parameter",json!({"object_name":"InnerTube","parameter":"Length","value":85})).unwrap();
        let exported=resumed.action("save",json!({"objects":["BodyTube","InnerTube"]})).unwrap();
        assert_eq!(exported["roundtrip_valid"],true);
    }
    assert_eq!(std::fs::read(source).unwrap(),original);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn shared_layout_bindings_survive_session_resume() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() { return; }
    let dir=std::env::temp_dir().join(format!("wright-cad-layout-{}",std::process::id()));
    let _=std::fs::remove_dir_all(&dir);
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("make_parameters",json!({"name":"Dims","parameters":{"Gap":{"value":12,"min":4,"max":20}}})).unwrap();
        session.action("make_part",json!({"spec":{"name":"Spacer","kind":"box","params":{"x":10,"y":10,"z":2}}})).unwrap();
        session.action("bind_parameter",json!({"object_name":"Spacer","parameter":"Height","source_object":"Dims","source_parameter":"Gap"})).unwrap();
    }
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("set_parameter",json!({"object_name":"Dims","parameter":"Gap","value":18})).unwrap();
        assert!(session.action("set_parameter",json!({"object_name":"Dims","parameter":"Gap","value":21})).is_err());
        let objects=session.action("inspect",json!({})).unwrap();
        let spacer=objects.as_array().unwrap().iter().find(|o|o["name"]=="Spacer").unwrap();
        assert_eq!(spacer["bbox_mm"][1][2],18.0);
        assert!(spacer["editable_parameters"].get("Height").is_none());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn typed_boolean_action_checkpoints_native_dependencies() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists() { return; }
    let dir=std::env::temp_dir().join(format!("wright-cad-boolean-{}",std::process::id()));
    let _=std::fs::remove_dir_all(&dir);
    {
        let mut session=wright_cad::Session::new(dir.clone());
        for name in ["Base","Tool"] {
            session.action("make_part",json!({"spec":{"name":name,"kind":"box","params":{"x":10,"y":10,"z":10}}})).unwrap();
        }
        session.action("place",json!({"object_name":"Tool","position":[5,0,0]})).unwrap();
        let cut=session.action("boolean",json!({"name":"Result","operation":"cut","base":"Base","tool":"Tool"})).unwrap();
        assert!((cut["volume_mm3"].as_f64().unwrap()-500.0).abs()<1e-6);
    }
    {
        let mut session=wright_cad::Session::new(dir.clone());
        session.action("set_parameter",json!({"object_name":"Base","parameter":"Width","value":12})).unwrap();
        assert!(session.action("place",json!({"object_name":"Tool","position":[50,0,0]})).is_err());
        let objects=session.action("inspect",json!({})).unwrap();
        let result=objects.as_array().unwrap().iter().find(|o|o["name"]=="Result").unwrap();
        assert_eq!(result["type"],"Part::Cut");
        assert!((result["volume_mm3"].as_f64().unwrap()-700.0).abs()<1e-6);
        assert!(session.action("save",json!({"objects":["Result","Base"]})).is_err());
        let saved=session.action("save",json!({"objects":["Result"]})).unwrap();
        assert_eq!(saved["roundtrip_valid"],true);
    }
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn atomic_parameter_bindings_checkpoint_and_rollback() {
    let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source=root.join("out/freecad-bind-many/fixture.FCStd");
    assert!(source.exists(),"run native test_bind_many.py fixture first");
    let dir=std::env::temp_dir().join(format!("wright-batch-bindings-{}",std::process::id()));
    let mut session=wright_cad::Session::new(dir.clone());
    session.action("open_document",json!({"path":source})).unwrap();
    let binding=|name:&str,offset:f64|json!({"object_name":name,"parameter":"Radius","source_object":"Dimensions","source_parameter":"Bore","offset_mm":offset});
    let result=session.action("bind_parameters",json!({"bindings":[binding("Tube_Outer",2.),binding("Tube_Bore",0.)]})).unwrap();
    assert_eq!(result["binding_count"],2);assert_eq!(result["atomic"],true);assert!(result["checkpoint"].is_string());
    let checkpoint=std::fs::read(dir.join("working.FCStd")).unwrap();
    assert!(session.action("bind_parameters",json!({"bindings":[binding("Tube_Outer",1.),binding("Tube_Bore",4.)]})).is_err());
    assert_eq!(std::fs::read(dir.join("working.FCStd")).unwrap(),checkpoint);
    session.action("set_parameter",json!({"object_name":"Dimensions","parameter":"Bore","value":4,"summary":true})).unwrap();
    let tree=session.action("inspect",json!({"objects":["Tube"]})).unwrap();
    let volume=tree[0]["volume_mm3"].as_f64().unwrap();
    assert!((volume-200.*std::f64::consts::PI).abs()<1e-6,"{tree}");
}
