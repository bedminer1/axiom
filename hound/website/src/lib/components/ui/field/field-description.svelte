<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils';

  let {
    class: className = '',
    children,
    ...restProps
  }: HTMLAttributes<HTMLParagraphElement> & { children?: Snippet } = $props();
</script>

<p data-slot="field-description" class={cn('text-xs leading-5 text-ink/55', className)} {...restProps}>
  {@render children?.()}
</p>
