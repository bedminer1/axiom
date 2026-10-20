<script lang="ts">
  import mascot from '$lib/assets/hound-shiba.webp';
</script>

<svelte:head>
  <title>About Hound</title>
  <meta name="description" content="Why we started Hound and the problem that inspired it." />
</svelte:head>

<main class="mx-auto max-w-6xl px-6 py-20 md:px-10 md:py-28">
  <section class="grid items-start gap-14 md:grid-cols-[1fr_0.45fr]">
    <div>
      <p class="mb-8 text-xs font-semibold uppercase tracking-[0.24em] text-orange">About</p>
      <h1 class="max-w-3xl text-[clamp(2.8rem,7vw,5.5rem)] font-semibold leading-[0.96] tracking-[-0.075em]">We want to make real work a little easier.</h1>
      <div class="mt-12 max-w-3xl space-y-8 text-sm leading-8 text-ink/72 md:text-base md:leading-9">
        <p>
          We're a group of students from <strong class="font-semibold text-ink">NUS School of Computing</strong>. We learn all this theory in school about fancy algorithms, AI and computer vision, and want to apply it to <strong class="font-semibold text-ink">improve real systems</strong>.
        </p>
        <p>
          From earlier conversations with people in construction, we've heard about <strong class="font-semibold text-ink">scattered photos and messy workflows</strong> getting in the way of inspection work.
        </p>
        <p>
          Hound is a fresh project built around that problem. We're working through the product step by step, with the people who know inspection work best.
        </p>
        <p>
          We hope to make photo organisation and report preparation <strong class="font-semibold text-ink">more automated and painless</strong>.
        </p>
      </div>
    </div>

    <img src={mascot} alt="Watercolour Shiba Inu wearing a yellow construction helmet" class="mx-auto mt-4 aspect-square w-full max-w-xs rounded-full border-[10px] border-paper object-cover shadow-[0_22px_65px_rgba(91,61,27,0.16)] md:mt-20" />
  </section>
</main>
