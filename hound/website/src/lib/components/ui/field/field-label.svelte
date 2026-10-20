<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLLabelAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils';

  let {
    class: className = '',
    children,
    ...restProps
  }: HTMLLabelAttributes & { children?: Snippet } = $props();
</script>

<label data-slot="field-label" class={cn('text-sm font-medium leading-none text-ink', className)} {...restProps}>
  {@render children?.()}
</label>
