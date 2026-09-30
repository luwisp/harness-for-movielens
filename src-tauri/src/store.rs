use crate::{model::{now, Conversation, Database, Job, Message, Rules, Settings, UploadedFile}, rules};
use std::{fs, io::{Read, Write}, path::PathBuf, sync::{atomic::{AtomicBool, Ordering}, Mutex}};
use tauri::{AppHandle, Emitter, Manager};

pub struct AppState {
    pub root: PathBuf,
    pub db: Mutex<Database>,
    pub runs: Mutex<std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    pub uploads: Mutex<std::collections::HashMap<String, UploadSession>>,
    pub source_busy: AtomicBool,
}

pub struct UploadSession {
    name: String, expected: u64, written: u64, path: PathBuf,
}

fn migrate_legacy_rules(state: &mut serde_json::Value) -> Result<(), String> {
    let legacy = &state["settings"]["rules"];
    if !legacy.is_object() || legacy.get("entries").is_some() { return Ok(()); }
    let mut updated = rules::default_rules();
    if let Some(value) = legacy["deduplicate"].as_bool() {
        for id in ["unique_users", "unique_movies", "unique_ratings"] {
            rules::update(&mut updated, id, Some(value), None)?;
        }
    }
    if let Some(value) = legacy["quarantine_orphans"].as_bool() {
        for id in ["rating_user_exists", "rating_movie_exists"] {
            rules::update(&mut updated, id, Some(value), None)?;
        }
    }
    if let Some(value) = legacy["trim_fields"].as_bool() {
        rules::update(&mut updated, "trim_fields", Some(value), None)?;
    }
    // Relative freshness settings have no equivalent in the fixed v2 historical window.
    state["settings"]["rules"] = serde_json::to_value(updated).map_err(|e| e.to_string())?;
    Ok(())
}

impl AppState {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let mut db: Database = match fs::read(root.join("state.json")) {
            Ok(bytes) => {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("读取状态失败: {e}"))?;
                migrate_legacy_rules(&mut value)?;
                serde_json::from_value(value).map_err(|e| format!("读取状态失败: {e}"))?
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Database::default(),
            Err(e) => return Err(e.to_string()),
        };
        rules::normalize(&mut db.settings.rules);
        for name in ["users.dat", "movies.dat", "ratings.dat"] {
            if !db.active_files.contains_key(name) {
                if let Some(file) = db.files.iter().rev().find(|f| f.name == name && f.hdfs_path.is_some()) {
                    db.active_files.insert(name.into(), file.id.clone());
                }
            }
        }
        for conversation in &mut db.conversations { if conversation.rules.is_none() { conversation.rules = Some(db.settings.rules.clone()); } else if let Some(rules) = &mut conversation.rules { rules::normalize(rules); } if conversation.rules_revision == 0 { conversation.rules_revision = 1; } }
        if db.source_revision == 0 && ["users.dat", "movies.dat", "ratings.dat"].iter().all(|name| db.active_files.contains_key(*name)) { db.source_revision = 1; }
        for job in &mut db.jobs {
            if job.status == "queued" || job.status == "running" {
                job.status = "failed".into(); job.stage = "应用重启".into();
                job.error = Some("应用退出时任务未完成；请重新发起任务".into()); job.updated_at = now();
            }
        }
        Ok(Self { root, db: Mutex::new(db), runs: Mutex::new(Default::default()), uploads: Mutex::new(Default::default()), source_busy: AtomicBool::new(false) })
    }

    pub fn change<T>(&self, f: impl FnOnce(&mut Database) -> Result<T, String>) -> Result<T, String> {
        let mut db = self.db.lock().map_err(|e| e.to_string())?;
        let result = f(&mut db)?;
        let bytes = serde_json::to_vec_pretty(&*db).map_err(|e| e.to_string())?;
        let temp = self.root.join("state.json.tmp");
        fs::write(&temp, bytes).map_err(|e| e.to_string())?;
        let destination = self.root.join("state.json");
        if cfg!(windows) && destination.exists() { fs::remove_file(&destination).map_err(|e| e.to_string())?; }
        fs::rename(temp, destination).map_err(|e| e.to_string())?;
        Ok(result)
    }

    pub fn snapshot(&self) -> Result<Database, String> {
        let db = self.db.lock().map_err(|e| e.to_string())?;
        serde_json::from_value(serde_json::to_value(&*db).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    pub fn update_job(&self, app: &AppHandle, id: &str, status: &str, stage: &str,
                      error: Option<String>, result: Option<serde_json::Value>) -> Result<(), String> {
        let job = self.change(|db| {
            let job = db.jobs.iter_mut().find(|j| j.id == id).ok_or("任务不存在")?;
            job.status = status.into(); job.stage = stage.into(); job.updated_at = now();
            if error.is_some() { job.error = error; }
            if result.is_some() { job.result = result; }
            Ok(job.clone())
        })?;
        app.emit("job-update", job).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn create_conversation(state: tauri::State<'_, std::sync::Arc<AppState>>) -> Result<Conversation, String> {
    state.change(|db| {
        let conversation = Conversation { id: uuid::Uuid::new_v4().to_string(),
            title: "新对话".into(), messages: vec![], file_ids: vec![], job_ids: vec![], updated_at: now(), rules: Some(db.settings.rules.clone()), rules_revision: 1 };
        db.conversations.push(conversation.clone()); Ok(conversation)
    })
}

#[tauri::command]
pub fn rename_conversation(state: tauri::State<'_, std::sync::Arc<AppState>>, id: String, title: String) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 80 { return Err("标题长度须为 1–80 字".into()); }
    state.change(|db| {
        let c = db.conversations.iter_mut().find(|c| c.id == id).ok_or("对话不存在")?;
        c.title = title.into(); c.updated_at = now(); Ok(())
    })
}

struct DeletionGuard<'a> { state: &'a AppState, id: String }
impl Drop for DeletionGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut runs) = self.state.runs.lock() { runs.remove(&self.id); }
    }
}

