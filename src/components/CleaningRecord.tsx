import type { RuleSpec } from "../lib/types";

export function CleaningRecord({
  record,
  catalog,
}: {
  record: any;
  catalog: RuleSpec[];
}) {
  const tables: Record<string, { label: string; file: string }> = {
    "0": { label: "用户", file: "users.dat" },
    "1": { label: "电影", file: "movies.dat" },
    "2": { label: "评分", file: "ratings.dat" },
  };
  const source = tables[String(record.table)];
  const action = record.action === "removed" ? "移除" : "规范化";
  return (
    <div className="cleaning-record">
      <strong>
        {action} {": "}
        {source ? `${source.file}（${source.label}表）` : "未知数据表"} 
      </strong>
      <div className="record-rule-list">
        {(record.rules || []).map((id: string) => {
          const rule = catalog.find((spec) => spec.id === id);
          return (
            <p key={id}>
              <b>{rule?.label || id}</b>
              <small>{rule?.description || "历史规则，当前目录无说明"}</small>
            </p>
          );
        })}
      </div>
      {record.line != null && <pre>原始：{String(record.line)}</pre>}
      {record.output != null && <pre>结果：{String(record.output)}</pre>}
    </div>
  );
}
