# 迭代一规则、评分与工具设计

## 规则目录

每个规则有稳定 ID、分类、名称、说明、开关和可选数值参数。三条基础解析约束必须开启，其余规则可独立关闭。清洗规则关闭只影响清洗是否处置该问题；五维评分使用固定评价口径并保存规则版本，避免通过关掉清洗检查直接抬高质量分数。数值区间为闭区间。

| 分类 | 规则 ID | 默认 | 判定或处理 |
| --- | --- | --- | --- |
| 基础解析 | `schema_delimiter` | 必开 | 完全没有 `::` 的记录报告分隔符错误；字段数量另行报告 |
| 基础解析 | `schema_fields` | 必开 | users/movies/ratings 分别须有 5/3/4 字段 |
| 基础解析 | `required_values` | 必开 | 所有字段必须有非空白内容 |
| 规范化 | `trim_fields` | 开 | 去除字段两端空白，记规范化动作 |
| 规范化 | `normalize_genres` | 开 | 去掉电影类型重复项并修剪类型名 |
| 用户字段 | `user_id_positive` | 开 | UserID 为规范正整数 |
| 用户字段 | `user_id_range` | 关 | 可调闭区间，预设 1–6040，仅检查 users.dat 的 UserID |
| 用户字段 | `user_gender_domain` | 开 | Gender 为 F 或 M |
| 用户字段 | `user_age_domain` | 开 | Age 为 1/18/25/35/45/50/56 |
| 用户字段 | `user_occupation_range` | 开 | Occupation 在可调范围，默认 0–20 |
| 用户字段 | `user_zip_format` | 关 | 可选检查五位或五位加四位邮编，仍按字符串存储 |
| 电影字段 | `movie_id_positive` | 开 | MovieID 为规范正整数 |
| 电影字段 | `movie_id_range` | 关 | 可调闭区间，预设 1–3952，仅检查 movies.dat 的 MovieID；不要求编号连续 |
| 电影字段 | `movie_genre_domain` | 开 | Genres 属于已知 18 个类型 |
| 电影字段 | `movie_genre_unique` | 开 | 单行电影类型无重复；规范化先去重时不再触发移除 |
| 电影字段 | `movie_title_year` | 关 | 可选要求标题以 `(YYYY)` 结尾 |
| 电影字段 | `movie_title_year_max` | 关 | 对已有 `(YYYY)` 后缀的年份检查可调上限，预设 2003；缺失后缀由上一条规则判断 |
| 评分字段 | `rating_user_id_positive` | 开 | 评分 UserID 为规范正整数 |
| 评分字段 | `rating_movie_id_positive` | 开 | 评分 MovieID 为规范正整数 |
| 评分字段 | `rating_integer` | 开 | Rating 为规范正整数；关闭后仍受范围检查 |
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
| 质量评估 | `score_up_to_date` | 开 | 历史时间适配性维度是否展示 |
| 质量评估 | `score_time_window` | 开 | 历史采集时间闭区间，默认 2000-04-01 至 2003-02-28 UTC |

执行顺序为：解析与必填检查 → 字段与类型规范化 → 字段合法性 → 业务键去重及跨表一致性。未通过前面检查的记录不再累计去重或关联错误。单个 `:` 可以是片名中的合法字符，所以分隔符规则只在整行完全没有 `::` 时单独报告；有部分 `::` 但结构错误时由字段数量规则报告。必填规则已经覆盖 Title、Zip-code 和其他字段的非空约束，因此不重复登记单独的“片名非空”“邮编非空”开关。ZIP 永远是字符串，保留前导零。ID 范围检查为可选源表约束，ratings.dat 的关联由“评分用户/电影存在”规则负责。

MovieLens README 明确给出 1–6040 的 UserID 范围、1–3952 的 MovieID 范围、五颗星整数评分、七类年龄、21 类职业及 18 个电影类型，但其声明并不保证数据现实正确，且 MovieID 不连续。片名年份上限、邮编模式和历史时间窗口属于本项目的可配置校验口径，并非 README 保证。电影类型、邮编和年份检查均不能证明信息在现实中真实。`actions.ndjson` 的一条移除记录可带多个规则 ID；规范化和移除分开统计，同一条不能把规则触发次数直接相加当作移除总量。没有数据修补或缺失值推断。Agent 和用户都能改变当前对话规则；UI 在 Agent 运行时只读，防止任务过程中快照变化。设置页保存默认规则，供新建对话复制。

## 五维评分