fn delete_conversation_inner(state: &AppState, id: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(id).map_err(|_| "对话 ID 无效")?;
    let mut runs = state.runs.lock().map_err(|e| e.to_string())?;
    if runs.contains_key(id) { return Err("对话运行期间不能删除，请先中断并等待结束".into()); }
    let db = state.snapshot()?;
    if !db.conversations.iter().any(|c| c.id == id) { return Err("对话不存在".into()); }
    let jobs: Vec<Job> = db.jobs.iter().filter(|job| job.conversation_id == id).cloned().collect();
    // This marker also prevents a new Agent run from starting while cleanup is in progress.
    runs.insert(id.into(), std::sync::Arc::new(AtomicBool::new(true)));
    drop(runs);
    let _guard = DeletionGuard { state, id: id.into() };
    crate::hadoop::delete_job_results(state, &db.settings, &jobs)?;
    for job in &jobs {
        uuid::Uuid::parse_str(&job.id).map_err(|_| format!("任务 ID 无效: {}", job.id))?;
        let dir = state.root.join("jobs").join(&job.id);
        if dir.exists() { fs::remove_dir_all(&dir).map_err(|e| format!("删除任务 {} 的本地数据失败: {e}", job.id))?; }
    }
    state.change(|db| {
        db.conversations.retain(|c| c.id != id);
        db.jobs.retain(|job| job.conversation_id != id);
        Ok(())
    })
}

#[tauri::command]
pub async fn delete_conversation(state: tauri::State<'_, std::sync::Arc<AppState>>, id: String) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || delete_conversation_inner(&state, &id))
        .await.map_err(|e| e.to_string())?
}

struct SourceBusyGuard<'a>(&'a AtomicBool);
impl Drop for SourceBusyGuard<'_> {
    fn drop(&mut self) { self.0.store(false, Ordering::SeqCst); }
}

