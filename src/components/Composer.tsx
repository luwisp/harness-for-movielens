import { useEffect, useRef, useState } from "react";
export function Composer({
  disabled,
  busy,
  onSend,
  onStop,
  onRules,
}: {
  disabled: boolean;
  busy: boolean;
  onSend: (v: string) => Promise<void>;
  onStop: () => void;
  onRules: () => void;
}) {
  const [value, setValue] = useState("");
  const [menu, setMenu] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (box.current) {
      box.current.style.height = "28px";
      box.current.style.height = Math.min(box.current.scrollHeight, 160) + "px";
    }
  }, [value]);
  const send = async () => {
    const content = value.trim();
    if (!content || disabled || busy) return;
    setValue("");
    try {
      await onSend(content);
    } catch {
      setValue(content);
    }
  };
  useEffect(() => {
    if (!menu) return;

    const handleClickOutside = (e: MouseEvent) => {
      if (
        menuRef.current &&
        !menuRef.current.contains(e.target as Node)
      ) {
        setMenu(false);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [menu]);
  return (
    <div className="composer-area">
      <div className="composer">
        <div className="composer-row">
          <div className="composer-menu" ref={menuRef}>
            <button
              className="icon-button"
              title="对话选项"
              onClick={() => setMenu(!menu)}
            >
              ⚙
            </button>
            {menu && (
              <div className="composer-popover">
                <button
                  onClick={() => {
                    setMenu(false);
                    onRules();
                  }}
                >
                  数据清洗规则与参数
                </button>
              </div>
            )}
          </div>
          <textarea
            ref={box}
            rows={1}
            value={value}
            placeholder="询问数据，或描述清洗与评估任务…"
            disabled={disabled || busy}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
            }}
          />
          {busy ? (
            <button className="send-button stop" onClick={onStop} title="中断">
              ■
            </button>
          ) : (
            <button
              className="send-button"
              title="发送"
              disabled={disabled || !value.trim()}
              onClick={() => void send()}
            >
              ↑
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
