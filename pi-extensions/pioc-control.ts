import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import type { Api, Model } from "@earendil-works/pi-ai";

type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh";
type CommandStatus = "running" | "ok" | "error";
type AgentStatus = "starting" | "idle" | "working" | "shutdown";

type ControlCommand = {
  id?: string;
  type?: string;
  provider?: string;
  modelId?: string;
  level?: ThinkingLevel;
  requestedAt?: string;
};

type LastCommand = {
  id?: string;
  type: string;
  status: CommandStatus;
  requestedAt?: string;
  completedAt?: string;
  message?: string;
};

const TELEMETRY_VERSION = 1;
const THINKING_LEVELS: ThinkingLevel[] = ["off", "minimal", "low", "medium", "high", "xhigh"];

const terminalId = process.env.PIOC_TERMINAL_ID?.trim() || undefined;
const profileId = process.env.PIOC_PROFILE_ID?.trim() || undefined;
const telemetryPath =
  process.env.PIOC_TELEMETRY_PATH?.trim() ||
  (process.env.PIOC_TELEMETRY_DIR?.trim() && terminalId
    ? join(process.env.PIOC_TELEMETRY_DIR.trim(), `terminal-${terminalId}.json`)
    : undefined);
const commandPath = process.env.PIOC_COMMAND_PATH?.trim() || undefined;
const controlEnabled = Boolean(telemetryPath && commandPath);

