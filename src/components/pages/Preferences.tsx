import { THEMES, THEME_META, TEXT_SIZES, type Theme } from "../../lib/theme";
import NetworkSettings from "./NetworkSettings";

interface PreferencesProps {
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  textSize: number;
  onTextSizeChange: (size: number) => void;
}

const IS_MAC =
  typeof navigator !== "undefined" &&
  (/mac/i.test(navigator.platform) || /mac/i.test(navigator.userAgent));

// 迷你预览：system 展示半浅半深两栏；light / dark 展示对应模式的单栏。
// 颜色取固定预览令牌（--seed-preview-light/dark），与当前界面模式无关。
function ThemeSwatch({ mode }: { mode: Theme }) {
  const halves = mode === "system" ? ["light", "dark"] : [mode];
  return (
    <div className="pref-theme-swatch">
      {halves.map((c) => (
        <span key={c} className={`pref-theme-swatch-half ${c}`} />
      ))}
    </div>
  );
}

export default function Preferences({
  theme,
  onThemeChange,
  textSize,
  onTextSizeChange,
}: PreferencesProps) {
  const shortcut = IS_MAC ? "⌘+/⌘-/⌘0" : "Ctrl+/Ctrl-/Ctrl0";
  return (
    <>
      <div className="content-header">
        <h1 className="content-title">常规</h1>
      </div>
      <div className="content-body">
        <div className="pref-section">
          <div className="pref-section-title">外观</div>
          <div className="pref-card">
            <div className="pref-section-desc">选择界面主题，更改后将即时生效</div>
            <div className="pref-theme-grid">
              {THEMES.map((id) => {
                const meta = THEME_META[id];
                const selected = theme === id;
                return (
                  <button
                    type="button"
                    key={id}
                    className={`pref-theme-card ${selected ? "selected" : ""}`}
                    onClick={() => onThemeChange(id)}
                    aria-pressed={selected}
                    data-tooltip={meta.description}
                  >
                    <ThemeSwatch mode={id} />
                    <div className="pref-theme-card-info">
                      <span className="pref-theme-card-text">
                        <span className="pref-theme-card-label">{meta.label}</span>
                        <span className="pref-theme-card-desc">{meta.description}</span>
                      </span>
                    </div>
                  </button>
                );
              })}
            </div>
          </div>
        </div>

        <div className="pref-section">
          <div className="pref-section-title">文字大小</div>
          <div className="pref-card">
            <div className="pref-section-desc">
              放大 Octopus 窗口里的一切；也可用 {shortcut} 快捷调整
            </div>
            <div className="pref-text-size-row" role="radiogroup" aria-label="文字大小">
              {TEXT_SIZES.map((size) => {
                const selected = textSize === size;
                return (
                  <button
                    type="button"
                    key={size}
                    className={`pref-text-size-opt ${selected ? "selected" : ""}`}
                    onClick={() => onTextSizeChange(size)}
                    role="radio"
                    aria-checked={selected}
                  >
                    {size}%
                  </button>
                );
              })}
            </div>
          </div>
        </div>

        <NetworkSettings />
      </div>
    </>
  );
}
