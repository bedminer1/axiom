<script lang="ts">
  import { onMount } from 'svelte';
  import { photoStore, photoDelete } from '$lib/photo-store';
  import { downloadReport } from '$lib/report';

  type Project = {
    id: string; name: string; address: string; owner?: string; noticeReference?: string;
    elevations: string[]; systems: string[]; usesDrone?: boolean;
    visualDate?: string; visualDuration?: string; visualMethod?: string; visualLimitations?: string;
    closeDate?: string; closeDuration?: string; closeMethod?: string; closeBasis?: string;
    closeCoverage?: Record<string, number>; previousCondition?: string; inspector?: string;
    closeAreas?: Record<string, { facade?: number; inspected?: number }>;
    visualDateAuto?: boolean; closeDateAuto?: boolean;
  };
  type FilterResult = {
    decision?: string;
    decision_conf?: number;
    reasons?: string[];
    shot_type?: string;
    meta?: { taken_at?: string; taken_at_source?: string };
    storeys_visible?: number;
    elements?: string[];
    location?: { elevation?: string; level_from?: number; level_to?: number; zone?: string };
    quality?: { w?: number; h?: number; blur_ok?: boolean; exposure_ok?: boolean };
    defects?: { component: string; condition: string; confidence: number; suggested_condition?: string; verification?: string[] }[];
    question?: string;
    match?: { status?: string; hint?: string; score?: number };
    matched_side?: string;
  };
  type Slot = {
    name: string; analysis: string; reviewed: boolean; source?: string; filter?: FilterResult;
    level?: string; zone?: string; defectType?: string; classification?: string;
    extent?: string; structuralConcern?: string; verification?: string;
    possibleCause?: string; recommendation?: string; timeframe?: string;
    immediateAction?: string; supervision?: string;
    photoDate?: string; storeysVisible?: number;
    side?: string; mapX?: number; mapY?: number; mapSource?: string;
    exteriorSystem?: string;
    repairSuggestion?: { recommendation?: string; timeframe?: string; immediate_action?: string };
  };
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
  let exporting = $state(false);
  let planUrl = $state('');
  let planName = $state('');
  let planWidth = $state(0);
  let planHeight = $state(0);
  const key = (section: string, elevation: string) => `${section}:${elevation}`;
  const closeIds = $derived(Object.keys(slots).filter((id) => id.startsWith('close:')));
  const hasVisualPhotos = $derived(project?.elevations.some((side) => !!urls[key('visual', side)]) ?? false);
  const coveredSystems = $derived(project?.systems.filter((system) => closeIds.some((id) => slots[id].exteriorSystem === system && slots[id].reviewed)).length ?? 0);
  const reviewed = $derived((project?.elevations.reduce((count, elevation) => count + Number(!!slots[key('visual', elevation)]?.reviewed), 0) ?? 0) + coveredSystems);
  const total = $derived((project?.elevations.length ?? 0) + (project?.systems.length ?? 0));
  const reportReady = $derived(total > 0 && reviewed === total && closeIds.every((id) => slots[id].reviewed));
  const maxStoreys = $derived(Math.max(0, ...Object.values(slots).map((slot) => slot.storeysVisible ?? 0)));
  const dateFrom = (stamp?: string) => /^\d{4}-\d{2}-\d{2}/.exec(stamp ?? '')?.[0] ?? '';
  const words = (text: string) => text.replaceAll('_', ' ');
  function guessSystem(elements: string[] = []) {
    const seen = elements.join(' ').toLowerCase();
    const match = project?.systems.find((system) => {
      const value = system.toLowerCase();
      return (/(glass|curtain)/.test(value) && /(glass|curtain)/.test(seen)) ||
        (value.includes('window') && seen.includes('window')) ||
        (/(concrete|plaster)/.test(value) && /(concrete|plaster)/.test(seen)) ||
        (/(brick|stone)/.test(value) && /(brick|stone|masonry)/.test(seen)) ||
        (value.includes('metal') && /(metal|cladding)/.test(seen));
    });
    return match || (project?.systems.length === 1 ? project.systems[0] : '');
  }
  function setCoverage(side: string, field: 'facade' | 'inspected', input: string) {
    if (!project) return;
    project.closeAreas ??= {};
    project.closeCoverage ??= {};
    project.closeAreas[side] = { ...project.closeAreas[side], [field]: input === '' ? undefined : Number(input) };
    const { facade, inspected } = project.closeAreas[side];
    if (facade && inspected != null && inspected <= facade) project.closeCoverage[side] = Math.round(inspected / facade * 1000) / 10;
    else delete project.closeCoverage[side];
    localStorage.setItem('hound-pfi-project', JSON.stringify(project));
  }
  function changeSystem(id: string, system: string) {
    slots[id].exteriorSystem = system;
    slots[id].reviewed = false;
    save();
  }
  const sides = ['North', 'East', 'South', 'West'];
  const asSide = (value?: string) => sides.find((side) => side.toLowerCase() === value?.toLowerCase() || side[0].toLowerCase() === value?.toLowerCase()) ?? '';
  function mapGuess(name: string, side: string) {
    const position = 28 + [...name].reduce((sum, letter) => sum + letter.charCodeAt(0), 0) % 44;
    if (side === 'North') return { x: position, y: 17 };
    if (side === 'East') return { x: 83, y: position };
    if (side === 'South') return { x: position, y: 83 };
    if (side === 'West') return { x: 17, y: position };
    return { x: 50, y: 50 };
  }
  function place(id: string, event: MouseEvent) {
    const area = event.currentTarget as HTMLElement;
    const bounds = area.getBoundingClientRect();
    slots[id].mapX = Math.round((event.clientX - bounds.left) / bounds.width * 100);
    slots[id].mapY = Math.round((event.clientY - bounds.top) / bounds.height * 100);
    slots[id].mapSource = 'reviewer placed';
    slots[id].reviewed = false;
    save();
  }
  function changeSide(id: string, side: string) {
    slots[id].side = side;
    if (slots[id].mapSource !== 'reviewer placed') {
      const point = mapGuess(slots[id].name, side);
      slots[id].mapX = point.x;
      slots[id].mapY = point.y;
      slots[id].mapSource = 'suggested from side';
    }
    slots[id].reviewed = false;
    save();
  }
  function resetMapPositions() {
    for (const id of Object.keys(slots).filter((value) => value.startsWith('close:'))) {
      const point = mapGuess(slots[id].name, slots[id].side ?? '');
      slots[id].mapX = point.x; slots[id].mapY = point.y;
      slots[id].mapSource = slots[id].side ? 'suggested from side' : 'rough guess';
      slots[id].reviewed = false;
    }
    save();
  }
  async function addPlan(file?: File) {
    if (!file || !project) return;
    if (!['image/jpeg', 'image/png', 'image/webp'].includes(file.type) || file.size > 12_000_000) { errors.plan = 'Choose a JPG, PNG or WebP plan under 12 MB.'; return; }
    await photoStore(`${project.id}:floor-plan`, file);
    const bitmap = await createImageBitmap(file);
    planWidth = bitmap.width; planHeight = bitmap.height; bitmap.close();
    if (planUrl) URL.revokeObjectURL(planUrl);
    planUrl = URL.createObjectURL(file);
    planName = file.name;
    resetMapPositions();
    errors.plan = '';
  }
  async function removePlan() {
    if (!project) return;
    await photoDelete(`${project.id}:floor-plan`);
    if (planUrl) URL.revokeObjectURL(planUrl);
    planUrl = ''; planName = ''; planWidth = 0; planHeight = 0;
    resetMapPositions();
  }
  async function removeClose(id: string) {
    if (!project) return;
    await photoDelete(`${project.id}:${id}`);
    if (urls[id]) URL.revokeObjectURL(urls[id]);
    delete urls[id]; delete slots[id]; delete errors[id];
    save();
  }
  function actionFor(condition: string) {
    if (/crack/i.test(condition)) return 'Check the crack and adjacent areas on site; specify repair after its extent and cause are confirmed.';
    if (/spall|loose|delaminat/i.test(condition)) return 'Assess and secure loose material where needed; inspect adjacent areas and specify repair.';
    if (/corros|rust/i.test(condition)) return 'Inspect affected metal and fixings; specify treatment or replacement after physical assessment.';
    return 'Inspect the affected element and similar areas; specify remedial works after confirming the defect.';
  }

  function save() {
    if (project) localStorage.setItem(`hound-photo-draft-${project.id}`, JSON.stringify(slots));
  }

  async function exportReport() {
    if (!project || exporting || !reportReady) return;
    exporting = true;
    errors.report = '';
    try {
      await downloadReport(project, slots);
    } catch (error) {
      errors.report = error instanceof Error ? error.message : 'Could not generate the Word document.';
    } finally {
      exporting = false;
    }
  }

  function markReviewed(id: string) {
    const slot = slots[id];
    if (!slot) return;
    const repair = slot.classification === 'Require Repair' || slot.classification === 'Unsafe';
    if (!slot.analysis.trim() || !slot.classification || !slot.structuralConcern ||
      (repair && (!slot.extent || !slot.recommendation?.trim() || !slot.timeframe?.trim())) ||
      (slot.classification === 'Unsafe' && !slot.immediateAction?.trim())) {
      errors[id] = 'Confirm the assessment fields before marking reviewed.';
      return;
    }
    errors[id] = '';
    slot.reviewed = true;
    save();
  }

  function edit(id: string, field: 'analysis' | 'level' | 'zone' | 'defectType' | 'classification' | 'extent' | 'structuralConcern' | 'verification' | 'possibleCause' | 'recommendation' | 'timeframe' | 'immediateAction' | 'supervision', value: string) {
    if (!slots[id]) return;
    slots[id][field] = value;
    if (field === 'classification' && (value === 'Require Repair' || value === 'Unsafe')) {
      slots[id].recommendation ||= slots[id].repairSuggestion?.recommendation || actionFor(slots[id].defectType ?? '');
      slots[id].timeframe ||= value === 'Unsafe' ? 'Urgent CP assessment' : 'CP to set after site assessment';
      if (value === 'Unsafe') {
        if (slots[id].timeframe === 'CP to set after site assessment') slots[id].timeframe = 'Urgent CP assessment';
        slots[id].immediateAction ||= slots[id].repairSuggestion?.immediate_action || 'Arrange urgent CP review and assess immediate precautions.';
      }
    }
    slots[id].reviewed = false;
    errors[id] = '';
    save();
  }

  async function addPhoto(section: string, elevation: string, file?: File, exteriorSystem = '') {
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
    slots[id] = { name: file.name, analysis: '', reviewed: false, exteriorSystem: exteriorSystem || slots[id]?.exteriorSystem };
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
        body: JSON.stringify({ slot: id, project, image, filename: file.name })
      });
      const result: { analysis: string; source?: string; filter?: FilterResult; repair_suggestion?: { recommendation?: string; timeframe?: string; immediate_action?: string }; error?: string } = await response.json();
      if (!response.ok) throw new Error(result.error || 'Analysis failed.');
      slots[id].analysis = result.analysis;
      slots[id].source = result.source;
      slots[id].filter = result.filter;
      if (section === 'close' && !slots[id].exteriorSystem) slots[id].exteriorSystem = guessSystem(result.filter?.elements);
      slots[id].repairSuggestion = result.repair_suggestion;
      const location = result.filter?.location;
      if (location?.level_from != null) slots[id].level = location.level_to != null && location.level_to !== location.level_from ? `${location.level_from}–${location.level_to}` : String(location.level_from);
      slots[id].zone = location?.zone ?? '';
      if (section === 'close') {
        const side = asSide(file.name.split(/[-_ ]/)[0]) || asSide(location?.elevation) || asSide(result.filter?.matched_side);
        const point = mapGuess(file.name, side);
        slots[id].side = side;
        slots[id].mapX = point.x;
        slots[id].mapY = point.y;
        slots[id].mapSource = side ? 'suggested from side' : 'rough guess';
      }
      slots[id].storeysVisible = result.filter?.storeys_visible != null ? Math.round(result.filter.storeys_visible) : undefined;
      slots[id].photoDate = dateFrom(result.filter?.meta?.taken_at);
      const defects = result.filter?.defects ?? [];
      const top = [...defects].sort((a, b) => b.confidence - a.confidence)[0];
      slots[id].defectType = top ? `${words(top.component)} · ${words(top.condition)}` : defects.length === 0 && result.filter ? 'None detected in photo' : '';
      slots[id].verification = [...new Set(defects.flatMap((defect) => defect.verification ?? []))].join('; ');
      const suggestions = defects.map((defect) => defect.suggested_condition);
      slots[id].classification = suggestions.includes('unsafe') ? 'Unsafe' : suggestions.includes('require_repair') ? 'Require Repair' : suggestions.includes('safe') || (result.filter?.decision === 'include' && !defects.length) ? 'Safe' : '';
      slots[id].structuralConcern = defects.length ? 'Further investigation needed' : 'No visible concern';
      slots[id].extent = defects.length ? 'Undetermined' : '';
      if (result.repair_suggestion || slots[id].classification === 'Require Repair' || slots[id].classification === 'Unsafe') {
        slots[id].recommendation = result.repair_suggestion?.recommendation?.trim() || actionFor(top?.condition ?? '');
        slots[id].timeframe = result.repair_suggestion?.timeframe?.trim() || (slots[id].classification === 'Unsafe' ? 'Urgent CP assessment' : 'CP to set after site assessment');
        if (slots[id].classification === 'Unsafe') slots[id].immediateAction = result.repair_suggestion?.immediate_action?.trim() || 'Arrange urgent CP review; assess whether access restriction or securing loose elements is needed.';
      }
      if (project && slots[id].photoDate) {
        const field = section === 'visual' ? 'visualDate' : 'closeDate';
        const auto = section === 'visual' ? 'visualDateAuto' : 'closeDateAuto';
        if (project[auto]) {
          project[field] = Object.entries(slots).filter(([slotId, slot]) => slotId.startsWith(`${section}:`) && slot.photoDate).map(([, slot]) => slot.photoDate!).sort()[0];
          localStorage.setItem('hound-pfi-project', JSON.stringify(project));
        }
      }
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
          if (id.startsWith('close:') && slots[id].mapX == null) {
            slots[id].side = slots[id].side || asSide(id.split(':')[1]);
            const point = mapGuess(slots[id].name, slots[id].side ?? '');
            slots[id].mapX = point.x; slots[id].mapY = point.y; slots[id].mapSource = 'rough guess';
          }
          const photo = await photoStore(`${project.id}:${id}`);
          if (photo) urls[id] = URL.createObjectURL(photo);
        }
        const plan = await photoStore(`${project.id}:floor-plan`);
        if (plan) {
          planUrl = URL.createObjectURL(plan); planName = plan instanceof File ? plan.name : 'Floor plan';
          const bitmap = await createImageBitmap(plan);
          planWidth = bitmap.width; planHeight = bitmap.height; bitmap.close();
        }
        save();
      }
    } catch {
      errors.page = 'Saved photos could not be loaded.';
    }
    ready = true;
    };
    void load();
    return () => { Object.values(urls).forEach(URL.revokeObjectURL); if (planUrl) URL.revokeObjectURL(planUrl); };
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
        <p class="text-xs font-semibold">{reviewed} of {total} evidence areas reviewed</p>
        <div class="mt-3 h-1.5 overflow-hidden rounded-full bg-ink/10" role="progressbar" aria-label="Evidence areas reviewed" aria-valuenow={reviewed} aria-valuemin="0" aria-valuemax={total}>
          <div class="h-full bg-orange" style={`width: ${total ? reviewed / total * 100 : 0}%`}></div>
        </div>
        {#if reportReady}
          <button type="button" onclick={exportReport} disabled={exporting}
            class="mt-5 w-full rounded-full bg-ink px-5 py-3 text-xs font-semibold text-paper hover:bg-orange hover:text-ink disabled:opacity-50">
            {exporting ? 'Preparing Word document…' : 'Download Word report'}
          </button>
          {#if errors.report}<p class="mt-2 text-xs text-red-700" role="alert">{errors.report}</p>{/if}
        {/if}
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
                {#if maxStoreys}<div><dt class="text-xs text-ink/50">Storeys</dt><dd class="mt-1">At least {maxStoreys} visible in photos · AI estimate</dd></div>{/if}
                <div><dt class="text-xs text-ink/50">Visual inspection</dt><dd class="mt-1">{project.visualDate || 'Date not entered'}{project.visualDateAuto ? ' (suggested)' : ''} · {project.visualMethod || 'Method not entered'}</dd></div>
                <div><dt class="text-xs text-ink/50">Close-range inspection</dt><dd class="mt-1">{project.closeDate || 'Date not entered'}{project.closeDateAuto ? ' (suggested)' : ''} · {project.closeMethod || 'Method not entered'}</dd></div>
                <div class="sm:col-span-2"><dt class="text-xs text-ink/50">Close-range coverage</dt><dd class="mt-1">{project.elevations.map((elevation) => `${elevation}: ${project?.closeCoverage?.[elevation] != null ? `${project.closeCoverage[elevation]}%` : 'not measured'}`).join(' · ')}</dd></div>
              </dl>
              <div class="border-t border-ink/10 pt-6">
                <div class="flex flex-wrap items-center justify-between gap-3">
                  <div><h3 class="text-sm font-semibold">Floor plan <span class="font-normal text-ink/45">(optional)</span></h3><p class="mt-1 text-xs text-ink/50">{planName || 'Section 03 will use a sketch from the visual photos in Section 02.'}</p></div>
                  <div class="flex items-center gap-3"><label class="cursor-pointer text-xs font-semibold text-orange">{planName ? 'Replace plan' : 'Add floor plan'}<input class="sr-only" type="file" accept="image/jpeg,image/png,image/webp" onchange={(event) => { void addPlan(event.currentTarget.files?.[0]); event.currentTarget.value = ''; }} /></label>{#if planName}<button type="button" onclick={() => void removePlan()} class="text-xs text-ink/45 hover:text-orange">Remove</button>{/if}</div>
                </div>
                {#if planUrl}<img src={planUrl} alt="Uploaded floor plan" class="mt-4 max-h-56 max-w-full rounded-lg border border-ink/10 object-contain" />{/if}
                {#if errors.plan}<p class="mt-2 text-xs text-red-700">{errors.plan}</p>{/if}
              </div>
            {:else}
              {#if section.id === 'close'}
                <p class="border-b border-ink/10 py-5 text-xs text-ink/50">{planName ? `Locations on ${planName}` : hasVisualPhotos ? 'Suggested locations on a sketch from Section 02 photos' : 'Add visual photos in Section 02 to create the location sketch'}</p>
                <div class="mt-5 space-y-2">
                  {#each project.systems as system}
                    <div class="flex items-center justify-between gap-3 rounded-lg border border-ink/10 px-4 py-3 text-sm">
                      <span>{system} <span class="text-xs text-ink/45">· {closeIds.filter((id) => slots[id]?.exteriorSystem === system).length} photos</span></span>
                      <label class="shrink-0 cursor-pointer text-xs font-semibold text-orange">Add photo<input class="sr-only" type="file" accept="image/jpeg,image/png,image/webp,image/gif" onchange={(event) => { void addPhoto('close', crypto.randomUUID(), event.currentTarget.files?.[0], system); event.currentTarget.value = ''; }} /></label>
                    </div>
                  {/each}
                </div>
                <details class="mt-6 border-b border-ink/10 pb-5 text-sm">
                  <summary class="cursor-pointer font-semibold">Close-range coverage <span class="font-normal text-ink/45">· measured area by elevation</span></summary>
                  <p class="mt-3 text-xs text-ink/55">Enter façade and inspected areas from the elevation plan. Coverage is calculated for each side.</p>
                  <div class="mt-4 space-y-3">
                    {#each project.elevations as side}
                      <div class="grid items-end gap-2 sm:grid-cols-[5rem_1fr_1fr_4rem]">
                        <span class="text-xs font-semibold">{side}</span>
                        <label class="text-xs">Façade area (m²)<input type="number" min="0" step="0.1" value={project.closeAreas?.[side]?.facade ?? ''} oninput={(event) => setCoverage(side, 'facade', event.currentTarget.value)} class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2 text-sm" /></label>
                        <label class="text-xs">Inspected area (m²)<input type="number" min="0" max={project.closeAreas?.[side]?.facade} step="0.1" value={project.closeAreas?.[side]?.inspected ?? ''} oninput={(event) => setCoverage(side, 'inspected', event.currentTarget.value)} class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2 text-sm" /></label>
                        <span class="pb-2 text-xs">{project.closeAreas?.[side]?.facade && (project.closeAreas?.[side]?.inspected ?? 0) > project.closeAreas[side].facade! ? 'Check areas' : project.closeCoverage?.[side] != null ? `${project.closeCoverage[side]}%` : '—'}</span>
                      </div>
                    {/each}
                  </div>
                </details>
              {/if}
              {#each section.id === 'visual' ? project.elevations : closeIds as elevation, index}
                {@const id = section.id === 'visual' ? key('visual', elevation) : elevation}
                <section class="border-b border-ink/10 py-7 last:border-0">
                  <div class="flex items-center justify-between gap-3">
                    <h3 class="text-sm font-semibold">{section.id === 'visual' ? `${elevation} elevation` : `Close-range photo ${index + 1}`}</h3>
                    <span class={`text-xs ${slots[id]?.reviewed ? 'text-emerald-700 dark:text-emerald-400' : 'text-ink/45'}`}>
                      {busy[id] ? 'Analyzing…' : slots[id]?.reviewed ? 'Reviewed' : slots[id]?.name ? 'Review needed' : 'Photo needed'}
                    </span>
                  </div>
                  <div class={`mt-4 grid gap-5 ${section.id === 'close' ? 'lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]' : 'sm:grid-cols-[11rem_minmax(0,1fr)]'}`}>
                    <div class={section.id === 'close' ? 'grid grid-cols-2 items-start gap-3' : ''}>
                      <div>
                      {#if urls[id]}
                        <div class="relative">
                          <img src={urls[id]} alt={section.id === 'visual' ? `${elevation} elevation inspection` : `Close-range inspection ${index + 1}`} class="aspect-[4/3] w-full rounded-lg object-cover" />
                          {#if busy[id]}
                            <div class="absolute inset-0 grid place-items-center rounded-lg bg-ink/60 text-paper" role="status" aria-label="Analyzing photo">
                              <div class="flex items-center gap-2 rounded-full bg-ink/80 px-4 py-2 text-xs font-semibold">
                                <span class="h-4 w-4 animate-spin rounded-full border-2 border-paper/30 border-t-paper" aria-hidden="true"></span>
                                Analyzing photo…
                              </div>
                            </div>
                          {/if}
                        </div>
                      {:else}
                        <div class="grid aspect-[4/3] place-items-center rounded-lg border border-dashed border-ink/20 text-xs text-ink/40">No photo</div>
                      {/if}
                      <label class="mt-2 inline-block cursor-pointer text-xs font-semibold text-orange hover:underline">
                        {slots[id]?.name ? 'Replace photo' : 'Add photo'}
                        <input class="sr-only" type="file" accept="image/jpeg,image/png,image/webp,image/gif" disabled={busy[id]}
                          onchange={(event) => { addPhoto(section.id, elevation, event.currentTarget.files?.[0]); event.currentTarget.value = ''; }} />
                      </label>
                      {#if section.id === 'close'}<button type="button" onclick={() => void removeClose(id)} class="ml-3 text-xs text-ink/45 hover:text-orange">Remove</button>{/if}
                      </div>
                      {#if section.id === 'close'}
                        <div>
                          <button type="button" onclick={(event) => place(id, event)} disabled={!planUrl && !hasVisualPhotos} aria-label="Place this photo on the plan" class="relative aspect-[4/3] w-full overflow-hidden rounded-lg border border-ink/20 bg-paper text-ink disabled:cursor-default">
                            {#if planUrl}
                              <img src={planUrl} alt="Uploaded floor plan" class="absolute inset-0 h-full w-full object-fill" />
                            {:else if hasVisualPhotos}
                              <span class="absolute inset-[16%] border-2 border-ink/35 bg-white/70"></span>
                              <span class="absolute left-1/2 top-0 -translate-x-1/2 text-[10px] font-bold">N</span><span class="absolute right-1 top-1/2 -translate-y-1/2 text-[10px] font-bold">E</span><span class="absolute bottom-0 left-1/2 -translate-x-1/2 text-[10px] font-bold">S</span><span class="absolute left-1 top-1/2 -translate-y-1/2 text-[10px] font-bold">W</span>
                              {#if urls['visual:North']}<img src={urls['visual:North']} alt="" class="absolute left-[38%] top-[7%] h-[13%] w-[24%] rounded-sm object-cover opacity-70" />{/if}
                              {#if urls['visual:East']}<img src={urls['visual:East']} alt="" class="absolute right-[7%] top-[38%] h-[24%] w-[13%] rounded-sm object-cover opacity-70" />{/if}
                              {#if urls['visual:South']}<img src={urls['visual:South']} alt="" class="absolute bottom-[7%] left-[38%] h-[13%] w-[24%] rounded-sm object-cover opacity-70" />{/if}
                              {#if urls['visual:West']}<img src={urls['visual:West']} alt="" class="absolute left-[7%] top-[38%] h-[24%] w-[13%] rounded-sm object-cover opacity-70" />{/if}
                            {:else}
                              <span class="absolute inset-0 grid place-items-center px-2 text-center text-[11px] text-ink/45">Visual photos needed</span>
                            {/if}
                            {#if planUrl || hasVisualPhotos}<span class="absolute grid h-5 w-5 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full border-2 border-white bg-orange text-[10px] font-bold shadow" style={`left:${slots[id]?.mapX ?? 50}%;top:${slots[id]?.mapY ?? 50}%`}>{index + 1}</span>{/if}
                          </button>
                          {#if planUrl || hasVisualPhotos}<p class="mt-2 text-[11px] leading-4 text-ink/50">{slots[id]?.mapSource || 'Rough guess'}{planWidth ? ` · x ${Math.round((slots[id]?.mapX ?? 50) * planWidth / 100)} px, y ${Math.round((slots[id]?.mapY ?? 50) * planHeight / 100)} px` : ''} · click to correct</p>{/if}
                        </div>
                      {/if}
                    </div>
                    <div>
                      <label for={`analysis-${id}`} class="text-xs font-semibold">Photo observation</label>
                      <textarea id={`analysis-${id}`} rows="5" placeholder="Add a photo to draft an observation"
                        value={slots[id]?.analysis ?? ''}
                        oninput={(event) => edit(id, 'analysis', event.currentTarget.value)}
                        class="mt-2 w-full rounded-lg border border-ink/15 bg-paper px-3 py-3 text-sm leading-6 outline-none focus-visible:border-orange"></textarea>
                      <p class="mt-2 text-xs text-ink/45" aria-live="polite">{busy[id] ? 'Checking the image and drafting an observation…' : slots[id]?.source === 'pfi-filter' ? 'PFI filter suggestion · confirm below' : slots[id]?.source ? 'SoCLaas suggestion · confirm below' : slots[id]?.name || ''}</p>
                      {#if slots[id]?.photoDate || slots[id]?.storeysVisible != null}
                        <p class="mt-1 text-xs text-ink/45">{slots[id]?.photoDate ? `Photo date ${slots[id].photoDate}` : ''}{slots[id]?.photoDate && slots[id]?.storeysVisible != null ? ' · ' : ''}{slots[id]?.storeysVisible != null ? `About ${slots[id].storeysVisible} storeys visible` : ''}</p>
                      {/if}
                      {#if slots[id]?.name}
                        <div class="mt-5 grid gap-3 sm:grid-cols-2">
                          {#if section.id === 'close'}
                            <label class="text-xs font-semibold">Exterior system<select value={slots[id]?.exteriorSystem ?? ''} onchange={(event) => changeSystem(id, event.currentTarget.value)} class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal"><option value="">Choose</option>{#each project.systems as system}<option value={system}>{system}</option>{/each}</select></label>
                            <label class="text-xs font-semibold">Side<select value={slots[id]?.side ?? ''} onchange={(event) => changeSide(id, event.currentTarget.value)} class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal"><option value="">Unknown</option>{#each sides as side}<option>{side}</option>{/each}</select></label>
                          {/if}
                          <label class="text-xs font-semibold">Level / storey<input value={slots[id]?.level ?? ''} oninput={(event) => edit(id, 'level', event.currentTarget.value)} placeholder="If identifiable" class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal" /></label>
                          <label class="text-xs font-semibold">Zone<input value={slots[id]?.zone ?? ''} oninput={(event) => edit(id, 'zone', event.currentTarget.value)} placeholder="e.g. bay near lift lobby" class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal" /></label>
                          <label class="text-xs font-semibold">Visible defect<input value={slots[id]?.defectType ?? ''} oninput={(event) => edit(id, 'defectType', event.currentTarget.value)} placeholder="None visible" class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal" /></label>
                          <label class="text-xs font-semibold">Condition<select value={slots[id]?.classification ?? ''} onchange={(event) => edit(id, 'classification', event.currentTarget.value)} class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal"><option value="">Select</option><option>Safe</option><option>Require Repair</option><option>Unsafe</option></select></label>
                          <label class="text-xs font-semibold">Extent<select value={slots[id]?.extent ?? ''} onchange={(event) => edit(id, 'extent', event.currentTarget.value)} class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal"><option value="">Select</option><option>Localized</option><option>Repeated</option><option>Systemic</option><option>Undetermined</option></select></label>
                          <label class="text-xs font-semibold">Façade integrity / structural concern<select value={slots[id]?.structuralConcern ?? ''} onchange={(event) => edit(id, 'structuralConcern', event.currentTarget.value)} class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal"><option value="">Select after review</option><option>No visible concern</option><option>Further investigation needed</option><option>Confirmed concern by CP</option></select></label>
                        </div>
                        <label class="mt-3 block text-xs font-semibold">Checks still needed<input value={slots[id]?.verification ?? ''} oninput={(event) => edit(id, 'verification', event.currentTarget.value)} placeholder="e.g. tapping, fixing check" class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal" /></label>
                        {#if slots[id]?.classification === 'Require Repair' || slots[id]?.classification === 'Unsafe'}
                          <div class="mt-5 border-t border-ink/10 pt-4">
                            <p class="text-xs font-semibold">Remedial action <span class="font-normal text-ink/45">· {slots[id]?.repairSuggestion ? 'SoCLaas draft' : 'draft'} for review</span></p>
                            <div class="mt-3 grid gap-3 sm:grid-cols-2">
                              <label class="text-xs font-semibold">Possible cause<input value={slots[id]?.possibleCause ?? ''} oninput={(event) => edit(id, 'possibleCause', event.currentTarget.value)} placeholder="If established" class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal" /></label>
                              <label class="text-xs font-semibold">Timeframe<input value={slots[id]?.timeframe ?? ''} oninput={(event) => edit(id, 'timeframe', event.currentTarget.value)} placeholder="e.g. within 3 months" class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal" /></label>
                              <label class="text-xs font-semibold sm:col-span-2">Recommendation<input value={slots[id]?.recommendation ?? ''} oninput={(event) => edit(id, 'recommendation', event.currentTarget.value)} placeholder="Action for this defect and similar areas" class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal" /></label>
                              <label class="text-xs font-semibold">CP supervision<select value={slots[id]?.supervision ?? ''} onchange={(event) => edit(id, 'supervision', event.currentTarget.value)} class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal"><option value="">To determine</option><option>Required</option><option>Not required</option></select></label>
                              {#if slots[id]?.classification === 'Unsafe'}<label class="text-xs font-semibold">Immediate action proposed<input value={slots[id]?.immediateAction ?? ''} oninput={(event) => edit(id, 'immediateAction', event.currentTarget.value)} placeholder="Precaution or action proposed" class="mt-1 w-full rounded-lg border border-ink/15 bg-paper px-3 py-2.5 text-sm font-normal" /></label>{/if}
                            </div>
                          </div>
                        {/if}
                        <div class="mt-4 flex justify-end"><button type="button" disabled={slots[id].reviewed} onclick={() => markReviewed(id)} class="text-xs font-semibold text-orange disabled:text-ink/35">{slots[id].reviewed ? 'Reviewed' : 'Mark reviewed'}</button></div>
                      {/if}
                      {#if slots[id]?.filter}
                        {@const result = slots[id].filter}
                        <details class="mt-3 border-t border-ink/10 pt-3 text-xs text-ink/65">
                          <summary class="cursor-pointer font-semibold">Photo details · {result.decision ?? 'review'} · {result.match?.status ?? 'unmatched'}</summary>
                          <div class="mt-3 space-y-2 leading-5">
                            {#if result.decision_conf != null}<p>Decision confidence: {Math.round(result.decision_conf * 100)}%</p>{/if}
                            {#if result.match?.score != null}<p>Slot match: {Math.round(result.match.score * 100)}%</p>{/if}
                            {#if result.shot_type}<p>Shot: {result.shot_type.replaceAll('_', ' ')}</p>{/if}
                            {#if result.elements?.length}<p>Façade elements: {result.elements.map(words).join(', ')}</p>{/if}
                            {#if result.meta?.taken_at_source}<p>Photo date source: {result.meta.taken_at_source}</p>{/if}
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
