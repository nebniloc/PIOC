<script lang="ts">
  import { Command as CommandPrimitive } from "bits-ui";
  import { cn } from "$lib/utils.js";

  let {
    ref = $bindable(null),
    class: className,
    children,
    ...restProps
  }: CommandPrimitive.RootProps = $props();
</script>

<CommandPrimitive.Root
  bind:ref
  data-slot="command"
  class={cn("flex h-full w-full flex-col overflow-hidden bg-popover text-popover-foreground", className)}
  {...restProps}
>
  {@render children?.()}
</CommandPrimitive.Root>
