<script lang="ts">
  import { onMount } from 'svelte';
  import { photoStore } from '$lib/photo-store';

  type Project = { id: string; name: string; address: string; elevations: string[]; systems: string[] };
  type FilterResult = {
    decision?: string;
    decision_conf?: number;
    reasons?: string[];
    shot_type?: string;
    location?: { level_from?: number; level_to?: number; zone?: string };
    quality?: { w?: number; h?: number; blur_ok?: boolean; exposure_ok?: boolean };
    defects?: { component: string; condition: string; confidence: number }[];
    question?: string;
    match?: { status?: string; hint?: string; score?: number };
  };
  type Slot = { name: string; analysis: string; reviewed: boolean; source?: string; filter?: FilterResult };
  const sections = [
    { id: 'building', number: '01', title: 'Building' },
    { id: 'visual', number: '02', title: 'Visual inspection' },
    { id: 'close', number: '03', title: 'Close-range inspection' }
  ] as const;

  let project = $state<Project | null>(null);
  let ready = $state(false);
  let active = $state<(typeof sections)[number]['id']>('building');
  let slots = $state<Record<string, Slot>>({});
  let urls = $state<Record<string, string>>({});
  let busy = $state<Record<string, boolean>>({});
  let errors = $state<Record<string, string>>({});
  const key = (section: string, elevation: string) => `${section}:${elevation}`;
  const reviewed = $derived(project?.elevations.reduce((count, elevation) =>
    count + Number(!!slots[key('visual', elevation)]?.reviewed) + Number(!!slots[key('close', elevation)]?.reviewed), 0) ?? 0);
  const total = $derived((project?.elevations.length ?? 0) * 2);

  function save() {
    if (project) localStorage.setItem(`hound-photo-draft-${project.id}`, JSON.stringify(slots));
  }

  async function addPhoto(section: string, elevation: string, file?: File) {
    if (!file) return;
    const id = key(section, elevation);
    if (!['image/jpeg', 'image/png', 'image/webp', 'image/gif'].includes(file.type) || file.size > 12_000_000) {
      errors[id] = 'Choose a JPG, PNG, WebP or GIF under 12 MB.';
      return;
    }
    errors[id] = '';
    busy[id] = true;
    if (urls[id]) URL.revokeObjectURL(urls[id]);
    urls[id] = URL.createObjectURL(file);
    slots[id] = { name: file.name, analysis: '', reviewed: false };
    save();
    try {
      await photoStore(`${project!.id}:${id}`, file);
      const image = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = () => reject(reader.error);
        reader.readAsDataURL(file);
      });
      const response = await fetch('/api/photo/analyze', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ slot: id, project, image })
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || 'Analysis failed.');
      slots[id].analysis = result.analysis;
      slots[id].source = result.source;
      slots[id].filter = result.filter;
      save();
    } catch (error) {
      errors[id] = error instanceof Error ? error.message : 'Analysis failed. Add an observation manually.';
    } finally {
      busy[id] = false;
    }
  }

  onMount(() => {
    const load = async () => {
    try {
      project = JSON.parse(localStorage.getItem('hound-pfi-project') ?? 'null');
      if (project) {
        slots = JSON.parse(localStorage.getItem(`hound-photo-draft-${project.id}`) ?? '{}');
        for (const id of Object.keys(slots)) {
          const photo = await photoStore(`${project.id}:${id}`);
          if (photo) urls[id] = URL.createObjectURL(photo);
        }
      }
    } catch {
      errors.page = 'Saved photos could not be loaded.';
    }
    ready = true;
    };
    void load();
    return () => Object.values(urls).forEach(URL.revokeObjectURL);
  });
</script>

<svelte:head>
  <title>Inspection draft — Hound</title>
</svelte:head>

