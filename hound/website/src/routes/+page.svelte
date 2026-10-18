<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import * as Field from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';

  const elevations = ['North', 'East', 'South', 'West'];
  const systemChoices = [
    'Large glass panels (curtain wall)',
    'Windows',
    'Concrete or plastered walls',
    'Brick or stone walls',
    'Metal panels',
    'Other',
    'Not sure yet'
  ];
  type Project = {
    id: string;
    name: string;
    address: string;
    owner: string;
    noticeReference: string;
    elevations: string[];
    systems: string[];
    usesDrone: boolean;
    visualDate?: string; visualDuration?: string; visualMethod?: string; visualLimitations?: string;
    closeDate?: string; closeDuration?: string; closeMethod?: string; closeBasis?: string;
    closeCoverage?: Record<string, number>; previousCondition?: string; inspector?: string;
    visualDateAuto?: boolean; closeDateAuto?: boolean;
    nameAuto?: boolean;
  };
  const today = () => {
    const date = new Date();
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
  };
  const nameFromAddress = (address: string) => address.split(/[\n,]/)[0].replace(/\b(?:Singapore|S)\s*\d{6}\b/i, '').trim();
  let project = $state<Project | null>(null);
  let ready = $state(false);
  let selectedElevations = $state<string[]>([...elevations]);
  let selectedSystems = $state<string[]>([]);
  let otherSystem = $state('');
  let error = $state('');
  let nameValue = $state('');
  let addressValue = $state('');
  let nameEdited = $state(false);
  let visualDateValue = $state('');
  let closeDateValue = $state('');
  let visualDateAuto = $state(true);
  let closeDateAuto = $state(true);

  onMount(() => {
    try {
      project = JSON.parse(localStorage.getItem('hound-pfi-project') ?? 'null');
      if (project) {
        nameValue = project.name || nameFromAddress(project.address);
        addressValue = project.address;
        nameEdited = !project.nameAuto;
        visualDateValue = project.visualDate ?? '';
        closeDateValue = project.closeDate ?? '';
        visualDateAuto = project.visualDateAuto ?? false;
        closeDateAuto = project.closeDateAuto ?? false;
        selectedElevations = project.elevations;
        selectedSystems = project.systems.filter((system) => systemChoices.includes(system));
        otherSystem = project.systems.find((system) => !systemChoices.includes(system)) ?? '';
        if (otherSystem) selectedSystems = [...selectedSystems, 'Other'];
      } else {
        visualDateValue = today();
        closeDateValue = today();
      }
    } catch {
      project = null;
    }
    ready = true;
  });

  function saveProject(event: SubmitEvent) {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement;
    const data = new FormData(form);
    const selectedElevations = data.getAll('elevations').map(String);
    const chosenSystems = data.getAll('systems').map(String);
    const customSystem = String(data.get('otherSystem') ?? '').trim();
    const systems = chosenSystems.flatMap((item) => item === 'Other' ? (customSystem ? [customSystem] : []) : [item]);

    if (!selectedElevations.length || !systems.length || (chosenSystems.includes('Other') && !customSystem)) {
      error = 'Choose at least one exterior type and describe “Other”.';
      return;
    }

    const updatedProject = {
      id: project?.id ?? crypto.randomUUID(),
      name: String(data.get('projectName') ?? '').trim(),
      nameAuto: !nameEdited,
      address: String(data.get('siteAddress') ?? '').trim(),
      owner: String(data.get('owner') ?? '').trim(),
      noticeReference: String(data.get('noticeReference') ?? '').trim(),
      elevations: selectedElevations,
      systems,
      usesDrone: data.get('usesDrone') === 'on',
      visualDate: String(data.get('visualDate') ?? ''),
      visualDuration: String(data.get('visualDuration') ?? '').trim(),
      visualMethod: String(data.get('visualMethod') ?? '').trim(),
      visualLimitations: String(data.get('visualLimitations') ?? '').trim(),
      closeDate: String(data.get('closeDate') ?? ''),
      closeDuration: String(data.get('closeDuration') ?? '').trim(),
      closeMethod: String(data.get('closeMethod') ?? '').trim(),
      closeBasis: String(data.get('closeBasis') ?? '').trim(),
      closeCoverage: Object.fromEntries(selectedElevations.map((elevation) => [elevation, Number(data.get(`coverage-${elevation}`) || 0)])),
      previousCondition: String(data.get('previousCondition') ?? '').trim(),
      inspector: String(data.get('inspector') ?? '').trim(),
      visualDateAuto,
      closeDateAuto
    };
    localStorage.setItem('hound-pfi-project', JSON.stringify(updatedProject));
    goto('/checklist');
  }
