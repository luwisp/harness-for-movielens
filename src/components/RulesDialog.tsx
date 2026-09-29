import { useEffect, useRef, useState } from "react";
import type { RuleSpec, Rules } from "../lib/types";
export function RulesDialog({
  catalog,
  rules,
  focusId,
  readOnly,
  version,
  onClose,
  onSave,
}: {
  catalog: RuleSpec[];
  rules: Rules;
  focusId?: string | null;
  readOnly: boolean;
  version?: string;
  onClose: () => void;
  onSave: (r: Rules) => Promise<void>;
}) {
  const [draft, setDraft] = useState<Rules>(structuredClone(rules));
  const [error, setError] = useState("");
  const focused = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (readOnly) setDraft(structuredClone(rules));
  }, [readOnly, rules]);
  useEffect(() => {
    focused.current?.scrollIntoView({ block: "center" });
  }, [focusId]);
  const categories = [...new Set(catalog.map((s) => s.category))];
  const change = (
    id: string,
    patch: Partial<{ enabled: boolean; value: any }>,
  ) =>
    setDraft((prev) => ({
      entries: { ...prev.entries, [id]: { ...prev.entries[id], ...patch } },
    }));
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="rules-modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-title">
          <div>
            <h2>数据清洗规则</h2>
            <p>
              每条规则独立生效。基础解析规则始终开启。
              {version && (
                <>
                  版本：<code>{version}</code>
                </>
              )}
            </p>
          </div>
          <button className="icon-button" onClick={onClose}>
            ×
          </button>
        </div>
        {readOnly && (
          <div className="notice">Agent 正在运行；规则暂时只读。</div>
        )}
        <div className="rules-scroll">
          {categories.map((category) => (
            <section key={category}>
              <h3>{category}</h3>
              {catalog
                .filter((s) => s.category === category)
                .map((spec) => {
                  const entry = draft.entries[spec.id];
                  if (!entry) return null;
                  const range = entry.value as {
                    min: number;
                    max: number;
                  } | null;
                  return (
                    <div
                      className={`rule-row ${focusId === spec.id ? "focused" : ""}`}
                      key={spec.id}
                      ref={focusId === spec.id ? focused : null}
                    >
                      <div className="rule-main">
                        <label>
                          <input
                            type="checkbox"
                            checked={entry.enabled}
                            disabled={readOnly || spec.required}
                            onChange={(e) =>
                              change(spec.id, { enabled: e.target.checked })
                            }
                          />
                          <strong>{spec.label}</strong>
                          {spec.required && <small>必要</small>}
                        </label>
                        <p>{spec.description}</p>
                        <code>{spec.id}</code>
                      </div>
                      {spec.input === "integer" && (
                        <input
                          aria-label={`${spec.label}参数`}
                          type="number"
                          min={spec.min}
                          max={spec.max}
                          value={Number(entry.value ?? 0)}
                          disabled={readOnly || !entry.enabled}
                          onChange={(e) =>
                            change(spec.id, { value: Number(e.target.value) })
                          }
                        />
                      )}
                      {spec.input === "range" && (
                        <div className="range-inputs">
                          <input
                            aria-label={`${spec.label}下限`}
                            type="number"
                            min={spec.min}
                            max={spec.max}
                            value={range?.min ?? 0}
                            disabled={readOnly || !entry.enabled}
                            onChange={(e) =>
                              change(spec.id, {
                                value: {
                                  ...range,
                                  min: Number(e.target.value),
                                },
                              })
                            }
                          />
                          <span>至</span>
                          <input
                            aria-label={`${spec.label}上限`}
                            type="number"
                            min={spec.min}
                            max={spec.max}
                            value={range?.max ?? 0}
                            disabled={readOnly || !entry.enabled}
                            onChange={(e) =>
                              change(spec.id, {
                                value: {
                                  ...range,
                                  max: Number(e.target.value),
                                },
                              })
                            }
                          />
                        </div>
                      )}
                    </div>
                  );
                })}
            </section>
          ))}
        </div>
        <div className="modal-footer">
          {error && <span className="error-text">{error}</span>}
          <button onClick={onClose}>关闭</button>
          {!readOnly && (
            <button
              className="primary"
              onClick={() => onSave(draft).catch((e) => setError(String(e)))}
            >
              保存规则
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
