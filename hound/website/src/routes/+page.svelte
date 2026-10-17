<script lang="ts">
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
  let selectedSystems = $state<string[]>([]);
  let error = $state('');

  function saveProject(event: SubmitEvent) {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement;
    const data = new FormData(form);
    const selectedElevations = data.getAll('elevations').map(String);
    const chosenSystems = data.getAll('systems').map(String);
    const otherSystem = String(data.get('otherSystem') ?? '').trim();
    const systems = chosenSystems.flatMap((item) => item === 'Other' ? (otherSystem ? [otherSystem] : []) : [item]);

    if (!selectedElevations.length || !systems.length || (chosenSystems.includes('Other') && !otherSystem)) {
      error = 'Choose at least one exterior type and describe “Other”.';
      return;
    }

    const project = {
      id: crypto.randomUUID(),
      name: String(data.get('projectName') ?? '').trim(),
      address: String(data.get('siteAddress') ?? '').trim(),
      owner: String(data.get('owner') ?? '').trim(),
      noticeReference: String(data.get('noticeReference') ?? '').trim(),
      elevations: selectedElevations,
      systems,
      usesDrone: data.get('usesDrone') === 'on'
    };
    localStorage.setItem('hound-pfi-project', JSON.stringify(project));
    goto('/checklist');
  }
</script>

<svelte:head>
  <title>New project — Hound</title>
  <meta name="description" content="Create a PFI inspection project." />
</svelte:head>

<main class="mx-auto max-w-6xl px-6 py-16 md:px-10 md:py-24">
  <section class="mx-auto max-w-2xl">
    <p class="mb-5 text-xs font-semibold uppercase tracking-[0.24em] text-orange">PFI report</p>
    <h1 class="text-[clamp(2.75rem,7vw,5rem)] font-semibold leading-[0.96] tracking-[-0.075em]">
      New project<span class="font-hand font-normal tracking-normal text-orange">.</span>
    </h1>

    <form
      class="mt-10 rounded-2xl border border-ink/10 bg-white/60 p-6 shadow-[0_18px_60px_rgba(17,17,17,0.06)] dark:bg-white/[0.03] md:p-9"
      onsubmit={saveProject}
    >
      <Field.Set>
        <Field.Group>
          <div class="grid gap-5 md:grid-cols-2">
            <Field.Field class="md:col-span-2">
              <Field.Label for="project-name">Building name <span class="text-orange">*</span></Field.Label>
              <Input id="project-name" name="projectName" placeholder="e.g. Orchard Central" autocomplete="organization-title" required />
            </Field.Field>

            <Field.Field class="md:col-span-2">
              <Field.Label for="site-address">Site address <span class="text-orange">*</span></Field.Label>
              <Input id="site-address" name="siteAddress" placeholder="Street address" autocomplete="street-address" required />
            </Field.Field>

            <Field.Field>
              <Field.Label for="owner">Owner / MCST <span class="text-ink/45">(optional)</span></Field.Label>
              <Input id="owner" name="owner" placeholder="" autocomplete="organization" />
            </Field.Field>

            <Field.Field>
              <Field.Label for="notice-reference">PFI notice ref <span class="text-ink/45">(optional)</span></Field.Label>
              <Input id="notice-reference" name="noticeReference" placeholder="e.g. 2026/01/1234" />
            </Field.Field>

            <Field.Field class="md:col-span-2">
              <div role="group" aria-labelledby="elevations-label">
                <Field.Label id="elevations-label">Elevations <span class="text-orange">*</span></Field.Label>
                <div class="grid grid-cols-2 gap-3 pt-3 sm:grid-cols-4">
                  {#each elevations as elevation}
                    <label class="flex min-h-11 items-center gap-2 rounded-lg border border-ink/15 px-3 text-sm">
                      <input type="checkbox" name="elevations" value={elevation} checked class="accent-orange" />
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
                  <Input class="mt-3" name="otherSystem" aria-label="Other exterior type" placeholder="Describe the exterior type" required />
                {/if}
              </div>
            </Field.Field>

            <Field.Field class="md:col-span-2">
              <label class="flex min-h-11 items-center gap-3 text-sm" for="uses-drone">
                <input id="uses-drone" type="checkbox" name="usesDrone" class="accent-orange" />
                Drone used for inspection
              </label>
            </Field.Field>
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
          Start inspection <span aria-hidden="true">→</span>
        </button>
      </div>
    </form>
  </section>
</main>
