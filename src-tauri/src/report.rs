use crate::{model::{Database, Job}, rules, store::AppState};
use serde_json::Value;
use std::{collections::HashSet, fs};

fn count(value: &Value) -> u64 { value.as_object().map(|m| m.values().filter_map(Value::as_u64).sum()).unwrap_or(0) }
fn score_check_label(id: &str) -> &str {
    match id {
        "schema_fields" => "字段数", "required_values" => "必填为空",
        "user_id_range" => "用户 ID 范围", "user_gender_domain" => "性别类别",
        "user_age_domain" => "年龄类别", "user_occupation_range" => "职业编码",
        "movie_id_range" => "电影 ID 范围", "movie_genre_domain" => "电影类型目录",
        "rating_user_id_range" => "评分用户 ID 范围", "rating_movie_id_range" => "评分电影 ID 范围",
        "rating_integer_1_5" => "评分须为 1–5 整数", "timestamp_integer" => "规范 Unix 秒",
        "historical_window" => "历史时间窗口", "duplicate_key" => "业务键重复",
        "attribute_conflict" => "同 ID 属性冲突", "event_rating_conflict" => "同事件评分冲突",
        "invalid_user_reference" => "用户引用无效", "invalid_movie_reference" => "电影引用无效",
        "cohort20_or_invalid_user" => "未达 20 部或用户无效", _ => id,
    }
}
pub fn humanize_legacy_report(body: &str) -> String {
    let mut output = String::new();
    let mut remaining = body;
    while let Some(index) = remaining.find("sha256:") {
        output.push_str(&remaining[..index]);
        let candidate = &remaining[index + 7..];
        if candidate.len() >= 64 && candidate.as_bytes()[..64].iter().all(|b| b.is_ascii_hexdigit()) {
            output.push_str("历史版本（编号未记录）");
            remaining = &candidate[64..];
        } else {
            output.push_str("sha256:");
            remaining = candidate;
        }
    }
    output.push_str(remaining);
    let mut output = output.replace("| 规则 ID |", "| 清洗规则 |");
    for spec in rules::catalog() {
        output = output.replace(&format!("`{}`", spec.id), &format!("{}：{}", spec.label, spec.description));
    }
    output
}
fn section(job: &Job) -> String {
    let result = match &job.result { Some(v) => v, None => return String::new() };
    let ordinal = if job.display_number == 0 { "历史任务".to_string() } else { format!("第 {} 次", job.display_number) };
    let mut out = format!("\n## {} · {}\n\n", match job.kind.as_str() {
        "cleaning" => "数据清洗", "comparison" => "评估对比", _ => "任务" }, ordinal);
    out.push_str(&format!("规则版本：{}。\n\n", result["rule_version_label"].as_str().unwrap_or("历史规则方案")));
    match job.kind.as_str() {
        "cleaning" => {
            let m = &result["metrics"]; let c = &m["counts"];
            out.push_str(&format!("原始记录：{}；保留：{}；移除：{}；规范化：{}。\n\n",
                count(&c["raw"]), count(&c["clean"]), count(&c["removed"]), count(&c["normalized"])));
            out.push_str(&format!("原始数据版本：`{}`；清洗数据版本：`{}`；算法版本：`{}`。\n\n",
                result["data_version_label"].as_str().unwrap_or("历史原始数据"),
                result["clean_data_version_label"].as_str().unwrap_or("历史清洗数据"),
                result["algorithm_version_label"].as_str().unwrap_or("历史清洗算法")));
            out.push_str("| 清洗规则 | 规则内容 | 触发移除次数 |\n|---|---|---:|\n");
            if let Some(map) = c["by_rule"].as_object() {
                let mut rows: Vec<_> = map.iter().collect();
                rows.sort_by(|a, b| b.1.as_u64().unwrap_or(0).cmp(&a.1.as_u64().unwrap_or(0)).then_with(|| a.0.cmp(b.0)));
                for (id, n) in rows {
                    let (label, description) = rules::describe(id);
                    out.push_str(&format!("| {} | {} | {} |\n", label, description, n.as_u64().unwrap_or(0)));
                }
            }
            out.push_str("\n所有逐条清洗动作见 `actions.ndjson`。同一记录可触发多条规则，因此各规则触发次数不可直接相加为移除条数。\n");
        },
        "comparison" => {
            out.push_str(&format!("对比任务：{} 与 {}。\n\n评分规范：v2。\n\n",
                result["before_label"].as_str().unwrap_or("前一次五维评估"),
                result["after_label"].as_str().unwrap_or("后一次五维评估")));
            out.push_str("`Q[d,t] = 100 × G[d,t] / E[d,t]`；`A[d,t] = 100 × E[d,t] / N[t]`；`Y[d,t] = 100 × G[d,t,清洗后] / N[t,原始]`。G 为合格行数，E 为可评估行数，N 为实际行数；三表等权平均，时间维度只适用 ratings。无分母时记 N/A。\n\n");
            out.push_str("`n[u] = |{MovieID : 用户 u 的可信评分引用唯一有效电影}|`；`C20 = 100 × #{u : n[u] ≥ 20} / 有效且唯一的用户数`。可信评分须为 1–5 规范整数、处于历史时间窗口且事件无冲突。低于 20 部的用户不会因此被清洗删除。\n\n");
            out.push_str("| 维度 | 清洗前 Q（%） | 清洗后 Q（%） | 变化（百分点） | 清洗后 A（%） | 有效留存 Y（%） |\n|---|---:|---:|---:|---:|---:|\n");
            for (id,label) in [("accurate","准确性"),("complete","完整性"),("unique","唯一性"),("consistent","一致性"),("up_to_date","时效性")] {
                let v = &result["dimensions"][id];
                let fmt = |x: &Value| x.as_f64().map(|n| format!("{n:.2}")).unwrap_or("N/A".into());
                out.push_str(&format!("| {label} | {} | {} | {} | {} | {} |\n", fmt(&v["before"]), fmt(&v["after"]), fmt(&v["change_pp"]), fmt(&v["coverage_after"]), fmt(&v["yield"])));
            }
            out.push_str("\n| 维度 | 数据表 | 清洗前 G/E | 清洗后 G/E | 原始行数 | 有效留存 Y（%） | 清洗后未通过的检查 |\n|---|---|---:|---:|---:|---:|---|\n");
            for (id,label) in [("accurate","准确性"),("complete","完整性"),("unique","唯一性"),("consistent","一致性"),("up_to_date","时效性")] {
                for (kind,table) in [("0","users"),("1","movies"),("2","ratings")] {
                    let v = &result["dimensions"][id]["tables"][kind];
                    if v.is_null() { continue; }
                    let n = |x: &Value| x.as_u64().unwrap_or(0);
                    let failures = v["after"]["failed_checks"].as_object().map(|checks| checks.iter()
                        .filter(|(_, n)| n.as_u64().unwrap_or(0) > 0)
                        .map(|(id, n)| format!("{} {}", score_check_label(id), n.as_u64().unwrap_or(0)))
                        .collect::<Vec<_>>().join("；")).filter(|s| !s.is_empty()).unwrap_or_else(|| "无".into());
                    out.push_str(&format!("| {label} | {table} | {}/{} | {}/{} | {} | {} | {} |\n",
                        n(&v["before"]["good"]), n(&v["before"]["eligible"]),
                        n(&v["after"]["good"]), n(&v["after"]["eligible"]),
                        n(&v["before"]["total"]),
                        v["yield"].as_f64().map(|x| format!("{x:.2}")).unwrap_or("N/A".into()), failures));
                }
            }
            let cohort = &result["cohort20_after"];
            out.push_str(&format!("\n20 部不同电影达标：{} / {} 名有效用户；未达标 {} 名。\n",
                cohort["qualified_users"].as_u64().unwrap_or(0), cohort["evaluable_users"].as_u64().unwrap_or(0),
                cohort["under_threshold_users"].as_u64().unwrap_or(0)));
        }, _ => {},
    }
    out
}

