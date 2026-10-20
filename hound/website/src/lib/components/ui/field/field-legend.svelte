<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils';

  let {
    class: className = '',
    children,
    ...restProps
  }: HTMLAttributes<HTMLLegendElement> & { children?: Snippet } = $props();
</script>

<legend data-slot="field-legend" class={cn('text-sm font-semibold text-ink', className)} {...restProps}>
  {@render children?.()}
</legend>
