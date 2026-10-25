use std::{env,fs,path::PathBuf};
use serde_json::Value;
use wright_agents::{codex::{CodexTransport,MODEL},client::{Request,Transport}};
#[tokio::main(flavor="current_thread")]
async fn main()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<_>=env::args().collect();
 if args.len()!=3{return Err("usage: wright-so-arm101-one-shot FROZEN_REQUEST_JSON OUTPUT_DIR".into());}
 let body:Value=serde_json::from_slice(&fs::read(&args[1])?)?;
 if body["model"]!=MODEL{return Err("exact Astra model required".into());}
 let out=PathBuf::from(&args[2]);fs::create_dir_all(&out)?;
 if out.join("request.json").exists(){return Err("attempt directory already used; refusing replacement".into());}
 fs::write(out.join("request.json"),serde_json::to_vec_pretty(&body)?)?;
 let request=Request{body,betas:vec![]};
 let response=match CodexTransport::from_env().send(&request).await{
  Ok(r)=>r,Err(e)=>{fs::write(out.join("transport-error.txt"),e.to_string())?;return Err(e.into());}
 };
 fs::write(out.join("response.json"),serde_json::to_vec_pretty(&response)?)?;
 let calls:Vec<_>=response["content"].as_array().ok_or("response has no content")?.iter().filter(|v|v["type"]=="tool_use").collect();
 if calls.len()!=1||calls[0]["name"]!="submit_candidate"{return Err("expected exactly one candidate submission".into());}
 let code=calls[0]["input"]["python"].as_str().ok_or("candidate has no Python")?;
 if code.len()>256*1024||code.is_empty(){return Err("candidate code byte bound exceeded".into());}
 fs::write(out.join("candidate.py"),code)?;
 fs::write(out.join("submission.json"),serde_json::to_vec_pretty(&calls[0]["input"])?)?;
 println!("Candidate recorded at {}; not executed or scored",out.display());Ok(())
}
