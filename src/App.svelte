<script lang="ts">
  import { onMount } from "svelte";
  import TerminalWorkspace from "$lib/components/TerminalWorkspace.svelte";
  import HotkeyList from "$lib/components/HotkeyList.svelte";

  let footerHotkeys = $state<string[]>([]);

  function handleHotkeysChange(hotkeys: string[]) {
    footerHotkeys = hotkeys;
  }
  onMount(() => {
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
