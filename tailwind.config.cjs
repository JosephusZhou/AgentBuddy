/** @type {import('tailwindcss').Config} */
/* 设计令牌桥：把 index.css 中的 --seed-* / --text-* / --space-* CSS 变量映射为
   Tailwind 工具类，使组件可以写 bg-seed-surface、text-seed-muted、rounded-seed-md 等，
   而不是散落硬编码色值。新增令牌时在此同步登记。 */
module.exports = {
  content: ["./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        seed: {
          bg: "var(--seed-bg)",
          fg: "var(--seed-fg)",
          surface: "var(--seed-surface)",
          "surface-alt": "var(--seed-surface-alt)",
          border: "var(--seed-border)",
          "border-subtle": "var(--seed-border-subtle)",
          primary: "var(--seed-primary)",
          "primary-fg": "var(--seed-primary-fg)",
          muted: "var(--seed-muted)",
          hover: "var(--seed-hover)",
          "active-bg": "var(--seed-active-bg)",
          "active-fg": "var(--seed-active-fg)",
          danger: "var(--seed-danger)",
          "danger-fg": "var(--seed-danger-fg)",
          "danger-bg": "var(--seed-danger-bg)",
          "input-border": "var(--seed-input-border)",
          "input-focus": "var(--seed-input-focus)",
          accent: "var(--seed-accent)",
          "fill-hover": "var(--seed-fill-hover)",
          "fill-active": "var(--seed-fill-active)",
        },
      },
      fontSize: {
        "seed-xs": "var(--text-xs)",
        "seed-sm": "var(--text-sm)",
        "seed-base": "var(--text-base)",
        "seed-lg": "var(--text-lg)",
        "seed-xl": "var(--text-xl)",
        "seed-2xl": "var(--text-2xl)",
      },
      borderRadius: {
        "seed-sm": "var(--seed-radius-sm)",
        seed: "var(--seed-radius)",
        "seed-md": "var(--seed-radius-md)",
        "seed-lg": "var(--seed-radius-lg)",
      },
      boxShadow: {
        "seed-card": "var(--seed-shadow-card)",
        "seed-pop": "var(--seed-shadow-pop)",
        "seed-modal": "var(--seed-shadow-modal)",
      },
      spacing: {
        "seed-1": "var(--space-1)",
        "seed-2": "var(--space-2)",
        "seed-3": "var(--space-3)",
        "seed-4": "var(--space-4)",
        "seed-5": "var(--space-5)",
        "seed-6": "var(--space-6)",
        "seed-8": "var(--space-8)",
        "seed-10": "var(--space-10)",
      },
    },
  },
  plugins: [],
};
