import { useEffect, useState } from "react";
import { api } from "../../lib/api";
import type { Page, Row } from "./types";
import { genres, groupName, statusName, value } from "./format";
const columns: Record<string, [string, string][]> = {
  predictions: [
    ["用户", "用户"],
    ["电影", "电影"],
    ["实际高分", "实际高分"],
    ["预测高分概率", "预测高分概率"],
    ["预测高分", "预测高分"],
  ],
  memberships: [
    ["user_id", "用户"],
    ["group", "归属"],
    ["status", "状态"],
  ],
  coordinates: [
    ["user_id", "用户"],
    ["coordinates", "坐标"],
    ["group", "归属"],
  ],
  profiles: [
    ["user_id", "用户"],
    ["history_count", "训练期评分数"],
    ["preferences", "类型偏好"],
  ],
};
function cell(row: Row, key: string) {
  if (key === "实际高分") return row[key] ? "是" : "否";
  if (key === "group") return groupName(row.group, row.noise);
  if (key === "status") return statusName(row.status);
  if (key === "coordinates")
    return (row[key] as number[]).map((n) => n.toFixed(4)).join("，");
  if (key === "preferences")
    return (row[key] as number[])
      .map((n, i) => [Object.values(genres)[i], n] as const)
      .filter(([, n]) => n > 0)
      .map(([g, n]) => `${g} ${(n * 100).toFixed(1)}%`)
      .join("；");
  return value(row[key], key === "预测高分概率" ? "probability" : key);
}
export function ResultRows({ jobId, table }: { jobId: string; table: string }) {
  const [page, setPage] = useState<Page | null>(null),
    [offset, setOffset] = useState(0),
    [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    setPage(null);
    setError("");
    api
      .analysisRows(jobId, table, offset, 20)
      .then((p) => {
        if (active) setPage(p);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [jobId, table, offset]);
  if (error) return <p role="alert">{error}</p>;
  if (!page) return <p>读取实际结果中…</p>;
  return (
    <div className="analysis-rows">
      <div className="analysis-table-scroll">
        <table>
          <thead>
            <tr>
              {columns[table]?.map(([k, l]) => (
                <th key={k}>{l}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {page.rows.map((r, i) => (
              <tr key={i}>
                {columns[table]?.map(([k]) => (
                  <td key={k}>{cell(r, k)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="analysis-pager">
        <span>
          {page.total === 0
            ? "无记录"
            : `${offset + 1}–${Math.min(offset + 20, page.total)} / ${page.total}`}
        </span>
        <button
          disabled={offset === 0}
          onClick={() => setOffset(Math.max(0, offset - 20))}
        >
          上一页
        </button>
        <button
          disabled={offset + 20 >= page.total}
          onClick={() => setOffset(offset + 20)}
        >
          下一页
        </button>
      </div>
    </div>
  );
}
