mod presentation;
mod runner;
mod tools;

use crate::{
    model::{Job, Settings},
    store::AppState,
};
pub use presentation::report_section;
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::AppHandle;
pub use tools::definitions;
static ANALYSIS_LOCK: Mutex<()> = Mutex::new(());
const ACTIONS: &[&str] = &["logistic", "tree", "kmeans", "dbscan", "svd", "cur"];

fn read(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn valid_id(id: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| "结果标识无效".into())
}
fn lock(cancel: &AtomicBool) -> Result<std::sync::MutexGuard<'static, ()>, String> {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("已中断".into());
        }
        match ANALYSIS_LOCK.try_lock() {
            Ok(g) => return Ok(g),
            Err(std::sync::TryLockError::Poisoned(e)) => return Ok(e.into_inner()),
            Err(_) => std::thread::sleep(Duration::from_millis(80)),
        }
    }
}
pub fn is_tool(name: &str) -> bool {
    name == "compare_analysis"
        || name == "list_analysis_assets"
        || name == "get_analysis_rows"
        || name
            .strip_prefix("analyze_")
            .is_some_and(|a| ACTIONS.contains(&a))
}
fn dataset(
    state: &AppState,
    app: &AppHandle,
    job: &Job,
    settings: &Settings,
    args: &Value,
    cancel: Arc<AtomicBool>,
) -> Result<Value, String> {
    let root = state.root.join("analysis").join("datasets");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    if let Some(id) = args["dataset_id"].as_str() {
        valid_id(id)?;
        return read(&root.join(id).join("manifest.json"))
            .map_err(|_| "数据不可用：指定分析数据集未登记".into());
    }
    let db = state.snapshot()?;
    let cleaning = db
        .jobs
        .iter()
        .rev()
        .find(|j| {
            j.conversation_id == job.conversation_id
                && j.kind == "cleaning"
                && j.status == "completed"
                && args["cleaning_job_id"].as_str().is_none_or(|id| j.id == id)
        })
        .ok_or("数据不可用：请先完成清洗和清洗后五维评估，或指定已登记 dataset_id")?;
    let cleaned = cleaning.result.as_ref().ok_or("清洗结果缺失")?;
    let assessment = db
        .jobs
        .iter()
        .rev()
        .filter(|j| j.status == "completed" && j.kind == "assessment")
        .filter_map(|j| j.result.as_ref())
        .find(|v| {
            v["source"]["cleaning_job_id"] == cleaning.id
                && v["data_version"] == cleaned["clean_data_version"]
                && v["rule_version"] == cleaned["rule_version"]
        })
        .ok_or("数据不可用：缺少与清洗版本、规则版本一致的清洗后评估报告")?;
    let t1 = assessment["metrics"]["t1"]
        .as_i64()
        .ok_or("评估报告缺少 T1")?;
    let t2 = assessment["metrics"]["t2"]
        .as_i64()
        .ok_or("评估报告缺少 T2")?;
    if t1 >= t2 {
        return Err("数据不可用：时间边界必须满足 T1 < T2".into());
    }
    use sha2::{Digest, Sha256};
    let identity = serde_json::to_vec(&json!([
        cleaning.id,
        cleaned["clean_data_version"],
        cleaned["rule_version"],
        t1,
        t2
    ]))
    .map_err(|e| e.to_string())?;
    let digest = Sha256::digest(identity);
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    let dataset_id = uuid::Uuid::from_bytes(bytes).to_string();
    let dir = root.join(&dataset_id);
    let mut files = serde_json::Map::new();
    for name in ["users.dat", "movies.dat", "ratings.dat"] {
        let path = state.root.join("jobs").join(&cleaning.id).join(name);
        if !path.is_file() {
            crate::hadoop::fetch_clean_file(
                state,
                cleaning,
                settings,
                name,
                &path,
                cancel.clone(),
            )?;
        }
        files.insert(name.into(), json!(path));
    }
    let info = json!({"id":dataset_id,"cleaning_job_id":cleaning.id,"label":cleaned["clean_data_version_label"],"rule_label":cleaned["rule_version_label"],
        "rule_version":cleaned["rule_version"],"clean_version":cleaned["clean_data_version"],"t1":t1,"t2":t2,"files":files});
    runner::execute(
        state,
        app,
        job,
        settings,
        &json!({"action":"prepare","dataset":info,"dataset_dir":dir,"assets_dir":state.root.join("analysis/assets")}),
        cancel,
    )
}
fn run_one(
    state: &AppState,
    app: &AppHandle,
    job: &Job,
    settings: &Settings,
    args: &Value,
    action: &str,
    cancel: Arc<AtomicBool>,
) -> Result<Value, String> {
    let _guard = lock(&cancel)?;
    state.update_job(
        app,
        &job.id,
        "running",
        "校验清洗数据与时间边界",
        None,
        None,
    )?;
    let info = dataset(state, app, job, settings, args, cancel.clone())?;
    let mut req = args.clone();
    req["action"] = json!(action);
    req["dataset_id"] = info["id"].clone();
    req["dataset_dir"] = json!(state
        .root
        .join("analysis/datasets")
        .join(info["id"].as_str().ok_or("数据标识缺失")?));
    req["assets_dir"] = json!(state.root.join("analysis/assets"));
    let result = runner::execute(state, app, job, settings, &req, cancel)?;
    let id = result["asset_id"].as_str().ok_or("结果未登记")?;
    valid_id(id)?;
    let src = state.root.join("analysis/assets").join(id);
    let dest = state.root.join("jobs").join(&job.id);
    for file in result["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if public_file(file) && src.join(file).is_file() {
            fs::copy(src.join(file), dest.join(file)).map_err(|e| e.to_string())?;
        }
    }
    Ok(result)
}
pub fn public_file(name: &str) -> bool {
    [
        "predictions.csv",
        "memberships.csv",
        "profiles.csv",
        "groups.csv",
        "coordinates.csv",
        "comparison.csv",
        "projection.svg",
    ]
    .contains(&name)
}
pub fn summary(job: &Job) -> Value {
    let mut result = job.result.clone().unwrap_or(Value::Null);
    if let Some(obj) = result.as_object_mut() {
        for key in ["files", "dependencies", "original_plot", "reduced_plot"] {
            obj.remove(key);
        }
    }
    json!({"job_id":job.id,"status":job.status,"error":job.error,"result":result})
}
pub async fn execute(
    app: &AppHandle,
    state: Arc<AppState>,
    conversation: &str,
    run: &str,
    cancel: Arc<AtomicBool>,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    if name == "list_analysis_assets" {
        return list_assets(&state);
    }
    if name == "get_analysis_rows" {
        return rows(
            &state,
            args["job_id"].as_str().ok_or("缺少 job_id")?,
            args["table"].as_str().ok_or("缺少 table")?,
            args["offset"].as_u64().unwrap_or(0) as usize,
            args["limit"].as_u64().unwrap_or(20) as usize,
            Some(conversation),
        );
    }
    let actions: Vec<&str> = if name == "compare_analysis" {
        match args["task"].as_str() {
            Some("classification") => vec!["logistic", "tree"],
            Some("clustering") => vec!["kmeans", "dbscan"],
            Some("reduction") => vec!["svd", "cur"],
            _ => return Err("task 必须是 classification、clustering 或 reduction".into()),
        }
    } else {
        vec![name
            .strip_prefix("analyze_")
            .filter(|a| ACTIONS.contains(a))
            .ok_or("未知分析工具")?]
    };
    let mut results = vec![];
    for action in actions {
        if cancel.load(Ordering::SeqCst) {
            return Err("已中断".into());
        }
        let cfg = state.snapshot()?.settings;
        let input = args.clone();
        let stop = cancel.clone();
        let action = action.to_string();
        let kind = format!("analysis_{action}");
        let job = crate::agent::create_and_run(
            app,
            state.clone(),
            conversation,
            run,
            &kind,
            None,
            "",
            move |job, state, app| run_one(&state, &app, &job, &cfg, &input, &action, stop),
        )
        .await?;
        results.push(summary(&job));
    }
    Ok(
        json!({"results":results,"note":"只解释已完成结果；失败任务不能视为已生成结果。卡片已展示必要字段，无需重复逐项报数。"}),
    )
}
pub fn list_assets(state: &AppState) -> Result<Value, String> {
    let mut datasets = vec![];
    let mut assets = vec![];
    for (part, marker, out) in [
        ("datasets", "manifest.json", &mut datasets),
        ("assets", "metadata.json", &mut assets),
    ] {
        if let Ok(entries) = fs::read_dir(state.root.join("analysis").join(part)) {
            for entry in entries.flatten() {
                if uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_ok() {
                    if let Ok(v) = read(&entry.path().join(marker)) {
                        out.push(if part=="datasets"{json!({"dataset_id":v["id"],"version":v["label"],"rule_version":v["rule_label"],"t1":v["t1"],"t2":v["t2"]})}else{json!({"model_id":v["id"],"task":v["task"],"version":v["version"],"dataset_id":v["dataset_id"],"parameters":v["parameters"]})});
                    }
                }
            }
        }
    }
    Ok(json!({"datasets":datasets,"assets":assets}))
}
pub fn rows(
    state: &AppState,
    id: &str,
    table: &str,
    offset: usize,
    limit: usize,
    conversation: Option<&str>,
) -> Result<Value, String> {
    if ![
        "predictions",
        "memberships",
        "profiles",
        "groups",
        "coordinates",
        "comparison",
        "original_plot",
        "reduced_plot",
    ]
    .contains(&table)
    {
        return Err("不支持的结果表".into());
    }
    let db = state.snapshot()?;
    let job = db
        .jobs
        .iter()
        .find(|j| {
            j.id == id
                && j.status == "completed"
                && j.kind.starts_with("analysis_")
                && conversation.is_none_or(|c| j.conversation_id == c)
        })
        .ok_or("结果尚未完成或不存在")?;
    let asset = job
        .result
        .as_ref()
        .and_then(|r| r["asset_id"].as_str())
        .ok_or("结果未登记")?;
    valid_id(asset)?;
    let file = fs::File::open(
        state
            .root
            .join("analysis/assets")
            .join(asset)
            .join(format!("{table}.jsonl")),
    )
    .map_err(|_| "该结果没有此表")?;
    let cap = if ["original_plot", "reduced_plot"].contains(&table) {
        10000
    } else {
        200
    };
    let mut total = 0;
    let mut rows = vec![];
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if total >= offset && rows.len() < limit.clamp(1, cap) {
            rows.push(serde_json::from_str::<Value>(&line).map_err(|e| e.to_string())?);
        }
        total += 1;
    }
    Ok(json!({"table":table,"total":total,"offset":offset,"rows":rows}))
}
pub fn check(state: &AppState, app: &AppHandle) -> Result<Value, String> {
    let _guard = lock(&AtomicBool::new(false))?;
    let job = Job {
        id: uuid::Uuid::new_v4().to_string(),
        ..Job::default()
    };
    let result = runner::execute(
        state,
        app,
        &job,
        &state.snapshot()?.settings,
        &json!({"action":"check"}),
        Arc::new(AtomicBool::new(false)),
    );
    let _ = fs::remove_dir_all(state.root.join("jobs").join(job.id));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tools_use_official_function_schema_and_specific_ui_descriptions() {
        let definitions = definitions();
        for action in ACTIONS {
            let name = format!("analyze_{action}");
            let tool = definitions
                .iter()
                .find(|v| v["function"]["name"] == name)
                .unwrap();
            assert_eq!(tool["type"], "function");
            assert_eq!(tool["function"]["parameters"]["type"], "object");
            assert!(tool["function"]["description"]
                .as_str()
                .unwrap()
                .contains("前端"));
        }
        assert!(definitions
            .iter()
            .any(|v| v["function"]["name"] == "compare_analysis"));
    }
    #[test]
    fn public_outputs_exclude_models_and_internal_identifiers() {
        assert!(public_file("coordinates.csv"));
        assert!(public_file("projection.svg"));
        for name in ["model.pkl", "metadata.json", "../state.json", "result.json"] {
            assert!(!public_file(name));
        }
        assert!(valid_id("../../state").is_err());
    }
}
