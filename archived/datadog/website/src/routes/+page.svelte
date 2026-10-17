<script lang="ts">
  import { onMount } from 'svelte';
  import mascot from '$lib/assets/datadog-shiba.webp';

  let dark = false;

  onMount(() => {
    dark = document.documentElement.classList.contains('dark');
  });

  function toggleTheme() {
    dark = !dark;
    document.documentElement.classList.toggle('dark', dark);
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    document.querySelector<HTMLMetaElement>('#theme-color')?.setAttribute('content', dark ? '#111110' : '#ffffff');
    localStorage.setItem('datadog-theme', dark ? 'dark' : 'light');
  }

  const features = [
    { number: '01', title: 'Organise evidence', text: 'Add DataDog to an existing Telegram group and it starts listening, sorting photos and tracking requirements. No clunky new app to onboard.' },
    { number: '02', title: 'Review photos', text: 'Use computer vision to surface possible defects for an inspector to review.' },
    { number: '03', title: 'Draft reports', text: 'Turn checked evidence into a structured first draft, ready for professional review.' }
  ];
</script>

<svelte:head>
  <title>DataDog — less admin, clearer inspections</title>
  <meta name="description" content="A Telegram assistant for organising inspection photos, checking evidence and drafting reports." />
  <link rel="icon" href="/favicon.png" />
</svelte:head>

<div class="min-h-screen overflow-hidden">
  <header class="mx-auto flex max-w-6xl items-center justify-between px-6 py-7 md:px-10">
    <a href="#top" class="flex items-center gap-3" aria-label="DataDog home">
      <img src={mascot} alt="DataDog Shiba mascot" class="h-11 w-11 rounded-full border border-ink/10 object-cover" />
      <span class="text-sm font-semibold tracking-[-0.03em]">DataDog</span>
    </a>
    <div class="flex items-center gap-5">
      <a href="#why" class="text-xs text-ink/55 transition hover:text-orange">Why we built it ↓</a>
      <button
        type="button"
        onclick={toggleTheme}
        aria-label={dark ? 'Switch to light mode' : 'Switch to dark mode'}
        class="grid h-9 w-9 place-items-center rounded-full border border-ink/15 text-sm text-ink/65 transition hover:border-orange hover:text-orange"
      >
        {dark ? '☼' : '◐'}
      </button>
    </div>
  </header>

  <main id="top">
    <section class="relative mx-auto grid min-h-[72vh] max-w-6xl items-center gap-10 px-6 pb-24 pt-10 md:grid-cols-[1fr_0.72fr] md:px-10 md:pt-20">
      <div class="relative z-10">
        <p class="mb-6 text-xs font-semibold uppercase tracking-[0.24em] text-orange">DataDog</p>
        <h1 class="max-w-3xl text-[clamp(3rem,7.2vw,6.6rem)] font-semibold leading-[0.94] tracking-[-0.075em]">
          Inspections<br />should be
          <span class="relative inline-block text-ink/45 after:absolute after:left-[-0.04em] after:right-[-0.04em] after:top-1/2 after:h-[0.08em] after:-rotate-3 after:bg-orange after:content-['']">hard</span>
          <span class="ml-2 inline-block rotate-[-7deg] font-hand text-[1.25em] font-normal tracking-normal text-orange md:ml-4">easy</span>
        </h1>
      </div>

      <a
        href="https://t.me/axiom_datadog_bot"
        target="_blank"
        rel="noreferrer"
        aria-label="Chat with the DataDog bot on Telegram"
        class="group relative mx-auto block w-full max-w-sm cursor-pointer rounded-full outline-none md:max-w-md"
      >
        <div class="dog-halo watercolor absolute -inset-5 opacity-70 transition duration-300 group-hover:scale-105 group-focus-visible:scale-105"></div>
        <img src={mascot} alt="Watercolour Shiba Inu wearing a yellow construction helmet" class="relative aspect-square w-full rounded-full border-[10px] border-paper object-cover shadow-[0_22px_65px_rgba(91,61,27,0.16)] transition duration-300 ease-out group-hover:-translate-y-2 group-hover:scale-[1.025] group-hover:rotate-1 group-hover:shadow-[0_30px_80px_rgba(91,61,27,0.28)] group-focus-visible:-translate-y-2 group-focus-visible:scale-[1.025] group-focus-visible:ring-4 group-focus-visible:ring-orange/40" />
        <span class="absolute bottom-7 left-1/2 z-10 -translate-x-1/2 translate-y-3 rounded-full bg-ink px-5 py-3 text-[11px] font-semibold text-paper opacity-0 shadow-lg transition duration-300 group-hover:translate-y-0 group-hover:opacity-100 group-focus-visible:translate-y-0 group-focus-visible:opacity-100">Open Telegram ↗</span>
        <p class="absolute -bottom-9 right-3 text-xs font-semibold text-ink/55 transition group-hover:text-orange group-focus-visible:text-orange">try it ↗</p>
      </a>
    </section>

    <section class="border-y border-ink/10 bg-white/35 dark:bg-white/[0.025]">
      <div class="mx-auto grid max-w-6xl md:grid-cols-3">
        {#each features as feature}
          <article class="border-b border-ink/10 px-7 py-12 last:border-b-0 md:border-b-0 md:border-r md:last:border-r-0 md:px-9">
            <span class="text-sm font-semibold text-orange">{feature.number}</span>
            <h2 class="mt-5 text-base font-semibold tracking-[-0.04em]">{feature.title}</h2>
            <p class="mt-3 text-xs leading-6 text-ink/58">{feature.text}</p>
          </article>
        {/each}
      </div>
    </section>

    <section id="why" class="mx-auto max-w-4xl px-6 py-28 md:px-10 md:py-40">
      <p class="mb-10 text-xl font-semibold tracking-[-0.05em] text-orange md:text-2xl">Why we're doing this</p>
      <div class="space-y-8 text-sm leading-8 text-ink/72 md:text-base md:leading-9">
        <p>
          We're a group of students from <strong class="font-semibold text-ink">NUS School of Computing</strong>. We learn all this theory in school about fancy algorithms, AI and computer vision, but we want to apply it and see how it can <strong class="font-semibold text-ink">improve real systems</strong>.
        </p>
        <p>
          From earlier conversations with people in construction, they've painted a picture of <strong class="font-semibold text-ink">scattered photos and messy workflows</strong> being the biggest pain in the ass.
        </p>
        <p>
          So we built a bot and named it <strong class="font-semibold text-ink">DataDog</strong>. Throw it all the messy data and it'll fetch it back as a <strong class="font-semibold text-ink">neat draft report</strong>.
        </p>
        <p>
          We hope to share this project with people interested in making photo parsing and report writing <strong class="font-semibold text-ink">more automated and painless</strong>.
        </p>
      </div>
    </section>
  </main>

  <footer class="border-t border-ink/10 px-6 py-9 text-center text-xs text-ink/55">
    Message me on
    <a class="font-semibold text-ink underline decoration-orange/70 underline-offset-4 transition hover:text-orange" href="https://t.me/dkffjlrtm" target="_blank" rel="noreferrer">Telegram @dkffjlrtm</a>
    <span class="mx-2 text-ink/25">·</span>
    or email
    <a class="font-semibold text-ink underline decoration-orange/70 underline-offset-4 transition hover:text-orange" href="mailto:e1528290@u.nus.edu">e1528290@u.nus.edu</a>
  </footer>
</div>
