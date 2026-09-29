# 迭代一规则、评分与工具设计

## 规则目录

每个规则有稳定 ID、分类、名称、说明、开关和可选数值参数。除前两条基础解析约束外，所有规则都可独立关闭。清洗规则关闭只影响清洗是否处置该问题；五维评分使用固定评价口径并保存规则版本，避免通过关掉清洗检查直接抬高质量分数。数值区间为闭区间。

| 分类 | 规则 ID | 默认 | 判定或处理 |
| --- | --- | --- | --- |
| 基础解析 | `schema_fields` | 必开 | users/movies/ratings 分别须有 5/3/4 字段 |
| 基础解析 | `required_values` | 必开 | 所有字段必须有非空白内容 |
| 规范化 | `trim_fields` | 开 | 去除字段两端空白，记规范化动作 |
| 规范化 | `normalize_genres` | 开 | 去掉电影类型重复项并修剪类型名 |
| 用户字段 | `user_id_positive` | 开 | UserID 为规范正整数 |
| 用户字段 | `user_gender_domain` | 开 | Gender 为 F 或 M |
| 用户字段 | `user_age_domain` | 开 | Age 为 1/18/25/35/45/50/56 |
| 用户字段 | `user_occupation_range` | 开 | Occupation 在可调范围，默认 0–20 |
| 用户字段 | `user_zip_format` | 关 | 可选检查五位或五位加四位邮编，仍按字符串存储 |
| 电影字段 | `movie_id_positive` | 开 | MovieID 为规范正整数 |
| 电影字段 | `movie_genre_domain` | 开 | Genres 属于已知 18 个类型 |
| 电影字段 | `movie_genre_unique` | 开 | 单行电影类型无重复；规范化先去重时不再触发移除 |
| 电影字段 | `movie_title_year` | 关 | 可选要求标题以 `(YYYY)` 结尾 |
| 评分字段 | `rating_user_id_positive` | 开 | 评分 UserID 为规范正整数 |
| 评分字段 | `rating_movie_id_positive` | 开 | 评分 MovieID 为规范正整数 |
| 评分字段 | `rating_integer` | 开 | Rating 为规范整数；关闭后仍受范围检查 |
| 评分字段 | `rating_range` | 开 | Rating 在可调范围，默认 1–5 |
| 评分字段 | `timestamp_integer` | 开 | Timestamp 为规范 Unix 秒整数 |
| 评分字段 | `timestamp_range` | 开 | Timestamp 在可调历史范围，默认 2000-01-01 至 2003-02-01 UTC |
| 去重 | `unique_users` | 开 | 按 UserID 去重，保留确定排序的首条 |
| 去重 | `unique_movies` | 开 | 按 MovieID 去重，保留确定排序的首条 |
| 去重 | `unique_ratings` | 开 | 按 UserID、MovieID、Timestamp 去重 |
| 关联一致性 | `consistent_users` | 开 | 相同 UserID 属性不能冲突 |
| 关联一致性 | `consistent_movies` | 开 | 相同 MovieID 标题和类型不能冲突 |
| 关联一致性 | `rating_user_exists` | 开 | 评分引用的用户须存在于保留用户表 |
| 关联一致性 | `rating_movie_exists` | 开 | 评分引用的电影须存在于保留电影表 |
| 质量评估 | `score_accurate` | 开 | 准确性维度是否展示 |
| 质量评估 | `score_complete` | 开 | 完整性维度是否展示 |
| 质量评估 | `score_unique` | 开 | 唯一性维度是否展示 |
| 质量评估 | `score_consistent` | 开 | 一致性维度是否展示 |
| 质量评估 | `score_freshness` | 开 | 时效性维度及窗口天数，默认 365 天 |
| 质量评估 | `score_reference` | 开 | 参照 Unix 秒；默认 0 为原始有效评分的最大时间 |

电影类型、邮编和年份检查均不能证明信息在现实中真实。`actions.ndjson` 的一条移除记录可带多个规则 ID；规范化和移除分开统计，同一条不能把规则触发次数直接相加当作移除总量。没有数据修补或缺失值推断。Agent 和用户都能改变当前对话规则；UI 在 Agent 运行时只读，防止任务过程中快照变化。设置页保存默认规则，供新建对话复制。

