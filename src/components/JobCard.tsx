import { useState } from "react";
import type { Job, RuleSpec } from "../lib/types";
const labels: Record<string, string> = {
  accurate: "准确性",
  complete: "完整性",
  unique: "唯一性",
  consistent: "一致性",
  up_to_date: "时效性",
};
const number = (v: unknown) => (typeof v === "number" ? `${v.toFixed(2)}%` : "N/A");
const total = (v: any) =>
  Object.values(v || {}).reduce<number>((a, b) => a + (Number(b) || 0), 0);
export function JobCard({
  job,
  catalog,
  onScoreRules,
  onRecords,
}: {
  job: Job;
  catalog: RuleSpec[];
  onScoreRules: () => void;
  onRecords: (j: Job) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const r = job.result;
  const m = r?.metrics;
  const ruleCounts = Object.entries(m?.counts?.by_rule || {}).sort(
    (a, b) => Number(b[1]) - Number(a[1]),
  );
  const title: Record<string, string> = {
    cleaning: "数据清洗",
    comparison: "五维评估对比",
  };
  return (
    <div className="job-card">
      <div className="job-head">
        <span className="job-icon">{job.kind === "cleaning" ? "◈" : "◇"}</span>
        <div>
          <strong>{title[job.kind] || job.kind}</strong>
          <small>
            任务 {job.display_number || job.id.slice(0, 8)} · {job.stage}
          </small>
        </div>
        <span className={`status ${job.status}`}>
          {(
            {
              completed: "已完成",
              running: "运行中",
              queued: "等待中",
              failed: "失败",
              cancelled: "已中断",
            } as Record<string, string>
          )[job.status] || job.status}
        </span>
      </div>
      {job.error && <p className="error-text">{job.error}</p>}
      {m && job.kind === "cleaning" && (
        <>
          <div className="stat-row">
            <span>
              原始 <b>{total(m.counts?.raw)}</b>
            </span>
            <span>
              保留 <b>{total(m.counts?.clean)}</b>
            </span>
            <span>
              移除 <b>{total(m.counts?.removed)}</b>
            </span>
            <span>
              规范化 <b>{total(m.counts?.normalized)}</b>
            </span>
          </div>
          <div className="rule-pills">
            {ruleCounts.length > 0 && (
              <p className="muted">触发移除的规则（按次数排序）</p>
            )}
            {ruleCounts.slice(0, expanded ? undefined : 5).map(([id, n]) => {
              const rule = catalog.find((spec) => spec.id === id);
              return (
                <span key={id}>
                  <strong>{rule?.label || id}</strong>
                  <small>
                    {rule?.description || "历史规则，当前目录无说明"}
                  </small>
                  <b>{String(n)} 次</b>
                </span>
              );
            })}
          </div>
          {ruleCounts.length > 5 && (
            <button
              className="text-button"
              onClick={() => setExpanded(!expanded)}
            >
              {expanded ? "收起" : "展开所有规则"}
            </button>
          )}
          <button className="text-button" onClick={() => onRecords(job)}>
            查看清洗记录与筛选 →
          </button>
          <p className="muted">
            {r?.data_version_label || "历史原始数据"} ·{" "}
            {r?.clean_data_version_label || "历史清洗数据"} ·{" "}
            {r?.rule_version_label || "历史规则方案"}
          </p>
        </>
      )}
      {r && job.kind === "comparison" && (
        <>
          <button
            className="text-button"
            title="查看评估规则与版本"
            onClick={onScoreRules}
          >
            ⓘ 评分规则与版本
          </button>
          <table>
            <thead>
              <tr>
                <th>维度</th>
                <th>清洗前</th>
                <th>清洗后</th>
                <th>变化</th>
                <th>清洗后覆盖</th>
                <th>有效留存</th>
              </tr>
            </thead>
            <tbody>
              {Object.entries(labels).map(([id, label]) => {
                const d = r.dimensions?.[id] || {};
                return (
                  <tr key={id}>
                    <td>{label}</td>
                    <td>{number(d.before)}</td>
                    <td>{number(d.after)}</td>
                    <td>
                      {typeof d.change_pp === "number"
                        ? `${d.change_pp >= 0 ? "+" : ""}${d.change_pp.toFixed(2)} pp`
                        : "N/A"}
                    </td>
                    <td>{number(d.coverage_after)}</td>
                    <td>{number(d.yield)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {r.cohort20_after && (
            <p className="muted">
              ≥20 部不同电影：{r.cohort20_after.qualified_users} / {r.cohort20_after.evaluable_users} 名有效用户
              （{number(r.cohort20_after.rate)}）；未达标用户仍保留。
            </p>
          )}
          <details>
            <summary>查看各表分子、分母与有效留存</summary>
            <table>
              <thead><tr><th>维度</th><th>表</th><th>清洗前 G/E</th><th>清洗后 G/E</th><th>原始行数</th><th>有效留存</th></tr></thead>
              <tbody>
                {Object.entries(labels).flatMap(([id, label]) =>
                  Object.entries(r.dimensions?.[id]?.tables || {}).map(([table, stats]: [string, any]) => (
                    <tr key={`${id}-${table}`}>
                      <td>{label}</td><td>{({"0":"users","1":"movies","2":"ratings"} as Record<string,string>)[table]}</td>
                      <td>{stats.before?.good} / {stats.before?.eligible}</td>
                      <td>{stats.after?.good} / {stats.after?.eligible}</td>
                      <td>{stats.before?.total}</td><td>{number(stats.yield)}</td>
                    </tr>
                  ))) }
              </tbody>
            </table>
          </details>
        </>
      )}
    </div>
  );
}
