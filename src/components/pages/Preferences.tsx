import { THEMES, type Theme, type ThemeCategory } from "../../lib/theme";

interface PreferencesProps {
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
}

// 分组顺序与标题：偏好设置里先浅色后深色。
const GROUPS: { category: ThemeCategory; title: string }[] = [
  { category: "light", title: "浅色主题" },
  { category: "dark", title: "深色主题" },
];

// 迷你预览：外层 data-theme 让内部元素直接吃到该主题的 --seed-* 令牌，
// 因此预览配色永远与真实主题保持一致，无需单独维护色值。
function ThemeSwatch() {
  return (
    <div className="pref-theme-swatch">
      <div className="pref-theme-swatch-bar">
        <span className="pref-theme-swatch-dot" />
        <span className="pref-theme-swatch-dot" />
        <span className="pref-theme-swatch-dot" />
      </div>
      <div className="pref-theme-swatch-body">
        <span className="pref-theme-swatch-accent" />
        <div className="pref-theme-swatch-lines">
          <span className="pref-theme-swatch-line" />
          <span className="pref-theme-swatch-line short" />
        </div>
      </div>
    </div>
  );
}

export default function Preferences({ theme, onThemeChange }: PreferencesProps) {
  return (
    <>
      <div className="content-header">
        <h1 className="content-title">偏好设置</h1>
      </div>
      <div className="content-body">
        <div className="pref-section">
          <div className="pref-section-title">外观</div>
          <div className="pref-section-desc">
            选择界面主题，更改后将即时生效
          </div>

          {GROUPS.map(({ category, title }) => {
            const items = THEMES.filter((t) => t.category === category);
            if (items.length === 0) return null;
            return (
              <div className="pref-theme-group" key={category}>
                <div className="pref-theme-group-title">{title}</div>
                <div className="pref-theme-grid">
                  {items.map((item) => {
                    const selected = theme === item.id;
                    return (
                      <button
                        type="button"
                        key={item.id}
                        className={`pref-theme-card ${selected ? "selected" : ""}`}
                        onClick={() => onThemeChange(item.id)}
                        aria-pressed={selected}
                        title={item.description}
                      >
                        {/* data-theme 局部作用域：让预览吃到对应主题令牌 */}
                        <div data-theme={item.id}>
                          <ThemeSwatch />
                        </div>
                        <div className="pref-theme-card-info">
                          <span className="pref-theme-card-radio">
                            <span className="pref-theme-card-radio-dot" />
                          </span>
                          <span className="pref-theme-card-text">
                            <span className="pref-theme-card-label">
                              {item.label}
                            </span>
                            <span className="pref-theme-card-desc">
                              {item.description}
                            </span>
                          </span>
                        </div>
                      </button>
                    );
                  })}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </>
  );
}
