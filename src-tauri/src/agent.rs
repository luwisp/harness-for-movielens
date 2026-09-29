use crate::{hadoop, model::{Database, Job, Rules, Settings}, report, rules, store::{self, AppState}};
use futures_util::StreamExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex}};
use tauri::{AppHandle, Emitter};

#[derive(Default)]
struct ToolCall { id: String, name: String, arguments: String }

fn tools() -> Value { json!([
    {"type":"function","function":{"name":"list_sources","description":"查看设置中共享的三个 MovieLens 数据源及 HDFS 路径。","parameters":{"type":"object","properties":{}}}},
    {"type":"function","function":{"name":"list_rules","description":"查看具体规则、开关、参数与分类。","parameters":{"type":"object","properties":{}}}},
    {"type":"function","function":{"name":"update_rule","description":"调整当前对话中的单条清洗或评分规则。基础解析规则不能关闭。","parameters":{"type":"object","properties":{"rule_id":{"type":"string"},"enabled":{"type":"boolean"},"integer_value":{"type":"integer"},"range_min":{"type":"integer"},"range_max":{"type":"integer"}},"required":["rule_id"]}}},
    {"type":"function","function":{"name":"run_cleaning","description":"仅清洗共享数据，返回清洗摘要和任务 ID。","parameters":{"type":"object","properties":{}}}},
    {"type":"function","function":{"name":"assess_quality","description":"对原始数据或指定清洗任务产物单独计算五维质量。","parameters":{"type":"object","properties":{"source":{"type":"string","enum":["raw","cleaned"]},"cleaning_job_id":{"type":"string"}},"required":["source"]}}},
    {"type":"function","function":{"name":"compare_assessments","description":"对比两次已完成的五维评估，要求规则版本和时效参照一致。","parameters":{"type":"object","properties":{"before_job_id":{"type":"string"},"after_job_id":{"type":"string"}},"required":["before_job_id","after_job_id"]}}},
    {"type":"function","function":{"name":"run_pipeline","description":"标准入口，顺序执行数据清洗、原始与清洗后五维评估、五维对比。界面显示清洗与对比卡片。","parameters":{"type":"object","properties":{}}}},
    {"type":"function","function":{"name":"get_cleaning_records","description":"按条件分页查看清洗动作记录，不一次返回全部记录。","parameters":{"type":"object","properties":{"job_id":{"type":"string"},"rule_id":{"type":"string"},"table":{"type":"string","enum":["0","1","2"]},"action":{"type":"string","enum":["removed","normalized"]},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["job_id"]}}},
    {"type":"function","function":{"name":"get_rule_statistics","description":"一次读取某次清洗的全部规则触发次数排行，包含中文规则名和具体规则内容；未指定 job_id 时读取本对话最近完成的清洗任务。询问哪条规则触发最多时优先直接调用此工具，不要逐条查询清洗记录。","parameters":{"type":"object","properties":{"job_id":{"type":"string"}}}}},
    {"type":"function","function":{"name":"list_jobs","description":"列出当前对话的历史任务 ID、类型、状态和结果摘要，供结果追问。","parameters":{"type":"object","properties":{}}}},
    {"type":"function","function":{"name":"get_job","description":"查询指定任务状态与摘要。","parameters":{"type":"object","properties":{"job_id":{"type":"string"}},"required":["job_id"]}}},
    {"type":"function","function":{"name":"preview_data","description":"预览清洗数据或 Markdown 报告的前 100 行。","parameters":{"type":"object","properties":{"job_id":{"type":"string"},"file":{"type":"string","enum":["users.dat","movies.dat","ratings.dat","actions.ndjson","report.md"]}},"required":["job_id","file"]}}},
    {"type":"function","function":{"name":"request_report","description":"仅在用户明确要求基于历史任务重新生成报告时调用。普通查看历史、规则统计或清洗记录无需报告。支持已完成的清洗、对比或统一流程任务 ID；报告在本轮结束后生成。","parameters":{"type":"object","properties":{"job_ids":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":10}},"required":["job_ids"]}}}
]) }
fn emit(app: &AppHandle, conversation_id: &str, run_id: &str, kind: &str, value: Value) {
    let _ = app.emit("agent-event", json!({"conversation_id":conversation_id,"run_id":run_id,"kind":kind,"value":value}));
}
async fn wait_cancel(cancel: &AtomicBool) {
    loop { if cancel.load(Ordering::SeqCst) { return; }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await; }
}
fn put_message(state: &AppState, app: &AppHandle, conversation_id: &str, run_id: &str,
    role: &str, content: String, kind: &str, job_id: Option<String>,
    tool_name: Option<String>, rule_id: Option<String>) -> Result<(), String> {
    store::add_message(state, conversation_id, role, content, Some(run_id.into()), kind, job_id, tool_name, rule_id)?;
    emit(app, conversation_id, run_id, "message", json!(true)); Ok(())
}
fn rule_count_rows(result: &Value) -> Vec<Value> {
    let mut rows: Vec<(String, u64)> = result["metrics"]["counts"]["by_rule"].as_object()
        .map(|map| map.iter().map(|(id, count)| (id.clone(), count.as_u64().unwrap_or(0))).collect())
        .unwrap_or_default();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows.into_iter().map(|(id, count)| {
        let (label, description) = rules::describe(&id);
        json!({"id":id,"label":label,"description":description,"count":count})
    }).collect()
}
fn rule_statistics(db: &Database, conversation_id: &str, job_id: Option<&str>) -> Result<Value, String> {
    let job = match job_id {
        Some(id) => db.jobs.iter().find(|job| job.id == id && job.conversation_id == conversation_id),
        None => db.jobs.iter().rev().find(|job| job.conversation_id == conversation_id && job.kind == "cleaning" && job.status == "completed"),
    }.ok_or("没有找到该对话中的清洗任务")?;
    if job.kind != "cleaning" || job.status != "completed" { return Err("指定任务不是已完成的清洗任务".into()); }
    let result = job.result.as_ref().ok_or("清洗结果缺失")?;
    let removed = &result["metrics"]["counts"]["removed"];
    let total_removed: u64 = removed.as_object().map(|counts| counts.values().filter_map(Value::as_u64).sum()).unwrap_or(0);
    Ok(json!({"job_id":job.id,"data_version":result["clean_data_version_label"],
        "rule_version":result["rule_version_label"],"removed_records":total_removed,
        "rules":rule_count_rows(result),"note":"这里统计触发移除的规则，规范化动作不计入。单条记录可能触发多条规则，规则触发次数不能直接相加为移除条数。"}))
}
fn concise_job(job: &Job) -> Value {
    let result = job.result.as_ref();
    json!({"job_id":job.id,"kind":job.kind,"status":job.status,"stage":job.stage,"error":job.error,
        "result":result.map(|v| match job.kind.as_str() {
            "cleaning" => json!({"counts":v["metrics"]["counts"],"rules":rule_count_rows(v),"examples":v["metrics"]["examples"],"rule_version":v["rule_version_label"]}),
            "assessment" => {
                let scores = &v["metrics"]["scores"];
                let mut percentages = serde_json::Map::new();
                for name in ["accurate", "complete", "unique", "consistent", "up_to_date"] {
                    let eligible = scores[format!("{name}_eligible")].as_f64().unwrap_or(0.0);
                    let value = if v["metrics"]["enabled"][name] == false || eligible == 0.0 { None }
                        else { Some(scores[format!("{name}_good")].as_f64().unwrap_or(0.0) / eligible * 100.0) };
                    percentages.insert(name.into(), json!(value));
                }
                json!({"percentages":percentages,"counts":scores,"enabled":v["metrics"]["enabled"],
                    "reference_timestamp":v["metrics"]["reference_timestamp"],"rule_version":v["rule_version_label"]})
            },
            "comparison" => json!({"dimensions":v["dimensions"],"rule_version":v["rule_version_label"]}),
            _ => v.clone() })})
}
async fn create_and_run<F>(app: &AppHandle, state: Arc<AppState>, conversation_id: &str,
    run_id: &str, kind: &str, parent_id: Option<String>, status_label: &str, f: F) -> Result<Job, String>
where F: FnOnce(Job, Arc<AppState>, AppHandle) -> Result<Value, String> + Send + 'static {
    let job = store::create_job(&state, conversation_id, kind, parent_id, Some(run_id.into()))?;
    let message_kind = if kind == "assessment" { "assessment_status" } else { "job" };
    put_message(&state, app, conversation_id, run_id, "tool", status_label.into(), message_kind,
        Some(job.id.clone()), Some(kind.into()), None)?;
    let job_copy = job.clone(); let app_copy = app.clone(); let state_copy = state.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || f(job_copy, state_copy, app_copy)).await.map_err(|e| e.to_string())?;
    match outcome {
        Ok(value) => state.update_job(app, &job.id, "completed", "完成", None, Some(value))?,
        Err(error) => { let status = if error == "已中断" { "cancelled" } else { "failed" };
            state.update_job(app, &job.id, status, "结束", Some(error), None)?; },
    }
    state.snapshot()?.jobs.into_iter().find(|j| j.id == job.id).ok_or("任务丢失".into())
}
fn active_rules(state: &AppState, conversation_id: &str) -> Result<Rules, String> {
    let db = state.snapshot()?;
    let conversation = db.conversations.iter().find(|c| c.id == conversation_id).ok_or("对话不存在")?;
    Ok(conversation.rules.clone().unwrap_or(db.settings.rules))
}
async fn cleaning(app: &AppHandle, state: Arc<AppState>, conversation_id: &str, run_id: &str,
    parent: Option<String>, cancel: Arc<AtomicBool>) -> Result<Job, String> {
    let db = state.snapshot()?; let files = store::active_files(&db)?;
    let settings = db.settings; let rules = active_rules(&state, conversation_id)?;
    create_and_run(app, state, conversation_id, run_id, "cleaning", parent, "",
        move |job, state, app| hadoop::run_cleaning(&state, &app, &job, &settings, &files, &rules, cancel)).await
}
async fn assessment(app: &AppHandle, state: Arc<AppState>, conversation_id: &str, run_id: &str,
    source: &str, cleaning_id: Option<&str>, parent: Option<String>,
    cancel: Arc<AtomicBool>, reference_override: Option<i64>) -> Result<Job, String> {
    let db = state.snapshot()?; let settings = db.settings.clone(); let rules = active_rules(&state, conversation_id)?;
    let (inputs, source_info, utf8) = match source {
        "raw" => {
            let files = store::active_files(&db)?;
            let mut hash = Sha256::new();
            for file in &files { hash.update(file.name.as_bytes()); hash.update(file.sha256.as_bytes()); }
            let inputs = files.iter().map(|f| f.hdfs_path.clone().ok_or("数据源未上传 HDFS".into())).collect::<Result<Vec<_>,String>>()?;
            (inputs, json!({"kind":"raw","data_version":format!("sha256:{:x}",hash.finalize()),"data_version_label":format!("原始数据 v{}",db.source_revision),"files":db.active_files}), false)
        },
        "cleaned" => { let id = cleaning_id.ok_or("缺少 cleaning_job_id")?;
            let job = db.jobs.iter().find(|j| j.id == id && j.conversation_id == conversation_id && j.kind == "cleaning" && j.status == "completed").ok_or("清洗任务不存在或尚未完成")?;
            let root = job.result.as_ref().and_then(|v| v["clean_hdfs"].as_str()).ok_or("清洗结果路径缺失")?;
            (["users.dat", "movies.dat", "ratings.dat"].iter().map(|n| format!("{root}/{n}")).collect(), json!({"kind":"cleaned","cleaning_job_id":id,"clean_data_version":job.result.as_ref().map(|v| &v["clean_data_version"]),"clean_data_version_label":job.result.as_ref().map(|v| &v["clean_data_version_label"])}), true) },
        _ => return Err("source 只能是 raw 或 cleaned".into()),
    };
    let status_label = if source == "raw" { "原始数据" } else { "清洗后数据" };
    create_and_run(app, state, conversation_id, run_id, "assessment", parent, status_label,
        move |job, state, app| hadoop::run_assessment(&state, &app, &job, &settings, &inputs, &rules, source_info, utf8, cancel, reference_override)).await
}
async fn comparison(app: &AppHandle, state: Arc<AppState>, conversation_id: &str, run_id: &str,
    first: &str, second: &str, parent: Option<String>) -> Result<Job, String> {
    let db = state.snapshot()?;
    let get = |id: &str| db.jobs.iter().find(|j| j.id == id && j.conversation_id == conversation_id && j.kind == "assessment" && j.status == "completed").cloned().ok_or_else(|| "评估任务不存在或尚未完成".to_string());
    let a = get(first)?; let b = get(second)?;
    create_and_run(app, state, conversation_id, run_id, "comparison", parent, "",
        move |job, state, _| hadoop::compare(&state, &a, &b, &job)).await
}
async fn execute_tool(app: &AppHandle, state: Arc<AppState>, conversation_id: &str,
    run_id: &str, cancel: Arc<AtomicBool>, call: &ToolCall) -> Value {
    let result: Result<Value, String> = async {
        let args: Value = serde_json::from_str(if call.arguments.trim().is_empty() { "{}" } else { &call.arguments }).map_err(|e| e.to_string())?;
        let db = state.snapshot()?;
        match call.name.as_str() {
            "list_sources" => Ok(json!({"version":format!("原始数据 v{}",db.source_revision),"files":db.active_files.iter().filter_map(|(name,id)| db.files.iter().find(|f| &f.id == id).map(|f| json!({"name":name,"hdfs_path":f.hdfs_path}))).collect::<Vec<_>>() })),
            "list_rules" => Ok(json!({"catalog":rules::catalog(),"rules":active_rules(&state, conversation_id)?})),
            "update_rule" => {
                let id = args["rule_id"].as_str().ok_or("缺少 rule_id")?;
                let enabled = match args.get("enabled") {
                    Some(value) => Some(value.as_bool().ok_or("enabled 必须为布尔值")?),
                    None => None,
                };
                let value = if let Some(number) = args.get("integer_value") { Some(number.clone()) }
                    else if args.get("range_min").is_some() || args.get("range_max").is_some() {
                        let min = args["range_min"].as_i64().ok_or("缺少 range_min")?;
                        let max = args["range_max"].as_i64().ok_or("缺少 range_max")?;
                        Some(json!({"min":min,"max":max}))
                    } else { None };
                let updated = store::set_rule_from_agent(&state, conversation_id, id, enabled, value)?;
                let (label, description) = rules::describe(id);
                let entry = &updated.entries[id];
                let state_text = if entry.enabled { "开启" } else { "关闭" };
                let parameter = entry.value.as_ref().map(|v| format!("；参数 {v}")).unwrap_or_default();
                put_message(&state, app, conversation_id, run_id, "tool",
                    format!("{label}：{description}（已{state_text}{parameter}）"),
                    "rule", None, Some(call.name.clone()), Some(id.into()))?;
                let revision = state.snapshot()?.conversations.into_iter().find(|c| c.id == conversation_id).ok_or("对话不存在")?.rules_revision;
                Ok(json!({"rule_id":id,"label":label,"description":description,"entry":entry,"rule_version":format!("规则方案 v{revision}")}))
            },
            "run_cleaning" => { let job = cleaning(app, state, conversation_id, run_id, None, cancel).await?;
                if job.status != "completed" { return Err(job.error.unwrap_or("清洗失败".into())); }
                Ok(concise_job(&job)) },
            "assess_quality" => { let source = args["source"].as_str().ok_or("缺少 source")?;
                let job = assessment(app, state, conversation_id, run_id, source,
                    args["cleaning_job_id"].as_str(), None, cancel, None).await?;
                if job.status != "completed" { return Err(job.error.unwrap_or("评估失败".into())); }
                Ok(concise_job(&job)) },
            "compare_assessments" => { let a = args["before_job_id"].as_str().ok_or("缺少 before_job_id")?;
                let b = args["after_job_id"].as_str().ok_or("缺少 after_job_id")?;
                let job = comparison(app, state, conversation_id, run_id, a, b, None).await?;
                if job.status != "completed" { return Err(job.error.unwrap_or("对比失败".into())); }
                Ok(concise_job(&job)) },
            "run_pipeline" => {
                let parent = store::create_job(&state, conversation_id, "pipeline", None, Some(run_id.into()))?;
                let outcome: Result<Value, String> = async {
                    state.update_job(app, &parent.id, "running", "数据清洗", None, None)?;
                    let clean = cleaning(app, state.clone(), conversation_id, run_id, Some(parent.id.clone()), cancel.clone()).await?;
                    if clean.status != "completed" { return Err(clean.error.unwrap_or("清洗失败".into())); }
                    state.update_job(app, &parent.id, "running", "原始数据五维评估", None, None)?;
                    let before = assessment(app, state.clone(), conversation_id, run_id, "raw", None, Some(parent.id.clone()), cancel.clone(), None).await?;
                    if before.status != "completed" { return Err(before.error.unwrap_or("评估失败".into())); }
                    state.update_job(app, &parent.id, "running", "清洗后五维评估", None, None)?;
                    let reference = before.result.as_ref().and_then(|v| v["metrics"]["reference_timestamp"].as_i64());
                    let after = assessment(app, state.clone(), conversation_id, run_id, "cleaned", Some(&clean.id), Some(parent.id.clone()), cancel.clone(), reference).await?;
                    if after.status != "completed" { return Err(after.error.unwrap_or("评估失败".into())); }
                    state.update_job(app, &parent.id, "running", "五维评估对比", None, None)?;
                    let compared = comparison(app, state.clone(), conversation_id, run_id, &before.id, &after.id, Some(parent.id.clone())).await?;
                    if compared.status != "completed" { return Err(compared.error.unwrap_or("对比失败".into())); }
                    let summary = json!({"cleaning_job_id":clean.id,"before_job_id":before.id,"after_job_id":after.id,"comparison_job_id":compared.id});
                    state.update_job(app, &parent.id, "completed", "完成", None, Some(summary.clone()))?;
                    Ok(json!({"pipeline_job_id":parent.id,"steps":summary,"comparison":compared.result.as_ref().map(|v| &v["dimensions"])}))
                }.await;
                if let Err(error) = &outcome {
                    let status = if cancel.load(Ordering::SeqCst) { "cancelled" } else { "failed" };
                    let _ = state.update_job(app, &parent.id, status, "结束", Some(error.clone()), None);
                }
                outcome
            },
            "get_cleaning_records" => {
                let id = args["job_id"].as_str().ok_or("缺少 job_id")?;
                let job = db.jobs.iter().find(|j| j.id == id && j.conversation_id == conversation_id).ok_or("任务不存在")?;
                let mut records = hadoop::cleaning_records(&state, job, args["rule_id"].as_str(), args["table"].as_str(), args["action"].as_str(),
                    args["offset"].as_u64().unwrap_or(0) as usize, args["limit"].as_u64().unwrap_or(20) as usize)?;
                if let Some(rows) = records["records"].as_array_mut() {
                    for row in rows {
                        let details: Vec<_> = row["rules"].as_array().into_iter().flatten().filter_map(Value::as_str)
                            .map(|id| { let (label, description) = rules::describe(id);
                                json!({"id":id,"label":label,"description":description}) }).collect();
                        row["rule_details"] = json!(details);
                    }
                }
                Ok(records)
            },
            "get_rule_statistics" => rule_statistics(&db, conversation_id, args["job_id"].as_str()),
            "list_jobs" => Ok(json!(db.jobs.iter().filter(|j| j.conversation_id == conversation_id)
                .rev().take(30).map(|j| json!({"job_id":j.id,"kind":j.kind,"status":j.status,"stage":j.stage})).collect::<Vec<_>>())),
            "get_job" => { let id = args["job_id"].as_str().ok_or("缺少 job_id")?;
                let job = db.jobs.iter().find(|j| j.id == id && j.conversation_id == conversation_id).ok_or("任务不存在")?;
                Ok(concise_job(job)) },
            "preview_data" => { let id = args["job_id"].as_str().ok_or("缺少 job_id")?;
                let name = args["file"].as_str().ok_or("缺少 file")?;
                if !db.jobs.iter().any(|j| j.id == id && j.conversation_id == conversation_id) { return Err("任务不存在".into()); }
                Ok(json!(hadoop::preview(&state, id, name)?)) },
            "request_report" => {
                let ids = args["job_ids"].as_array().ok_or("缺少 job_ids")?;
                if ids.is_empty() || ids.len() > 10 { return Err("一次只能选择 1–10 个任务".into()); }
                let mut accepted = Vec::new();
                for id in ids {
                    let id = id.as_str().ok_or("job_ids 须为任务 ID 列表")?;
                    let job = db.jobs.iter().find(|j| j.id == id && j.conversation_id == conversation_id &&
                        j.status == "completed" && ["cleaning", "comparison", "pipeline"].contains(&j.kind.as_str()))
                        .ok_or_else(|| format!("任务 {id} 不是已完成的清洗、对比或统一流程"))?;
                    accepted.push(job.id.clone());
                }
                for id in &accepted {
                    put_message(&state, app, conversation_id, run_id, "tool", String::new(),
                        "report_request", Some(id.clone()), Some(call.name.clone()), None)?;
                }
                Ok(json!({"accepted_job_ids":accepted,"status":"本轮结束后生成 Markdown 报告"}))
            },
            _ => Err("未知工具".into()),
        }
    }.await;
    match result { Ok(value) => json!({"ok":true,"data":value}), Err(error) => json!({"ok":false,"error":error}) }
}
async fn stream_response(client: &reqwest::Client, settings: &Settings, messages: &[Value],
                         app: &AppHandle, conversation_id: &str, run_id: &str,
                         cancel: &AtomicBool, tool_choice: Value, partial: &Arc<Mutex<String>>) -> Result<(String, Vec<ToolCall>), String> {
    let url = format!("{}/chat/completions", settings.api_base.trim_end_matches('/'));
    let request = client.post(url).bearer_auth(&settings.api_key)
        .json(&json!({"model":settings.model,"messages":messages,"tools":tools(),"tool_choice":tool_choice,"thinking":{"type":"disabled"},"stream":true}));
    let response = tokio::select! {
        response = request.send() => response.map_err(|e| e.to_string())?,
        _ = wait_cancel(cancel) => return Err("已中断".into()),
    };
    let status = response.status();
    if !status.is_success() {
        let detail = tokio::time::timeout(std::time::Duration::from_secs(5), response.text())
            .await.ok().and_then(Result::ok).unwrap_or_default();
        return Err(format!("DeepSeek HTTP {status}: {}", detail.chars().take(500).collect::<String>()));
    }
    let mut stream = response.bytes_stream(); let mut buffer: Vec<u8> = Vec::new(); let mut answer = String::new();
    let mut calls: BTreeMap<usize, ToolCall> = BTreeMap::new(); let mut completed = false;
    loop {
        if cancel.load(Ordering::SeqCst) { return Err("已中断".into()); }
        let chunk = tokio::select! {
            chunk = stream.next() => chunk,
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
        };
        let Some(chunk) = chunk else { break; };
        buffer.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
        while let Some((end, separator)) = buffer.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2))
            .or_else(|| buffer.windows(4).position(|w| w == b"\r\n\r\n").map(|i| (i, 4))) {
            let event = String::from_utf8(buffer[..end].to_vec()).map_err(|e| e.to_string())?.replace('\r', "");
            buffer.drain(..end + separator);
            for line in event.lines().filter_map(|line| line.strip_prefix("data: ")) {
                if line == "[DONE]" { completed = true; continue; }
                let data: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
                let delta = &data["choices"][0]["delta"];
                if let Some(text) = delta["content"].as_str() {
                    answer.push_str(text); emit(app, conversation_id, run_id, "delta", json!(text));
                    if let Ok(mut output) = partial.lock() { output.push_str(text); }
                }
                if let Some(items) = delta["tool_calls"].as_array() {
                    for item in items {
                        let index = item["index"].as_u64().ok_or("工具调用索引缺失")? as usize;
                        let call = calls.entry(index).or_default();
                        if let Some(id) = item["id"].as_str() { call.id.push_str(id); }
                        if let Some(name) = item["function"]["name"].as_str() { call.name.push_str(name); }
                        if let Some(args) = item["function"]["arguments"].as_str() { call.arguments.push_str(args); }
                    }
                }
            }
        }
    }
    if cancel.load(Ordering::SeqCst) { return Err("已中断".into()); }
    if !completed { return Err("DeepSeek 流中途结束，未收到 [DONE]".into()); }
    let calls: Vec<ToolCall> = calls.into_values().collect();
    if calls.iter().any(|call| call.id.is_empty() || call.name.is_empty()) {
        return Err("DeepSeek 工具调用缺少 ID 或函数名".into());
    }
    Ok((answer, calls))
}

async fn title(client: &reqwest::Client, settings: &Settings, prompt: &str) -> Option<String> {
    let response = client.post(format!("{}/chat/completions", settings.api_base.trim_end_matches('/')))
        .bearer_auth(&settings.api_key).json(&json!({"model":settings.model,"thinking":{"type":"disabled"},"stream":false,
            "messages":[{"role":"system","content":"为用户请求取一个简短中文对话标题，只输出标题内容本身，不要输出前缀或说明，最多20字。"},
                        {"role":"user","content":prompt}]})).send().await.ok()?;
    let value: Value = response.json().await.ok()?;
    let name = value["choices"][0]["message"]["content"].as_str()?.trim().trim_matches('"').to_string();
    if name.is_empty() { None } else { Some(name.chars().take(20).collect()) }
}

pub async fn run(app: AppHandle, state: Arc<AppState>, conversation_id: String,
    run_id: String, cancel: Arc<AtomicBool>) {
    let partial = Arc::new(Mutex::new(String::new()));
    let outcome = run_inner(&app, &state, &conversation_id, &run_id, cancel.clone(), &partial).await;
    if let Err(error) = outcome {
        let text = partial.lock().map(|s| s.clone()).unwrap_or_default();
        if !text.is_empty() { let _ = put_message(&state, &app, &conversation_id, &run_id,
            "assistant", format!("{text}\n\n[输出中断：{error}]"), "text", None, None, None); }
        emit(&app, &conversation_id, &run_id, "error", json!(error));
    }
    match report::generate(&state, &conversation_id, &run_id) {
        Ok(Some(job)) => { let _ = put_message(&state, &app, &conversation_id, &run_id,
            "report", String::new(), "report", Some(job.id), None, None); },
        Err(error) => emit(&app, &conversation_id, &run_id, "error", json!(format!("报告生成失败: {error}"))),
        _ => {},
    }
    if let Ok(mut runs) = state.runs.lock() { runs.remove(&conversation_id); }
    emit(&app, &conversation_id, &run_id, "done", json!(true));
}
async fn run_inner(app: &AppHandle, state: &Arc<AppState>, conversation_id: &str,
    run_id: &str, cancel: Arc<AtomicBool>, partial: &Arc<Mutex<String>>) -> Result<(), String> {
    let db = state.snapshot()?; let settings = db.settings;
    let conversation = db.conversations.into_iter().find(|c| c.id == conversation_id).ok_or("对话不存在")?;
    let prompt = conversation.messages.last().map(|m| m.content.clone()).unwrap_or_default();
    let client = reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(10)).build().map_err(|e| e.to_string())?;
    if conversation.title == "新对话" {
        let state = state.clone(); let app = app.clone(); let client = client.clone();
        let settings = settings.clone(); let id = conversation_id.to_string(); let run = run_id.to_string();
        let cancel = cancel.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(Some(name)) = tokio::time::timeout(std::time::Duration::from_secs(12), title(&client, &settings, &prompt)).await {
                if cancel.load(Ordering::SeqCst) { return; }
                let _ = state.change(|db| { if let Some(c) = db.conversations.iter_mut().find(|c| c.id == id) {
                    if c.title == "新对话" { c.title = name.clone(); } } Ok(()) });
                emit(&app, &id, &run, "title", json!(name));
            }
        });
    }
    let rules_context = serde_json::to_string(&active_rules(state, conversation_id)?).map_err(|e| e.to_string())?;
    let job_context = state.snapshot()?.jobs.into_iter().filter(|j| j.conversation_id == conversation_id)
        .rev().take(12).map(|j| format!("{}:{}:{}", j.id, j.kind, j.status)).collect::<Vec<_>>().join(", ");
    let mut messages = vec![json!({"role":"system","content":format!(
        "你是 MovieLens 1M 数据治理 Agent。使用 DeepSeek 原生 function calling。普通清洗比较需求优先调用 run_pipeline；用户只要清洗、单独五维评估或指定两次评估对比时调用对应工具。数据源是设置中共享的 users.dat/movies.dat/ratings.dat，不在对话中上传。需要更改规则时调用 update_rule，可先 list_rules。询问哪条清洗规则触发最多或触发次数排行时，直接调用 get_rule_statistics，未指定任务 ID 就使用最近完成的清洗任务；它一次返回全部规则计数，无需先 list_jobs，也不要逐条调用 get_cleaning_records。该统计工具的结果不会单独显示在前端，你必须在回复中列出相关规则的中文名称、判定内容和触发次数，并说明多规则可同时触发。向用户说明规则时不要只写规则 ID。普通查看历史任务、统计或记录绝不请求生成报告；只有用户明确要求基于历史结果新建报告时才调用 request_report。前端会展示规则变化、清洗数量与示例、五维对比分数及变化，且自动报告收录本轮新产生的清洗和对比；这些卡片中的数字不要逐项复述，主要解释原因、局限、建议。单独调用 assess_quality 时前端不显示结果卡片且不生成评估报告，你必须在回复中讲述五维结果及必要数值。不要向用户输出 SHA 哈希或内部版本指纹，只使用工具给出的易读版本名。支持 GFM Markdown：标题、列表、表格、代码、链接；不支持原始 HTML。不要编造 Hadoop 结果。准确性只验证已知取值域，时效性按历史参照。规则: {rules_context}。已有任务：{job_context}。追问其他旧结果时可先调用 list_jobs 或 get_job。")})];
    for m in conversation.messages.iter().filter(|m| m.kind == "text" && (m.role == "user" || m.role == "assistant")).rev().take(20).collect::<Vec<_>>().into_iter().rev() {
        messages.push(json!({"role":m.role,"content":m.content}));
    }
    for _ in 0..10 {
        if cancel.load(Ordering::SeqCst) { return Err("已中断".into()); }
        if let Ok(mut p) = partial.lock() { p.clear(); }
        let (text, calls) = stream_response(&client, &settings, &messages, app, conversation_id, run_id, &cancel, json!("auto"), partial).await?;
        if !text.is_empty() { put_message(state, app, conversation_id, run_id, "assistant", text.clone(), "text", None, None, None)?; }
        if calls.is_empty() { return Ok(()); }
        let tool_calls = calls.iter().map(|c| json!({"id":c.id,"type":"function",
            "function":{"name":c.name,"arguments":if c.arguments.is_empty() { "{}" } else { &c.arguments }}})).collect::<Vec<_>>();
        messages.push(json!({"role":"assistant","content":text,"tool_calls":tool_calls}));
        for call in &calls {
            if cancel.load(Ordering::SeqCst) { return Err("已中断".into()); }
            emit(app, conversation_id, run_id, "tool", json!(call.name));
            if ["list_sources", "list_rules", "list_jobs", "get_cleaning_records", "get_rule_statistics", "get_job", "preview_data", "request_report"].contains(&call.name.as_str()) {
                let _ = put_message(state, app, conversation_id, run_id, "tool",
                    match call.name.as_str() {
                        "list_sources" => "查看共享文件", "list_rules" => "查看规则目录",
                        "list_jobs" => "查看历史任务", "get_cleaning_records" => "查询清洗记录",
                        "get_rule_statistics" => "统计规则触发次数", "get_job" => "查看任务状态",
                        "preview_data" => "预览结果数据", "request_report" => "准备历史任务报告", _ => "调用工具",
                    }.into(), "tool_call", None, Some(call.name.clone()), None);
            }
            let result = execute_tool(app, state.clone(), conversation_id, run_id, cancel.clone(), call).await;
            messages.push(json!({"role":"tool","tool_call_id":call.id,"content":result.to_string()}));
        }
    }
    Err("Agent 工具调用轮数超限".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Job;

    #[test]
    fn latest_cleaning_rule_ranking_is_available_in_one_call() {
        let mut db = Database::default();
        db.jobs.push(Job { id: "old".into(), conversation_id: "c".into(), kind: "cleaning".into(),
            status: "completed".into(), result: Some(json!({"metrics":{"counts":{"by_rule":{"schema_fields":9},"removed":{"0":9}}}})), ..Job::default() });
        db.jobs.push(Job { id: "new".into(), conversation_id: "c".into(), kind: "cleaning".into(),
            status: "completed".into(), result: Some(json!({"metrics":{"counts":{"by_rule":{"schema_fields":3,"required_values":8},"removed":{"0":8}}}})), ..Job::default() });
        let ranking = rule_statistics(&db, "c", None).unwrap();
        assert_eq!(ranking["job_id"], "new");
        assert_eq!(ranking["rules"][0]["label"], "必填字段非空");
        assert_eq!(ranking["rules"][0]["count"], 8);
        assert_eq!(ranking["removed_records"], 8);
    }
}
