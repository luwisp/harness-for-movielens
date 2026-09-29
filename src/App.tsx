import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { api } from "./lib/api";
import type {
  AgentEvent,
  Conversation,
  Database,
  Job,
  RuleSpec,
  Rules,
  Settings,
} from "./lib/types";
import { Titlebar } from "./components/Titlebar";
import { Sidebar } from "./components/Sidebar";
import { Composer } from "./components/Composer";
import { RulesDialog } from "./components/RulesDialog";
import { ScoreRulesDialog } from "./components/ScoreRulesDialog";
import { ChatTimeline } from "./components/ChatTimeline";
import { CleaningRecord } from "./components/CleaningRecord";
import { SettingsView } from "./components/SettingsView";
import { DeleteConversationDialog } from "./components/DeleteConversationDialog";
import "./App.css";
function App() {
  const [db, setDb] = useState<Database | null>(null);
  const [catalog, setCatalog] = useState<RuleSpec[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const activeRef = useRef<string | null>(null);
  const [view, setView] = useState<"chat" | "settings">("chat");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const [sidebarWidth, setSidebarWidth] = useState(245);
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [draft, setDraft] = useState("");
  const [notice, setNotice] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<Conversation | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [rulesTarget, setRulesTarget] = useState<{
    scope: "conversation" | "settings" | "snapshot";
    focus?: string | null;
    rules?: Rules;
    version?: string;
  } | null>(null);
  const [preview, setPreview] = useState<{
    name: string;
    lines: string[];
  } | null>(null);
  const [report, setReport] = useState<{ job: Job; markdown: string } | null>(
    null,
  );
  const [scoreRulesJob, setScoreRulesJob] = useState<Job | null>(null);
  const [records, setRecords] = useState<{
    job: Job;
    rows: any[];
    total: number;
    offset: number;
    rule: string;
    table: string;
    action: string;
  } | null>(null);
  const bottom = useRef<HTMLDivElement>(null);
  const refresh = async () => {
    const data = await api.state();
    setDb(data);
    return data;
  };
  useEffect(() => {
    void Promise.all([refresh(), api.catalog()])
      .then(([data, specs]) => {
        setCatalog(specs);
        setSettings(data.settings);
        const latest = [...data.conversations].sort((a, b) =>
          b.updated_at.localeCompare(a.updated_at),
        )[0];
        if (latest) {
          setActive(latest.id);
          activeRef.current = latest.id;
        }
      })
      .catch((e) => setNotice(String(e)));
    const off1 = listen<AgentEvent>("agent-event", (e) => {
      const event = e.payload;
      if (event.conversation_id === activeRef.current) {
        if (event.kind === "delta") setDraft((v) => v + event.value);
        if (event.kind === "message" || event.kind === "tool") setDraft("");
        if (event.kind === "error") setNotice(String(event.value));
      }
      if (event.kind === "done") {
        setRunning((v) => {
          const next = new Set(v);
          next.delete(event.conversation_id);
          return next;
        });
        setDraft("");
      }
      if (["message", "done", "title", "error"].includes(event.kind))
        void refresh().catch((e) => setNotice(String(e)));
    });
    const off2 = listen<Job>("job-update", () => {
      void refresh().catch((e) => setNotice(String(e)));
    });
    return () => {
      void off1.then((f) => f());
      void off2.then((f) => f());
    };
  }, []);
  useEffect(() => {
    bottom.current?.scrollIntoView({ behavior: "smooth" });
  }, [db, draft, active]);
  const conversation = db?.conversations.find((c) => c.id === active) || null;
  const jobs = db?.jobs.filter((j) => j.conversation_id === active) || [];
  const busy = active !== null && running.has(active);
  const busyAny = running.size > 0;
  const select = (id: string) => {
    setActive(id);
    activeRef.current = id;
    setView("chat");
    setDraft("");
    setNotice("");
  };
  const create = async () => {
    try {
      const c = await api.createConversation();
      select(c.id);
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };
  const rename = async (c: Conversation) => {
    const title = window.prompt("修改对话名称", c.title);
    if (title === null) return;
    try {
      await api.rename(c.id, title);
      await refresh();
    } catch (e) {
      setNotice(String(e));
    }
  };
  const deleteConversation = async () => {
    if (!deleteTarget || deleting) return;
    const id = deleteTarget.id;
    setDeleting(true);
    try {
      await api.deleteConversation(id);
      const data = await refresh();
      if (activeRef.current === id) {
        const next = [...data.conversations].sort((a, b) =>
          b.updated_at.localeCompare(a.updated_at),
        )[0];
        activeRef.current = next?.id || null;
        setActive(next?.id || null);
        setView("chat");
        setDraft("");
        setRulesTarget(null);
        setPreview(null);
      }
      setReport((value) => (value?.job.conversation_id === id ? null : value));
      setRecords((value) => (value?.job.conversation_id === id ? null : value));
      setScoreRulesJob((value) =>
        value?.conversation_id === id ? null : value,
      );
      setDeleteTarget(null);
      setNotice("对话及其任务数据已删除");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setDeleting(false);
    }
  };
  const send = async (content: string) => {
    if (!active) return;
    const id = active;
    setRunning((v) => new Set(v).add(id));
    setNotice("");
    try {
      await api.start(id, content);
      await refresh();
    } catch (e) {
      setRunning((v) => {
        const next = new Set(v);
        next.delete(id);
        return next;
      });
      setNotice(String(e));
      throw e;
    }
  };
  const stop = () => {
    if (active)
      void api
        .stop(active)
        .then(() => setNotice("正在中断…"))
        .catch((e) => setNotice(String(e)));
  };
  const download = (job: Job, name: string) => {
    void api
      .download(job, name)
      .then((path) => setNotice(`已保存到 ${path}`))
      .catch((e) => setNotice(String(e)));
  };
  const showPreview = (job: Job, name: string) => {
    void api
      .preview(job, name)
      .then((lines) => setPreview({ name, lines }))
      .catch((e) => setNotice(String(e)));
  };
  const openReport = (job: Job) => {
    if (report?.job.id === job.id) {
      setReport(null);
      return;
    }
    void invoke<string>("read_report", { jobId: job.id })
      .then((markdown) => setReport({ job, markdown }))
      .catch((e) => setNotice(String(e)));
  };
  const openRecords = async (
    job: Job,
    offset = 0,
    rule = "",
    table = "",
    action = "",
  ) => {
    try {
      const value = await api.records(
        job,
        offset,
        20,
        rule || undefined,
        table || undefined,
        action || undefined,
      );
      setRecords({
        job,
        rows: value.records,
        total: value.total,
        offset,
        rule,
        table,
        action,
      });
    } catch (e) {
      setNotice(String(e));
    }
  };
  const saveRules = async (rules: Rules) => {
    if (!rulesTarget) return;
    if (rulesTarget.scope === "settings") {
      if (settings) {
        const changed = { ...settings, rules };
        await api.saveSettings(changed);
        setSettings(changed);
        await refresh();
      }
    } else if (active) {
      await api.updateRules(active, rules);
      await refresh();
    }
    setRulesTarget(null);
  };
  const dialogRules =
    rulesTarget?.scope === "settings"
      ? settings?.rules
      : rulesTarget?.scope === "snapshot"
        ? rulesTarget.rules
        : conversation?.rules;
  return (
    <div className="app-shell">
      <Titlebar
        collapsed={collapsed}
        onToggle={() => setCollapsed((v) => !v)}
      />
      <div className="app-body">
        {!collapsed && (
          <Sidebar
            conversations={db?.conversations || []}
            active={active}
            view={view}
            width={sidebarWidth}
            onResize={setSidebarWidth}
            onNew={() => void create()}
            onSelect={select}
            onRename={(c) => void rename(c)}
            onDelete={setDeleteTarget}
            onSettings={() => {
              setSettings(db?.settings || null);
              setView("settings");
            }}
          />
        )}
        <main className="main">
          {view === "settings" && settings ? (
            <SettingsView
              settings={settings}
              setSettings={setSettings}
              catalog={catalog}
              busy={busyAny}
              configDirty={Boolean(
                db && JSON.stringify(settings) !== JSON.stringify(db.settings),
              )}
              onSaved={() => {
                void refresh();
                setNotice("设置已保存");
              }}
              onRules={() => setRulesTarget({ scope: "settings" })}
              onNotice={setNotice}
            />
          ) : (
            <>
              {/* <div className="chat-header">
                <strong>{conversation?.title || "MovieLens 数据治理"}</strong>
                <small>数据清洗 · 五维评估 · 报告</small>
              </div> */}
              <div className="chat-scroll">
                <ChatTimeline
                  conversation={conversation}
                  jobs={jobs}
                  catalog={catalog}
                  draft={draft}
                  busy={busy}
                  onRules={(focus) =>
                    setRulesTarget({ scope: "conversation", focus })
                  }
                  onScoreRules={setScoreRulesJob}
                  onDownload={download}
                  onPreview={showPreview}
                  onRecords={(job) => void openRecords(job)}
                  onOpenReport={openReport}
                  report={report}
                />
                <div ref={bottom} />
              </div>
              <Composer
                disabled={!conversation}
                busy={busy}
                onSend={send}
                onStop={stop}
                onRules={() => setRulesTarget({ scope: "conversation" })}
              />
            </>
          )}
        </main>
      </div>
      {notice && (
        <div className="toast" role="status">
          {notice}
          <button onClick={() => setNotice("")}>×</button>
        </div>
      )}
      {deleteTarget && (
        <DeleteConversationDialog
          conversation={deleteTarget}
          busy={deleting}
          onClose={() => setDeleteTarget(null)}
          onConfirm={() => void deleteConversation()}
        />
      )}
      {rulesTarget && dialogRules && (
        <RulesDialog
          key={`${rulesTarget.scope}-${rulesTarget.focus || ""}`}
          catalog={catalog}
          rules={dialogRules}
          focusId={rulesTarget.focus}
          readOnly={
            rulesTarget.scope === "snapshot" ||
            (rulesTarget.scope === "conversation" && busy) ||
            (rulesTarget.scope === "settings" && busyAny)
          }
          version={rulesTarget.version}
          onClose={() => setRulesTarget(null)}
          onSave={saveRules}
        />
      )}
      {scoreRulesJob && (
        <ScoreRulesDialog
          job={scoreRulesJob}
          onClose={() => setScoreRulesJob(null)}
        />
      )}
      {preview && (
        <div className="modal-backdrop" onMouseDown={() => setPreview(null)}>
          <div
            className="reader-modal"
            onMouseDown={(e) => e.stopPropagation()}
          >
            <div className="modal-title">
              <h2>{preview.name} · 前 100 行</h2>
              <button onClick={() => setPreview(null)}>×</button>
            </div>
            <pre>{preview.lines.join("\n") || "文件为空"}</pre>
          </div>
        </div>
      )}
      {records && (
        <div className="modal-backdrop" onMouseDown={() => setRecords(null)}>
          <div
            className="reader-modal"
            onMouseDown={(e) => e.stopPropagation()}
          >
            <div className="modal-title">
              <h2>清洗记录 · {records.total} 条</h2>
              <button onClick={() => setRecords(null)}>×</button>
            </div>
            <div className="record-filters">
              <select
                value={records.rule}
                onChange={(e) =>
                  setRecords({ ...records, rule: e.target.value })
                }
              >
                <option value="">全部规则</option>
                {Object.keys(
                  records.job.result?.metrics?.counts?.by_rule || {},
                ).map((id) => {
                  const rule = catalog.find((spec) => spec.id === id);
                  return (
                    <option key={id} value={id}>
                      {rule ? `${rule.label}：${rule.description}` : id}
                    </option>
                  );
                })}
              </select>
              <select
                value={records.table}
                onChange={(e) =>
                  setRecords({ ...records, table: e.target.value })
                }
              >
                <option value="">全部表</option>
                <option value="0">users</option>
                <option value="1">movies</option>
                <option value="2">ratings</option>
              </select>
              <select
                value={records.action}
                onChange={(e) =>
                  setRecords({ ...records, action: e.target.value })
                }
              >
                <option value="">全部动作</option>
                <option value="removed">移除</option>
                <option value="normalized">规范化</option>
              </select>
              <button
                onClick={() =>
                  void openRecords(
                    records.job,
                    0,
                    records.rule,
                    records.table,
                    records.action,
                  )
                }
              >
                筛选
              </button>
            </div>
            <div className="records-body">
              {records.rows.map((r, i) => (
                <CleaningRecord key={i} record={r} catalog={catalog} />
              ))}
            </div>
            <div className="modal-footer">
              <button
                disabled={records.offset === 0}
                onClick={() =>
                  void openRecords(
                    records.job,
                    Math.max(0, records.offset - 20),
                    records.rule,
                    records.table,
                    records.action,
                  )
                }
              >
                上一页
              </button>
              <span>
                {records.offset + 1}–
                {Math.min(records.offset + 20, records.total)} / {records.total}
              </span>
              <button
                disabled={records.offset + 20 >= records.total}
                onClick={() =>
                  void openRecords(
                    records.job,
                    records.offset + 20,
                    records.rule,
                    records.table,
                    records.action,
                  )
                }
              >
                下一页
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
export default App;
