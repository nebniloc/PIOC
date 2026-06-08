<script lang="ts">
  import { onMount } from "svelte";
  import TerminalWorkspace from "$lib/components/TerminalWorkspace.svelte";
  import HotkeyList from "$lib/components/HotkeyList.svelte";

  const isDevBuild = import.meta.env.MODE === "development";
  const appTitle = isDevBuild ? "PIOC Dev" : "PIOC";

  let footerHotkeys = $state<string[]>([]);

  function handleHotkeysChange(hotkeys: string[]) {
    footerHotkeys = hotkeys;
  }
  onMount(() => {
    document.title = appTitle;
    document.documentElement.classList.add("dark");
    document.documentElement.style.colorScheme = "dark";
  });
</script>

<main class="h-screen overflow-hidden bg-background text-foreground">
  <section class="mx-auto flex h-screen w-full max-w-none flex-col gap-4 p-8">
    <TerminalWorkspace onHotkeysChange={handleHotkeysChange} />
    {#if footerHotkeys.length > 0}
      <footer class="shrink-0 text-center text-xs text-muted-foreground/70">
        <HotkeyList items={footerHotkeys} />
      </footer>
    {/if}
  </section>
</main>

{#if isDevBuild}
  <aside
    aria-label="Development build"
    class="pointer-events-none fixed right-3 top-3 rounded-full border border-destructive bg-destructive px-3 py-1 text-xs font-semibold uppercase tracking-wide text-foreground shadow-lg"
  >
    DEV BUILD
  </aside>
  <div aria-hidden="true" class="pointer-events-none fixed inset-0 border-2 border-destructive"></div>
{/if}
