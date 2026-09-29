import { useState } from "react";
import type { RuleSpec, Settings, SourceStatus } from "../lib/types";
import { api } from "../lib/api";
export function SettingsView({
  settings,
  setSettings,
  catalog,
  busy,
  configDirty,
  onSaved,
  onRules,
  onNotice,
}: {
  settings: Settings;
  setSettings: (s: Settings) => void;
  catalog: RuleSpec[];
  busy: boolean;
  configDirty: boolean;
  onSaved: () => void;
  onRules: () => void;
  onNotice: (s: string) => void;
}) {
  const [status, setStatus] = useState<SourceStatus | null>(null);
  const [uploading, setUploading] = useState("");
  const refresh = () =>
    api
      .sourceStatus()
      .then(setStatus)
      .catch((e) => onNotice(String(e)));
  // useEffect(() => {
  //   refresh();
  // }, []);
  const field = (label: string, key: keyof Settings, secret = false) => (
    <label>
      {label}
      <input
        type={secret ? "password" : "text"}
        value={String(settings[key])}
        onChange={(e) => setSettings({ ...settings, [key]: e.target.value })}
      />
    </label>
  );
  const upload = async (file: File | null, name: string) => {
    if (!file) return;
    if (file.name !== name) {
      onNotice(`请选择 ${name}`);
      return;
    }
    setUploading(name);
    try {
      await api.upload(file, (p) => onNotice(`${name} 上传 ${p}%`));
      await refresh();
      onNotice(`${name} 已上传并激活`);
    } catch (e) {
      onNotice(String(e));
    } finally {
      setUploading("");
    }
  };
  return (
    <div className="settings-page">
      <div className="page-heading">
        <h1>设置</h1>
        {/* <p>配置 Agent、Hadoop 以及三个供所有对话共用的数据源。</p> */}
      </div>
      <section>
        <h2>数据源初始化</h2>
        <p className="muted">
          上传或替代后立即写入 HDFS。新任务使用当前版本，旧任务保留原版本。
        </p>
        <div className="source-grid">
          {(["users.dat", "movies.dat", "ratings.dat"] as const).map((name) => (
            <div className="source-card" key={name}>
              <div>
                <strong>{name}</strong>
                <span
                  className={`source-dot ${status?.[name]?.exists ? "ok" : ""}`}
                />
              </div>
              <small>
                {status?.[name]?.exists
                  ? "HDFS 已存在"
                  : status
                    ? "HDFS 未找到或检查失败"
                    : "待检查"}
              </small>
              <code title={status?.[name]?.hdfs_path || ""}>
                {status?.[name]?.hdfs_path || "尚无路径"}
              </code>
              <label className="upload-button">
                {configDirty
                  ? "先保存设置"
                  : uploading === name
                    ? "上传中…"
                    : status?.[name]?.file
                      ? "替代文件"
                      : "上传文件"}
                <input
                  type="file"
                  accept=".dat"
                  disabled={Boolean(uploading) || busy || configDirty}
                  hidden
                  onChange={(e) => {
                    void upload(e.target.files?.[0] || null, name);
                    e.target.value = "";
                  }}
                />
              </label>
            </div>
          ))}
        </div>
        <button onClick={refresh}>刷新 HDFS 状态</button>
      </section>
      <section>
        <h2>Agent API</h2>
        {field("DeepSeek API Key", "api_key", true)}
        {field("API Base", "api_base")}
        {field("模型", "model")}
      </section>
      <section>
        <h2>Hadoop</h2>
        <label>
          连接方式
          <select
            value={settings.hadoop_mode}
            onChange={(e) =>
              setSettings({ ...settings, hadoop_mode: e.target.value })
            }
          >
            <option value="local">本地 Hadoop 客户端</option>
            <option value="ssh">远端 SSH Hadoop 主机</option>
          </select>
        </label>
        {field("HDFS 根路径", "hdfs_root")}
        {settings.hadoop_mode === "local" ? (
          <>
            {field("Hadoop 可执行文件", "hadoop_bin")}
            {field("HADOOP_CONF_DIR", "hadoop_conf_dir")}
            {field("Streaming jar 路径", "streaming_jar")}
            {field("Python 命令", "python_bin")}
          </>
        ) : (
          <>
            {field("SSH 目标（需密钥登录）", "ssh_target")}
            {field("远端 Hadoop 命令", "remote_hadoop_bin")}
            {field("远端 Python 命令", "remote_python_bin")}
            {field("远端 Streaming jar 路径", "streaming_jar")}
          </>
        )}
      </section>
      <section>
        <h2>默认清洗与评分规则</h2>
        <p className="muted">
          新对话复制此处默认值。共 {catalog.length} 条规则。
        </p>
        <button disabled={busy || Boolean(uploading)} onClick={onRules}>
          查看与编辑默认规则 →
        </button>
      </section>
      <div className="settings-actions">
        <button
          className="primary"
          disabled={busy || Boolean(uploading)}
          onClick={() =>
            api
              .saveSettings(settings)
              .then(onSaved)
              .catch((e) => onNotice(String(e)))
          }
        >
          保存设置
        </button>
      </div>
    </div>
  );
}