fn selected_jobs<'a>(db: &'a Database, conversation_id: &str, run_id: &str) -> Result<Vec<&'a Job>, String> {
    let conversation = db.conversations.iter().find(|c| c.id == conversation_id).ok_or("对话不存在")?;
    let mut selected: HashSet<String> = db.jobs.iter().filter(|j| j.conversation_id == conversation_id &&
        j.run_id.as_deref() == Some(run_id) && j.status == "completed")
        .map(|j| j.id.clone()).collect();
    for message in &conversation.messages {
        if message.run_id.as_deref() == Some(run_id) && message.kind == "report_request" {
            if let Some(id) = &message.job_id { selected.insert(id.clone()); }
        }
    }
    let parents: Vec<_> = selected.iter().filter(|id| db.jobs.iter().any(|j| &j.id == *id && j.kind == "pipeline")).cloned().collect();
    for id in parents {
        for child in db.jobs.iter().filter(|j| j.parent_id.as_deref() == Some(&id)) {
            selected.insert(child.id.clone());
        }
    }
    let jobs: Vec<_> = db.jobs.iter().filter(|j| j.conversation_id == conversation_id &&
        selected.contains(&j.id) && j.status == "completed" &&
        (j.kind == "cleaning" || (j.kind == "comparison" &&
            j.result.as_ref().is_some_and(|v| v["score_spec_version"] == "quality-spec-v2")))).collect();
    Ok(jobs)
}

