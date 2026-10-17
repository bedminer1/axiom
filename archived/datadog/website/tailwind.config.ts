import type { Config } from 'tailwindcss';

export default {
  darkMode: 'class',
  content: ['./src/**/*.{html,js,svelte,ts}'],
  theme: {
    extend: {
      colors: {
        ink: 'rgb(var(--ink) / <alpha-value>)',
        paper: 'rgb(var(--paper) / <alpha-value>)',
        orange: '#e88928',
        'orange-soft': '#f5c77f'
      },
      fontFamily: {
        mono: ['JetBrains Mono', 'ui-monospace', 'monospace'],
        hand: ['Reenie Beanie', 'Comic Sans MS', 'cursive']
      }
    }
  },
  plugins: []
} satisfies Config;
