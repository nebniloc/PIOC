<script lang="ts">
  import { Command as CommandPrimitive } from "bits-ui";
  import { cn } from "$lib/utils.js";

  let {
    ref = $bindable(null),
    class: className,
    heading,
    children,
    ...restProps
  }: CommandPrimitive.GroupProps & { heading?: string } = $props();
</script>

<CommandPrimitive.Group
  bind:ref
  data-slot="command-group"
  class={cn("overflow-hidden p-1 text-foreground", className)}
  {...restProps}
>
  {#if heading}
    <CommandPrimitive.GroupHeading
      data-slot="command-group-heading"
      class="px-2 py-1.5 text-[0.68rem] font-medium uppercase tracking-wider text-muted-foreground"
    >
      {heading}
    </CommandPrimitive.GroupHeading>
  {/if}
  <CommandPrimitive.GroupItems data-slot="command-group-items">
    {@render children?.()}
  </CommandPrimitive.GroupItems>
</CommandPrimitive.Group>
