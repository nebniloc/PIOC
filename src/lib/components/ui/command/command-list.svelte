<script lang="ts">
  import { Command as CommandPrimitive } from "bits-ui";
  import { cn } from "$lib/utils.js";

  let {
    ref = $bindable(null),
    class: className,
    children,
    ...restProps
  }: CommandPrimitive.ListProps = $props();
</script>

<CommandPrimitive.List
  bind:ref
  data-slot="command-list"
  class={cn("command-list-scrollbar min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-1", className)}
  {...restProps}
>
  {@render children?.()}
</CommandPrimitive.List>

<style>
  :global(.command-list-scrollbar) {
    scrollbar-color: color-mix(in oklab, var(--muted-foreground) 36%, transparent) transparent;
    scrollbar-gutter: stable;
    scrollbar-width: thin;
  }

  :global(.command-list-scrollbar::-webkit-scrollbar) {
    width: 0.5rem;
  }

  :global(.command-list-scrollbar::-webkit-scrollbar-track) {
    background: transparent;
  }

  :global(.command-list-scrollbar::-webkit-scrollbar-thumb) {
    background-color: color-mix(in oklab, var(--muted-foreground) 28%, transparent);
    border: 0.125rem solid transparent;
    background-clip: content-box;
  }

  :global(.command-list-scrollbar::-webkit-scrollbar-thumb:hover) {
    background-color: color-mix(in oklab, var(--muted-foreground) 44%, transparent);
  }
</style>
