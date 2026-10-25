//! Real FreeCAD, scripted agents: no model/network calls or API spend.
use serde_json::{json,Value};
use std::path::Path;
use std::sync::Arc;
use wright_agents::{channel,harness,tools,Budget,Client,RunConfig};
use wright_agents::fake::{reply,tool_use,ScriptedTransport};
use wright_core::{Artifacts,Bom,CheckId,Domain,Finished,Report,Source};

struct Fixture;
#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)] enum C { Valid }
impl CheckId for C {
    const ALL:&'static [Self]=&[C::Valid];
    fn name(self)->&'static str {"fixture"}
    fn describe(self)->&'static str {"Protocol test domain only"}
}
impl Domain for Fixture {
    const NAME:&'static str="cad_fixture";
    type Spec=Value;type Design=Value;type Check=C;
    fn spec_schema()->Value {json!({"type":"object"})}
    fn design(_: &Value)->anyhow::Result<Value>{Ok(json!({"test_fixture":true}))}
    fn check(_: &Value,_: &Value)->Finished{let mut r=Report::<C>::new();r.margin(C::Valid,1.,"",Source::Geometry,"Test fixture only");r.finish()}
    fn bom(_: &Value)->Bom{Bom::default()}
    fn build(_: &Value,_: &Path)->anyhow::Result<Artifacts>{Ok(Artifacts::default())}
    fn objective(_: &Value)->(f64,&'static str){(0.,"test")}
}
fn cad(id:&str,action:&str,args:Value)->Value {
    tool_use(id,"cad_action",json!({"action":action,"args_json":args.to_string()}))
}
fn response(call:Value)->Value {reply("claude-sonnet-5-5",vec![call],100,10)}

#[tokio::test]
async fn specialists_share_native_document_and_failures_reach_agent() {
    if !Path::new(&format!("{}/Applications/FreeCAD.app",std::env::var("HOME").unwrap())).exists(){
        eprintln!("SKIP native CAD harness integration: FreeCAD not installed");return;
    }
    let out=std::env::temp_dir().join(format!("wright-cad-agent-{}",std::process::id()));
    let calls=vec![
        response(cad("makeA","make_part",json!({"spec":{"name":"A","kind":"box","params":{"x":10,"y":10,"z":10}}}))),
        response(tool_use("delegate","delegate",json!({"role":"generalist","task":"Extend native A to 20 mm"}))),
        response(cad("specialistEdit","set_parameter",json!({"object_name":"A","parameter":"Length","value":20}))),
        response(tool_use("report","report",json!({"summary":"A extended"}))),
        response(cad("makeB","make_part",json!({"spec":{"name":"B","kind":"box","params":{"x":10,"y":10,"z":10}}}))),
        response(cad("collision","check",json!({"objects":["A","B"],"contacts":[["A","B"]]}))),
        response(cad("place","place",json!({"object_name":"B","position":[20,0,0]}))),
        response(cad("touching","check",json!({"objects":["A","B"],"contacts":[["A","B"]]}))),
        response(cad("save","save",json!({"objects":["A","B"]}))),
        response(tool_use("design","design",json!({}))),
        response(tool_use("submit","submit",json!({"summary":"Protocol fixture complete"}))),
        response(tool_use("verdict","verdict",json!({"summary":"Protocol fixture checked"}))),
    ];
    let transport=Arc::new(ScriptedTransport::new(calls));
    let (tx,mut events)=channel();
    let client=Client::new(transport.clone(),Budget::new(10.,45.,None),tx,"cad-test");
    let mut config=RunConfig::new("cad-test","Verify CAD integration",json!({}));
    config.out_dir=Some(out.clone());config.max_rounds=1;
    let summary=harness::run(client,tools::<Fixture>(),config).await;
    let requests=transport.requests.lock().unwrap();
    let results:Vec<Value>=requests.iter().flat_map(|r|r.body["messages"].as_array().unwrap().iter())
        .flat_map(|m|m["content"].as_array().into_iter().flatten())
        .filter(|v|v["type"]=="tool_result").cloned().collect();
    let find=|id:&str|results.iter().find(|v|v["tool_use_id"]==id).unwrap_or_else(||panic!("missing {id}: {results:?}"));
    assert_eq!(find("collision")["is_error"],true);
    assert_ne!(find("touching")["is_error"],true);
    assert!(find("touching")["content"].as_str().unwrap().contains("\"passed\": true"));
    assert_ne!(find("specialistEdit")["is_error"],true);
    assert!(out.join("cad/assembly.FCStd").exists(),"{summary:?}");
    assert!(out.join("cad/assembly.step").exists());
    assert!(out.join("cad/assembly.glb").exists());
    let mut files=vec![];
    while let Ok(event)=events.try_recv() {
        if let wright_core::Event::Artifact {path,..}=event { files.push(path); }
    }
    assert!(files.iter().any(|p|p.ends_with("assembly.FCStd")));
    assert!(files.iter().any(|p|p.ends_with("assembly.step")));
    assert!(files.iter().any(|p|p.ends_with("assembly.glb")));
    std::fs::remove_dir_all(out).unwrap();
}
