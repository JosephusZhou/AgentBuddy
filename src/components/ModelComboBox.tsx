import { useState, useEffect, useRef, type CSSProperties } from "react";

const ChevronIcon = ({ open }: { open?: boolean }) => (
  <svg
    width="16"
    height="16"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    style={{
      transform: open ? "rotate(180deg)" : undefined,
      transition: "transform 0.15s ease",
    }}
  >
    <path d="m6 9 6 6 6-6" />
  </svg>
);

export interface ModelComboBoxProps {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  /** Available model options (e.g. from a remote fetch). When empty, renders a plain input. */
  options: string[];
  disabled?: boolean;
  placeholder?: string;
  style?: CSSProperties;
  /** When provided, renders a clear option with this label at the top of the dropdown. */
  clearLabel?: string;
}

/**
 * 可编辑模型组合框。
 *
 * 行为：
 *  - 始终渲染为输入框；`options` 为空时退化为纯文本输入。
 *  - 点击 / 聚焦时弹出下拉列表。
 *  - 输入时实时筛选列表（大小写不敏感包含匹配）。
 *  - 可从筛选后的列表点选回填，也可手动输入列表外的值。
 *  - 离焦后固定输入内容，再次点击回到输入状态并重新弹出下拉框。
 *  - Escape 关闭下拉但不关闭外层弹窗（capture 阶段拦截）。
 *  - `clearLabel` 提供时，下拉顶部显示一个清空选项（如"跟随默认模型"）。
 */
export function ModelComboBox({
  id,
  value,
  onChange,
  options,
  disabled,
  placeholder,
  style,
  clearLabel,
}: ModelComboBoxProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const blurTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  /* Close on disabled */
  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  /* Click outside to close */
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [open]);

  /* Escape to close (capture phase — stop propagation so parent modal stays open) */
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
      inputRef.current?.blur();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open]);

  /* Cleanup blur timer on unmount */
  useEffect(() => {
    return () => {
      if (blurTimer.current) clearTimeout(blurTimer.current);
    };
  }, []);

  /* No options and no clear label → plain input */
  if (options.length === 0 && !clearLabel) {
    return (
      <input
        type="text"
        className="form-input"
        id={id}
        style={style}
        placeholder={placeholder}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        disabled={disabled}
        autoComplete="off"
        spellCheck={false}
      />
    );
  }

  /* Filter options based on current input value (case-insensitive contains) */
  const q = value.trim().toLowerCase();
  const filtered = q ? options.filter((m) => m.toLowerCase().includes(q)) : options;

  const handleFocus = () => {
    if (blurTimer.current) {
      clearTimeout(blurTimer.current);
      blurTimer.current = null;
    }
    if (!disabled) setOpen(true);
  };

  const handleBlur = () => {
    /* Delay closing so option mousedown-click registers before input loses focus */
    blurTimer.current = setTimeout(() => setOpen(false), 150);
  };

  const selectOption = (model: string) => {
    onChange(model);
    setOpen(false);
    inputRef.current?.blur();
  };

  return (
    <div
      className={`model-combobox ${open ? "open" : ""} ${disabled ? "disabled" : ""}`}
      style={style}
      ref={rootRef}
    >
      <input
        ref={inputRef}
        id={id}
        className="form-input model-combobox-input"
        placeholder={placeholder}
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          if (!open) setOpen(true);
        }}
        onFocus={handleFocus}
        onBlur={handleBlur}
        disabled={disabled}
        autoComplete="off"
        spellCheck={false}
      />
      <span className="model-combobox-chevron" aria-hidden>
        <ChevronIcon open={open} />
      </span>
      {open && (
        <div className="app-select-menu" role="listbox">
          {clearLabel && (
            <button
              type="button"
              role="option"
              aria-selected={!value}
              className={`app-select-option ${!value ? "selected" : ""}`}
              onMouseDown={(e) => {
                e.preventDefault();
                selectOption("");
              }}
            >
              <span className="app-select-option-title">{clearLabel}</span>
            </button>
          )}
          {filtered.length === 0 ? (
            <div className="app-select-empty">无匹配结果</div>
          ) : (
            filtered.map((model) => (
              <button
                key={model}
                type="button"
                role="option"
                aria-selected={model === value}
                className={`app-select-option ${model === value ? "selected" : ""}`}
                onMouseDown={(e) => {
                  e.preventDefault();
                  selectOption(model);
                }}
              >
                <span className="app-select-option-title">{model}</span>
              </button>
            ))
          )}
        </div>
      )}
    </div>
  );
}
