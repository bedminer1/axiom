<script lang="ts">
  import { onMount } from 'svelte';
  import '../app.css';
  let { children } = $props();

  let dark = $state(false);

  onMount(() => {
    dark = document.documentElement.classList.contains('dark');
  });

  function toggleTheme() {
    dark = !dark;
    document.documentElement.classList.toggle('dark', dark);
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    document.querySelector<HTMLMetaElement>('#theme-color')?.setAttribute('content', dark ? '#111110' : '#ffffff');
    localStorage.setItem('hound-theme', dark ? 'dark' : 'light');
  }
</script>

<div class="min-h-screen overflow-hidden">
  <header class="mx-auto flex max-w-6xl items-center justify-between px-6 py-7 md:px-10">
    <a href="/" class="text-sm font-semibold tracking-[-0.03em]" aria-label="Hound home">Hound</a>
    <nav class="flex items-center gap-5" aria-label="Main navigation">
      <a href="/about" class="text-xs text-ink/55 transition hover:text-orange">About</a>
      <a href="https://github.com/bedminer1/axiom/blob/main/hound/legal/PRIVACY.md" class="text-xs text-ink/55 transition hover:text-orange">Privacy</a>
      <a href="https://github.com/bedminer1/axiom/blob/main/hound/legal/TERMS.md" class="text-xs text-ink/55 transition hover:text-orange">Terms</a>
      <button
        type="button"
        onclick={toggleTheme}
        aria-label={dark ? 'Switch to light mode' : 'Switch to dark mode'}
        class="grid h-9 w-9 place-items-center rounded-full border border-ink/15 text-sm text-ink/65 transition hover:border-orange hover:text-orange"
      >
        {dark ? '☼' : '◐'}
      </button>
    </nav>
  </header>

{@render children()}

  <footer class="border-t border-ink/10 px-6 py-8 text-center text-xs text-ink/55">
    Hound <span class="mx-2 text-ink/25">·</span> Building inspections, in progress
  </footer>
</div>
