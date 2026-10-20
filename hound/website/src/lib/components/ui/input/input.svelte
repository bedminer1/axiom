<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils';

  let { class: className = '', ...restProps }: HTMLInputAttributes = $props();
</script>

<input
  data-slot="input"
  class={cn(
    'h-11 w-full rounded-lg border border-ink/15 bg-paper px-3.5 text-sm text-ink outline-none transition placeholder:text-ink/35 focus-visible:border-orange focus-visible:ring-4 focus-visible:ring-orange/15 disabled:cursor-not-allowed disabled:opacity-50',
    className
  )}
  {...restProps}
/>
