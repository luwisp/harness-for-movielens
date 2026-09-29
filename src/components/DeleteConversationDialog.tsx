import type { Conversation } from "../lib/types";

export function DeleteConversationDialog({
  conversation,
  busy,
  onClose,
  onConfirm,
}: {
  conversation: Conversation;
  busy: boolean;
  onClose: () => void;
  onConfirm: () => void;
}) {
  return (
    <div
      className="modal-backdrop"
      onMouseDown={() => !busy && onClose()}
      role="presentation"
    >
      <div
        className="reader-modal delete-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="delete-conversation-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="modal-title">
          <h2 id="delete-conversation-title">删除对话</h2>
        </div>
        <p>
          确定删除“{conversation.title}”？消息、报告和任务结果会从应用数据及
          HDFS 中删除，无法恢复。
        </p>
        <p className="muted">
          设置页的共享数据文件和已经下载到本机的副本会保留。
        </p>
        <div className="modal-footer">
          <button disabled={busy} onClick={onClose}>
            取消
          </button>
          <button className="danger-button" disabled={busy} onClick={onConfirm}>
            {busy ? "正在删除…" : "删除对话"}
          </button>
        </div>
      </div>
    </div>
  );
}
