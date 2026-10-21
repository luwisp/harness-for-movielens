export const genres: Record<string, string> = {
  Action: "动作",
  Adventure: "冒险",
  Animation: "动画",
  "Children's": "儿童",
  Comedy: "喜剧",
  Crime: "犯罪",
  Documentary: "纪录片",
  Drama: "剧情",
  Fantasy: "奇幻",
  "Film-Noir": "黑色电影",
  Horror: "恐怖",
  Musical: "歌舞",
  Mystery: "悬疑",
  Romance: "爱情",
  "Sci-Fi": "科幻",
  Thriller: "惊悚",
  War: "战争",
  Western: "西部",
};
export const labels: Record<string, string> = {
  preference_loss: "未来高分类型预测损失",
  event_coverage: "未来事件分组覆盖率",
  evaluated_events: "评价事件数",
  precision: "高分预测准确率",
  recall: "高分识别率",
  f1: "综合表现（F1）",
  roc_auc: "ROC 曲线面积",
  pr_auc: "精确率召回率曲线面积",
  silhouette: "轮廓系数",
  davies_bouldin: "群体分离指数",
  clusters: "群体数量",
  coverage: "有效分组覆盖率",
  noise_rate: "噪声占比",
};
export const parameterLabels: Record<string, string> = {
  C: "正则强度倒数",
  seed: "随机种子",
  threshold: "高分判定阈值",
  max_depth: "最大树深",
  min_samples_leaf: "叶节点最少样本",
  ccp_alpha: "剪枝强度",
  criterion: "划分准则",
  n_clusters: "群体数量",
  n_init: "初始化次数",
  eps: "邻域半径",
  min_samples: "密度最少样本",
  min_history: "最低历史评分数",
  dimensions: "输出维数",
  information_threshold: "信息保留目标",
  row_samples: "采样用户数",
  clustering_algorithm: "对照聚类算法",
  clustering_parameters: "对照聚类参数",
};
export function value(v: unknown, key = ""): string {
  if (
    v === null ||
    v === undefined ||
    (typeof v === "number" && !Number.isFinite(v))
  )
    return "无法计算";
  if (typeof v === "number")
    return ["coverage", "noise_rate", "retained", "probability"].includes(key)
      ? `${(v * 100).toFixed(1)}%`
      : Number.isInteger(v)
        ? String(v)
        : v.toFixed(3);
  if (typeof v === "boolean") return v ? "是" : "否";
  return String(v === "gini" ? "基尼不纯度" : v);
}
export function groupName(group: unknown, noise: unknown = false) {
  return noise || group === -1
    ? "噪声点"
    : group === null || group === undefined
      ? "未分组"
      : `群体 ${Number(group)}`;
}
export function statusName(s: unknown) {
  return (
    (
      {
        completed: "已分组",
        insufficient_history: "历史不足",
        no_preference_signal: "无高分偏好信号",
      } as Record<string, string>
    )[String(s)] || String(s)
  );
}
export function time(stamp: number) {
  return new Date(stamp * 1000)
    .toISOString()
    .replace("T", " ")
    .replace(".000Z", " UTC");
}
