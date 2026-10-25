//! Persistent FreeCAD transport. Native document state stays in one worker.
//! A transport timeout kills that worker; it never silently starts an empty
//! replacement and claims the old document survived.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

pub struct Worker {
    child: Child,
    input: ChildStdin,
    output: Receiver<Result<String, String>>,
    next_id: u64,
    timeout: Duration,
    failed: bool,
}

impl Worker {
    pub fn spawn(launcher: &Path, timeout: Duration) -> Result<Self> {
        let mut child = Command::new(launcher).stdin(Stdio::piped()).stdout(Stdio::piped())
            .stderr(Stdio::inherit()).spawn().with_context(|| format!("launch FreeCAD: {}", launcher.display()))?;
        let input = child.stdin.take().context("worker stdin missing")?;
        let output_pipe = child.stdout.take().context("worker stdout missing")?;
        let (tx, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output_pipe).lines() {
                let value = line.map_err(|e| e.to_string());
                if tx.send(value).is_err() { break; }
            }
        });
        let mut worker = Self { child, input, output, next_id: 1, timeout, failed: false };
        let ready = worker.receive()?;
        if ready["jsonrpc"] != "2.0" || ready["method"] != "ready" || ready["params"]["backend"] != "FreeCAD" {
            bail!("unexpected CAD startup response: {ready}");
        }
        Ok(worker)
    }

    fn receive(&mut self) -> Result<Value> {
        let received = self.output.recv_timeout(self.timeout);
        match received {
            Ok(Ok(line)) => match serde_json::from_str(&line) {
                Ok(value) => Ok(value),
                Err(e) => { self.stop(); Err(e).context("invalid CAD protocol JSON") }
            },
            other => { self.stop(); bail!("CAD transport stopped: {other:?}; native in-memory document is unavailable") }
        }
    }

    fn stop(&mut self) {
        self.failed = true;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        if self.failed { bail!("CAD worker has stopped; no automatic empty-document restart"); }
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({"jsonrpc":"2.0", "id":id,"method":method,"params":params});
        let sent = writeln!(self.input,"{request}").and_then(|_| self.input.flush());
        if let Err(e) = sent { self.stop(); return Err(e).context("write CAD request"); }
        let reply = self.receive()?;
        if reply["jsonrpc"] != "2.0" || reply["id"] != id {
            self.stop(); bail!("mismatched CAD reply: {reply}");
        }
        if let Some(error) = reply.get("error") { bail!("FreeCAD rejected request: {error}"); }
        reply.get("result").cloned().context("CAD reply has neither result nor error")
    }
}
impl Drop for Worker { fn drop(&mut self) { self.stop(); } }

/// One lazily started session shared by the lead and its specialists. Outputs
/// are scoped to its run, rather than accepting agent-selected filesystem paths.
pub struct Session {
    worker: Option<Worker>,
    directory: PathBuf,
    launcher: PathBuf,
}
impl Session {
    pub fn new(directory: PathBuf) -> Self {
        let launcher = std::env::var_os("WRIGHT_CAD_LAUNCHER").map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cad/freecad/run.sh"));
        Self { worker: None, directory, launcher }
    }
    pub fn action(&mut self, action: &str, mut params: Value) -> Result<Value> {
        if !["component_catalog","analyze_wing","inspect","open_document","import_component","import_step","make_part","convert_spline_loft","boolean","mirror","make_parameters","bind_parameter","bind_parameters","set_parameter","set_profile","place","check","check_containment","check_translation","render","save","save_preview"].contains(&action) {
            bail!("unsupported CAD action {action}");
        }
        if !params.is_object() { bail!("CAD parameters must be a JSON object"); }
        if ["save","save_preview","analyze_wing"].contains(&action) {
            if params.get("directory").is_some() { bail!("CAD output directory is managed by the run"); }
            std::fs::create_dir_all(&self.directory)?;
            params["directory"] = json!(std::fs::canonicalize(&self.directory)?);
        }
        if self.worker.is_none() {
            let mut worker = Worker::spawn(&self.launcher,Duration::from_secs(60))?;
            let checkpoint=self.directory.join("working.FCStd");
            if checkpoint.exists() {
                worker.call("restore",json!({"path":std::fs::canonicalize(checkpoint)?}))?;
            } else {
                worker.call("document",json!({"name":"Wright"}))?;
            }
            self.worker = Some(worker);
        }
        let worker=self.worker.as_mut().unwrap();
        // Assembly booleans and exchange validation can outlast primitive edits.
        // Keep a finite bound while preserving the ordinary request timeout.
        let ordinary_timeout=worker.timeout;
        if ["open_document","import_component","import_step","convert_spline_loft","check","check_containment","check_translation","save","save_preview","analyze_wing","bind_parameters"].contains(&action) { worker.timeout=Duration::from_secs(300); }
        let response=worker.call(action,params);
        worker.timeout=ordinary_timeout;
        let mut result=response?;
        if ["open_document","import_component","import_step","make_part","convert_spline_loft","boolean","mirror","make_parameters","bind_parameter","bind_parameters","set_parameter","set_profile","place"].contains(&action) {
            std::fs::create_dir_all(&self.directory)?;
            let saved=worker.call("checkpoint",json!({"directory":std::fs::canonicalize(&self.directory)?}))
                .context("CAD edit succeeded in memory but saving its checkpoint failed")?;
            if let Some(object)=result.as_object_mut() { object.insert("checkpoint".into(),saved["checkpoint"].clone()); }
        }
        Ok(result)
    }
}