export default function piocControl(pi: ExtensionAPI) {
  let latestCtx: ExtensionContext | undefined;
  let sessionStartedAt = Date.now();
  let agentStartedAt: number | undefined;
  let totalWorkingMs = 0;
  let status: AgentStatus = "starting";
  let turnCount = 0;
  let currentTurnIndex = 0;
  let activeToolCalls = new Map<string, { name: string; startedAt: number }>();
  let lastCommand: LastCommand | undefined;
  let commandOffset = 0;
  let commandBuffer = "";
  let commandTimer: ReturnType<typeof setInterval> | undefined;
  let telemetryTimer: ReturnType<typeof setInterval> | undefined;
  const processedCommandIds = new Set<string>();

  function nowIso() {
    return new Date().toISOString();
  }

  function numberValue(value: unknown) {
    return typeof value === "number" && Number.isFinite(value) ? value : 0;
  }

  function elapsedWorkingMs() {
    return totalWorkingMs + (agentStartedAt ? Date.now() - agentStartedAt : 0);
  }

  function formatCompactTokens(tokens: number) {
    if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
    if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
    return String(tokens);
  }

  function formatDuration(ms: number) {
    const seconds = Math.floor(ms / 1000);
    const minutes = Math.floor(seconds / 60);
    const hours = Math.floor(minutes / 60);
    if (hours > 0) return `${hours}h ${minutes % 60}m`;
    if (minutes > 0) return `${minutes}m ${seconds % 60}s`;
    return `${seconds}s`;
  }

  function modelKey(model?: Model<Api>) {
    return model ? `${model.provider}/${model.id}` : "none";
  }

  function modelDisplayName(model?: Model<Api>) {
    if (!model) return "No model";
    return model.name || model.id;
  }

  function serializeModel(ctx: ExtensionContext, model: Model<Api>) {
    return {
      provider: model.provider,
      id: model.id,
      key: modelKey(model),
      name: model.name || model.id,
      api: model.api,
      reasoning: Boolean(model.reasoning),
      input: model.input,
      contextWindow: model.contextWindow,
      maxTokens: model.maxTokens,
      hasAuth: ctx.modelRegistry.hasConfiguredAuth(model),
    };
  }

  function getModels(ctx: ExtensionContext) {
    try {
      return ctx.modelRegistry
        .getAll()
        .map((model) => serializeModel(ctx, model))
        .sort((left, right) => {
          const providerCompare = left.provider.localeCompare(right.provider);
          return providerCompare === 0 ? left.name.localeCompare(right.name) : providerCompare;
        });
    } catch (error) {
      return [];
    }
  }

  function collectStats(ctx: ExtensionContext) {
    const stats = {
      userMessages: 0,
      assistantMessages: 0,
      toolCalls: 0,
      toolResults: 0,
      totalMessages: 0,
      tokens: {
        input: 0,
        output: 0,
        cacheRead: 0,
        cacheWrite: 0,
        total: 0,
      },
      cost: 0,
      contextUsage: undefined as undefined | { tokens: number | null; contextWindow: number; percent: number | null },
    };

    const entries = ctx.sessionManager.getBranch();
    for (const entry of entries) {
      if (entry.type !== "message") continue;

      const message = entry.message as any;
      if (!message || typeof message !== "object") continue;

      stats.totalMessages += 1;
      if (message.role === "user") stats.userMessages += 1;
      if (message.role === "assistant") {
        stats.assistantMessages += 1;
        const content = Array.isArray(message.content) ? message.content : [];
        stats.toolCalls += content.filter((block: any) => block?.type === "toolCall").length;

        const usage = message.usage ?? {};
        stats.tokens.input += numberValue(usage.input);
        stats.tokens.output += numberValue(usage.output);
        stats.tokens.cacheRead += numberValue(usage.cacheRead);
        stats.tokens.cacheWrite += numberValue(usage.cacheWrite);
        stats.tokens.total +=
          numberValue(usage.totalTokens) ||
          numberValue(usage.input) + numberValue(usage.output) + numberValue(usage.cacheRead) + numberValue(usage.cacheWrite);
        stats.cost += numberValue(usage.cost?.total);
      }
      if (message.role === "toolResult") stats.toolResults += 1;
    }

    const contextUsage = ctx.getContextUsage();
    if (contextUsage) {
      const tokens = typeof contextUsage.tokens === "number" ? contextUsage.tokens : null;
      const contextWindow = contextUsage.contextWindow;
      stats.contextUsage = {
        tokens,
        contextWindow,
        percent: tokens === null || !contextWindow ? null : Math.round((tokens / contextWindow) * 1000) / 10,
      };
    }

    return stats;
  }

  function ensureTelemetryDirectory() {
    if (!telemetryPath) return;
    mkdirSync(dirname(telemetryPath), { recursive: true });
  }

  function writeTelemetry(ctx = latestCtx) {
    if (!ctx || !telemetryPath) return;

    try {
      ensureTelemetryDirectory();
      const stats = collectStats(ctx);
      const currentModel = ctx.model;
      const currentWorkingMs = agentStartedAt ? Date.now() - agentStartedAt : 0;
      const payload = {
        version: TELEMETRY_VERSION,
        controlReady: controlEnabled,
        terminalId,
        profileId,
        pid: process.pid,
        cwd: ctx.cwd,
        mode: ctx.mode,
        updatedAt: nowIso(),
        sessionStartedAt: new Date(sessionStartedAt).toISOString(),
        sessionFile: ctx.sessionManager.getSessionFile(),
        sessionId: ctx.sessionManager.getSessionId(),
        sessionName: ctx.sessionManager.getSessionName(),
        status,
        isWorking: status === "working",
        turnCount,
        currentTurnIndex,
        currentWorkingMs,
        totalWorkingMs: elapsedWorkingMs(),
        model: currentModel
          ? {
              provider: currentModel.provider,
              id: currentModel.id,
              key: modelKey(currentModel),
              name: modelDisplayName(currentModel),
              api: currentModel.api,
              reasoning: Boolean(currentModel.reasoning),
              contextWindow: currentModel.contextWindow,
              maxTokens: currentModel.maxTokens,
            }
          : undefined,
        thinkingLevel: pi.getThinkingLevel(),
        thinkingLevels: THINKING_LEVELS,
        models: getModels(ctx),
        activeTools: pi.getActiveTools(),
        activeToolCalls: [...activeToolCalls.entries()].map(([id, tool]) => ({
          id,
          name: tool.name,
          startedAt: new Date(tool.startedAt).toISOString(),
          elapsedMs: Date.now() - tool.startedAt,
        })),
        stats,
        lastCommand,
      };

      writeFileSync(telemetryPath, `${JSON.stringify(payload, null, 2)}\n`, "utf8");

      ctx.ui.setTitle(`pi ${status === "working" ? "working" : "idle"} - ${modelDisplayName(currentModel)}`);

    } catch (error) {
      console.error(`[pioc-control] failed to write telemetry: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  function resolveModelFromArgs(ctx: ExtensionContext, args: string) {
    const trimmed = args.trim();
    if (!trimmed) return undefined;

    const [first, ...rest] = trimmed.split(/\s+/);
    let provider: string | undefined;
    let modelId: string | undefined;

    if (rest.length > 0) {
      provider = first;
      modelId = rest.join(" ");
    } else {
      const slashIndex = first.indexOf("/");
      if (slashIndex > 0) {
        provider = first.slice(0, slashIndex);
        modelId = first.slice(slashIndex + 1);
      } else {
        const matches = ctx.modelRegistry
          .getAll()
          .filter((model) => model.id === first || model.name === first || model.id.includes(first) || model.name?.includes(first));
        return matches.length === 1 ? matches[0] : undefined;
      }
    }

    return provider && modelId ? ctx.modelRegistry.find(provider, modelId) : undefined;
  }

  async function setModel(ctx: ExtensionContext, provider: string | undefined, modelId: string | undefined) {
    if (!provider || !modelId) throw new Error("set_model requires provider and modelId");

    const model = ctx.modelRegistry.find(provider, modelId);
    if (!model) throw new Error(`Model not found: ${provider}/${modelId}`);

    const success = await pi.setModel(model);
    if (!success) throw new Error(`No configured API key for ${provider}/${modelId}`);
  }

  function setThinkingLevel(level: unknown) {
    if (!THINKING_LEVELS.includes(level as ThinkingLevel)) {
      throw new Error(`Unsupported thinking level: ${String(level)}`);
    }

    pi.setThinkingLevel(level as ThinkingLevel);
  }

  async function handleControlCommand(command: ControlCommand, ctx: ExtensionContext) {
    const id = command.id ?? `${Date.now()}`;
    if (processedCommandIds.has(id)) return;
    processedCommandIds.add(id);

    lastCommand = {
      id,
      type: command.type ?? "unknown",
      status: "running",
      requestedAt: command.requestedAt,
    };
    writeTelemetry(ctx);

    try {
      switch (command.type) {
        case "set_model":
          await setModel(ctx, command.provider, command.modelId);
          lastCommand = {
            ...lastCommand,
            status: "ok",
            completedAt: nowIso(),
            message: `Model set to ${command.provider}/${command.modelId}`,
          };
          break;
        case "set_thinking_level":
          setThinkingLevel(command.level);
          lastCommand = {
            ...lastCommand,
            status: "ok",
            completedAt: nowIso(),
            message: `Thinking level set to ${command.level}`,
          };
          break;
        case "refresh":
          ctx.modelRegistry.refresh();
          lastCommand = {
            ...lastCommand,
            status: "ok",
            completedAt: nowIso(),
            message: "Telemetry refreshed",
          };
          break;
        default:
          throw new Error(`Unsupported PIOC command: ${String(command.type)}`);
      }
    } catch (error) {
      lastCommand = {
        ...lastCommand,
        status: "error",
        completedAt: nowIso(),
        message: error instanceof Error ? error.message : String(error),
      };
      ctx.ui.notify(lastCommand.message ?? "PIOC command failed", "error");
    } finally {
      writeTelemetry(ctx);
    }
  }

  async function pollCommands() {
    if (!commandPath || !latestCtx) return;

    try {
      if (!existsSync(commandPath)) {
        writeFileSync(commandPath, "", "utf8");
        commandOffset = 0;
        return;
      }

      const text = readFileSync(commandPath, "utf8");
      if (text.length < commandOffset) {
        commandOffset = 0;
        commandBuffer = "";
      }

      const chunk = text.slice(commandOffset);
      commandOffset = text.length;
      if (!chunk) return;

      commandBuffer += chunk;
      const lines = commandBuffer.split(/\r?\n/);
      commandBuffer = lines.pop() ?? "";

      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        const command = JSON.parse(trimmed) as ControlCommand;
        await handleControlCommand(command, latestCtx);
      }
    } catch (error) {
      lastCommand = {
        type: "poll",
        status: "error",
        completedAt: nowIso(),
        message: error instanceof Error ? error.message : String(error),
      };
      writeTelemetry(latestCtx);
    }
  }

  function startCommandPoller() {
    if (!commandPath || commandTimer) return;

    try {
      mkdirSync(dirname(commandPath), { recursive: true });
      if (!existsSync(commandPath)) writeFileSync(commandPath, "", "utf8");
      commandOffset = readFileSync(commandPath, "utf8").length;
    } catch (error) {
      console.error(`[pioc-control] failed to initialize command file: ${error instanceof Error ? error.message : String(error)}`);
    }

    commandTimer = setInterval(() => void pollCommands(), 500);
  }

  function stopCommandPoller() {
    if (commandTimer) clearInterval(commandTimer);
    commandTimer = undefined;
  }

  function startTelemetryHeartbeat() {
    if (!telemetryPath || telemetryTimer) return;
    telemetryTimer = setInterval(() => writeTelemetry(), 1000);
  }

  function stopTelemetryHeartbeat() {
    if (telemetryTimer) clearInterval(telemetryTimer);
    telemetryTimer = undefined;
  }

  pi.on("session_start", async (_event, ctx) => {
    latestCtx = ctx;
    sessionStartedAt = Date.now();
    agentStartedAt = undefined;
    totalWorkingMs = 0;
    status = "idle";
    turnCount = 0;
    currentTurnIndex = 0;
    activeToolCalls.clear();

    ctx.ui.setStatus("pioc-control", undefined);
    startCommandPoller();
    startTelemetryHeartbeat();
    writeTelemetry(ctx);
  });

  pi.on("session_shutdown", async (_event, ctx) => {
    latestCtx = ctx;
    if (agentStartedAt) {
      totalWorkingMs += Date.now() - agentStartedAt;
      agentStartedAt = undefined;
    }
    status = "shutdown";
    activeToolCalls.clear();
    writeTelemetry(ctx);
    stopCommandPoller();
    stopTelemetryHeartbeat();
  });

  pi.on("agent_start", async (_event, ctx) => {
    latestCtx = ctx;
    status = "working";
    agentStartedAt = Date.now();
    writeTelemetry(ctx);
  });

  pi.on("agent_end", async (_event, ctx) => {
    latestCtx = ctx;
    if (agentStartedAt) {
      totalWorkingMs += Date.now() - agentStartedAt;
      agentStartedAt = undefined;
    }
    status = "idle";
    activeToolCalls.clear();
    writeTelemetry(ctx);
  });

  pi.on("turn_start", async (event, ctx) => {
    latestCtx = ctx;
    turnCount += 1;
    currentTurnIndex = event.turnIndex ?? turnCount;
    writeTelemetry(ctx);
  });

  pi.on("turn_end", async (_event, ctx) => {
    latestCtx = ctx;
    writeTelemetry(ctx);
  });

  pi.on("message_end", async (_event, ctx) => {
    latestCtx = ctx;
    writeTelemetry(ctx);
  });

  pi.on("tool_execution_start", async (event, ctx) => {
    latestCtx = ctx;
    activeToolCalls.set(event.toolCallId, { name: event.toolName, startedAt: Date.now() });
    writeTelemetry(ctx);
  });

  pi.on("tool_execution_end", async (event, ctx) => {
    latestCtx = ctx;
    activeToolCalls.delete(event.toolCallId);
    writeTelemetry(ctx);
  });

  pi.on("model_select", async (_event, ctx) => {
    latestCtx = ctx;
    writeTelemetry(ctx);
  });

  pi.on("thinking_level_select", async (_event, ctx) => {
    latestCtx = ctx;
    writeTelemetry(ctx);
  });

  pi.registerCommand("pioc-model", {
    description: "Set the active model for PIOC. Usage: /pioc-model provider/model",
    handler: async (args, ctx) => {
      latestCtx = ctx;
      const directModel = resolveModelFromArgs(ctx, args);
      let model = directModel;

      if (!model && !args.trim()) {
        const options = ctx.modelRegistry
          .getAll()
          .filter((candidate) => ctx.modelRegistry.hasConfiguredAuth(candidate))
          .map((candidate) => modelKey(candidate));
        const choice = await ctx.ui.select("Select model", options);
        if (!choice) return;
        model = resolveModelFromArgs(ctx, choice);
      }

      if (!model) {
        ctx.ui.notify("Model not found. Use /pioc-model provider/model.", "error");
        return;
      }

      await setModel(ctx, model.provider, model.id);
      ctx.ui.notify(`Model set to ${modelKey(model)}`, "info");
      writeTelemetry(ctx);
    },
  });

  pi.registerCommand("pioc-thinking", {
    description: "Set thinking level for PIOC. Usage: /pioc-thinking off|minimal|low|medium|high|xhigh",
    handler: async (args, ctx) => {
      latestCtx = ctx;
      const requested = args.trim() as ThinkingLevel;
      const level = THINKING_LEVELS.includes(requested)
        ? requested
        : ((await ctx.ui.select("Select thinking level", THINKING_LEVELS)) as ThinkingLevel | undefined);
      if (!level) return;
      setThinkingLevel(level);
      ctx.ui.notify(`Thinking level set to ${level}`, "info");
      writeTelemetry(ctx);
    },
  });

  pi.registerCommand("pioc-refresh", {
    description: "Refresh PIOC model/stat telemetry",
    handler: async (_args, ctx) => {
      latestCtx = ctx;
      ctx.modelRegistry.refresh();
      writeTelemetry(ctx);
      ctx.ui.notify("PIOC telemetry refreshed", "info");
    },
  });

  pi.registerCommand("pioc-stats", {
    description: "Show PIOC token usage and working-time stats",
    handler: async (_args, ctx) => {
      latestCtx = ctx;
      const stats = collectStats(ctx);
      const context = stats.contextUsage?.percent == null ? "unknown" : `${stats.contextUsage.percent}%`;
      ctx.ui.notify(
        `Tokens ${formatCompactTokens(stats.tokens.total)} · context ${context} · turns ${turnCount} · tools ${stats.toolCalls} · working ${formatDuration(elapsedWorkingMs())}`,
        "info",
      );
      writeTelemetry(ctx);
    },
  });
}
