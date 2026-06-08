<script lang="ts">
  import { Command as CommandPrimitive } from "bits-ui";
  import RiSearchLine from "remixicon-svelte/icons/search-line";
  import { cn } from "$lib/utils.js";

  let {
    ref = $bindable(null),
    value = $bindable(""),
    class: className,
    ...restProps
  }: CommandPrimitive.InputProps = $props();
</script>

<div
  data-slot="command-input-wrapper"
  class="flex h-10 items-center gap-2 border-b border-border px-3 text-muted-foreground [&_svg:not([class*='size-'])]:size-4"
>
  <RiSearchLine aria-hidden="true" />
  <CommandPrimitive.Input
    bind:ref
    bind:value
    data-slot="command-input"
    class={cn(
      "flex h-10 w-full min-w-0 bg-transparent py-2 text-xs text-foreground outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50",
      className
    )}
    {...restProps}
  />
</div>