评分作业独立于清洗作业，在 Hadoop Streaming reducer 内执行。使用固定 `quality-spec-v2`，清洗规则开关不改变评分定义。令 `G[d,t]` 为维度 d 在表 t 的合格行数、`E[d,t]` 为可评估行数、`N[t]` 为该表实际行数：

```text
Q[d,t] = 100 × G[d,t] / E[d,t]
A[d,t] = 100 × E[d,t] / N[t]
Y[d,t] = 100 × G[d,t,清洗后] / N[t,原始]
Q[d]   = Σ Q[d,t] / 适用且可评估的表数
```

分母为零时记 N/A。三表适用的维度按表等权汇总；时间维度仅适用 ratings。Q 是质量分，A 是评估覆盖率，Y 是有效留存率；对比须采用同一原始数据版本、评分规范版本、算法版本及历史时间窗口。清洗规则允许不同，才可比较不同清洗方案；评分维度开关关闭时不显示分数和变化。

| 维度 | 分母 | 合格条件 |
| --- | --- | --- |
| Accurate | 结构完整行 | 规范整数、官方 ID 范围、性别/年龄/职业/评分/电影类型取值有效；不代表现实真实 |
| Complete | 三表所有行 | 字段数正确、必填非空白；用户还须有至少 20 部不同电影的可信评分 |
| Unique | 业务键可解析行 | 用户/电影 ID 或评分事件键不重复 |
| Consistent | 业务键可解析行 | 同 ID 属性无冲突、评分事件无不同值、评分跨表引用唯一有效 |
| Up-to-date | 结构完整的 ratings 行 | 规范 Unix 秒位于配置的历史采集时间窗口 |

令 `n[u] = |{MovieID : 用户 u 的可信评分引用有效电影}|`，`C20 = 100 × #{u : n[u] ≥ 20} / 有效且唯一的用户数`。不达标用户仅影响完整性评分，清洗不会因为这项指标删除他们。`T1/T2` 分别为各次评估历史窗口内时间戳排序后的 70%/85% 位置；后续迭代的训练、验证、测试边界需绑定同一个清洗数据版本。当前尚未实现迭代二，`T1/T2` 只记录，不用于建模。旧评分任务不按 v2 重解释，也不能与 v2 对比。

## 作业与版本

共享源文件在设置页按 UUID 上传到 `{hdfs_root}/sources/{file_id}/`，只有 HDFS 上传成功才切换活跃版本。作业按 UUID 写入 `{hdfs_root}/jobs/{job_id}/`。清洗结果以 UTF-8 写出；原始文件按 ISO-8859-1 解码，文件无表头，使用 `::` 分隔。内部仍以三份输入、三份输出、规则 JSON 和 `govern.py` 的 SHA-256 校验版本兼容性；对用户展示“原始数据 vN”“清洗数据 vN”“规则方案 vN”以及应用语义版本号。原始数据在三份文件齐备后按替换次数递增，清洗数据按该对话的清洗任务序号递增，规则方案按该对话的实际规则变更递增。旧状态从 v1 开始迁移。

清洗记录查看器显示每条记录来自 `users.dat`、`movies.dat` 或 `ratings.dat`，并支持规则、表和动作筛选；清洗卡片不重复展示样例。删除对话会清理其所有任务的本地目录，以及清洗和评估任务的 HDFS 目录和可定位的 SSH 临时目录，然后删除消息与任务元数据；清理失败保留元数据供重试。共享源文件和已经下载的副本不属于对话删除范围。新任务记录创建时的 Hadoop 连接与作业目录；旧任务没有连接快照时按当前连接设置尝试清理。

DeepSeek 使用官方 Chat Completions 的 `tools` / `tool_calls`，流式消息与工具调用按时间线保存。工具接口：`list_sources`、`list_rules`、`update_rule`、`run_cleaning`、`assess_quality`、`compare_assessments`、`run_pipeline`、`get_cleaning_records`、`get_rule_statistics`、`list_jobs`、`get_job`、`preview_data`、`request_report`。`get_rule_statistics` 直接返回单次清洗的规则触发排行、中文名称和判定内容；逐条记录仍由 `get_cleaning_records` 分页读取。`run_pipeline` 顺序生成清洗、原始评估、清洗后评估、对比子任务；独立评估不显示结果卡片。自动报告 hook 只汇总本轮新产生的清洗和对比任务；读取历史任务不会触发报告，明确请求历史报告时由 `request_report` 指定任务。报告还可在中断后汇总已经完成的步骤。只有清洗任务产生可下载的数据文件，报告作为独立 Markdown 附件下载；各轮附件位于对应轮次消息的下方。
