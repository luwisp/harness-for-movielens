import { Fragment } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Conversation, Job, Message, RuleSpec } from "../lib/types";
import { formatUtcTimestamp } from "../lib/time";
import { JobCard } from "./JobCard";
export function MarkdownView({ content }: { content: string }) {
  return (
    <div className="markdown">
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => (
            <a
              href={href}
              onClick={(e) => {
                e.preventDefault();
                if (href && /^https?:\/\//.test(href)) void openUrl(href);
              }}
            >
              {children}
            </a>
          ),
        }}
      >
        {content}
      </Markdown>
    </div>
  );
}
export function ChatTimeline({
  conversation,
  jobs,
  catalog,
  draft,
  busy,
  onRules,
  onScoreRules,
  onDownload,
  onPreview,
  onRecords,
  onOpenReport,
  report,
}: {
  conversation: Conversation | null;
  jobs: Job[];
  catalog: RuleSpec[];
  draft: string;
  busy: boolean;
  onRules: (id?: string) => void;
  onScoreRules: (job: Job) => void;
  onDownload: (j: Job, n: string) => void;
  onPreview: (j: Job, n: string) => void;
  onRecords: (j: Job) => void;
  onOpenReport: (j: Job) => void;
  report: { job: Job; markdown: string } | null;
}) {
  const byId = new Map(jobs.map((j) => [j.id, j]));
  if (!conversation)
    return (
      <>
      {/* <div className="welcome">
        <h1>MovieLens 数据治理</h1>
        <p>新建对话开始。</p>
      </div> */}
      </>
    );
  const renderAttachments = (
    runId: string | null | undefined,
    active = false,
  ) => {
    if (!runId || (active && busy)) return null;
    const attachments = jobs
      .filter((job) => job.run_id === runId && job.status === "completed")
      .flatMap((job) => {
        const names =
          job.kind === "cleaning"
            ? ["users.dat", "movies.dat", "ratings.dat", "actions.ndjson"]
            : job.kind === "report"
              ? ["report.md"]
              : [];
        return names
          .filter((name) => job.result?.files?.includes(name))
          .map((name) => ({ job, name }));
      });
    if (attachments.length === 0) return null;
    return (
      <section className="attachment-section">
        <h3>本轮下载文件</h3>
        {attachments.map(({ job, name }) => (
          <div className="attachment-row" key={`${job.id}-${name}`}>
            <span>
              {name}{" "}
              <small>
                {job.kind === "report"
                  ? "Markdown 报告"
                  : job.result?.clean_data_version_label || "历史清洗数据"}
              </small>
            </span>
            {job.kind === "cleaning" && (
              <button onClick={() => onPreview(job, name)}>预览</button>
            )}
            <button onClick={() => onDownload(job, name)}>下载 ↓</button>
          </div>
        ))}
      </section>
    );
  };
  return (
    <div className="chat-content">
      {conversation.messages.length === 0 && (
        <>
        {/* <div className="hint">
          <strong>可以这样开始</strong>
          <p>“”</p>
        </div> */}
        </>
      )}
      {conversation.messages.map((m, i) => {
        const previousRunId = conversation.messages[i - 1]?.run_id;
        if (
          m.kind === "text" &&
          m.role === "user" &&
          previousRunId &&
          previousRunId !== m.run_id
        ) {
          return (
            <Fragment key={i}>
              {renderAttachments(previousRunId)}
              <MessageView message={m} />
            </Fragment>
          );
        }
        if (m.kind === "job" && m.job_id) {
          const job = byId.get(m.job_id);
          if (job?.kind === "comparison" && job.status === "completed" &&
            job.result?.score_spec_version !== "quality-spec-v2") return null;
          return job && ["cleaning", "comparison"].includes(job.kind) ? (
            <JobCard
              key={i}
              job={job}
              jobs={jobs}
              catalog={catalog}
              onScoreRules={() => onScoreRules(job)}
              onRecords={onRecords}
            />
          ) : null;
        }
        if (m.kind === "assessment_status" && m.job_id) {
          const job = byId.get(m.job_id);
          if (!job) return null;
          if (job.status === "completed" && job.result?.score_spec_version !== "quality-spec-v2") return null;
          const state =
            (
              {
                queued: "等待中",
                running: "运行中",
                completed: "已完成",
                failed: "失败",
                cancelled: "已中断",
              } as Record<string, string>
            )[job.status] || job.status;
          return (
            <div key={i} className="tool-line assessment-status">
              <span>◇</span>
              <span>{m.content ? `${m.content} · ` : ""}五维评估 #
                {job.display_number || job.id.slice(0, 8)} · {state}
                {job.status === "running" ? `：${job.stage}` : ""}
              </span>
              {/* {job.status === "completed" && (
                <div className="assessment-times">
                  <span>T1：{formatUtcTimestamp(job.result?.metrics?.t1)}</span>
                  <span>T2：{formatUtcTimestamp(job.result?.metrics?.t2)}</span>
                </div>
              )} */}
            </div>
          );
        }
        if (m.kind === "tool_call")
          return (
            <div key={i} className="tool-line">
              <span>◇</span> {m.content}
            </div>
          );
        if (m.kind === "rule")
          return (
            <div
              key={i}
              className="tool-line"
              role="link"
              tabIndex={0}
              onClick={() => onRules(m.rule_id || undefined)}
              onKeyDown={(e) => {
                if (e.key === "Enter") onRules(m.rule_id || undefined);
              }}
            >
              <span>⚙</span> {m.content} <small>查看规则 →</small>
            </div>
          );
        if (m.kind === "report" && m.job_id) {
          const job = byId.get(m.job_id);
          return job ? (
            <div key={i}>
              <div
                className="report-card"
                onClick={() => onOpenReport(job)}
                role="link"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter") onOpenReport(job);
                }}
              >
                <span>▤</span>
                <div>
                  <strong>本轮数据治理报告</strong>
                  <small>
                    点击{report?.job.id === job.id ? "收起" : "阅读"} Markdown
                    报告
                  </small>
                </div>
                <b>{report?.job.id === job.id ? "收起 ↑" : "查看 →"}</b>
              </div>
              {report?.job.id === job.id && (
                <article className="inline-report">
                  <div className="inline-report-head">
                    <strong>数据治理报告</strong>
                  </div>
                  <MarkdownView content={report.markdown} />
                </article>
              )}
            </div>
          ) : null;
        }
        if (m.kind === "text") return <MessageView key={i} message={m} />;
        return null;
      })}
      {draft && (
        <div className="message assistant">
          <div className="avatar">AI</div>
          <div className="bubble">
            <MarkdownView content={draft} />
            <span className="cursor">▍</span>
          </div>
        </div>
      )}
      {renderAttachments(
        conversation.messages[conversation.messages.length - 1]?.run_id,
        true,
      )}
    </div>
  );
}
function MessageView({ message }: { message: Message }) {
  return (
    <div className={`message ${message.role}`}>
      {message.role !== "user" && <div className="avatar">AI</div>}
      <div className="bubble">
        {message.role === "assistant" ? (
          <MarkdownView content={message.content} />
        ) : (
          message.content
        )}
      </div>
      {message.role === "user" && <div className="avatar">你</div>}
    </div>
  );
}
