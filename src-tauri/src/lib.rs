mod agent;
mod hadoop;
mod model;
mod report;
mod rules;
mod store;

use model::Database;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use store::AppState;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
fn get_state(state: State<'_, Arc<AppState>>) -> Result<Database, String> { state.snapshot() }

#[tauri::command]
fn preview_result(state: State<'_, Arc<AppState>>, job_id: String, name: String) -> Result<Vec<String>, String> {
    hadoop::preview(&state, &job_id, &name)
}

#[tauri::command]
fn download_result(app: AppHandle, state: State<'_, Arc<AppState>>, job_id: String, name: String) -> Result<String, String> {
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let short_id = job_id.get(..8).ok_or("任务 ID 无效")?;
    let destination = dir.join(format!("ml1m-{short_id}-{name}"));
    hadoop::export(&state, &job_id, &name, &destination.to_string_lossy())?;
    Ok(destination.to_string_lossy().into())
}

#[tauri::command]
fn stop_chat(state: State<'_, Arc<AppState>>, conversation_id: String) -> Result<(), String> {
    if let Some(flag) = state.runs.lock().map_err(|e| e.to_string())?.get(&conversation_id) {
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn start_chat(app: AppHandle, state: State<'_, Arc<AppState>>, conversation_id: String,
              content: String) -> Result<String, String> {
    let content = content.trim();
    if content.is_empty() || content.chars().count() > 10000 { return Err("消息长度须为 1–10000 字".into()); }
    let db = state.snapshot()?;
    if !db.conversations.iter().any(|c| c.id == conversation_id) { return Err("对话不存在".into()); }
    if db.settings.api_key.trim().is_empty() { return Err("请先在设置中填写 DeepSeek API Key".into()); }
    let mut runs = state.runs.lock().map_err(|e| e.to_string())?;
    if state.source_busy.load(Ordering::SeqCst) { return Err("共享数据文件正在上传，请稍后发送".into()); }
    if runs.contains_key(&conversation_id) { return Err("当前对话仍在运行".into()); }
    let flag = Arc::new(AtomicBool::new(false));
    runs.insert(conversation_id.clone(), flag.clone());
    drop(runs);
    let run_id = uuid::Uuid::new_v4().to_string();
    if let Err(error) = store::add_message(&state, &conversation_id, "user", content.to_string(), Some(run_id.clone()), "text", None, None, None) {
        state.runs.lock().map_err(|e| e.to_string())?.remove(&conversation_id);
        return Err(error);
    }
    let state_arc = state.inner().clone();
    let result_id = run_id.clone();
    tauri::async_runtime::spawn(async move {
        agent::run(app, state_arc, conversation_id, run_id, flag).await;
    });
    Ok(result_id)
}


#[tauri::command]
fn rule_catalog() -> Vec<rules::RuleSpec> { rules::catalog() }

#[tauri::command]
async fn source_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || hadoop::source_status(&state)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn read_report(state: State<'_, Arc<AppState>>, job_id: String) -> Result<String, String> {
    let db = state.snapshot()?;
    if !db.jobs.iter().any(|j| j.id == job_id && j.kind == "report" && j.status == "completed") {
        return Err("报告不存在".into());
    }
    let body = std::fs::read_to_string(state.root.join("jobs").join(job_id).join("report.md")).map_err(|e| e.to_string())?;
    Ok(report::humanize_legacy_report(&body))
}

#[tauri::command]
fn cleaning_records(state: State<'_, Arc<AppState>>, job_id: String, rule_id: Option<String>,
    table: Option<String>, action: Option<String>, offset: usize, limit: usize) -> Result<serde_json::Value, String> {
    let db = state.snapshot()?;
    let job = db.jobs.iter().find(|j| j.id == job_id).ok_or("任务不存在")?;
    hadoop::cleaning_records(&state, job, rule_id.as_deref(), table.as_deref(), action.as_deref(), offset, limit)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = AppState::new(app.handle()).map_err(std::io::Error::other)?;
            app.manage(Arc::new(state)); Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state, store::create_conversation, store::rename_conversation, store::delete_conversation,
            store::save_settings, store::update_rules, rule_catalog, source_status, cleaning_records, read_report, store::begin_upload, store::append_upload_chunk,
            store::finish_upload, store::abort_upload, start_chat, stop_chat,
            preview_result, download_result
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Tauri application");
}