## 五维评分

评分作业独立于清洗作业，在 Hadoop Streaming reducer 内执行。每维为 `100 × good / eligible`，没有适用行时为 N/A。清洗前后使用同一规则版本和同一个历史参照时间。评分维度开关关闭时不显示分数和变化。清洗开关关闭不改变评分定义，因而保留下来的问题会如实影响评估。

| 维度 | 分母 | 合格条件 |
| --- | --- | --- |
| Accurate | 三表所有行 | 字段结构正确、必填非空，且字段满足默认已知格式和目录（可选邮编与片名年份检查不计入）；不代表现实真实 |
| Complete | 三表所有行 | 字段数正确且必填字段非空白 |
| Unique | 三表所有行 | 用户/电影 ID 或评分事件键不重复 |
| Consistent | 三表所有行 | 相同 ID 属性无冲突、电影类型无重复、评分跨表引用存在 |
| Up-to-date | ratings 所有行 | 时间戳位于 `[reference - days×86400, reference]` |

评分默认参照从**原始数据中落在配置历史范围的有效时间戳**取最大值；统一流程把这个参照传给清洗后评估，保持可比。独立对比工具拒绝规则版本、算法版本或时效参照不一致的两次评估。`T1/T2` 分别为各次评估有效评分时间戳排序后的 70%/85% 位置；后续迭代的训练、验证、测试边界需绑定同一个清洗数据版本。当前尚未实现迭代二，`T1/T2` 只记录，不用于建模。

## 作业与版本

共享源文件在设置页按 UUID 上传到 `{hdfs_root}/sources/{file_id}/`，只有 HDFS 上传成功才切换活跃版本。作业按 UUID 写入 `{hdfs_root}/jobs/{job_id}/`。清洗结果以 UTF-8 写出；原始文件按 ISO-8859-1 解码，文件无表头，使用 `::` 分隔。内部仍以三份输入、三份输出、规则 JSON 和 `govern.py` 的 SHA-256 校验版本兼容性；对用户展示“原始数据 vN”“清洗数据 vN”“规则方案 vN”以及应用语义版本号。原始数据在三份文件齐备后按替换次数递增，清洗数据按该对话的清洗任务序号递增，规则方案按该对话的实际规则变更递增。旧状态从 v1 开始迁移。

清洗记录查看器显示每条记录来自 `users.dat`、`movies.dat` 或 `ratings.dat`，并支持规则、表和动作筛选；清洗卡片不重复展示样例。删除对话会清理其所有任务的本地目录，以及清洗和评估任务的 HDFS 目录和可定位的 SSH 临时目录，然后删除消息与任务元数据；清理失败保留元数据供重试。共享源文件和已经下载的副本不属于对话删除范围。新任务记录创建时的 Hadoop 连接与作业目录；旧任务没有连接快照时按当前连接设置尝试清理。

DeepSeek 使用官方 Chat Completions 的 `tools` / `tool_calls`，流式消息与工具调用按时间线保存。工具接口：`list_sources`、`list_rules`、`update_rule`、`run_cleaning`、`assess_quality`、`compare_assessments`、`run_pipeline`、`get_cleaning_records`、`get_rule_statistics`、`list_jobs`、`get_job`、`preview_data`、`request_report`。`get_rule_statistics` 直接返回单次清洗的规则触发排行、中文名称和判定内容；逐条记录仍由 `get_cleaning_records` 分页读取。`run_pipeline` 顺序生成清洗、原始评估、清洗后评估、对比子任务；独立评估不显示结果卡片。自动报告 hook 只汇总本轮新产生的清洗和对比任务；读取历史任务不会触发报告，明确请求历史报告时由 `request_report` 指定任务。报告还可在中断后汇总已经完成的步骤。只有清洗任务产生可下载的数据文件，报告作为独立 Markdown 附件下载；各轮附件位于对应轮次消息的下方。


