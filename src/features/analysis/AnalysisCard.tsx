import { useEffect, useState } from "react";
import type { Job } from "../../lib/types";
import { api } from "../../lib/api";
import type { AnalysisResult, Metrics, Point } from "./types";
import {
  genres,
  groupName,
  labels,
  parameterLabels,
  statusName,
  time,
  value,
} from "./format";
import { ScatterPlot } from "./ScatterPlot";
import { ResultRows } from "./ResultRows";
import "./analysis.css";
const titles: Record<string, string> = {
  analysis_logistic: "逻辑回归",
  analysis_tree: "CART 决策树",
  analysis_kmeans: "KMeans 用户分群",
  analysis_dbscan: "DBSCAN 用户分群",
  analysis_svd: "截断 SVD 降维",
  analysis_cur: "CUR 降维",
};
const states: Record<string, string> = {
  queued: "等待中",
  running: "运行中",
  completed: "已完成",
  failed: "失败",
  cancelled: "已中断",
};
function MetricTable({
  sets,
  names,
  hideNoise = false,
}: {
  sets: (Metrics | undefined)[];
  names: string[];
  hideNoise?: boolean;
}) {
  const keys = Object.keys(labels).filter(
    (k) => !(hideNoise && k === "noise_rate") && sets.some((s) => s && k in s),
  );
  return (
    <div className="analysis-table-scroll">
      <table>
        <thead>
          <tr>
            <th>评价指标</th>
            {names.map((n) => (
              <th key={n}>{n}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {keys.map((k) => (
            <tr key={k}>
              <td>{labels[k]}</td>
              {sets.map((s, i) => (
                <td key={i}>{value(s?.[k], k)}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
function Params({ data }: { data: Record<string, unknown> }) {
  return (
    <dl className="analysis-config">
      {Object.entries(parameterLabels)
        .filter(([k]) => k in data)
        .map(([k, l]) => (
          <div key={k}>
            <dt>{l}</dt>
            <dd>
              {k === "clustering_parameters" ? (
                <Params data={data[k] as Record<string, unknown>} />
              ) : (
                typeof data[k] === "number" ? String(data[k]) : value(data[k])
              )}
            </dd>
          </div>
        ))}
    </dl>
  );
}
function Projection({
  jobId,
  result,
}: {
  jobId: string;
  result: AnalysisResult;
}) {
  const [plots, setPlots] = useState<Point[][] | null>(null),
    [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    const all = async (table: string) =>
      (await api.analysisRows(jobId, table, 0, 10000))
        .rows as unknown as Point[];
    Promise.all([all("original_plot"), all("reduced_plot")])
      .then((p) => {
        if (active) setPlots(p);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [jobId]);
  const r = result.resources || {};
  return (
    <>
      <div className="analysis-highlights">
        <div>
          <span>信息保留</span>
          <strong>{value(result.information?.retained, "retained")}</strong>
        </div>
        <div>
          <span>画像维数</span>
          <strong>
            {result.original_dimensions} → {result.dimensions}
          </strong>
        </div>
      </div>
      <p className="analysis-caption">
        信息保留按原始画像的重建误差计算。
        {result.algorithm === "CUR"
          ? "CUR 坐标对应实际选中的电影类型。"
          : "SVD 坐标对应训练期画像的主要变化方向。"}
      </p>
      {error ? (
        <p role="alert">坐标读取失败：{error}</p>
      ) : plots ? (
        <div className="analysis-plots">
          <ScatterPlot
            title="原始画像上的聚类"
            points={plots[0]}
            axes={["原始画像主方向 1", "原始画像主方向 2"]}
          />
          <ScatterPlot
            title="降维表示上的聚类"
            points={plots[1]}
            axes={(result.axes || ["方向 1", "方向 2"]).map(
              (a) => genres[a] || a,
            )}
          />
        </div>
      ) : (
        <p>读取真实坐标中…</p>
      )}
      <MetricTable
        sets={result.comparison as unknown as Metrics[]}
        names={["原始画像", "降维表示"]}
        hideNoise={result.parameters.clustering_algorithm === "kmeans"}
      />
      {result.comparison_notes?.filter(Boolean).map((n, i) => (
        <p className="analysis-caption" key={i}>
          {i === 0 ? "原始画像" : "降维表示"}：{n}
        </p>
      ))}
      <details>
        <summary>在相同原始空间检查分组变化</summary>
        <MetricTable
          sets={[
            result.reference_metrics?.original,
            result.reference_metrics?.reduced,
          ]}
          names={["原始分组", "降维后分组"]}
          hideNoise={result.parameters.clustering_algorithm === "kmeans"}
        />
        <p>
          分组一致性：{value(result.reference_metrics?.agreement)}
          。仅比较共同的非噪声用户。{result.reference_metrics?.note}
        </p>
      </details>
      <dl className="analysis-resources">
        <div>
          <dt>降维用时</dt>
          <dd>{value(r.reduction_seconds)} 秒</dd>
        </div>
        <div>
          <dt>聚类对照总用时</dt>
          <dd>{value(r.clustering_seconds)} 秒</dd>
        </div>
        <div>
          <dt>画像矩阵大小</dt>
          <dd>
            {value(r.original_matrix_bytes / 1024)} →{" "}
            {value(r.reduced_matrix_bytes / 1024)} KB
          </dd>
        </div>
        <div>
          <dt>模型大小</dt>
          <dd>{value(r.model_bytes / 1024)} KB</dd>
        </div>
        <div>
          <dt>工作进程峰值内存</dt>
          <dd>{value(r.peak_rss_bytes / 1024 / 1024)} MB</dd>
        </div>
      </dl>
      <details>
        <summary>查看降维坐标</summary>
        <ResultRows jobId={jobId} table="coordinates" />
      </details>
    </>
  );
}
export function AnalysisCard({ job }: { job: Job }) {
  const result = job.result as AnalysisResult | null;
  return (
    <section className="analysis-card" aria-label={titles[job.kind]}>
      <header>
        <h3>{titles[job.kind]}</h3>
        <span className={`analysis-state ${job.status}`}>
          {states[job.status] || "未完成"}
        </span>
      </header>
      {result && job.status === "completed" && (
        <p className="analysis-caption">{result.version}</p>
      )}
      {job.status !== "completed" ? (
        <p role={job.error ? "alert" : undefined}>
          {job.error || job.stage || "等待执行"}
        </p>
      ) : (
        result && (
          <>
            {result.task === "classification" && (
              <>
                {result.target && (
                  <div className="analysis-prediction">
                    <p>
                      用户 {result.target.user_id} · 电影{" "}
                      {result.target.movie_id}《{result.target.movie_title}》
                    </p>
                    <strong>
                      {result.target.positive ? "预测高分" : "预测非高分"}
                    </strong>
                    <span>高分指评分至少 4 分</span>
                    <div className="analysis-probability">
                      <i
                        style={{ width: `${result.target.probability * 100}%` }}
                      />
                    </div>
                    <p>
                      高分概率{" "}
                      <b>{value(result.target.probability, "probability")}</b>
                      {result.target.cold_start ? " · 冷启动预测" : ""}
                    </p>
                  </div>
                )}
                <MetricTable
                  sets={[result.metrics.validation, result.metrics.test]}
                  names={["验证期", "最终测试期"]}
                />
                {Object.entries(result.metric_notes || {})
                  .filter(([, n]) => n)
                  .map(([k, n]) => (
                    <p key={k} className="analysis-caption">
                      {k === "validation" ? "验证期" : "测试期"}：{n}
                    </p>
                  ))}
                <details>
                  <summary>查看实际预测样例</summary>
                  <ResultRows jobId={job.id} table="predictions" />
                </details>
              </>
            )}
            {result.task === "clustering" && (
              <>
                {result.memberships && (
                  <div className="analysis-members">
                    {result.memberships.map((m) => (
                      <span key={m.user_id}>
                        用户 {m.user_id}
                        <b>
                          {m.status === "completed"
                            ? groupName(m.group, m.noise)
                            : statusName(m.status)}
                        </b>
                      </span>
                    ))}
                  </div>
                )}
                <MetricTable
                  sets={[result.metrics]}
                  names={["本次结果"]}
                  hideNoise={result.algorithm === "KMeans"}
                />
                {result.metric_note && (
                  <p className="analysis-caption">{result.metric_note}</p>
                )}
                {result.temporal_metrics && (
                  <details>
                    <summary>验证期选择与最终测试</summary>
                    <MetricTable
                      sets={[
                        result.temporal_metrics.validation,
                        result.temporal_metrics.test,
                      ]}
                      names={["验证期", "最终测试期"]}
                    />
                    <p>{result.temporal_metrics.selection}</p>
                  </details>
                )}
                <div className="analysis-groups">
                  {result.groups?.map((g) => (
                    <details key={g.group}>
                      <summary>
                        {groupName(g.group)} · {g.size} 人
                        <span>
                          {g.genres
                            .map((p) => genres[p.name] || p.name)
                            .join("、")}
                        </span>
                      </summary>
                      <p>
                        平均类型偏好：
                        {g.genres
                          .map(
                            (p) =>
                              `${genres[p.name] || p.name} ${(p.share * 100).toFixed(1)}%`,
                          )
                          .join("；")}
                      </p>
                      <p>
                        训练期代表电影：
                        {g.movies
                          .map((m) => `《${m.title}》（电影 ${m.movie_id}）`)
                          .join("、") || "无可用内容"}
                      </p>
                      <p>
                        群体按训练期高分电影的类型分布归纳，代表电影按群体高分次数选取。
                      </p>
                    </details>
                  ))}
                </div>
                <details>
                  <summary>查看用户群体归属</summary>
                  <ResultRows jobId={job.id} table="memberships" />
                </details>
                <details>
                  <summary>查看训练期用户画像</summary>
                  <ResultRows jobId={job.id} table="profiles" />
                </details>
              </>
            )}
            {result.task === "reduction" && (
              <Projection jobId={job.id} result={result} />
            )}
            <details className="analysis-evidence">
              <summary>实际参数、版本与适用限制</summary>
              <p>
                {result.version} · {result.data_version} · {result.rule_version}
              </p>
              <p>
                训练期截至 {time(result.split.train_end)}；验证期截至{" "}
                {time(result.split.validation_end)}；之后为最终测试期。
              </p>
              <p>
                训练 {result.split.counts.train} 条 · 验证{" "}
                {result.split.counts.validation} 条 · 测试{" "}
                {result.split.counts.test} 条评分
              </p>
              <Params data={result.parameters} />
              {result.limitations.map((l, i) => (
                <p key={i}>{l}</p>
              ))}
              <p>以上为实际模型预测或训练期统计描述，不代表因果关系。</p>
            </details>
          </>
        )
      )}
    </section>
  );
}
