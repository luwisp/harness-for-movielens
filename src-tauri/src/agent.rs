use crate::{hadoop, model::{Database, Job, Rules, Settings}, report, rules, store::{self, AppState}};
use futures_util::StreamExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex}};
use tauri::{AppHandle, Emitter};

#[derive(Default)]
struct ToolCall { id: String, name: String, arguments: String }

fn tools() -> Value { json!([
    {"type":"function","function":{"name":"list_sources","description":"查看设置中共享的三个 MovieLens 数据源及 HDFS 路径。前端只显示调用记录，不显示返回的文件清单；需要向用户说明查询结果。","parameters":{"type":"object","properties":{}}}},
    {"type":"function","function":{"name":"list_rules","description":"查看具体规则、开关、参数与分类。前端只显示调用记录，不显示返回的规则目录；需要向用户说明相关规则的中文名称、内容和状态。","parameters":{"type":"object","properties":{}}}},
    {"type":"function","function":{"name":"update_rule","description":"调整当前对话中的单条清洗或评分规则，基础解析规则不能关闭。调用成功后，前端会显示被修改规则的名称、内容、开关和参数；用户点击该记录可打开对应规则设置。无需逐字复述修改记录。","parameters":{"type":"object","properties":{"rule_id":{"type":"string"},"enabled":{"type":"boolean"},"integer_value":{"type":"integer"},"range_min":{"type":"integer"},"range_max":{"type":"integer"}},"required":["rule_id"]}}},
    {"type":"function","function":{"name":"run_cleaning","description":"仅清洗共享数据。前端会显示任务状态和进度；完成后显示原始、保留、移除、规范化数量及规则触发排行，用户可按规则、表和动作筛选具体清洗记录。清洗数据和动作记录在本轮下方提供下载，本轮结束时自动生成包含清洗结果的可阅读、可下载 Markdown 报告。无需复述卡片数字，主要解释原因与局限。","parameters":{"type":"object","properties":{}}}},
    {"type":"function","function":{"name":"assess_quality","description":"对原始数据或指定清洗任务产物单独计算五维质量、评估覆盖率、T1/T2 时间切分点及每位用户至少评价 20 部不同电影的达标率；低于 20 不删除用户。前端显示评估任务进度、状态和完成后的 T1/T2（UTC 与 Unix 秒），不显示五维分数卡片，也不把单独评估写入自动报告；需要在回复中讲述五维结果，T1/T2 已展示，无需重复，除非用户询问。","parameters":{"type":"object","properties":{"source":{"type":"string","enum":["raw","cleaned"]},"cleaning_job_id":{"type":"string"}},"required":["source"]}}},
    {"type":"function","function":{"name":"compare_assessments","description":"对比两次已完成的五维评估，要求评分规范与历史时间窗口一致。前端会显示任务状态、五维清洗前后分数、评估覆盖率、有效留存率、20 部不同电影达标率及变化，以及两次评估各自的 T1/T2（UTC 与 Unix 秒）；可查看数学公式与评分规则。本轮结束时自动生成可阅读、可下载 Markdown 报告。无需逐项复述已展示的数值，主要解释变化。","parameters":{"type":"object","properties":{"before_job_id":{"type":"string"},"after_job_id":{"type":"string"}},"required":["before_job_id","after_job_id"]}}},
    {"type":"function","function":{"name":"run_pipeline","description":"标准入口，顺序执行数据清洗、原始与清洗后五维评估、五维对比。前端按顺序显示清洗卡片、两次评估的进度状态与各自的 T1/T2，以及五维对比卡片；清洗卡片有数量、规则触发排行和可筛选的具体记录，对比卡片有五维分数、覆盖率、有效留存、20 部不同电影达标率、变化、两组 T1/T2 及公式化评分规则入口。低于 20 部电影的用户不会被删除。清洗文件在本轮下方可下载，本轮结束时自动生成可阅读、可下载的清洗与对比报告。无需重复卡片数据，主要解释结果。","parameters":{"type":"object","properties":{}}}},
    {"type":"function","function":{"name":"get_cleaning_records","description":"按规则、表、动作等条件分页查看某次清洗的具体动作记录，不一次返回全部记录。前端只显示查询调用记录，不展示本次返回的记录；需要在回复中概述所查记录及其来源表和触发规则。读取历史记录不会生成报告。","parameters":{"type":"object","properties":{"job_id":{"type":"string"},"rule_id":{"type":"string"},"table":{"type":"string","enum":["0","1","2"]},"action":{"type":"string","enum":["removed","normalized"]},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["job_id"]}}},
    {"type":"function","function":{"name":"get_rule_statistics","description":"一次读取某次清洗的全部规则触发次数排行，包含中文规则名和具体规则内容；未指定 job_id 时读取本对话最近完成的清洗任务。询问哪条规则触发最多时优先直接调用此工具，不要逐条查询清洗记录。前端只显示调用记录，不展示排行；需要在回复中说明规则名称、判定内容和次数。读取统计不会生成报告。","parameters":{"type":"object","properties":{"job_id":{"type":"string"}}}}},
    {"type":"function","function":{"name":"list_jobs","description":"列出当前对话的历史任务 ID、类型和状态，供结果追问。前端只显示调用记录，不展示返回的任务列表；需要在回复中说明相关任务。读取历史任务不会生成报告。","parameters":{"type":"object","properties":{}}}},
    {"type":"function","function":{"name":"get_job","description":"查询指定任务状态与结果摘要。前端只显示调用记录，不为历史任务重新显示结果卡片；需要在回复中讲述查询到的状态和相关结果。读取历史任务不会生成报告。","parameters":{"type":"object","properties":{"job_id":{"type":"string"}},"required":["job_id"]}}},
    {"type":"function","function":{"name":"preview_data","description":"读取清洗数据或 Markdown 报告的前 100 行。前端只显示调用记录，不展示本次预览内容；需要在回复中摘述用户请求的内容。预览不会生成新报告。","parameters":{"type":"object","properties":{"job_id":{"type":"string"},"file":{"type":"string","enum":["users.dat","movies.dat","ratings.dat","actions.ndjson","report.md"]}},"required":["job_id","file"]}}},
    {"type":"function","function":{"name":"request_report","description":"仅在用户明确要求基于历史任务重新生成报告时调用，支持已完成的清洗、对比或统一流程任务 ID。前端先显示准备报告的调用记录，本轮结束后显示可直接阅读的 Markdown 报告卡片，并在本轮下方提供下载。普通查看历史、规则统计或清洗记录无需报告。","parameters":{"type":"object","properties":{"job_ids":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":10}},"required":["job_ids"]}}}
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
                json!({"dimensions":v["metrics"]["dimensions"],"cohort20":v["metrics"]["cohort20"],
                    "score_spec_version":v["score_spec_version"],"time_window":v["metrics"]["time_window"],
                    "t1":v["metrics"]["t1"],"t2":v["metrics"]["t2"],
                    "rule_version":v["rule_version_label"]})
            },
            "comparison" => json!({"dimensions":v["dimensions"],"time_boundaries":v["time_boundaries"],
                "rule_version":v["rule_version_label"]}),
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
            (["users.dat", "movies.dat", "ratings.dat"].iter().map(|n| format!("{root}/{n}")).collect(), json!({"kind":"cleaned","cleaning_job_id":id,"source_data_version":job.result.as_ref().map(|v| &v["data_version"]),"clean_data_version":job.result.as_ref().map(|v| &v["clean_data_version"]),"clean_data_version_label":job.result.as_ref().map(|v| &v["clean_data_version_label"])}), true) },
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
                    let summary = json!({"score_spec_version":"quality-spec-v2","cleaning_job_id":clean.id,"before_job_id":before.id,"after_job_id":after.id,"comparison_job_id":compared.id});
                    state.update_job(app, &parent.id, "completed", "完成", None, Some(summary.clone()))?;
                    Ok(json!({"pipeline_job_id":parent.id,"steps":summary,
                        "comparison":compared.result.as_ref().map(|v| &v["dimensions"]),
                        "time_boundaries":compared.result.as_ref().map(|v| &v["time_boundaries"])}))
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
        "
你是 MovieLens 1M 数据治理 Agent。
MovieLens 1M，简称 `ml-1m`，由明尼苏达大学 GroupLens Research 发布，是一个稳定的电影推荐基准数据集。该数据集包含 2000 年加入 MovieLens 的用户所产生的电影评分，并于 2003 年 2 月发布。

| 项目 | 数据规模或范围 |
| --- | --- |
| 用户数量 | 6,040 位匿名用户 |
| 电影记录数量 | 3,883 条，约 3,900 部电影 |
| 评分数量 | 1,000,209 条 |
| 用户范围 | 2000 年加入 MovieLens 的用户 |
| 发布时间 | 2003 年 2 月 |
| 评分范围 | 1 至 5，仅包含整数评分 |

数据集由三个 `.dat` 文件组成，文件不包含表头，字段之间使用 `::` 分隔，文本采用 ISO-8859-1 编码：

| 文件 | 主要字段 | 内容 |
| --- | --- | --- |
| `ratings.dat` | `UserID::MovieID::Rating::Timestamp` | 用户对电影的整数评分及评分时间 |
| `users.dat` | `UserID::Gender::Age::Occupation::Zip-code` | 用户自愿填写的性别、年龄段、职业和邮编信息 |
| `movies.dat` | `MovieID::Title::Genres` | 电影标题和类型信息 |

官方 MovieLens 1M 的用户标识已经匿名化，入选用户至少评价过 20 部电影；当前上传数据可能含异常，必须以工具评估结果为准。`users.dat` 中的人口属性由用户自愿填写，GroupLens 未核验其准确性；年龄和职业使用类别编码。电影类型使用竖线分隔，一部电影可以属于多个类型。MovieID 的最大值不代表实际电影数量，因为编号并不连续。

你可以使用多种工具对 MovieLens 1M 进行清理和诊断。根据用户要求调用工具，解释结果的原因、局限和建议。每个工具的 description 已说明前端会展示什么：已展示的内容不要重复复述，未展示的查询结果需要在回复中讲述。
- 普通清洗比较需求优先调用 run_pipeline；用户只要清洗、单独五维评估或指定两次评估对比时调用对应工具。
- 需要更改规则时调用 update_rule，可先 list_rules；询问规则触发次数排行时直接调用 get_rule_statistics。向用户说明规则时使用中文名称和具体判定内容，不要只写规则 ID。单条记录可能触发多条规则。
- 普通查看历史任务、统计或记录无需请求报告；只有用户明确要求基于历史结果新建报告时才调用 request_report。
不要向用户输出 SHA 哈希或内部版本指纹，只使用工具给出的易读版本名。
支持 GFM Markdown：标题、列表、表格、代码、链接；不支持原始 HTML。
不要编造 Hadoop 结果。准确性只验证已知取值域，时效性按历史参照。规则: {rules_context}。已有任务：{job_context}。
追问其他旧结果时可先调用 list_jobs 或 get_job。
请尽量输出简洁的信息。始终使用中文回答。")})];
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
