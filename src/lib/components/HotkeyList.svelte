<script lang="ts">
  import * as Kbd from "$lib/components/ui/kbd";
  import { cn } from "$lib/utils.js";

  type HotkeyItem = string | { shortcut?: string; label: string };
  type ParsedHotkeyItem = { shortcut?: string; label?: string };

  let {
    items,
    separator = "·",
    class: className,
  }: {
    items: HotkeyItem[];
    separator?: string;
    class?: string;
  } = $props();

  const shortcutOnlyPattern = /^(?:(?:CTRL|SHIFT|ALT|META|CMD|OPTION)|[A-Z0-9])(?:\+(?:(?:CTRL|SHIFT|ALT|META|CMD|OPTION)|[A-Z0-9]))+$/i;
  const labeledShortcutPattern = /^([A-Z0-9+]+):\s*(.*)$/i;

  function parseItem(item: HotkeyItem): ParsedHotkeyItem {
    if (typeof item !== "string") {
      return item;
    }

    const labeledShortcut = item.match(labeledShortcutPattern);
    if (labeledShortcut) {
      return { shortcut: labeledShortcut[1], label: labeledShortcut[2] };
    }

    if (shortcutOnlyPattern.test(item)) {
      return { shortcut: item };
    }

    return { label: item };
  }

  function shortcutParts(shortcut: string) {
    return shortcut.split("+").map((part) => part.trim()).filter(Boolean);
  }

  function formatKey(key: string) {
    const upperKey = key.toUpperCase();
    if (upperKey === "CTRL") return "Ctrl";
    if (upperKey === "SHIFT") return "Shift";
    if (upperKey === "ALT") return "Alt";
    if (upperKey === "META") return "Meta";
    if (upperKey === "CMD") return "Cmd";
    if (upperKey === "OPTION") return "Option";
    return key.length === 1 ? upperKey : key;
  }
</script>

<span class={cn("inline-flex flex-wrap items-center justify-center gap-x-2 gap-y-1", className)}>
  {#each items as item, index}
    {@const parsedItem = parseItem(item)}
    {#if index > 0}
      <span class="text-muted-foreground/50">{separator}</span>
    {/if}
    <span class="inline-flex items-center gap-1">
      {#if parsedItem.shortcut}
        <Kbd.Group>
          {#each shortcutParts(parsedItem.shortcut) as key, keyIndex}
            {#if keyIndex > 0}
              <span class="text-muted-foreground/70">+</span>
            {/if}
            <Kbd.Root>{formatKey(key)}</Kbd.Root>
          {/each}
        </Kbd.Group>
      {/if}
      {#if parsedItem.label}
        <span>{parsedItem.label}</span>
      {/if}
    </span>
  {/each}
</span>
