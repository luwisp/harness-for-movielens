export function formatUtcTimestamp(value: unknown): string {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) return "N/A";
  const date = new Date(value * 1000);
  if (Number.isNaN(date.getTime())) return "N/A";
  return `${date.toISOString().replace("T", " ").replace(".000Z", " UTC")}（Unix 秒 ${value}）`;
}
