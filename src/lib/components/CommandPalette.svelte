<script lang="ts">
  import RiRobot2Line from "remixicon-svelte/icons/robot-2-line";
  import * as Command from "$lib/components/ui/command";
  import * as Dialog from "$lib/components/ui/dialog";
  import HotkeyList from "$lib/components/HotkeyList.svelte";
  import type { CommandPaletteCommand } from "$lib/command-palette";

  type CommandGroup = {
    heading: string;
    commands: CommandPaletteCommand[];
  };

  let {
    open = $bindable(false),
    commands,
    onOrchestrationPrompt,
    onInstallPackageCommand,
  }: {
    open?: boolean;
    commands: CommandPaletteCommand[];
    onOrchestrationPrompt?: (prompt: string) => void | Promise<void>;
    onInstallPackageCommand?: (command: string) => void | Promise<void>;
  } = $props();

  let search = $state("");
  let commandGroups = $derived.by<CommandGroup[]>(() => {
    const groups = new Map<string, CommandPaletteCommand[]>();

    for (const command of commands) {
      const heading = command.group ?? "Commands";
      const groupCommands = groups.get(heading);

      if (groupCommands) {
        groupCommands.push(command);
      } else {
        groups.set(heading, [command]);
      }
    }

    return Array.from(groups, ([heading, groupedCommands]) => ({ heading, commands: groupedCommands }));
  });
  let prompt = $derived(search.trim());
  let canPromptOrchestrationAgent = $derived(Boolean(prompt && onOrchestrationPrompt));
  let canInstallPackageCommand = $derived(Boolean(prompt && onInstallPackageCommand && isPackageInstallPrompt(prompt)));

  $effect(() => {
    if (open) {
      search = "";
    }
  });

  function commandValue(command: CommandPaletteCommand) {
    return [command.title, command.description, command.group, command.shortcut, ...(command.keywords ?? [])]
      .filter(Boolean)
      .join(" ");
  }

  function runCommand(command: CommandPaletteCommand) {
    if (command.disabled) return;

    open = false;
    void command.run();
  }

  function isPackageInstallPrompt(value: string) {
    const normalized = value.trim().toLowerCase();

    return Boolean(
      normalized.startsWith("pi install ") ||
        normalized.startsWith("install ") ||
        normalized.startsWith("npm:") ||
        normalized.startsWith("git:") ||
        normalized.startsWith("git@") ||
        normalized.startsWith("https://") ||
        normalized.startsWith("http://") ||
        normalized.startsWith("ssh://") ||
        normalized.startsWith("./") ||
        normalized.startsWith("../") ||
        normalized.startsWith("~"),
    );
  }

  function submitInstallPackageCommand() {
    if (!prompt || !onInstallPackageCommand) return;

    open = false;
    void onInstallPackageCommand(prompt);
    search = "";
  }
  function submitOrchestrationPrompt() {
    if (!prompt || !onOrchestrationPrompt) return;

    open = false;
    void onOrchestrationPrompt(prompt);
    search = "";
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="h-[28rem] max-h-[calc(100vh-4rem)] overflow-hidden p-0 sm:max-w-xl" showCloseButton={false}>
    <Dialog.Title class="sr-only">Command palette</Dialog.Title>
    <Command.Root label="Command palette" loop class="rounded-none">
      <Command.Input bind:value={search} autofocus placeholder="Type a command, function, or future agent prompt…" />
      <Command.List>
        <Command.Empty>No matching commands found.</Command.Empty>

        {#each commandGroups as group (group.heading)}
          <Command.Group heading={group.heading} value={group.heading}>
            {#each group.commands as command (command.id)}
              <Command.Item
                value={commandValue(command)}
                keywords={command.keywords}
                disabled={command.disabled}
                onSelect={() => runCommand(command)}
              >
                <span class="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span class="truncate font-medium text-foreground">{command.title}</span>
                  {#if command.description}
                    <span class="truncate text-muted-foreground">{command.description}</span>
                  {/if}
                </span>
                {#if command.shortcut}
                  <HotkeyList items={[command.shortcut]} class="ml-auto shrink-0 justify-end text-muted-foreground" />
                {/if}
              </Command.Item>
            {/each}
          </Command.Group>
        {/each}

        {#if canInstallPackageCommand}
          <Command.Group heading="Pi add-ons" value="Pi add-ons" forceMount>
            <Command.Item
              value={`Install Pi add-on ${prompt}`}
              keywords={["install", "pi", "addon", "package", "extension", "skill", prompt]}
              forceMount
              onSelect={submitInstallPackageCommand}
            >
              <span class="flex min-w-0 flex-1 flex-col gap-0.5">
                <span class="truncate font-medium text-foreground">Install Pi add-on</span>
                <span class="truncate text-muted-foreground">{prompt}</span>
              </span>
            </Command.Item>
          </Command.Group>
        {/if}

        {#if prompt}
          <Command.Group heading="Orchestration agent" value="Orchestration agent" forceMount>
            <Command.Item
              value={`Prompt orchestration agent ${prompt}`}
              keywords={["agent", "ask", "orchestration", "prompt", prompt]}
              disabled={!canPromptOrchestrationAgent}
              forceMount
              onSelect={submitOrchestrationPrompt}
            >
              <RiRobot2Line aria-hidden="true" />
              <span class="flex min-w-0 flex-1 flex-col gap-0.5">
                <span class="truncate font-medium text-foreground">Ask orchestration agent</span>
                <span class="truncate text-muted-foreground">{prompt}</span>
              </span>
              {#if !canPromptOrchestrationAgent}
                <span class="shrink-0 text-muted-foreground">NYI</span>
              {/if}
            </Command.Item>
          </Command.Group>
        {/if}
      </Command.List>
    </Command.Root>
  </Dialog.Content>
</Dialog.Root>
