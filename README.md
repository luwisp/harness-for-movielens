# MovieLens 数据治理 Agent · 迭代一

Tauri 2 + React + Rust + Hadoop Streaming + DeepSeek function calling。三个 MovieLens `.dat` 文件在设置页初始化到 HDFS，作为所有对话共享的数据源。Agent 可以逐条调整规则，独立调用清洗、五维评估、对比，或调用顺序执行的统一流程。已完成的清洗与对比任务自动汇总成可阅读和下载的 Markdown 报告。

## 启动

需要 Node.js、Rust/Tauri 2 构建依赖、Hadoop 客户端、Hadoop Streaming jar；运行 MapReduce 的节点需要 Python 3。支持 Windows 本地 Hadoop 客户端和通过 SSH 使用远端 Hadoop 主机。SSH 模式需要客户端 `ssh`、密钥非交互登录，以及远端 Hadoop/Python/Streaming jar。

```sh
npm install
npm run tauri dev
```

在设置页先保存 DeepSeek API Key、Hadoop 连接配置，再分别上传 `users.dat`、`movies.dat`、`ratings.dat`。上传采用分块传输，写入 HDFS 成功后才激活新版本；设置页可刷新检查三个文件的实际 HDFS 存在状态。替换某文件后，后续任务使用新版本，已有任务的输入路径和哈希保留。首次使用建议运行 `hadoop fs -ls /` 检查集群连通性。若没有本地 Hadoop，选择 SSH 模式并填写远端命令和 jar 路径。

## CI 与发布

推送到 `main` 或提交 PR 时，[CI](.github/workflows/ci.yml) 会在 Ubuntu 22.04 检查前端构建、Rust 测试和 Python 清洗测试。推送 `v<版本号>` 标签时，[Release](.github/workflows/release.yml) 会检查标签与 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 的版本一致，然后构建 Windows x64 NSIS `setup.exe`、Linux x64 AppImage 和 `.deb`。两个平台都成功后才会公开 GitHub Release；失败时草稿保留供排查。

首次发布可直接使用当前 `1.0.0` 版本。后续先同步修改上述三个版本号，并更新 `package-lock.json` 与 `src-tauri/Cargo.lock`，再提交并推送标签，例如：

```sh
node scripts/verify-release-version.mjs v1.0.0
git tag v1.0.0
git push origin v1.0.0
```

标签应指向已包含工作流的提交。Windows `.exe` 是安装程序；Linux AppImage 下载后需添加执行权限。发布包只包含桌面应用，不内置 Hadoop、Python、MovieLens 数据或 DeepSeek API Key；使用本地 Hadoop 模式时仍需安装 Hadoop，远端模式需配置 SSH。Windows 安装包目前未签名，可能出现系统安全提示。

随后新建对话，输入“清洗共享数据并比较清洗前后的五维质量”。Agent 正常选择 `run_pipeline`，界面展示清洗和对比作业卡片；中间的评估以简短状态行显示运行进度，不显示独立结果卡片。单独请求五维评估时，由 Agent 在回复中解释结果。输入框左侧设置菜单可打开当前对话的清洗规则弹窗；对比卡片的“评分规则”会打开五维评分口径窗口。Agent 运行期间规则弹窗只读。Agent 用 `update_rule` 修改一条规则时，变更会出现在时间线上并可点击定位。流式回答可随时中断。

## 输出

- 清洗任务可下载三份 UTF-8 `.dat` 和 `actions.ndjson`；逐条动作保留稳定规则 ID，界面和报告同时给出中文规则名与判定内容。
- “查看清洗记录与筛选”逐条标出来源文件和表，可按规则、表、动作筛选；清洗卡片只保留规则触发次数和记录入口。
- 独立评估结果交给 Agent 解释，不生成结果卡片或报告章节；对比卡片显示五维清洗前、清洗后和百分点变化。
- 对话报告 `report.md` 自动汇总本轮已完成的清洗和对比任务；在对话中阅读。每轮清洗数据与报告的下载入口显示在该轮消息下方。

下载会把所选文件复制到系统 Downloads 目录。内部继续使用 SHA-256 检查数据、规则与算法的一致性，界面和 Markdown 报告使用“原始数据 vN / 清洗数据 vN / 规则方案 vN”等易读名称。Agent 可以通过 `get_rule_statistics` 一次读取最近或指定清洗任务的全部规则触发次数排行；`get_cleaning_records` 用于按规则、表和动作筛选逐条日志。普通历史查询不生成报告；用户明确要求重做历史报告时才使用 `request_report`。评分准确性只验证可观察的格式与取值域，不能证明现实真实性；移除异常记录会改善分数，但不等于已修复原始数据。评分和规则设计见 [迭代一设计](docs/iteration1-design.md)。

侧边栏可删除对话。确认后，应用先删除该对话清洗和评估任务的 HDFS 目录、SSH 临时文件及本地任务目录，再删除对话消息和任务记录；Hadoop 清理失败时保留对话，便于重试。设置页共享的三份源文件不会随对话删除；已复制到 Downloads 的文件由用户自行管理。新任务保存其 Hadoop 连接与 HDFS 目录，以便修改设置后仍能清理；旧任务缺少连接快照时使用当前 Hadoop 设置。

## 工程结构

| 位置 | 责任 |
| --- | --- |
| `src-tauri/src/rules.rs`、`resources/rule_catalog.json` | 规则目录、默认值、参数校验、版本 |
| `src-tauri/resources/govern.py` | Hadoop Streaming 清洗器与独立评分器 |
| `src-tauri/src/hadoop.rs` | 本地/SSH Hadoop、文件初始化、任务状态与结果 |
| `src-tauri/src/agent.rs` | DeepSeek 原生 function calling、流式输出、工具编排 |
| `src-tauri/src/report.rs` | 每轮自动 Markdown 报告 |
| `src-tauri/src/store.rs` | 对话、共享文件、规则和任务持久化 |
| `src/components/` | 独立页面组件和时间线卡片 |

应用状态和 API Key 当前存于 Tauri 应用数据目录 `state.json`，任务产物存于 `jobs/{job_id}/`。尚未接入系统凭据保险箱，因此应在可信本机使用。任务中断会终止 DeepSeek 流或 Hadoop 客户端，并尽力杀掉可识别的 Hadoop 作业。应用重启时未结束任务会标记失败，避免显示虚假的成功结果。
