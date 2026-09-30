import type { Job } from "../lib/types";

const dimensions = [
  {
    id: "accurate", name: "可验证正确性", tables: "users、movies、ratings",
    formula: "G₍准确,t₎ = 满足固定字段取值条件的可评估行数",
    detail: "完整行才可评估。用户 ID∈[1,6040]、性别∈{F,M}、年龄∈{1,18,25,35,45,50,56}、职业∈[0,20]；电影 ID∈[1,3952]、类型属于 18 种目录；评分用户/电影 ID 在上述范围，Rating∈{1,2,3,4,5}。整数必须采用规范十进制写法。时间戳归时间维度；现实真实性无法由数据集证明。",
  },
  {
    id: "complete", name: "完整性", tables: "users、movies、ratings",
    formula: "G₍完整,users₎ = 结构完整且 nᵤ ≥ 20 的用户行数",
    detail: "三表分别有 5、3、4 个字段且均非空白；users 还要求每名有效用户至少评价 20 部不同电影。nᵤ = |{ MovieID ∣ 用户 u 的可信评分引用有效电影 }|。未达标用户仅影响完整性分数，不因这项指标被清洗删除。未观察到的用户－电影组合不算缺失。",
  },
  {
    id: "unique", name: "唯一性", tables: "users、movies、ratings",
    formula: "G₍唯一,t₎ = 不同业务键的数量",
    detail: "用户键 UserID；电影键 MovieID；评分事件键 (UserID, MovieID, Timestamp)。同一用户在不同时间再次评价同一电影，属于不同事件。结构完整且业务键可解析的行进入分母。",
  },
  {
    id: "consistent", name: "一致性", tables: "users、movies、ratings",
    formula: "G₍一致,t₎ = 所在键组无冲突且引用有效的行数",
    detail: "同一用户/电影 ID 的属性不得冲突；评分引用的用户和电影须唯一且有效；同一评分事件不得出现不同 Rating。按整个键组判断，不受记录顺序影响。",
  },
  {
    id: "up_to_date", name: "历史时间适配性", tables: "ratings",
    formula: "G₍时间,ratings₎ = #{ r ∣ 起点 ≤ Timestamp(r) ≤ 终点 }",
    detail: "Timestamp 必须是规范 Unix 秒，并位于配置的历史采集闭区间。固定历史快照不按距今天数扣分；T1/T2 只是描述性切分点。",
  },
] as const;

export function ScoreRulesDialog({ job, onClose }: { job: Job; onClose: () => void }) {
  const result = job.result;
  const entries = result?.rules?.entries || {};
  const window = result?.time_window || entries.score_time_window?.value;
  const date = (seconds: unknown) => typeof seconds === "number"
    ? new Date(seconds * 1000).toISOString().replace("T", " ").replace(".000Z", " UTC")
    : "未记录";
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="reader-modal score-rules-modal" onMouseDown={(e) => e.stopPropagation()}
        role="dialog" aria-modal="true" aria-label="五维评分规则">
        <div className="modal-title">
          <div><h2>五维评分规则</h2><p>评分规范 v2 · {result?.algorithm_version_label || "评分算法"}</p></div>
          <button onClick={onClose} aria-label="关闭">×</button>
        </div>
        <p className="muted">评分口径固定，与数据清洗规则的开关和参数分开。无可评估行时显示 N/A。</p>
        <div className="score-formulas">
          <code>Q₍d,t₎ = 100 × G₍d,t₎ / E₍d,t₎</code>
          <code>A₍d,t₎ = 100 × E₍d,t₎ / Nₜ</code>
          <code>Y₍d,t₎ = 100 × G₍d,t,清洗后₎ / N₍t,原始₎</code>
          <code>Q₍d₎ = Σₜ Q₍d,t₎ / 适用且可评估的表数</code>
          <p>G：合格行数；E：可评估行数；N：实际行数；Q：质量分；A：评估覆盖率；Y：有效留存率。三表等权，时间维度只适用于 ratings。五维之间不再加权成一个总分。</p>
          <code>C₂₀ = 100 × #{'{'}u ∣ nᵤ ≥ 20{'}'} / 有效且唯一的用户数</code>
        </div>
        <div className="score-rule-list">
          {dimensions.map((dimension) => (
            <section key={dimension.id}>
              <div><strong>{dimension.name}</strong><span>{entries[`score_${dimension.id}`]?.enabled === false ? "已关闭" : "已开启"}</span></div>
              <small>适用表：{dimension.tables}</small>
              <p><code>{dimension.formula}</code></p>
              <p>{dimension.detail}</p>
            </section>
          ))}
        </div>
        <p className="muted">历史时间窗口：{date(window?.min)} 至 {date(window?.max)}（含端点）</p>
      </div>
    </div>
  );
}
