use serde_json::{json, Value};
pub fn definitions() -> Vec<Value> {
    let mut out = vec![];
    for (name, title, params) in [
        (
            "logistic",
            "逻辑回归",
            json!({"C":{"type":"number","exclusiveMinimum":0},"threshold":{"type":"number","minimum":0,"maximum":1}}),
        ),
        (
            "tree",
            "CART 决策树",
            json!({"max_depth":{"type":"integer","minimum":1},"min_samples_leaf":{"type":"integer","minimum":1},"ccp_alpha":{"type":"number","minimum":0},"threshold":{"type":"number","minimum":0,"maximum":1}}),
        ),
        (
            "kmeans",
            "KMeans",
            json!({"n_clusters":{"type":"integer","minimum":2},"n_init":{"type":"integer","minimum":1},"min_history":{"type":"integer","minimum":1}}),
        ),
        (
            "dbscan",
            "DBSCAN",
            json!({"eps":{"type":"number","exclusiveMinimum":0},"min_samples":{"type":"integer","minimum":2},"min_history":{"type":"integer","minimum":1}}),
        ),
        (
            "svd",
            "截断 SVD",
            json!({"dimensions":{"type":"integer","minimum":1},"information_threshold":{"type":"number","exclusiveMinimum":0,"maximum":1},"min_history":{"type":"integer","minimum":1}}),
        ),
        (
            "cur",
            "CUR",
            json!({"dimensions":{"type":"integer","minimum":1},"information_threshold":{"type":"number","exclusiveMinimum":0,"maximum":1},"row_samples":{"type":"integer","minimum":1},"min_history":{"type":"integer","minimum":1}}),
        ),
    ] {
        let mut parameters = params;
        parameters["seed"] = json!({"type":"integer","minimum":0});
        let mut properties = json!({"dataset_id":{"type":"string","description":"list_analysis_assets 返回的已登记数据集；省略时使用本对话最近清洗和匹配的清洗后评估"},"cleaning_job_id":{"type":"string"},"model_id":{"type":"string","description":"已登记模型，不可编造"},"mode":{"type":"string","enum":["fit","predict","lookup"]},"user_id":{"type":"integer","minimum":1},"movie_id":{"type":"integer","minimum":1},"user_ids":{"type":"array","items":{"type":"integer","minimum":1},"maxItems":10000},"parameters":{"type":"object","properties":parameters,"additionalProperties":false}});
        if name == "svd" || name == "cur" {
            properties["clustering_algorithm"] =
                json!({"type":"string","enum":["kmeans","dbscan"]});
            properties["clustering_parameters"] = json!({"type":"object","properties":{"n_clusters":{"type":"integer","minimum":2},"eps":{"type":"number","exclusiveMinimum":0},"min_samples":{"type":"integer","minimum":2},"n_init":{"type":"integer","minimum":1}},"additionalProperties":false});
        }
        let display = if name == "logistic" || name == "tree" {
            "目标用户与电影、预测类别及高分概率、模型版本、算法参数、验证与测试指标、时间切分与适用限制"
        } else if name == "kmeans" || name == "dbscan" {
            "用户群体归属、噪声标记、群体摘要与代表电影、历史不足说明、实际参数、评价指标与版本"
        } else {
            "信息保留、实际维数、二维散点图及坐标、原始与降维聚类对照、实际参数、版本、资源开销与局限"
        };
        out.push(json!({"type":"function","function":{"name":format!("analyze_{name}"),"description":format!("独立执行{title}分析或读取已登记结果。只使用匹配清洗版本和清洗后评估的 T1/T2。分类须同时给 user_id/movie_id；predict/lookup 只读现有模型，缺失时明确失败。前端显示进度，完成后展示{display}；本轮新增分析结果自动写入报告，历史读取不生成报告。必要结果文件在本轮下方下载。不要逐项复述卡片数字，解释差异、证据和局限。"),"parameters":{"type":"object","properties":properties,"additionalProperties":false}}}));
    }
    out.push(json!({"type":"function","function":{"name":"compare_analysis","description":"用户未指定算法时的标准入口：classification 顺序调用逻辑回归与 CART；clustering 调用 KMeans 与 DBSCAN；reduction 调用截断 SVD 与 CUR。两种结果使用同一数据和时间切分。前端按顺序展示两个独立结果及所需指标，可直接对照；报告自动汇总实际成功的新结果。不要从某个失败工具推测结果。分类须给 user_id/movie_id。","parameters":{"type":"object","properties":{"task":{"type":"string","enum":["classification","clustering","reduction"]},"dataset_id":{"type":"string"},"cleaning_job_id":{"type":"string"},"user_id":{"type":"integer","minimum":1},"movie_id":{"type":"integer","minimum":1},"user_ids":{"type":"array","items":{"type":"integer","minimum":1}},"parameters":{"type":"object","description":"两种算法共用的 seed/min_history；降维可用 dimensions 或 information_threshold，不能同时给"}},"required":["task"]}}}));
    out.push(json!({"type":"function","function":{"name":"list_analysis_assets","description":"查询已登记的数据集和六种算法模型版本，用于复用及后续迭代。前端仅展示调用记录，需要说明相关人类可读版本。此操作不生成报告。","parameters":{"type":"object","properties":{}}}}));
    out.push(json!({"type":"function","function":{"name":"get_analysis_rows","description":"分页读取实际预测、画像、群体或降维坐标。前端仅展示调用记录，需回答用户追问；历史查询不生成报告。","parameters":{"type":"object","properties":{"job_id":{"type":"string"},"table":{"type":"string","enum":["predictions","profiles","groups","memberships","coordinates","comparison"]},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["job_id","table"]}}}));
    out
}
