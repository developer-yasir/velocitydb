import type { Config } from "tailwindcss";

const config: Config = {
  darkMode: ["class"],
  content: [
    "./src/pages/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/components/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/app/**/*.{js,ts,jsx,tsx,mdx}",
  ],
  theme: {
    extend: {
      colors: {
        background: "#080a0f",
        foreground: "#f3f4f6",
        sidebar: {
          DEFAULT: "#0b0e14",
          border: "rgba(255, 255, 255, 0.07)",
          accent: "#121721",
        },
        surface: {
          DEFAULT: "#0f131c",
          subtle: "#151b27",
          border: "rgba(255, 255, 255, 0.08)",
          hover: "#1a2232",
        },
        velocity: {
          cyan: "#00f2fe",
          blue: "#0070f3",
          orange: "#f97316",
          green: "#22c55e",
          purple: "#a855f7",
        },
      },
      fontFamily: {
        mono: ["var(--font-mono)", "JetBrains Mono", "Fira Code", "ui-monospace", "monospace"],
        sans: ["var(--font-sans)", "Inter", "system-ui", "-apple-system", "sans-serif"],
      },
      boxShadow: {
        glow: "0 0 25px -5px rgba(0, 242, 254, 0.15)",
        "glow-orange": "0 0 25px -5px rgba(249, 115, 22, 0.15)",
        "glow-green": "0 0 20px -5px rgba(34, 197, 94, 0.2)",
      },
      animation: {
        "pulse-subtle": "pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite",
        "ping-slow": "ping 2.5s cubic-bezier(0, 0, 0.2, 1) infinite",
      },
    },
  },
  plugins: [],
};

export default config;