pub fn generate(state: &AppState, conversation_id: &str, run_id: &str) -> Result<Option<Job>, String> {
    let db = state.snapshot()?;
    let jobs = selected_jobs(&db, conversation_id, run_id)?;
    if jobs.is_empty() { return Ok(None); }
    let report = crate::store::create_job(state, conversation_id, "report", None, Some(run_id.into()))?;
    let mut body = format!("# MovieLens 数据治理报告\n\n生成时间：{}。\n\n本报告自动汇总本轮工具调用涉及的已完成任务结果。\n", crate::model::now());
    for job in &jobs { body.push_str(&section(job)); }
    body.push_str("\n---\n\n评分使用任务记录的规则方案；清洗数据文件和本报告可在本轮消息下方下载。\n");
    let dir = state.root.join("jobs").join(&report.id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("report.md"), &body).map_err(|e| e.to_string())?;
    state.change(|db| { let job = db.jobs.iter_mut().find(|j| j.id == report.id).ok_or("报告任务不存在")?;
        job.status = "completed".into(); job.stage = "报告已生成".into();
        job.result = Some(serde_json::json!({"files":["report.md"],"source_job_ids":jobs.iter().map(|j| &j.id).collect::<Vec<_>>() }));
        job.updated_at = crate::model::now(); Ok(()) })?;
    Ok(Some(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Conversation, Message};
    #[test]
    fn referenced_pipeline_expands_completed_children() {
        let mut db = Database::default();
        db.conversations.push(Conversation { id: "c".into(), messages: vec![Message {
            run_id: Some("new-run".into()), kind: "report_request".into(), job_id: Some("p".into()), ..Message::default()
        }], ..Conversation::default() });
        db.jobs.push(Job { id: "p".into(), conversation_id: "c".into(), kind: "pipeline".into(),
            status: "completed".into(), ..Job::default() });
        db.jobs.push(Job { id: "clean".into(), conversation_id: "c".into(), kind: "cleaning".into(),
            parent_id: Some("p".into()), status: "completed".into(), ..Job::default() });
        db.jobs.push(Job { id: "failed".into(), conversation_id: "c".into(), kind: "assessment".into(),
            parent_id: Some("p".into()), status: "failed".into(), ..Job::default() });
        assert_eq!(selected_jobs(&db, "c", "new-run").unwrap().iter().map(|j| j.id.as_str()).collect::<Vec<_>>(), vec!["clean"]);
    }
    #[test]
    fn standalone_assessment_is_not_reported() {
        let mut db = Database::default();
        db.conversations.push(Conversation { id: "c".into(), ..Conversation::default() });
        db.jobs.push(Job { id: "a".into(), conversation_id: "c".into(), kind: "assessment".into(),
            run_id: Some("run".into()), status: "completed".into(), ..Job::default() });
        assert!(selected_jobs(&db, "c", "run").unwrap().is_empty());
    }
    #[test]
    fn reading_historical_cleaning_does_not_create_report() {
        let mut db = Database::default();
        db.conversations.push(Conversation { id: "c".into(), messages: vec![Message {
            run_id: Some("query".into()), kind: "tool_ref".into(), job_id: Some("old".into()), ..Message::default()
        }], ..Conversation::default() });
        db.jobs.push(Job { id: "old".into(), conversation_id: "c".into(), kind: "cleaning".into(),
            run_id: Some("old-run".into()), status: "completed".into(), ..Job::default() });
        assert!(selected_jobs(&db, "c", "query").unwrap().is_empty());
    }
    #[test]
    fn old_hashes_are_hidden_in_report_view() {
        let hash = format!("sha256:{}", "a".repeat(64));
        assert_eq!(humanize_legacy_report(&format!("版本 {hash}")), "版本 历史版本（编号未记录）");
        let old = humanize_legacy_report("| 规则 ID | 触发次数 |\n| `schema_fields` | 2 |");
        assert!(old.contains("字段数量：users/movies/ratings 必须分别有 5/3/4 个字段"));
    }
}
