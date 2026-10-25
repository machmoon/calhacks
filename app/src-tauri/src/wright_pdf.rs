//! Bounded, local PDF staging for native CAD requests.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::{Command, Stdio}, time::{Duration, Instant}};

const MAX_BYTES: usize = 20 * 1024 * 1024;

#[derive(Serialize)]
pub struct PdfAttachment {
    id: String,
    name: String,
    sha256: String,
    pages: usize,
    preview: String,
    limitations: Vec<String>,
}

pub fn prepared_path(id: &str) -> Result<PathBuf, String> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err("Invalid PDF attachment ID".into());
    }
    Ok(crate::wright::wright_root()?.join("uploads/cad-pdf").join(id).join("prepared/manifest.json"))
}

fn prepare(path: Option<String>, name: Option<String>, bytes: Option<Vec<u8>>) -> Result<PdfAttachment, String> {
    let (data, display_name) = match (path, bytes) {
        (Some(path), None) => {
            let source = PathBuf::from(path);
            let meta = fs::metadata(&source).map_err(|e| format!("Cannot read PDF: {e}"))?;
            if !meta.is_file() || meta.len() > MAX_BYTES as u64 { return Err("Choose one PDF up to 20 MiB".into()); }
            let data = fs::read(&source).map_err(|e| e.to_string())?;
            (data, source.file_name().unwrap_or_default().to_string_lossy().into_owned())
        }
        (None, Some(data)) => (data, name.unwrap_or_else(|| "specification.pdf".into())),
        _ => return Err("Provide either a dropped PDF path or file bytes".into()),
    };
    if data.len() > MAX_BYTES || !data.starts_with(b"%PDF-") { return Err("Choose a valid PDF up to 20 MiB".into()); }
    let id = format!("{:x}", Sha256::digest(&data));
    let manifest = prepared_path(&id)?;
    let directory = manifest.parent().unwrap().parent().unwrap();
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let source = directory.join("source.pdf");
    if source.exists() {
        let existing = fs::read(&source).map_err(|e| e.to_string())?;
        if existing != data { return Err("Stored PDF hash conflict".into()); }
    } else { fs::write(&source, &data).map_err(|e| e.to_string())?; }
    if !manifest.is_file() {
        let root = crate::wright::wright_root()?;
        let log_path = directory.join("prepare.log");
        let log = fs::File::create(&log_path).map_err(|e| e.to_string())?;
        let mut child = Command::new("bash")
            .arg("-c").arg(r#"source "$1" && shift && exec python3 "$@""#)
            .arg("wright-pdf").arg(root.join("scripts/env.sh"))
            .arg(root.join("cad/project_runner/pdf_input.py"))
            .arg("prepare").arg(&source).arg(manifest.parent().unwrap())
            .arg("--name").arg(&display_name)
            .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log)).stdin(Stdio::null())
            .spawn().map_err(|e| format!("Cannot start PDF extraction: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    let detail = fs::read_to_string(&log_path).unwrap_or_default();
                    return Err(format!("PDF preparation failed: {}", detail.chars().rev().take(1800).collect::<String>().chars().rev().collect::<String>()));
                }
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill(); let _ = child.wait();
                return Err("PDF preparation timed out; no CAD run started".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if value["sha256"].as_str() != Some(&id) { return Err("Prepared PDF hash does not match the attachment".into()); }
    let pages = value["pages"].as_u64().ok_or("PDF page count missing")? as usize;
    if !(1..=4).contains(&pages) { return Err("Use a PDF with 1–4 pages".into()); }
    Ok(PdfAttachment { id: id.clone(), name: display_name, sha256: id, pages,
        preview: value["preview"].as_str().unwrap_or_default().to_string(),
        limitations: value["limitations"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default() })
}

#[tauri::command]
pub async fn wright_attach_pdf(path: Option<String>, name: Option<String>, bytes: Option<Vec<u8>>) -> Result<PdfAttachment, String> {
    tauri::async_runtime::spawn_blocking(move || prepare(path, name, bytes)).await.map_err(|e| e.to_string())?
}
