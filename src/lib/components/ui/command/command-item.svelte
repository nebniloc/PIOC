<script lang="ts">
  import { Command as CommandPrimitive } from "bits-ui";
  import { cn } from "$lib/utils.js";

  let {
    ref = $bindable(null),
    class: className,
    children,
    ...restProps
  }: CommandPrimitive.ItemProps = $props();
</script>

<CommandPrimitive.Item
  bind:ref
  data-slot="command-item"
  class={cn(
    "relative flex cursor-default select-none items-center gap-2 rounded-none px-2 py-2 text-xs outline-none transition-colors data-[disabled]:pointer-events-none data-[selected]:bg-accent data-[selected]:text-accent-foreground data-[disabled]:opacity-50 [&_svg:not([class*='size-'])]:size-4 [&_svg]:shrink-0",
    className
  )}
  {...restProps}
>
  {@render children?.()}
</CommandPrimitive.Item>