#[tauri::command]
pub fn save_settings(state: tauri::State<'_, std::sync::Arc<AppState>>, settings: Settings) -> Result<(), String> {
    let runs = state.runs.lock().map_err(|e| e.to_string())?;
    if !runs.is_empty() || state.source_busy.load(Ordering::SeqCst) {
        return Err("Agent 运行或文件上传期间不能替换全局设置".into());
    }
    if !["local", "ssh"].contains(&settings.hadoop_mode.as_str()) { return Err("Hadoop 模式无效".into()); }
    if !settings.hdfs_root.starts_with('/') || settings.hdfs_root.contains("..") {
        return Err("HDFS 根路径须为绝对路径且不含 ..".into());
    }
    rules::validate(&settings.rules)?;
    if settings.hadoop_mode == "ssh" && !valid_ssh_target(&settings.ssh_target) {
        return Err("SSH 目标须为 user@host 或 host".into());
    }
    state.change(|db| { db.settings = settings; Ok(()) })
}

pub fn valid_ssh_target(target: &str) -> bool {
    !target.is_empty() && target.len() <= 253 && target.chars().all(|c| c.is_ascii_alphanumeric()
        || matches!(c, '@' | '.' | '-' | '_' )) && !target.starts_with('-')
}

pub fn add_message(state: &AppState, conversation_id: &str, role: &str, content: String,
    run_id: Option<String>, kind: &str, job_id: Option<String>, tool_name: Option<String>, rule_id: Option<String>) -> Result<(), String> {
    state.change(|db| {
        let c = db.conversations.iter_mut().find(|c| c.id == conversation_id).ok_or("对话不存在")?;
        c.messages.push(Message { role: role.into(), content, created_at: now(),
            run_id, kind: kind.into(), job_id, tool_name, rule_id });
        c.updated_at = now(); Ok(())
    })
}

#[tauri::command]
pub fn begin_upload(state: tauri::State<'_, std::sync::Arc<AppState>>,
                    name: String, size: u64) -> Result<String, String> {
    if !matches!(name.as_str(), "users.dat" | "movies.dat" | "ratings.dat") {
        return Err("仅支持 MovieLens 的 users.dat、movies.dat、ratings.dat".into());
    }
    if size == 0 || size > 100_000_000 { return Err("文件大小须为 1 字节到 100 MB".into()); }
    let runs = state.runs.lock().map_err(|e| e.to_string())?;
    if !runs.is_empty() { return Err("Agent 运行期间不能替换共享文件".into()); }
    if state.source_busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("已有文件正在上传".into());
    }
    let result = (|| {
        let id = uuid::Uuid::new_v4().to_string();
        let dir = state.root.join("uploads").join(&id);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(&name);
        fs::File::create(&path).map_err(|e| e.to_string())?;
        state.uploads.lock().map_err(|e| e.to_string())?.insert(id.clone(),
            UploadSession { name, expected: size, written: 0, path });
        Ok(id)
    })();
    if result.is_err() { state.source_busy.store(false, Ordering::SeqCst); }
    result
}

#[tauri::command]
pub fn append_upload_chunk(state: tauri::State<'_, std::sync::Arc<AppState>>, id: String,
                           bytes: Vec<u8>) -> Result<u64, String> {
    if bytes.is_empty() || bytes.len() > 1_048_576 { return Err("分块大小须为 1 字节到 1 MiB".into()); }
    let mut sessions = state.uploads.lock().map_err(|e| e.to_string())?;
    let session = sessions.get_mut(&id).ok_or("上传会话不存在")?;
    if session.written + bytes.len() as u64 > session.expected { return Err("收到的数据超过文件大小".into()); }
    fs::OpenOptions::new().append(true).open(&session.path).map_err(|e| e.to_string())?
        .write_all(&bytes).map_err(|e| e.to_string())?;
    session.written += bytes.len() as u64;
    Ok(session.written)
}

