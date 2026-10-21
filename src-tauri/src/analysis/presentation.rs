use crate::model::Job;
use serde_json::Value;
fn label(k: &str) -> &str {
    match k {
        "preference_loss" => "未来高分类型预测损失",
        "event_coverage" => "未来事件分组覆盖率",
        "evaluated_events" => "评价事件数",
        "reduction_seconds" => "降维用时（秒）",
        "clustering_seconds" => "聚类对照用时（秒）",
        "peak_rss_bytes" => "进程峰值内存（字节）",
        "original_matrix_bytes" => "原始矩阵（字节）",
        "reduced_matrix_bytes" => "降维矩阵（字节）",
        "model_bytes" => "模型（字节）",
        "silhouette_samples" => "轮廓系数抽样数",
        "C" => "正则强度倒数",
        "seed" => "随机种子",
        "threshold" => "高分判定阈值",
        "max_depth" => "最大树深",
        "min_samples_leaf" => "叶节点最少样本",
        "ccp_alpha" => "剪枝强度",
        "criterion" => "划分准则",
        "n_clusters" => "群体数量",
        "n_init" => "初始化次数",
        "eps" => "邻域半径",
        "min_samples" => "密度最少样本",
        "min_history" => "最低历史评分数",
        "dimensions" => "输出维数",
        "information_threshold" => "信息保留目标",
        "row_samples" => "采样用户数",
        "clustering_algorithm" => "对照聚类算法",
        "clustering_parameters" => "对照聚类参数",
        "precision" => "高分预测准确率",
        "recall" => "高分识别率",
        "f1" => "综合表现 F1",
        "roc_auc" => "ROC 曲线面积",
        "pr_auc" => "精确率召回率曲线面积",
        "silhouette" => "轮廓系数",
        "davies_bouldin" => "群体分离指数",
        "coverage" => "有效分组覆盖率",
        "noise_rate" => "噪声占比",
        "clusters" => "群体数量",
        "agreement" => "分组一致性",
        "retained" => "信息保留率",
        "relative_error" => "相对重建误差",
        _ => "",
    }
}
fn show(v: &Value) -> String {
    match v {
        Value::Null => "无法计算".into(),
        Value::Number(n) if n.is_f64() => format!("{:.4}", n.as_f64().unwrap_or_default()),
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}
fn table(out: &mut String, title: &str, v: &Value) {
    if let Some(obj) = v.as_object() {
        out.push_str(&format!(
            "\n### {title}\n\n| 指标 | 结果 |\n| --- | --- |\n"
        ));
        for (k, v) in obj {
            if v.is_object() || label(k).is_empty() {
                continue;
            }
            out.push_str(&format!(
                "| {} | {} |\n",
                label(k),
                if title.contains("配置") || title.contains("参数") {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                } else {
                    show(v)
                }
            ));
        }
    }
}
pub fn report_section(job: &Job) -> String {
    let Some(v) = &job.result else {
        return String::new();
    };
    let mut out = format!(
        "\n## {}：{}\n\n数据：{}。规则：{}。\n\n训练截止：{}；验证截止：{}；其后为最终测试期。\n",
        show(&v["algorithm"]),
        show(&v["version"]),
        show(&v["data_version"]),
        show(&v["rule_version"]),
        date(&v["split"]["train_end"]),
        date(&v["split"]["validation_end"])
    );
    match v["task"].as_str() {
        Some("classification") => {
            if let Some(t) = v.get("target") {
                out.push_str(&format!(
                    "\n目标用户：{}；电影：{}；预测{}；高分概率：{}。\n",
                    show(&t["user_id"]),
                    show(&t["movie_id"]),
                    if t["positive"] == true {
                        "高分"
                    } else {
                        "非高分"
                    },
                    show(&t["probability"])
                ));
            }
            table(&mut out, "验证期指标", &v["metrics"]["validation"]);
            table(&mut out, "测试期指标", &v["metrics"]["test"]);
        }
        Some("clustering") => {
            table(&mut out, "群体指标", &v["metrics"]);
            table(&mut out, "验证期指标", &v["temporal_metrics"]["validation"]);
            table(&mut out, "测试期指标", &v["temporal_metrics"]["test"]);
            out.push_str("\n### 群体摘要\n");
            for g in v["groups"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "\n群体 {}：{} 人；主要偏好 {}；代表内容 {}。\n",
                    g["group"].as_i64().unwrap_or(0).to_string(),
                    show(&g["size"]),
                    g["genres"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x["name"].as_str())
                        .collect::<Vec<_>>()
                        .join("、"),
                    g["movies"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x["title"].as_str())
                        .collect::<Vec<_>>()
                        .join("、")
                ));
            }
        }
        Some("reduction") => {
            out.push_str(&format!(
                "\n原始维数：{}；实际维数：{}。\n",
                show(&v["original_dimensions"]),
                show(&v["dimensions"])
            ));
            table(&mut out, "信息保留", &v["information"]);
            table(&mut out, "原始空间", &v["comparison"][0]);
            table(&mut out, "降维空间", &v["comparison"][1]);
            table(
                &mut out,
                "共同样本：原始分组",
                &v["reference_metrics"]["original"],
            );
            table(
                &mut out,
                "共同样本：降维后分组",
                &v["reference_metrics"]["reduced"],
            );
            table(&mut out, "分组一致性", &v["reference_metrics"]);
            table(&mut out, "资源开销", &v["resources"]);
        }
        _ => (),
    }
    table(&mut out, "实际配置", &v["parameters"]);
    table(
        &mut out,
        "对照聚类参数",
        &v["parameters"]["clustering_parameters"],
    );
    for note in [
        &v["metric_note"],
        &v["metric_notes"]["validation"],
        &v["metric_notes"]["test"],
    ] {
        if let Some(s) = note.as_str() {
            out.push_str(&format!("\n{s}\n"));
        }
    }
    out.push_str("\n### 适用限制\n");
    for l in v["limitations"].as_array().into_iter().flatten() {
        out.push_str(&format!("\n{}\n", show(l)));
    }
    out
}
fn date(v: &Value) -> String {
    v.as_i64()
        .and_then(|n| chrono::DateTime::from_timestamp(n, 0))
        .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "未登记".into())
}