</script>

<svelte:head>
  <title>{project ? 'Edit building' : 'New project'} — Hound</title>
  <meta name="description" content="Create a PFI inspection project." />
</svelte:head>

<main class="mx-auto max-w-6xl px-6 py-16 md:px-10 md:py-24">
  {#if ready}
  <section class="mx-auto max-w-2xl">
    <p class="mb-5 text-xs font-semibold uppercase tracking-[0.24em] text-orange">PFI report</p>
    <h1 class="text-[clamp(2.75rem,7vw,5rem)] font-semibold leading-[0.96] tracking-[-0.075em]">
      {project ? 'Edit building' : 'New project'}<span class="font-hand font-normal tracking-normal text-orange">.</span>
    </h1>

    <form
      class="mt-10 rounded-2xl border border-ink/10 bg-white/60 p-6 shadow-[0_18px_60px_rgba(17,17,17,0.06)] dark:bg-white/[0.03] md:p-9"
      onsubmit={saveProject}
    >
      <Field.Set>
        <Field.Group>
          <div class="grid gap-5 md:grid-cols-2">
            <Field.Field class="md:col-span-2">
              <Field.Label for="site-address">Site address <span class="text-orange">*</span></Field.Label>
              <Input id="site-address" name="siteAddress" value={addressValue} oninput={(event) => { addressValue = event.currentTarget.value; if (!nameEdited) nameValue = nameFromAddress(addressValue); }} placeholder="Street address" autocomplete="street-address" required />
            </Field.Field>

            <Field.Field class="md:col-span-2">
              <Field.Label for="project-name">Building name or label <span class="text-orange">*</span></Field.Label>
              <Input id="project-name" name="projectName" value={nameValue} oninput={(event) => { nameValue = event.currentTarget.value; nameEdited = true; }} placeholder="Suggested from address" autocomplete="organization-title" required />
            </Field.Field>

            <Field.Field>
              <Field.Label for="owner">Owner / MCST <span class="text-ink/45">(optional)</span></Field.Label>
              <Input id="owner" name="owner" value={project?.owner ?? ''} placeholder="" autocomplete="organization" />
            </Field.Field>

            <Field.Field>
              <Field.Label for="notice-reference">PFI notice ref <span class="text-ink/45">(optional)</span></Field.Label>
              <Input id="notice-reference" name="noticeReference" value={project?.noticeReference ?? ''} placeholder="e.g. 2026/01/1234" />
            </Field.Field>

            <Field.Field class="md:col-span-2">
              <div role="group" aria-labelledby="elevations-label">
                <Field.Label id="elevations-label">Elevations <span class="text-orange">*</span></Field.Label>
                <div class="grid grid-cols-2 gap-3 pt-3 sm:grid-cols-4">
                  {#each elevations as elevation}
                    <label class="flex min-h-11 items-center gap-2 rounded-lg border border-ink/15 px-3 text-sm">
                      <input type="checkbox" name="elevations" value={elevation} bind:group={selectedElevations} class="accent-orange" />
                      {elevation}
                    </label>
                  {/each}
                </div>
              </div>
            </Field.Field>

            <Field.Field class="md:col-span-2">
              <div role="group" aria-labelledby="systems-label">
                <Field.Label id="systems-label">What makes up the building’s exterior? <span class="text-orange">*</span></Field.Label>
                <p class="mt-2 text-xs text-ink/55">Select all that apply.</p>
                <div class="mt-3 grid gap-2 sm:grid-cols-2">
                  {#each systemChoices as system}
                    <label class="flex min-h-11 items-center gap-3 rounded-lg border border-ink/15 px-3 text-sm">
                      <input type="checkbox" name="systems" value={system} bind:group={selectedSystems} class="accent-orange" />
                      {system}
                    </label>
                  {/each}
                </div>
                {#if selectedSystems.includes('Other')}
                  <Input class="mt-3" name="otherSystem" value={otherSystem} oninput={(event) => otherSystem = event.currentTarget.value} aria-label="Other exterior type" placeholder="Describe the exterior type" required />
                {/if}
              </div>
            </Field.Field>

            <Field.Field class="md:col-span-2">
              <label class="flex min-h-11 items-center gap-3 text-sm" for="uses-drone">
                <input id="uses-drone" type="checkbox" name="usesDrone" checked={project?.usesDrone ?? false} class="accent-orange" />
                Drone used for inspection
              </label>
            </Field.Field>

            <div class="md:col-span-2 border-t border-ink/10 pt-5"><h2 class="text-sm font-semibold">Inspection details</h2></div>
            <Field.Field><Field.Label for="inspector">Inspector</Field.Label><Input id="inspector" name="inspector" value={project?.inspector ?? ''} /></Field.Field>
            <Field.Field><Field.Label for="previous-condition">Previous inspection findings</Field.Label><Input id="previous-condition" name="previousCondition" value={project?.previousCondition ?? ''} placeholder="If available" /></Field.Field>
            <Field.Field><Field.Label for="visual-date">Visual inspection date {visualDateAuto ? '(suggested)' : ''}</Field.Label><Input id="visual-date" type="date" name="visualDate" value={visualDateValue} oninput={(event) => { visualDateValue = event.currentTarget.value; visualDateAuto = false; }} /></Field.Field>
            <Field.Field><Field.Label for="visual-duration">Visual inspection duration</Field.Label><Input id="visual-duration" name="visualDuration" value={project?.visualDuration ?? ''} placeholder="e.g. 2 days" /></Field.Field>
            <Field.Field class="md:col-span-2"><Field.Label for="visual-method">Visual inspection method</Field.Label><Input id="visual-method" name="visualMethod" value={project?.visualMethod ?? ''} placeholder="e.g. Ground and drone survey" /></Field.Field>
            <Field.Field class="md:col-span-2"><Field.Label for="visual-limitations">Areas not inspected or deviations</Field.Label><Input id="visual-limitations" name="visualLimitations" value={project?.visualLimitations ?? ''} placeholder="Leave blank if none reported" /></Field.Field>
            <Field.Field><Field.Label for="close-date">Close-range inspection date {closeDateAuto ? '(suggested)' : ''}</Field.Label><Input id="close-date" type="date" name="closeDate" value={closeDateValue} oninput={(event) => { closeDateValue = event.currentTarget.value; closeDateAuto = false; }} /></Field.Field>
            <Field.Field><Field.Label for="close-duration">Close-range inspection duration</Field.Label><Input id="close-duration" name="closeDuration" value={project?.closeDuration ?? ''} placeholder="e.g. 1 day" /></Field.Field>
            <Field.Field class="md:col-span-2"><Field.Label for="close-method">Close-range method and tools</Field.Label><Input id="close-method" name="closeMethod" value={project?.closeMethod ?? ''} placeholder="e.g. Gondola access, visual and tapping checks" /></Field.Field>
            <Field.Field class="md:col-span-2"><Field.Label for="close-basis">Why these close-range areas?</Field.Label><Input id="close-basis" name="closeBasis" value={project?.closeBasis ?? ''} placeholder="e.g. Representative façade types and visual defects" /></Field.Field>
            <div class="md:col-span-2 grid gap-3 sm:grid-cols-4">
              {#each selectedElevations as elevation}
                <Field.Field><Field.Label for={`coverage-${elevation}`}>{elevation} close-range area %</Field.Label><Input id={`coverage-${elevation}`} type="number" min="0" max="100" step="0.1" name={`coverage-${elevation}`} value={project?.closeCoverage?.[elevation] ?? ''} /></Field.Field>
              {/each}
            </div>
          </div>
        </Field.Group>
      </Field.Set>

      {#if error}
        <p class="mt-4 text-sm text-red-700" role="alert">{error}</p>
      {/if}

      <div class="mt-6 flex justify-end border-t border-ink/10 pt-6">
        <button
          type="submit"
          class="inline-flex min-h-11 items-center justify-center gap-3 rounded-full bg-ink px-5 text-xs font-semibold text-paper transition hover:bg-orange hover:text-ink focus-visible:outline-none focus-visible:ring-4 focus-visible:ring-orange/25"
        >
          {project ? 'Save changes' : 'Start inspection'} <span aria-hidden="true">→</span>
        </button>
      </div>
    </form>
  </section>
  {/if}
</main>
