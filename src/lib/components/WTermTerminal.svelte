<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Button, buttonVariants } from "$lib/components/ui/button";
  import * as Popover from "$lib/components/ui/popover";
  import { ScrollArea } from "$lib/components/ui/scroll-area";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { WTerm } from "@wterm/dom";
  import RiCloseLine from "remixicon-svelte/icons/close-line";
  import "@wterm/dom/css";

  type TerminalKind = "pi" | "shell";

  type PtyDataPayload = {
    id: number;
    data: string;
  };

  type PtyExitPayload = {
    id: number;
  };

  type PtyReadyPayload = {
    id: number;
  };

  type PtyErrorPayload = {
    id: number;
    error: string;
  };

  type PiocAgentStatus = "starting" | "idle" | "working" | "shutdown";
  type TerminalAgentState = "starting" | "idle" | "working" | "closing" | "exited" | "error";

  type PiocControlModel = {
    provider: string;
    id: string;
    key?: string;
    name: string;
    reasoning?: boolean;
    contextWindow?: number;
    maxTokens?: number;
    hasAuth?: boolean;
  };

  type PiocControlTelemetry = {
    controlReady?: boolean;
    updatedAt?: string;
    status?: PiocAgentStatus;
    isWorking?: boolean;
    totalWorkingMs?: number;
    currentWorkingMs?: number;
    turnCount?: number;
    model?: PiocControlModel;
    thinkingLevel?: string;
    thinkingLevels?: string[];
    models?: PiocControlModel[];
    activeToolCalls?: Array<{ id: string; name: string; elapsedMs?: number }>;
    stats?: {
      userMessages?: number;
      assistantMessages?: number;
      toolCalls?: number;
      toolResults?: number;
      totalMessages?: number;
      tokens?: {
        input?: number;
        output?: number;
        cacheRead?: number;
        cacheWrite?: number;
        total?: number;
      };
      cost?: number;
      contextUsage?: {
        tokens?: number | null;
        contextWindow?: number;
        percent?: number | null;
      };
    };
    lastCommand?: {
      id?: string;
      type?: string;
      status?: "running" | "ok" | "error";
      message?: string;
      completedAt?: string;
    };
  };

  type PiocControlCommand =
    | { type: "set_model"; provider: string; modelId: string }
    | { type: "set_thinking_level"; level: string }
    | { type: "refresh" };

  const DEFAULT_THINKING_LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh"];
  const PIOC_SELECT_CLASS = "pioc-select dark:bg-input/30 border-input focus-visible:border-ring focus-visible:ring-ring/50 h-8 rounded-none border bg-transparent px-2.5 py-1 text-xs text-foreground transition-colors focus-visible:ring-1 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 outline-none";

  let {
    id,
    kind = "pi",
    piProfileId,
    piProfileName,
    workingDirectory,
    autoFocus = true,
    closing = false,
    layoutActive = true,
    layoutKey = "",
    onReady,
    onClosed,
    onClose,
    onSelect,
    onUserInput,
    closeDisabled = false,
    selected = false,
  }: {
    id: number;
    kind?: TerminalKind;
    piProfileId?: string;
    piProfileName?: string;
    workingDirectory?: string;
    autoFocus?: boolean;
    closing?: boolean;
    layoutActive?: boolean;
    layoutKey?: string;
    onReady?: (id: number) => void;
    onClosed?: (id: number) => void;
    onClose?: () => void;
    onSelect?: () => void;
    onUserInput?: (id: number) => void;

    closeDisabled?: boolean;
    selected?: boolean;
  } = $props();

  let rootElement: HTMLElement;
  let hostElement: HTMLDivElement;
  let viewportElement = $state<HTMLElement | null>(null);
  let terminal: WTerm | null = null;
  let disposeTerminal: ((notifyClosed?: boolean) => Promise<void>) | null = null;
  let scheduleHostResize: (() => void) | null = null;
  let observeViewportResize: ((element: HTMLElement | null) => void) | null = null;
  const statusLabels = {
    pi: {
      starting: "Starting Pi…",
      closing: "Closing Pi…",
      ready: "Connected to Pi",
      exited: "Pi exited",
      unableToStart: "Unable to start Pi.",
    },
    shell: {
      starting: "Starting terminal…",
      closing: "Closing terminal…",
      ready: "Terminal ready",
      exited: "Terminal exited",
      unableToStart: "Unable to start terminal.",
    },
  } satisfies Record<TerminalKind, Record<"starting" | "closing" | "ready" | "exited" | "unableToStart", string>>;

  function statusLabel(label: keyof (typeof statusLabels)["pi"]) {
    return statusLabels[kind][label];
  }

  let status = $state(statusLabel("starting"));
  let agentWorking = $state(false);
  let activitySummary = $state(statusLabel("starting"));
  let piocControl = $state<PiocControlTelemetry | null>(null);
  let piocControlError = $state("");
  let piocControlBusy = $state(false);


  $effect(() => {
    if (autoFocus && !closing) {
      terminal?.focus();
    }
  });

  $effect(() => {
    void layoutKey;
    if (layoutActive && !closing) {
      scheduleHostResize?.();
    }
  });

  $effect(() => {
    observeViewportResize?.(viewportElement);
  });
  $effect(() => {
    if (closing) {
      void disposeTerminal?.(true);
    }
  });

  function formatTokens(value?: number) {
    const tokens = typeof value === "number" && Number.isFinite(value) ? value : 0;
    if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
    if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
    return `${tokens}`;
  }

  function formatDuration(ms?: number) {
    const totalMs = typeof ms === "number" && Number.isFinite(ms) ? ms : 0;
    const seconds = Math.floor(totalMs / 1000);
    const minutes = Math.floor(seconds / 60);
    const hours = Math.floor(minutes / 60);

    if (hours > 0) return `${hours}h ${minutes % 60}m`;
    if (minutes > 0) return `${minutes}m ${seconds % 60}s`;
    return `${seconds}s`;
  }

  function formatCost(value?: number) {
    if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) return "$0.00";
    return value < 0.01 ? `$${value.toFixed(4)}` : `$${value.toFixed(2)}`;
  }

  function formatContextUsage(telemetry = piocControl) {
    const percent = telemetry?.stats?.contextUsage?.percent;
    if (typeof percent !== "number") return "ctx --";
    return `ctx ${percent.toFixed(percent >= 10 ? 0 : 1)}%`;
  }

  function piocTelemetryStatus() {
    const value = piocControl?.status;
    return value === "starting" || value === "idle" || value === "working" || value === "shutdown"
      ? value
      : undefined;
  }

  function terminalAgentState(): TerminalAgentState {
    if (closing || status === statusLabel("closing")) return "closing";
    if (status === statusLabel("unableToStart") || piocControlError) return "error";

    if (kind !== "pi") {
      if (status === statusLabel("starting")) return "starting";
      if (status === statusLabel("exited")) return "exited";
      return agentWorking ? "working" : "idle";
    }

    if (status === statusLabel("exited")) return "exited";

    const eventStatus = piocTelemetryStatus();
    if (eventStatus === "shutdown") return "exited";
    if (eventStatus === "working" || (piocControl?.activeToolCalls?.length ?? 0) > 0) return "working";
    if (eventStatus === "idle") return "idle";

    return "starting";
  }


  function terminalStateText() {
    switch (terminalAgentState()) {
      case "working":
        return "Working";
      case "idle":
        return "Idle";
      case "closing":
        return "Closing";
      case "exited":
        return "Exited";
      case "error":
        return "Error";
      default:
        return "Starting";
    }
  }

  function terminalStateDotClass() {
    switch (terminalAgentState()) {
      case "working":
        return "size-2 shrink-0 rounded-full bg-amber-400";
      case "idle":
        return "size-2 shrink-0 rounded-full bg-emerald-500";
      case "error":
        return "size-2 shrink-0 rounded-full bg-destructive";
      case "exited":
      case "closing":
        return "size-2 shrink-0 rounded-full bg-muted-foreground";
      default:
        return "size-2 shrink-0 rounded-full bg-sky-400";
    }
  }

  function terminalStateLabel() {
    return kind === "pi" ? `Agent is ${terminalStateText().toLowerCase()}` : `Terminal is ${terminalStateText().toLowerCase()}`;
  }

  function terminalHeaderTitle() {
    if (kind !== "pi") return "Terminal";
    return `${terminalStateText()}${piProfileName ? ` · ${piProfileName}` : ""}`;
  }

  function piocModelValue(model?: PiocControlModel) {
    return model ? JSON.stringify([model.provider, model.id]) : "";
  }

  function currentPiocModelValue() {
    return piocModelValue(piocControl?.model);
  }

  function piocThinkingLevels() {
    return piocControl?.thinkingLevels?.length ? piocControl.thinkingLevels : DEFAULT_THINKING_LEVELS;
  }
  function piocAvailableModels() {
    return (piocControl?.models ?? []).filter((model) => model.hasAuth !== false);
  }

  function piocSummary() {
    if (kind !== "pi" || !piocControl) return activitySummary;

    const stats = piocControl.stats;
    const tokens = formatTokens(stats?.tokens?.total);
    const cost = formatCost(stats?.cost);
    const working = formatDuration(piocControl.totalWorkingMs);
    const context = formatContextUsage();
    const model = piocControl.model?.name ?? "No model";
    const thinkingLevel = piocControl.thinkingLevel ? ` · ${piocControl.thinkingLevel}` : "";
    const tools = piocControl.activeToolCalls?.length
      ? ` · tool: ${piocControl.activeToolCalls.map((tool) => tool.name).join(", ")}`
      : "";

    return `${model}${thinkingLevel} · ${tokens} tok · ${context} · ${cost} · ${working}${tools}`;
  }

  function piocCommandMessage() {
    if (piocControlError) return piocControlError;
    if (piocControl?.lastCommand?.status === "error") return piocControl.lastCommand.message ?? "PIOC command failed";
    if (piocControl?.lastCommand?.status === "running") return "Applying…";
    return "";
  }

  async function loadPiocControlTelemetry() {
    if (kind !== "pi") return null;

    try {
      const telemetry = await invoke<PiocControlTelemetry | null>("pioc_control_read", { id });
      piocControl = telemetry;
      piocControlError = "";
      return telemetry;
    } catch (error) {
      piocControlError = error instanceof Error ? error.message : String(error);
      return null;
    }
  }

  async function sendPiocControlCommand(command: PiocControlCommand) {
    if (kind !== "pi" || piocControlBusy) return;

    piocControlBusy = true;
    piocControlError = "";
    try {
      await invoke("pioc_control_command", { id, command });
      window.setTimeout(() => void loadPiocControlTelemetry(), 250);
    } catch (error) {
      piocControlError = error instanceof Error ? error.message : String(error);
    } finally {
      piocControlBusy = false;
    }
  }

  function handlePiocModelChange(event: Event) {
    const value = (event.currentTarget as HTMLSelectElement).value;
    if (!value) return;

    try {
      const [provider, modelId] = JSON.parse(value) as [string, string];
      if (!provider || !modelId) return;
      void sendPiocControlCommand({ type: "set_model", provider, modelId });
    } catch (error) {
      piocControlError = error instanceof Error ? error.message : String(error);
    }
  }

  function handlePiocThinkingChange(event: Event) {
    const level = (event.currentTarget as HTMLSelectElement).value;
    if (!level) return;
    void sendPiocControlCommand({ type: "set_thinking_level", level });
  }

  onMount(() => {
    if (kind !== "pi") return;

    let cancelled = false;
    let timer: number | null = null;

    async function poll() {
      if (cancelled) return;
      const telemetry = await loadPiocControlTelemetry();
      if (cancelled) return;
      timer = window.setTimeout(poll, telemetry?.status === "working" ? 500 : 1000);
    }

    timer = window.setTimeout(poll, 500);

    return () => {
      cancelled = true;
      if (timer !== null) {
        window.clearTimeout(timer);
      }
    };
  });
  onMount(() => {
    let cancelled = false;
    let ptyReady = false;
    let stopStarted = false;
    let nextTerminal: WTerm | null = null;
    let unlistenData: UnlistenFn | null = null;
    let unlistenExit: UnlistenFn | null = null;
    let unlistenReady: UnlistenFn | null = null;
    let unlistenError: UnlistenFn | null = null;
    let scrollTimer: number | null = null;
    let activityTimer: number | null = null;
    let stickToBottom = true;
    let scrollListenerElement: HTMLElement | null = null;

    let inputBuffer = "";
    let pendingTask: string | null = null;
    let resizeObserver: ResizeObserver | null = null;
    let resizeFrame: number | null = null;
    let observedViewportElement: HTMLElement | null = null;
    let lastPtyResizeCols: number | null = null;
    let lastPtyResizeRows: number | null = null;
    let footerRowsObserver: MutationObserver | null = null;
    let footerRowsTimer: number | null = null;

    function scrollContainer() {
      return viewportElement ?? hostElement;
    }

    function terminalRowHeight() {
      return Number.parseFloat(getComputedStyle(hostElement).getPropertyValue("--term-row-height")) || 17;
    }

    function trailingHiddenFooterHeight() {
      if (kind !== "pi") return 0;

      let height = 0;
      let row = hostElement.querySelector<HTMLElement>(".term-grid > .term-row:last-child");
      while (row?.classList.contains("pioc-terminal-footer-row-hidden")) {
        height += row.getBoundingClientRect().height || terminalRowHeight();
        const previousRow = row.previousElementSibling;
        row = previousRow instanceof HTMLElement ? previousRow : null;
      }

      return height;
    }

    function maxScrollTop(element = scrollContainer()) {
      const maxScroll = Math.max(0, element.scrollHeight - element.clientHeight);
      return Math.max(0, maxScroll - trailingHiddenFooterHeight());
    }

    function bottomTolerance() {
      return Math.max(48, terminalRowHeight() * 3);
    }

    function isNearBottom() {
      const element = scrollContainer();
      return maxScrollTop(element) - element.scrollTop <= bottomTolerance();
    }

    function scrollToBottom() {
      const element = scrollContainer();
      element.scrollTop = maxScrollTop(element);
      stickToBottom = true;
    }

    function updateStickToBottomFromScroll() {
      stickToBottom = isNearBottom();
    }

    function attachScrollListener() {
      const element = scrollContainer();
      if (scrollListenerElement === element) return;

      scrollListenerElement?.removeEventListener("scroll", updateStickToBottomFromScroll);
      scrollListenerElement = element;
      scrollListenerElement.addEventListener("scroll", updateStickToBottomFromScroll, { passive: true });
      updateStickToBottomFromScroll();
    }

    function measureTerminalSize() {
      const row = document.createElement("div");
      row.className = "term-row";
      row.style.visibility = "hidden";
      row.style.position = "absolute";
      row.style.pointerEvents = "none";

      const probe = document.createElement("span");
      probe.textContent = "W";
      row.appendChild(probe);
      hostElement.appendChild(row);

      const charWidth = probe.getBoundingClientRect().width;
      const rowHeight = row.getBoundingClientRect().height;
      row.remove();

      if (charWidth <= 0 || rowHeight <= 0) return null;

      const availableElement = viewportElement ?? hostElement;
      const hostStyles = getComputedStyle(hostElement);
      const horizontalPadding =
        (Number.parseFloat(hostStyles.paddingLeft) || 0) + (Number.parseFloat(hostStyles.paddingRight) || 0);
      const verticalPadding =
        (Number.parseFloat(hostStyles.paddingTop) || 0) + (Number.parseFloat(hostStyles.paddingBottom) || 0);
      const availableWidth = Math.max(0, availableElement.clientWidth - horizontalPadding);
      const availableHeight = Math.max(0, availableElement.clientHeight - verticalPadding);

      return {
        cols: Math.max(1, Math.floor(availableWidth / charWidth)),
        rows: Math.max(1, Math.floor(availableHeight / rowHeight)),
      };
    }

    function syncPtySize(targetTerminal = terminal ?? nextTerminal) {
      if (!targetTerminal || !ptyReady || cancelled) return;

      const cols = targetTerminal.cols;
      const rows = targetTerminal.rows;
      if (cols === lastPtyResizeCols && rows === lastPtyResizeRows) return;

      lastPtyResizeCols = cols;
      lastPtyResizeRows = rows;
      void invoke("pty_resize", { id, cols, rows }).catch(() => {
        if (lastPtyResizeCols === cols && lastPtyResizeRows === rows) {
          lastPtyResizeCols = null;
          lastPtyResizeRows = null;
        }
      });
    }

    function resizeTerminalToHost(targetTerminal = terminal ?? nextTerminal) {
      if (!targetTerminal || !layoutActive || cancelled || closing) return;

      attachScrollListener();
      const shouldScroll = stickToBottom || isNearBottom();
      const size = measureTerminalSize();
      if (!size) return;

      if (size.cols !== targetTerminal.cols || size.rows !== targetTerminal.rows) {
        targetTerminal.resize(size.cols, size.rows);
      }

      syncPtySize(targetTerminal);

      if (shouldScroll) {
        scheduleScrollToBottom();
      }
    }

    function scheduleResizeToHost() {
      if (resizeFrame !== null) return;

      resizeFrame = window.requestAnimationFrame(() => {
        resizeFrame = null;
        resizeTerminalToHost();
      });
    }

    function observeViewportResizeTarget(element: HTMLElement | null) {
      if (!resizeObserver || observedViewportElement === element) return;

      if (observedViewportElement) {
        resizeObserver.unobserve(observedViewportElement);
      }

      observedViewportElement = element;
      if (observedViewportElement) {
        resizeObserver.observe(observedViewportElement);
      }

      attachScrollListener();
      if (layoutActive) {
        scheduleResizeToHost();
      }
    }

    scheduleHostResize = scheduleResizeToHost;
    observeViewportResize = observeViewportResizeTarget;

    resizeObserver = new ResizeObserver(() => {
      if (layoutActive) {
        scheduleResizeToHost();
      }
    });
    resizeObserver.observe(rootElement);
    resizeObserver.observe(hostElement);
    observeViewportResizeTarget(viewportElement);
    window.addEventListener("resize", scheduleResizeToHost);

    if (layoutActive) {
      scheduleResizeToHost();
    }
    function normalizeTerminalRowText(value: string) {
      return value.replace(/\u00a0/g, " ").replace(/\s+/g, " ").trim();
    }

    function isWorkingDirectoryFooterRow(value: string) {
      const cwd = workingDirectory?.trim();
      if (!cwd) return false;

      const variants = [cwd, cwd.replace(/\\/g, "/"), cwd.replace(/\//g, "\\")]
        .map(normalizeTerminalRowText)
        .filter(Boolean);

      return variants.some((variant) =>
        value === variant || value.startsWith(`${variant} (`) || value.startsWith(`${variant} •`),
      );
    }

    function isPiBuiltinFooterRow(value: string) {
      if (kind !== "pi") return false;

      const text = normalizeTerminalRowText(value);
      if (!text) return false;

      if (isWorkingDirectoryFooterRow(text)) return true;
      if (/^(?:↑[\d.,]+(?:[kKmM])?\s+↓[\d.,]+(?:[kKmM])?\s+)?\$\d+(?:\.\d+)?(?: \(sub\))? .*[%?]\/\d+(?:\.\d+)?[kKmM]?(?: \(auto\))?/.test(text)) return true;
      if (/^[○●×]\s+.*\bthinking\b.*\btok\b.*\bctx\b/.test(text)) return true;

      return false;
    }

    function suppressPiFooterRows() {
      if (kind !== "pi") return;

      hostElement.querySelectorAll<HTMLElement>(".term-row").forEach((row) => {
        row.classList.toggle("pioc-terminal-footer-row-hidden", isPiBuiltinFooterRow(row.textContent ?? ""));
      });

      if (stickToBottom) {
        scrollToBottom();
      }
    }
    function scheduleSuppressPiFooterRows() {
      if (kind !== "pi" || footerRowsTimer !== null) return;

      footerRowsTimer = window.setTimeout(() => {
        footerRowsTimer = null;
        window.requestAnimationFrame(suppressPiFooterRows);
      }, 0);
    }

    function scheduleScrollToBottom() {
      if (scrollTimer !== null) {
        window.clearTimeout(scrollTimer);
      }

      scrollTimer = window.setTimeout(() => {
        scrollTimer = null;
        window.requestAnimationFrame(() => {
          if (cancelled) return;

          scrollToBottom();
          window.requestAnimationFrame(() => {
            if (!cancelled && stickToBottom) {
              scrollToBottom();
            }
          });
        });
      }, 0);
    }



    function truncateActivity(value: string) {
      const normalized = value.replace(/\s+/g, " ").trim();
      return normalized.length > 96 ? `${normalized.slice(0, 95)}…` : normalized;
    }

    function scheduleIdle() {
      if (activityTimer !== null) {
        window.clearTimeout(activityTimer);
      }

      activityTimer = window.setTimeout(() => {
        activityTimer = null;
        agentWorking = false;
        if (activitySummary === "Agent is working") {
          activitySummary = "Waiting for task";
        }
      }, 3000);
    }

    function markBackendActivity(data: string) {
      if (kind === "pi" || !data.trim() || !pendingTask || cancelled || closing) return;

      agentWorking = true;
      scheduleIdle();
    }

    function skipEscapeSequence(data: string, startIndex: number) {
      if (data[startIndex + 1] !== "[") return startIndex;

      let index = startIndex + 1;
      while (index + 1 < data.length) {
        index += 1;
        const sequenceCharacter = data[index];
        if (sequenceCharacter >= "@" && sequenceCharacter <= "~") break;
      }

      return index;
    }

    function trackUserInput(data: string) {
      let hasTypedInput = false;

      for (let index = 0; index < data.length; index += 1) {
        const character = data[index];

        if (character === "\x1b") {
          index = skipEscapeSequence(data, index);
          continue;
        }

        if (character === "\r" || character === "\n") {
          hasTypedInput = true;
          const submittedTask = truncateActivity(inputBuffer);
          inputBuffer = "";

          if (submittedTask) {
            pendingTask = submittedTask;
            activitySummary = submittedTask;
            agentWorking = false;
            if (activityTimer !== null) {
              window.clearTimeout(activityTimer);
              activityTimer = null;
            }
          }
          continue;
        }

        if (character === "\b" || character === "\x7f") {
          hasTypedInput = true;
          inputBuffer = inputBuffer.slice(0, -1);
          continue;
        }

        if (character === "\t") {
          hasTypedInput = true;
          continue;
        }

        if (character >= " ") {
          hasTypedInput = true;
          inputBuffer += character;
        }
      }

      return hasTypedInput;
    }

    function sanitizeInputForPty(data: string) {
      if (kind !== "pi") return data;

      // Ctrl+D sends EOT (\x04), which terminates the Pi process and leaves the
      // embedded terminal with no live backend. Ignore it for Pi sessions; keep
      // normal shell terminals unchanged so EOF still works there.
      return data.replace(/\x04/g, "");
    }

    async function stopTerminal(notifyClosed = false) {
      if (stopStarted) return;

      stopStarted = true;
      cancelled = true;
      ptyReady = false;
      status = statusLabel("closing");
      agentWorking = false;
      unlistenData?.();
      unlistenExit?.();
      unlistenReady?.();
      unlistenError?.();
      if (scrollListenerElement) {
        scrollListenerElement.removeEventListener("scroll", updateStickToBottomFromScroll);
        scrollListenerElement = null;
      }
      if (scrollTimer !== null) {
        window.clearTimeout(scrollTimer);
        scrollTimer = null;
      }
      if (activityTimer !== null) {
        window.clearTimeout(activityTimer);
        activityTimer = null;
      }
      footerRowsObserver?.disconnect();
      footerRowsObserver = null;
      if (footerRowsTimer !== null) {
        window.clearTimeout(footerRowsTimer);
        footerRowsTimer = null;
      }

      const terminalToDestroy = terminal ?? nextTerminal;
      terminal = null;
      nextTerminal = null;
      terminalToDestroy?.destroy();

      if (notifyClosed) {
        onClosed?.(id);
      }

      try {
        await invoke("pty_kill", { id });
      } catch {
        // The session may already be gone if the process exited during teardown.
      }
    }

    async function startTerminal() {
      status = statusLabel("starting");
      activitySummary = statusLabel("starting");
      piocControl = null;
      piocControlError = "";
      nextTerminal = new WTerm(hostElement, {
        cols: 80,
        rows: 24,
        cursorBlink: true,
        autoResize: false,
        onData: (data) => {
          if (!ptyReady || cancelled) return;

          const ptyInput = sanitizeInputForPty(data);
          if (!ptyInput) return;

          onSelect?.();
          if (trackUserInput(ptyInput)) {
            onUserInput?.(id);
            stickToBottom = true;
            scheduleScrollToBottom();
          }
          void invoke("pty_write", { id, data: ptyInput });
        },
        onResize: () => {
          syncPtySize(nextTerminal);
        },
      });

      try {
        unlistenData = await listen<PtyDataPayload>("pty:data", (event) => {
          if (event.payload.id === id && !cancelled) {
            const shouldScroll = stickToBottom || isNearBottom();

            nextTerminal?.write(event.payload.data);
            scheduleSuppressPiFooterRows();
            markBackendActivity(event.payload.data);

            if (shouldScroll) {
              stickToBottom = true;
              scheduleScrollToBottom();
            } else {
              stickToBottom = false;
            }
          }
        });
        unlistenExit = await listen<PtyExitPayload>("pty:exit", (event) => {
          if (event.payload.id === id && !cancelled) {
            ptyReady = false;
            status = statusLabel("exited");
            agentWorking = false;
            activitySummary = statusLabel("exited");
          }
        });
        unlistenReady = await listen<PtyReadyPayload>("pty:ready", (event) => {
          if (event.payload.id === id && !cancelled) {
            ptyReady = true;
            lastPtyResizeCols = null;
            lastPtyResizeRows = null;
            status = statusLabel("ready");
            agentWorking = false;
            activitySummary = kind === "shell" ? statusLabel("ready") : "Waiting for task";
            pendingTask = null;
            if (activityTimer !== null) {
              window.clearTimeout(activityTimer);
              activityTimer = null;
            }
            onReady?.(id);
            resizeTerminalToHost(nextTerminal);
            syncPtySize(nextTerminal);
            scheduleResizeToHost();

            if (autoFocus) {
              nextTerminal?.focus();
            }
          }
        });
        unlistenError = await listen<PtyErrorPayload>("pty:error", (event) => {
          if (event.payload.id === id && !cancelled) {
            const message = event.payload.error || statusLabel("unableToStart");
            ptyReady = false;
            status = statusLabel("unableToStart");
            agentWorking = false;
            activitySummary = message;
            piocControl = null;
            piocControlError = kind === "pi" ? message : "";
            pendingTask = null;
          }
        });

        if (cancelled) return;

        await nextTerminal.init();
        if (kind === "pi") {
          footerRowsObserver?.disconnect();
          footerRowsObserver = new MutationObserver(scheduleSuppressPiFooterRows);
          footerRowsObserver.observe(hostElement, { childList: true, subtree: true, characterData: true });
          scheduleSuppressPiFooterRows();
        }
        if (cancelled) return;

        resizeTerminalToHost(nextTerminal);
        scheduleSuppressPiFooterRows();
        terminal = nextTerminal;
        await invoke("pty_start", {
          request: {
            id,
            cols: nextTerminal.cols,
            rows: nextTerminal.rows,
            mode: kind,
            workingDirectory: workingDirectory || null,
            piProfileId: kind === "pi" ? piProfileId || null : null,
          },
        });
        if (cancelled) {
          void invoke("pty_kill", { id });
        }
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error || statusLabel("unableToStart"));
        status = statusLabel("unableToStart");
        agentWorking = false;
        activitySummary = message;
        piocControl = null;
        piocControlError = kind === "pi" ? message : "";
        nextTerminal?.destroy();
      }
    }

    disposeTerminal = stopTerminal;
    void startTerminal();

    if (closing) {
      void stopTerminal(true);
    }

    return () => {
      window.removeEventListener("resize", scheduleResizeToHost);
      resizeObserver?.disconnect();
      if (resizeFrame !== null) {
        window.cancelAnimationFrame(resizeFrame);
      }
      scheduleHostResize = null;
      observeViewportResize = null;
      void stopTerminal(false);
      disposeTerminal = null;
    };
  });
</script>

<section
  bind:this={rootElement}
  class="flex min-h-0 flex-1 flex-col overflow-hidden border border-border bg-card text-card-foreground transition-shadow"
  style={selected
    ? "outline: 1px solid rgba(34, 211, 238, 0.38); outline-offset: -1px; box-shadow: 0 0 14px rgba(34, 211, 238, 0.24), inset 0 0 10px rgba(34, 211, 238, 0.08);"
    : "outline: 1px solid transparent; outline-offset: -1px; box-shadow: none;"}
  role="group"
  aria-label={kind === "shell" ? `Terminal ${id}` : `Pi instance ${id}`}
  onpointerdown={() => onSelect?.()}
>
  <div class="flex items-center justify-between gap-2 border-b px-3 py-2">
    {#if kind === "pi"}
      <Popover.Root>
        <Popover.Trigger
          type="button"
          class={buttonVariants({
            variant: "ghost",
            class: "h-auto min-w-0 flex-1 shrink justify-start gap-2 px-1.5 py-1 text-left",
          })}
          title="Open Pi controls"
          aria-label={`Open controls for ${piProfileName ?? `Pi instance ${id}`}`}
          onpointerdown={(event) => event.stopPropagation()}
        >
          <span
            class={terminalStateDotClass()}
            title={terminalStateLabel()}
            aria-label={terminalStateLabel()}
          ></span>
          <div class="flex min-w-0 flex-col">
            <p class="truncate text-xs font-medium">{terminalHeaderTitle()}</p>
            <p class="truncate text-xs text-muted-foreground" title={piocSummary()}>{piocSummary()}</p>
          </div>
        </Popover.Trigger>
        <Popover.Content align="start" sideOffset={8} class="w-80 max-w-[calc(100vw-2rem)]">
          <Popover.Header>
            <Popover.Title>{terminalHeaderTitle()}</Popover.Title>
            <Popover.Description>{piocSummary()}</Popover.Description>
          </Popover.Header>

          <div class="flex items-center gap-2 rounded-none border border-border bg-muted px-2 py-1.5 text-xs">
            <span
              class={terminalStateDotClass()}
              aria-hidden="true"
            ></span>
            <span class="font-medium">{terminalStateText()}</span>
            <span class="min-w-0 truncate text-muted-foreground">
              {piocControl?.controlReady ? `${piocControl.model?.name ?? "No model"} · ${piocControl.thinkingLevel ?? "thinking"}` : "Controls loading…"}
            </span>
          </div>

          {#if piocCommandMessage()}
            <p class={piocControl?.lastCommand?.status === "error" || piocControlError ? "rounded-none border border-border bg-muted px-2 py-1.5 text-xs text-destructive" : "rounded-none border border-border bg-muted px-2 py-1.5 text-xs text-muted-foreground"} title={piocCommandMessage()}>
              {piocCommandMessage()}
            </p>
          {/if}

          <div class="flex flex-col gap-2">
            <label class="flex flex-col gap-1 text-xs text-foreground">
              <span class="font-medium">Model</span>
              <select
                class={`${PIOC_SELECT_CLASS} w-full`}
                aria-label="Pi model"
                title="Pi model"
                value={currentPiocModelValue()}
                disabled={!piocControl?.controlReady || piocControlBusy || piocAvailableModels().length === 0}
                onchange={handlePiocModelChange}
              >
                {#if piocAvailableModels().length === 0}
                  <option value="">Model</option>
                {/if}
                {#each piocAvailableModels() as model (`${model.provider}/${model.id}`)}
                  <option value={piocModelValue(model)}>
                    {model.name} · {model.provider}
                  </option>
                {/each}
              </select>
            </label>

            <label class="flex flex-col gap-1 text-xs text-foreground">
              <span class="font-medium">Thinking level</span>
              <select
                class={`${PIOC_SELECT_CLASS} w-full`}
                aria-label="Pi thinking level"
                title="Pi thinking level"
                value={piocControl?.thinkingLevel ?? ""}
                disabled={!piocControl?.controlReady || piocControlBusy}
                onchange={handlePiocThinkingChange}
              >
                {#each piocThinkingLevels() as level (level)}
                  <option value={level}>{level}</option>
                {/each}
              </select>
            </label>
          </div>

          {#if !piocControl?.controlReady}
            <p class="text-xs text-muted-foreground">Controls become available after Pi finishes starting.</p>
          {/if}
        </Popover.Content>
      </Popover.Root>
    {:else}
      <div class="flex min-w-0 flex-1 items-center gap-2 text-left">
        <span
          class={terminalStateDotClass()}
          title={terminalStateLabel()}
          aria-label={terminalStateLabel()}
        ></span>
        <div class="flex min-w-0 flex-col">
          <p class="truncate text-xs font-medium">{terminalHeaderTitle()}</p>
          <p class="truncate text-xs text-muted-foreground" title={piocSummary()}>{piocSummary()}</p>
        </div>
      </div>
    {/if}

    {#if onClose}
      <Button
        variant="ghost"
        size="icon-sm"
        type="button"
        title={kind === "shell" ? `Close terminal ${id}` : `Close Pi instance ${id}`}
        aria-label={kind === "shell" ? `Close terminal ${id}` : `Close Pi instance ${id}`}
        disabled={closeDisabled}
        onpointerdown={(event) => event.stopPropagation()}
        onclick={(event) => {
          event.stopPropagation();
          onClose?.();
        }}
      >
        <RiCloseLine data-icon="inline-start" aria-hidden="true" />
      </Button>
    {:else if status && status !== statusLabel("ready")}
      <p class="text-xs text-muted-foreground">{status}</p>
    {/if}
  </div>
  <ScrollArea
    bind:viewportRef={viewportElement}
    type="auto"
    class="terminal-scroll-area min-h-0 flex-1 bg-background text-left"
    aria-label={`wterm terminal ${id}`}
  >
    <div
      bind:this={hostElement}
      class="terminal-host min-h-full w-full bg-background text-left"
      aria-label={`wterm terminal ${id}`}
    ></div>
  </ScrollArea>
</section>

<style>
  .terminal-host {
    --term-font-family: var(--font-mono);
    --term-font-size: 13px;
    --term-bg: var(--background);
    --term-fg: var(--foreground);
    --term-cursor: var(--foreground);
    --term-color-0: var(--background);
    --term-color-7: var(--foreground);
    --term-color-15: var(--foreground);
    border-radius: 0;
    box-shadow: none;
  }

  :global(.terminal-host.wterm) {
    height: auto !important;
    min-height: 100%;
    overflow: visible !important;
  }
  /*
   * @wterm/dom uses the final cell background to paint full-width row/grid
   * backgrounds and a 1px row seam. Pi's cards/tool blocks rely on that for
   * complete right edges and continuous filled areas.
   *
   * The bad case is only a final-column cursor/reverse-video cell: propagating
   * that cell turns the whole row/grid into the cursor color. Clear just that
   * propagated background while preserving normal terminal backgrounds.
   */
  :global(.terminal-host .term-row:has(> .term-cursor:last-child)) {
    background: transparent !important;
    box-shadow: none !important;
  }

  :global(.terminal-host .term-grid:has(> .term-row:last-child > .term-cursor:last-child)) {
    background: transparent !important;
  }
  :global(.terminal-host .term-row.pioc-terminal-footer-row-hidden) {
    visibility: hidden;
  }

  :global(select.pioc-select),
  :global(select.pioc-select option) {
    background-color: var(--background);
    color: var(--foreground);
  }

  :global(select.pioc-select option) {
    border-radius: 0;
  }

  :global(select.pioc-select option:checked) {
    background-color: var(--muted);
    color: var(--foreground);
  }

  :global(select.pioc-select option:disabled) {
    display: none;
  }

</style>
