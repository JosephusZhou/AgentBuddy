// 主题三模式：全应用主题的唯一事实源。
// 合法值仅 system（跟随系统）/ light / dark，写入 <html data-theme> 与后端 config.json。
// 样式上参照 magpie：system 不设置 data-theme，由 CSS `prefers-color-scheme` 接管；
// light / dark 显式设置 data-theme 强制对应模式。

export type Theme = "system" | "light" | "dark";

/** 常规设置中的展示顺序。 */
export const THEMES: readonly Theme[] = ["system", "light", "dark"];

export const DEFAULT_THEME: Theme = "system";

/** 文字大小档位（%），与 magpie 一致：100 / 110 / 125 / 150。 */
export const TEXT_SIZES: readonly number[] = [100, 110, 125, 150];

/** 默认档位：100%（用户要求）。非法值与缺失配置均回退到这一档。 */
export const DEFAULT_TEXT_SIZE = 100;

export const THEME_META: Record<Theme, { label: string; description: string }> = {
  system: { label: "跟随系统", description: "自动跟随操作系统的浅色 / 深色外观" },
  light: { label: "浅色", description: "始终保持浅色外观" },
  dark: { label: "深色", description: "始终保持深色外观" },
};

const THEME_SET: ReadonlySet<string> = new Set(THEMES);

export interface AppConfig {
  theme: Theme;
  textSize: number;
}

/** 未知/非法值一律回退默认（system），保证 data-theme 永远有效。 */
function normalizeTheme(value: unknown): Theme {
  return typeof value === "string" && THEME_SET.has(value) ? (value as Theme) : DEFAULT_THEME;
}

/** 存储值是否已是合法主题（用于判断启动时是否需要还原回写）。 */
function isKnownTheme(value: unknown): value is Theme {
  return typeof value === "string" && THEME_SET.has(value);
}

/** 非法文字大小一律回退默认（100）。 */
function normalizeTextSize(value: unknown): number {
  return typeof value === "number" && TEXT_SIZES.includes(value) ? value : DEFAULT_TEXT_SIZE;
}

/** 应用主题：system 移除 data-theme（CSS 媒体查询接管），light/dark 显式设置。 */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === "system") {
    root.removeAttribute("data-theme");
  } else {
    root.setAttribute("data-theme", theme);
  }
}

/**
 * 系统外观变化监听：system 模式下 CSS 会自动重绘（窗口 appearance 已为
 * 跟随系统，prefers-color-scheme 反映真实系统），这里只负责通知上层
 * 重新同步原生窗口外观（macOS traffic lights / Windows 标题栏）。
 * 返回取消订阅函数。
 */
export function subscribeSystemTheme(onChange: () => void): () => void {
  const mql = window.matchMedia("(prefers-color-scheme: dark)");
  if (typeof mql.addEventListener === "function") {
    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }
  mql.addListener(onChange);
  return () => mql.removeListener(onChange);
}

/**
 * 将当前主题同步到原生窗口外观（macOS traffic lights / Windows 标题栏）。
 * 直接透传 system/light/dark：system 时后端把窗口恢复为跟随系统
 * （macOS setAppearance:nil / Windows DWM -1）。注意不能在这里用
 * matchMedia 把 system 解析成 light/dark——macOS 上 WKWebView 的
 * prefers-color-scheme 跟随窗口 NSAppearance，若窗口还停留在深色，
 * 解析结果会是错误的 dark，导致窗口永远无法恢复跟随系统。
 */
export async function syncWindowAppearance(theme: Theme): Promise<void> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("set_window_appearance", { category: theme });
  } catch {
    // Non-tauri environment — no-op.
  }
}

/**
 * 应用文字大小（webview 层页面缩放）：写入 CSS `--zoom` 变量，供与原生元素
 * （macOS traffic lights 等）对齐的尺寸反算使用（见 index.css 的
 * `calc(px / var(--zoom, 1))`）。真正的页面缩放由后端 set_text_size 完成。
 */
export function applyTextSize(percent: number): void {
  const next = normalizeTextSize(percent);
  document.documentElement.style.setProperty("--zoom", String(next / 100));
}

export async function loadAppConfig(): Promise<AppConfig> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const config = await (invoke("get_app_config") as Promise<{
      theme?: string;
      textSize?: number;
    }>);
    if (isKnownTheme(config?.theme)) {
      await syncWindowAppearance(config.theme);
      return { theme: config.theme, textSize: normalizeTextSize(config?.textSize) };
    }
    // 配置文件里是旧的多主题 id 或非法值（后端启动时已改写为 system，
    // 这里再兜底一次，保证 data-theme 与 config 一致）。
    const fallback = await saveTheme(DEFAULT_THEME);
    return { theme: fallback, textSize: normalizeTextSize(config?.textSize) };
  } catch {
    // Browser / non-tauri preview: fall back to default.
    return { theme: DEFAULT_THEME, textSize: DEFAULT_TEXT_SIZE };
  }
}

export async function saveTheme(theme: Theme): Promise<Theme> {
  const next = normalizeTheme(theme);
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const config = await (invoke("set_theme", { theme: next }) as Promise<{ theme?: string }>);
    const saved = normalizeTheme(config?.theme);
    await syncWindowAppearance(saved);
    return saved;
  } catch {
    return next;
  }
}

/** 保存文字大小（后端校验档位并应用 webview 缩放），返回实际落盘值。 */
export async function saveTextSize(percent: number): Promise<number> {
  const next = normalizeTextSize(percent);
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const config = await (invoke("set_text_size", { percent: next }) as Promise<{
      textSize?: number;
    }>);
    return normalizeTextSize(config?.textSize);
  } catch {
    return next;
  }
}
