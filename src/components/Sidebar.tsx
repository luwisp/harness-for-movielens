import { useRef } from "react";
import type { Conversation } from "../lib/types";

const minWidth = 185;
const maxWidth = () =>
  Math.max(minWidth, Math.min(480, window.innerWidth - 420));

export function Sidebar({
  conversations,
  active,
  view,
  width,
  onResize,
  onNew,
  onSelect,
  onRename,
  onDelete,
  onSettings,
}: {
  conversations: Conversation[];
  active: string | null;
  view: string;
  width: number;
  onResize: (width: number) => void;
  onNew: () => void;
  onSelect: (id: string) => void;
  onRename: (c: Conversation) => void;
  onDelete: (c: Conversation) => void;
  onSettings: () => void;
}) {
  const drag = useRef<{ pointerId: number; x: number; width: number } | null>(
    null,
  );
  const clamp = (value: number) =>
    Math.max(minWidth, Math.min(maxWidth(), value));
  return (
    <aside id="conversation-sidebar" className="sidebar" style={{ width }}>
      <button className="new-button" onClick={onNew} title="新建对话">
        <span>＋</span>
        <span>新建对话</span>
      </button>
      <div className="side-label">历史对话</div>
      <div className="conversation-list">
        {[...conversations]
          .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
          .map((c) => (
            <div
              className={`conversation-row ${active === c.id && view === "chat" ? "selected" : ""}`}
              key={c.id}
            >
              <button title={c.title} onClick={() => onSelect(c.id)}>
                <span className="conversation-title">{c.title}</span>
              </button>
              <button
                className="rename"
                title="重命名"
                onClick={() => onRename(c)}
              >
                ✎
              </button>
              <button
                className="delete"
                title={`删除对话：${c.title}`}
                aria-label={`删除对话：${c.title}`}
                onClick={() => onDelete(c)}
              >
                ×
              </button>
            </div>
          ))}
      </div>
      <button className="settings-link" onClick={onSettings} title="设置">
        <span>⚙</span>
        <span>设置</span>
      </button>
      <div
        className="sidebar-resizer"
        role="separator"
        aria-label="调整侧边栏宽度"
        aria-orientation="vertical"
        aria-valuemin={minWidth}
        aria-valuemax={maxWidth()}
        aria-valuenow={Math.round(Math.min(width, maxWidth()))}
        tabIndex={0}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          drag.current = {
            pointerId: event.pointerId,
            x: event.clientX,
            width:
              event.currentTarget.parentElement?.getBoundingClientRect()
                .width || width,
          };
          event.currentTarget.setPointerCapture(event.pointerId);
          event.preventDefault();
        }}
        onPointerMove={(event) => {
          if (drag.current?.pointerId === event.pointerId) {
            onResize(
              clamp(drag.current.width + event.clientX - drag.current.x),
            );
            event.preventDefault();
          }
        }}
        onPointerUp={(event) => {
          if (drag.current?.pointerId !== event.pointerId) return;
          drag.current = null;
          if (event.currentTarget.hasPointerCapture(event.pointerId)) {
            event.currentTarget.releasePointerCapture(event.pointerId);
          }
        }}
        onLostPointerCapture={(event) => {
          if (drag.current?.pointerId === event.pointerId) drag.current = null;
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
            onResize(
              clamp(
                Math.min(width, maxWidth()) +
                  (event.key === "ArrowRight" ? 10 : -10),
              ),
            );
            event.preventDefault();
          }
        }}
      />
    </aside>
  );
}
