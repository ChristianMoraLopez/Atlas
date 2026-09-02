/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        ink: "#17201f",
        pine: "#0f4f45",
        mint: "#d9efe8",
        cream: "#f6f3ea",
        coral: "#e9704f"
      },
      fontFamily: {
        sans: ["Aptos", "Segoe UI", "sans-serif"],
        display: ["Georgia", "Cambria", "serif"]
      },
      boxShadow: { panel: "0 20px 60px rgba(12, 48, 42, .10)" }
    }
  },
  plugins: []
};
