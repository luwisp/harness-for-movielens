# MovieLens 数据治理 Agent · 迭代一

基于Tauri 2 + React + Rust + Hadoop Streaming + DeepSeek function calling的MovieLens数据治理agent

支持远程ssh连接hadoop或本地hadoop

## 启动

需要 Node.js、Rust，有hadoop和python3的主机（本地或服务器）

如果使用远程服务器需要ssh，并配置免密登录

### 运行
```sh
npm install
npm run tauri dev
```
### 配置
打开设置界面：
- api： 请填写一个deepseek的api key, 别的模型暂时未测试
- hadoop连接：可以选择远程ssh连接hadoop或本地hadoop
  请根据提示填写Hadoop Streaming jar、python3、hadoop路径或命令
- movieLens数据集：在设置里上传movieLens数据集的三个文件


## 使用

在设置完毕后，可新建对话，在对话内与agent沟通

agent拥有如下能力：数据清洗、五维评估、查看设置数据清洗规则、查看数据、生成报告、查看统计数据等等

另外：

- 清洗任务可下载三份 UTF-8 `.dat` 和 `actions.ndjson`；逐条动作保留稳定规则 ID，界面和报告同时给出中文规则名与判定内容。
- “查看清洗记录与筛选”逐条标出来源文件和表，可按规则、表、动作筛选；清洗卡片只保留规则触发次数和记录入口。
- 独立评估结果交给 Agent 解释，不生成结果卡片或报告章节；对比卡片显示五维清洗前、清洗后和百分点变化。
- 对话报告 `report.md` 自动汇总本轮已完成的清洗和对比任务；在对话中阅读。每轮清洗数据与报告的下载入口显示在该轮消息下方。

下载会把所选文件复制到系统 Downloads 目录。

聊天界面和 Markdown 报告使用“原始数据 vN / 清洗数据 vN / 规则方案 vN”等易读名称，当有变化时版本号会增加1,便于区别。

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

## 其他文档

具体规则和结果见`docs/iteration1-design.md`