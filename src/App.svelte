<script lang="ts">
  import { onMount } from "svelte";
  import packageJson from "../package.json";
  import TerminalWorkspace from "$lib/components/TerminalWorkspace.svelte";
  import HotkeyList from "$lib/components/HotkeyList.svelte";
  import { Button } from "$lib/components/ui/button";
  import RiDownloadCloudLine from "remixicon-svelte/icons/download-cloud-line";

  type AppUpdateAction = { disabled: boolean; title: string; run: () => void };

  const isDevBuild = import.meta.env.MODE === "development";
  const appTitle = isDevBuild ? "PIOC Dev" : "PIOC";
  const appVersionLabel = `${appTitle} v${packageJson.version}`;
  const commandPaletteHotkey = ["CTRL+K: Command Palette"];

  let appUpdateAction = $state<AppUpdateAction | null>(null);

  function handleAppUpdateActionChange(action: AppUpdateAction) {
    appUpdateAction = action;
  }

  onMount(() => {
    document.title = appTitle;
    document.documentElement.classList.add("dark");
    document.documentElement.style.colorScheme = "dark";
  });
</script>

<main class="h-screen overflow-hidden bg-background text-foreground">
  <section class="mx-auto flex h-screen w-full max-w-none flex-col gap-0 px-0 pb-0 pt-0">
    <TerminalWorkspace onAppUpdateActionChange={handleAppUpdateActionChange} />
    <footer class="flex shrink-0 items-center justify-between px-4 pb-2 pt-1 text-xs text-muted-foreground">
      <HotkeyList items={commandPaletteHotkey} class="justify-start" />
      <div class="flex shrink-0 items-center gap-2">
        {#if !isDevBuild}
          <Button
            variant="ghost"
            size="icon-xs"
            class="cursor-pointer text-muted-foreground hover:bg-muted/60 hover:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-25"
            disabled={!appUpdateAction || appUpdateAction.disabled}
            aria-label="Check for app updates"
            title={appUpdateAction?.title ?? "Check for app updates"}
            onclick={() => appUpdateAction?.run()}
          >
            <RiDownloadCloudLine class="text-muted-foreground" data-icon="inline-start" aria-hidden="true" />
          </Button>
        {/if}
        <span class="text-muted-foreground">{appVersionLabel}</span>
      </div>
    </footer>
  </section>
</main>
