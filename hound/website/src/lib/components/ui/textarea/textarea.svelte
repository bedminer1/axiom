<script lang="ts">
  import type { HTMLTextareaAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils';

  let { class: className = '', ...restProps }: HTMLTextareaAttributes = $props();
</script>

<textarea
  data-slot="textarea"
  class={cn(
    'min-h-28 w-full resize-y rounded-lg border border-ink/15 bg-paper px-3.5 py-3 text-sm leading-6 text-ink outline-none transition placeholder:text-ink/35 focus-visible:border-orange focus-visible:ring-4 focus-visible:ring-orange/15 disabled:cursor-not-allowed disabled:opacity-50',
    className
  )}
  {...restProps}
></textarea>
