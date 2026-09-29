import type { Job } from "../lib/types";

const dimensions = [
  {
    id: "accurate",
    name: "准确性",
    scope: "三表全部记录",
    rule: "结构与必填字段正确，用户、电影、评分和时间戳满足已知格式与取值域。邮编格式和片名年份不计入。",
  },
  {
    id: "complete",
    name: "完整性",
    scope: "三表全部记录",
    rule: "字段数正确，所有必填字段去除首尾空格后均非空。",
  },
  {
    id: "unique",
    name: "唯一性",
    scope: "三表全部记录",
    rule: "用户 ID、电影 ID 或评分事件键（用户 ID、电影 ID、时间戳）不重复。",
  },
  {
    id: "consistent",
    name: "一致性",
    scope: "三表全部记录",
    rule: "同 ID 的属性无冲突；电影类型不重复；评分引用的用户和电影存在。",
  },
  {
    id: "up_to_date",
    name: "时效性",
    scope: "ratings 全部记录",
    rule: "评分时间戳位于历史参照时间向前指定天数的闭区间内。",
  },
] as const;

export function ScoreRulesDialog({
  job,
  onClose,
}: {
  job: Job;
  onClose: () => void;
}) {
  const result = job.result;
  const entries = result?.rules?.entries || {};
  const days = entries.score_freshness?.value ?? 365;
  const reference = result?.reference_timestamp;
  const referenceText =
    typeof reference === "number"
      ? `${new Date(reference * 1000).toLocaleString()}（Unix ${reference}）`
      : "无有效参照时间";
  const configuredReference = entries.score_reference?.value;
  const referenceSource =
    entries.score_reference?.enabled !== false &&
    typeof configuredReference === "number" &&
    configuredReference > 0
      ? "按设置的参照时间；对比两侧共用同一参照"
      : "未指定时取评估输入的有效评分最大时间戳；统一流程两侧共用原始数据参照";
  const range = (id: string) => {
    const value = entries[id]?.value;
    return value &&
      typeof value === "object" &&
      "min" in value &&
      "max" in value
      ? `${value.min}～${value.max}`
      : "未记录";
  };
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div
        className="reader-modal score-rules-modal"
        onMouseDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label="五维评分规则"
      >
        <div className="modal-title">
          <div>
            <h2>五维评分规则</h2>
            <p>
              {result?.rule_version_label || "历史规则方案"} ·{" "}
              {result?.algorithm_version_label || "历史评分算法"}
            </p>
          </div>
          <button onClick={onClose} aria-label="关闭">
            ×
          </button>
        </div>
        <p className="muted">
          每维分数 = 合格记录数 / 适用记录数 × 100；无适用记录时显示
          N/A。
        </p>
        <div className="score-rule-list">
          {dimensions.map((dimension) => {
            const setting =
              entries[
                dimension.id === "up_to_date"
                  ? "score_freshness"
                  : `score_${dimension.id}`
              ];
            return (
              <section key={dimension.id}>
                <div>
                  <strong>{dimension.name}</strong>
                  <span>
                    {setting?.enabled === false ? "已关闭" : "已开启"}
                  </span>
                </div>
                <small>分母：{dimension.scope}</small>
                <p>{dimension.rule}</p>
                {dimension.id === "accurate" && (
                  <details>
                    <summary>查看具体取值条件</summary>
                    <ul>
                      <li>
                        用户、电影及评分引用 ID 均为正整数；性别为 F/M；年龄组为
                        1、18、25、35、45、50、56。
                      </li>
                      <li>
                        职业编号：{range("user_occupation_range")}；电影类型属于
                        MovieLens 目录。
                      </li>
                      <li>
                        评分为整数且在 {range("rating_range")}；时间戳为整数且在{" "}
                        {range("timestamp_range")}。
                      </li>
                    </ul>
                  </details>
                )}
              </section>
            );
          })}
        </div>
        <p className="muted">
          时效窗口：{days} 天 · 参照时间：{referenceText}
        </p>
        <p className="muted">参照来源：{referenceSource}</p>
        {/* <p className="muted">
          准确性仅检查可观察的格式和取值域，不保证现实世界的事实正确。
        </p> */}
      </div>
    </div>
  );
}