#[tauri::command]
pub fn finish_upload(state: tauri::State<'_, std::sync::Arc<AppState>>, id: String) -> Result<UploadedFile, String> {
    use sha2::{Digest, Sha256};
    let session = state.uploads.lock().map_err(|e| e.to_string())?.remove(&id).ok_or("上传会话不存在")?;
    let _busy_guard = SourceBusyGuard(&state.source_busy);
    if session.written != session.expected {
        let _ = fs::remove_file(&session.path);
        let _ = fs::remove_dir(session.path.parent().ok_or("上传路径无效")?);
        return Err("文件未完整上传".into());
    }
    let mut hasher = Sha256::new(); let mut file_handle = fs::File::open(&session.path).map_err(|e| e.to_string())?;
    let mut buffer = [0_u8; 65536];
    loop { let size = file_handle.read(&mut buffer).map_err(|e| e.to_string())?;
        if size == 0 { break; } hasher.update(&buffer[..size]); }
    let file = UploadedFile { id, conversation_id: String::new(), name: session.name.clone(),
        sha256: format!("{:x}", hasher.finalize()), bytes: session.expected,
        local_path: session.path.to_string_lossy().into(), hdfs_path: None, uploaded_at: now() };
    state.change(|db| { db.files.push(file.clone()); Ok(()) })?;
    match crate::hadoop::initialize_file(&state, &file) {
        Ok(saved) => Ok(saved),
        Err(error) => {
            let _ = state.change(|db| { db.files.retain(|f| f.id != file.id); Ok(()) });
            let _ = fs::remove_file(&file.local_path);
            Err(error)
        }
    }
}

#[tauri::command]
pub fn abort_upload(state: tauri::State<'_, std::sync::Arc<AppState>>, id: String) -> Result<(), String> {
    if let Some(session) = state.uploads.lock().map_err(|e| e.to_string())?.remove(&id) {
        let _ = fs::remove_file(&session.path);
        let _ = fs::remove_dir(session.path.parent().ok_or("上传路径无效")?);
        state.source_busy.store(false, Ordering::SeqCst);
    }
    Ok(())
}

pub fn create_job(state: &AppState, conversation_id: &str, kind: &str, parent_id: Option<String>, run_id: Option<String>) -> Result<Job, String> {
    state.change(|db| {
        let display_number = db.jobs.iter().filter(|j| j.conversation_id == conversation_id && j.kind == kind).count() as u64 + 1;
        let c = db.conversations.iter_mut().find(|c| c.id == conversation_id).ok_or("对话不存在")?;
        let job = Job { id: uuid::Uuid::new_v4().to_string(), conversation_id: conversation_id.into(),
            status: "queued".into(), stage: "等待 Hadoop".into(), error: None, result: None,
            created_at: now(), updated_at: now(), kind: kind.into(), parent_id, run_id, display_number };
        c.job_ids.push(job.id.clone()); db.jobs.push(job.clone()); Ok(job)
    })
}

#[tauri::command]
pub fn update_rules(state: tauri::State<'_, std::sync::Arc<AppState>>, conversation_id: String,
    rules: Rules) -> Result<(), String> {
    rules::validate(&rules)?;
    let runs = state.runs.lock().map_err(|e| e.to_string())?;
    if runs.contains_key(&conversation_id) {
        return Err("Agent 运行期间不能从界面修改规则".into());
    }
    state.change(|db| {
        let c = db.conversations.iter_mut().find(|c| c.id == conversation_id).ok_or("对话不存在")?;
        if c.rules.as_ref() != Some(&rules) { c.rules_revision += 1; }
        c.rules = Some(rules); c.updated_at = now(); Ok(())
    })
}

pub fn set_rule_from_agent(state: &AppState, conversation_id: &str, id: &str,
    enabled: Option<bool>, value: Option<serde_json::Value>) -> Result<Rules, String> {
    state.change(|db| {
        let c = db.conversations.iter_mut().find(|c| c.id == conversation_id).ok_or("对话不存在")?;
        let mut changed = c.rules.clone().unwrap_or_else(|| db.settings.rules.clone());
        rules::update(&mut changed, id, enabled, value)?;
        c.rules_revision += 1; c.rules = Some(changed.clone()); c.updated_at = now(); Ok(changed)
    })
}

