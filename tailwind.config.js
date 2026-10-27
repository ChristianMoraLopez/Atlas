/** @type {import('tailwindcss').Config} */
const token = name => `rgb(var(--atlas-${name}) / <alpha-value>)`;

export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      // Colours come from src/design/tokens.css so the personal accent applies everywhere.
      colors: {
        ink: token("ink"),
        pine: token("accent"),
        "pine-deep": token("accent-deep"),
        mint: token("accent-soft"),
        glow: token("glow"),
        cream: token("paper"),
        sheet: token("surface"),
        night: token("night"),
        coral: token("signal"),
        brass: token("brass")
      },
      fontFamily: {
        sans: ["Instrument Sans Variable", "Segoe UI", "system-ui", "sans-serif"],
        display: ["Fraunces Variable", "Georgia", "Cambria", "serif"],
        mono: ["JetBrains Mono Variable", "Consolas", "ui-monospace", "monospace"]
      },
      boxShadow: { panel: "var(--atlas-shadow-sheet)" }
    }
  },
  plugins: []
};
