/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        obsidian: {
          950: "#060911",
          900: "#0b101b",
          800: "#111827",
          700: "#1c2638",
        },
      },
    },
  },
  plugins: [],
};