pub fn active_files(db: &Database) -> Result<Vec<UploadedFile>, String> {
    ["users.dat", "movies.dat", "ratings.dat"].iter().map(|name| {
        let id = db.active_files.get(*name).ok_or_else(|| format!("请在设置中初始化 {name}"))?;
        db.files.iter().find(|file| &file.id == id).cloned().ok_or_else(|| format!("{name} 的文件记录缺失"))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_settings_rules_keep_user_choices() {
        let mut value = serde_json::json!({"settings":{"rules":{
            "deduplicate":false,"quarantine_orphans":false,"trim_fields":false,
            "freshness_days":30,"reference_timestamp":1000000000
        }}});
        migrate_legacy_rules(&mut value).unwrap();
        let rules: Rules = serde_json::from_value(value["settings"]["rules"].clone()).unwrap();
        assert!(!rules.entries["unique_ratings"].enabled);
        assert!(!rules.entries["rating_user_exists"].enabled);
        assert_eq!(rules.entries["score_time_window"].value,
            Some(serde_json::json!({"min":954547200,"max":1046476799})));
    }

    #[test]
    fn deleting_conversation_removes_its_local_jobs_and_keeps_shared_sources() {
        let root = std::env::temp_dir().join(format!("ml1m-delete-test-{}", uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4().to_string();
        let job_id = uuid::Uuid::new_v4().to_string();
        let work = root.join("jobs").join(&job_id);
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("report.md"), "report").unwrap();
        let mut db = Database::default();
        db.conversations.push(Conversation { id: id.clone(), ..Conversation::default() });
        db.jobs.push(Job { id: job_id.clone(), conversation_id: id.clone(), kind: "report".into(), ..Job::default() });
        db.files.push(UploadedFile { id: uuid::Uuid::new_v4().to_string(), conversation_id: String::new(),
            name: "users.dat".into(), sha256: String::new(), bytes: 1, local_path: String::new(),
            hdfs_path: Some("/ml1m-agent/sources/shared/users.dat".into()), uploaded_at: now() });
        let state = AppState { root: root.clone(), db: Mutex::new(db), runs: Mutex::new(Default::default()),
            uploads: Mutex::new(Default::default()), source_busy: AtomicBool::new(false) };
        delete_conversation_inner(&state, &id).unwrap();
        let saved = state.snapshot().unwrap();
        assert!(saved.conversations.is_empty());
        assert!(saved.jobs.is_empty());
        assert_eq!(saved.files.len(), 1);
        assert!(!work.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_hdfs_cleanup_keeps_conversation_and_job_for_retry() {
        let root = std::env::temp_dir().join(format!("ml1m-delete-test-{}", uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4().to_string();
        let job_id = uuid::Uuid::new_v4().to_string();
        let work = root.join("jobs").join(&job_id);
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("actions.ndjson"), "record").unwrap();
        let mut db = Database::default();
        db.settings.hadoop_bin = root.join("missing-hadoop").to_string_lossy().into();
        db.conversations.push(Conversation { id: id.clone(), ..Conversation::default() });
        db.jobs.push(Job { id: job_id.clone(), conversation_id: id.clone(), kind: "cleaning".into(),
            result: Some(serde_json::json!({"hdfs_job_root":format!("/ml1m-agent/jobs/{job_id}")})), ..Job::default() });
        let state = AppState { root: root.clone(), db: Mutex::new(db), runs: Mutex::new(Default::default()),
            uploads: Mutex::new(Default::default()), source_busy: AtomicBool::new(false) };
        assert!(delete_conversation_inner(&state, &id).is_err());
        let saved = state.snapshot().unwrap();
        assert_eq!(saved.conversations.len(), 1);
        assert_eq!(saved.jobs.len(), 1);
        assert!(work.join("actions.ndjson").exists());
        assert!(state.runs.lock().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
