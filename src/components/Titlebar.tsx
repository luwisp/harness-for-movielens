import { getCurrentWindow } from "@tauri-apps/api/window";
export function Titlebar({
  onToggle,
  collapsed,
}: {
  onToggle: () => void;
  collapsed: boolean;
}) {
  const win = getCurrentWindow();
  return (
    <div className="titlebar" data-tauri-drag-region>
      <button
        className="icon-button"
        title={collapsed ? "展开侧边栏" : "收起侧边栏"}
        aria-label={collapsed ? "展开侧边栏" : "收起侧边栏"}
        aria-expanded={!collapsed}
        onClick={onToggle}
      >
        ☰
      </button>
      <div className="titlebar-name" data-tauri-drag-region>
        MovieLens Agent
      </div>
      <div className="window-actions">
        <button title="最小化" onClick={() => win.minimize()}>
          ─
        </button>
        <button title="最大化或还原" onClick={() => win.toggleMaximize()}>
          □
        </button>
        <button title="关闭" className="close" onClick={() => win.close()}>
          ×
        </button>
      </div>
    </div>
  );
}