<main class="mx-auto max-w-6xl px-6 pb-24 pt-12 md:px-10">
  {#if !ready}
    <p class="text-sm text-ink/55">Loading…</p>
  {:else if !project}
    <div class="mx-auto max-w-2xl">
      <h1 class="text-4xl font-semibold tracking-tight">No project yet.</h1>
      <a class="mt-5 inline-block text-sm text-orange" href="/">Create project →</a>
    </div>
  {:else}
    <div class="mb-10 border-b border-ink/15 pb-8">
      <p class="text-xs font-semibold uppercase tracking-[0.18em] text-orange">Periodic Façade Inspection · Draft</p>
      <h1 class="mt-3 text-4xl font-semibold tracking-[-0.06em] md:text-5xl">{project.name}</h1>
      <p class="mt-2 text-sm text-ink/55">{project.address}</p>
    </div>

    <div class="grid gap-10 md:grid-cols-[13rem_minmax(0,1fr)]">
      <aside class="md:sticky md:top-8 md:self-start">
        <p class="text-xs font-semibold">{reviewed} of {total} photos reviewed</p>
        <div class="mt-3 h-1.5 overflow-hidden rounded-full bg-ink/10" role="progressbar" aria-label="Photos reviewed" aria-valuenow={reviewed} aria-valuemin="0" aria-valuemax={total}>
          <div class="h-full bg-orange" style={`width: ${total ? reviewed / total * 100 : 0}%`}></div>
        </div>
        <nav class="mt-7 flex gap-2 overflow-x-auto md:flex-col" aria-label="Report sections">
          {#each sections as section}
            <button type="button" onclick={() => active = section.id} aria-current={active === section.id ? 'step' : undefined}
              class={`min-w-max rounded-lg px-3 py-3 text-left text-xs transition ${active === section.id ? 'bg-ink text-paper' : 'text-ink/60 hover:bg-ink/5'}`}>
              <span class="mr-3 opacity-50">{section.number}</span>{section.title}
            </button>
          {/each}
        </nav>
        <a class="mt-7 inline-block text-xs text-ink/50 hover:text-orange" href="/">Edit building details →</a>
      </aside>

      <article class="min-w-0 rounded-2xl border border-ink/10 bg-white/60 p-6 dark:bg-white/[0.03] md:p-9">
        {#each sections as section}
          {#if active === section.id}
            <header class="border-b border-ink/10 pb-6">
              <p class="text-xs font-semibold text-orange">Section {section.number}</p>
              <h2 class="mt-2 text-2xl font-semibold tracking-[-0.04em]">{section.title}</h2>
            </header>
            {#if section.id === 'building'}
              <dl class="grid gap-6 py-7 text-sm sm:grid-cols-2">
                <div><dt class="text-xs text-ink/50">Building</dt><dd class="mt-1">{project.name}</dd></div>
                <div><dt class="text-xs text-ink/50">Address</dt><dd class="mt-1">{project.address}</dd></div>
                <div><dt class="text-xs text-ink/50">Elevations</dt><dd class="mt-1">{project.elevations.join(', ')}</dd></div>
                <div><dt class="text-xs text-ink/50">Exterior</dt><dd class="mt-1">{project.systems.join(', ')}</dd></div>
              </dl>
            {:else}
              {#each project.elevations as elevation}
                {@const id = key(section.id, elevation)}
                <section class="border-b border-ink/10 py-7 last:border-0">
                  <div class="flex items-center justify-between gap-3">
                    <h3 class="text-sm font-semibold">{elevation} elevation</h3>
                    <span class={`text-xs ${slots[id]?.reviewed ? 'text-emerald-700 dark:text-emerald-400' : 'text-ink/45'}`}>
                      {slots[id]?.reviewed ? 'Reviewed' : slots[id]?.name ? 'Review needed' : 'Photo needed'}
                    </span>
                  </div>
                  <div class="mt-4 grid gap-5 sm:grid-cols-[11rem_minmax(0,1fr)]">
                    <div>
                      {#if urls[id]}
                        <img src={urls[id]} alt={`${elevation} elevation inspection`} class="aspect-[4/3] w-full rounded-lg object-cover" />
                      {:else}
                        <div class="grid aspect-[4/3] place-items-center rounded-lg border border-dashed border-ink/20 text-xs text-ink/40">No photo</div>
                      {/if}
                      <label class="mt-2 inline-block cursor-pointer text-xs font-semibold text-orange hover:underline">
                        {slots[id]?.name ? 'Replace photo' : 'Add photo'}
                        <input class="sr-only" type="file" accept="image/jpeg,image/png,image/webp,image/gif" disabled={busy[id]}
                          onchange={(event) => { addPhoto(section.id, elevation, event.currentTarget.files?.[0]); event.currentTarget.value = ''; }} />
                      </label>
                    </div>
                    <div>
                      <label for={`analysis-${id}`} class="text-xs font-semibold">Photo observation</label>
                      <textarea id={`analysis-${id}`} rows="5" placeholder="Add a photo to draft an observation"
                        value={slots[id]?.analysis ?? ''}
                        oninput={(event) => { if (slots[id]) { slots[id].analysis = event.currentTarget.value; slots[id].reviewed = false; save(); } }}
                        class="mt-2 w-full rounded-lg border border-ink/15 bg-paper px-3 py-3 text-sm leading-6 outline-none focus-visible:border-orange"></textarea>
                      <div class="mt-2 flex items-center justify-between gap-3">
                        <span class="text-xs text-ink/45">{busy[id] ? 'Analyzing photo…' : slots[id]?.source === 'pfi-filter' ? 'PFI filter suggestion' : slots[id]?.source ? 'DeepSeek fallback' : slots[id]?.name || ''}</span>
                        {#if slots[id]?.analysis?.trim()}
                          <button type="button" disabled={slots[id].reviewed}
                            onclick={() => { slots[id].reviewed = true; save(); }}
                            class="text-xs font-semibold text-orange disabled:text-ink/35">{slots[id].reviewed ? 'Reviewed' : 'Mark reviewed'}</button>
                        {/if}
                      </div>
                      {#if slots[id]?.filter}
                        {@const result = slots[id].filter}
                        <details class="mt-3 border-t border-ink/10 pt-3 text-xs text-ink/65">
                          <summary class="cursor-pointer font-semibold">Photo details · {result.decision ?? 'review'} · {result.match?.status ?? 'unmatched'}</summary>
                          <div class="mt-3 space-y-2 leading-5">
                            {#if result.decision_conf != null}<p>Decision confidence: {Math.round(result.decision_conf * 100)}%</p>{/if}
                            {#if result.match?.score != null}<p>Slot match: {Math.round(result.match.score * 100)}%</p>{/if}
                            {#if result.shot_type}<p>Shot: {result.shot_type.replaceAll('_', ' ')}</p>{/if}
                            {#if result.location?.level_from != null}<p>Level: {result.location.level_from}{result.location.level_to != null && result.location.level_to !== result.location.level_from ? `–${result.location.level_to}` : ''}{result.location.zone ? ` · ${result.location.zone}` : ''}</p>{/if}
                            {#if result.quality}<p>Image: {result.quality.w} × {result.quality.h}{result.quality.blur_ok === false ? ' · blurry' : ''}{result.quality.exposure_ok === false ? ' · exposure issue' : ''}</p>{/if}
                            {#if result.defects?.length}<p>Visible defects: {result.defects.map((defect) => `${defect.component} · ${defect.condition} (${Math.round(defect.confidence * 100)}%)`).join(', ')}</p>{/if}
                            {#if result.reasons?.length}<p>Filter notes: {result.reasons.join(', ').replaceAll('_', ' ')}</p>{/if}
                            {#if result.match?.hint}<p>{result.match.hint}</p>{/if}
                            {#if result.question}<p>Check: {result.question}</p>{/if}
                          </div>
                        </details>
                      {/if}
                      {#if errors[id]}<p class="mt-2 text-xs text-red-700" role="alert">{errors[id]}</p>{/if}
                    </div>
                  </div>
                </section>
              {/each}
            {/if}
            <div class="mt-5 flex justify-end">
              {#if section.id !== 'close'}
                <button type="button" onclick={() => active = section.id === 'building' ? 'visual' : 'close'
                  } class="rounded-full bg-ink px-5 py-3 text-xs font-semibold text-paper hover:bg-orange hover:text-ink">Next section →</button>
              {/if}
            </div>
          {/if}
        {/each}
        {#if errors.page}<p class="mt-4 text-xs text-red-700" role="alert">{errors.page}</p>{/if}
      </article>
    </div>
  {/if}
</main>
