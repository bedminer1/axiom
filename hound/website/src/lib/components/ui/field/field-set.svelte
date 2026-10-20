<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLFieldsetAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils';

  let {
    class: className = '',
    children,
    ...restProps
  }: HTMLFieldsetAttributes & { children?: Snippet } = $props();
</script>

<fieldset data-slot="field-set" class={cn('grid gap-5', className)} {...restProps}>
  {@render children?.()}
</fieldset>
