import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { createReadTool, withFileMutationQueue } from "@earendil-works/pi-coding-agent";
import { Text } from "@earendil-works/pi-tui";
import { Type } from "typebox";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

// PIOC bundled fork of @jerryan/pi-hashline-edit.
// Kept self-contained so desktop builds can load it without installing npm package deps.

const PACKAGE = { name: "pioc-hashline", version: "0.1.1" };
const DEFAULT_LIMIT = 2000;
const MAX_OUTPUT_BYTES = 50 * 1024;
const ANCHOR_SEP = "#";
const CONTENT_SEP = "│";

const IMAGE_MIME_BY_EXTENSION: Record<string, string> = {
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".png": "image/png",
  ".gif": "image/gif",
  ".webp": "image/webp",
};

type ReadParams = {
  path: string;
  offset?: number;
  limit?: number;
  raw?: boolean;
};

type EditEntry = {
  range: string[];
  lines: string[];
};

type EditParams = {
  path: string;
  edits: EditEntry[];
};

type FileParts = {
  lines: string[];
  eol: string;
  finalNewline: boolean;
};

type LastEdit = {
  path: string;
  absolutePath: string;
  previousContent: string;
  previousFileHash: string;
};

type ResolvedEdit = {
  edit: EditEntry;
  index: number;
  start: number;
  end: number;
};

function stripAt(path: string): string {
  return path.startsWith("@") ? path.slice(1) : path;
}

function extensionOf(path: string): string {
  const match = path.toLowerCase().match(/\.[^.\\/]+$/);
  return match?.[0] ?? "";
}

function imageMimeTypeFromExtension(path: string): string | undefined {
  return IMAGE_MIME_BY_EXTENSION[extensionOf(path)];
}

function startsWith(buffer: Buffer, bytes: number[]): boolean {
  if (buffer.length < bytes.length) return false;
  return bytes.every((byte, index) => buffer[index] === byte);
}

function startsWithAscii(buffer: Buffer, offset: number, text: string): boolean {
  if (buffer.length < offset + text.length) return false;
  for (let index = 0; index < text.length; index++) {
    if (buffer[offset + index] !== text.charCodeAt(index)) return false;
  }
  return true;
}

function readUint32BE(buffer: Buffer, offset: number): number {
  return (
    ((buffer[offset] ?? 0) * 0x1000000) +
    ((buffer[offset + 1] ?? 0) << 16) +
    ((buffer[offset + 2] ?? 0) << 8) +
    (buffer[offset + 3] ?? 0)
  );
}

function isPng(buffer: Buffer): boolean {
  return buffer.length >= 16 && readUint32BE(buffer, 8) === 13 && startsWithAscii(buffer, 12, "IHDR");
}

function isAnimatedPng(buffer: Buffer): boolean {
  let offset = 8;
  while (offset + 8 <= buffer.length) {
    const chunkLength = readUint32BE(buffer, offset);
    const chunkTypeOffset = offset + 4;
    if (startsWithAscii(buffer, chunkTypeOffset, "acTL")) return true;
    if (startsWithAscii(buffer, chunkTypeOffset, "IDAT")) return false;

    const nextOffset = offset + 8 + chunkLength + 4;
    if (nextOffset <= offset || nextOffset > buffer.length) return false;
    offset = nextOffset;
  }
  return false;
}

function imageMimeTypeFromBuffer(buffer: Buffer): string | undefined {
  if (startsWith(buffer, [0xff, 0xd8, 0xff])) return buffer[3] === 0xf7 ? undefined : "image/jpeg";
  if (startsWith(buffer, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) {
    return isPng(buffer) && !isAnimatedPng(buffer) ? "image/png" : undefined;
  }
  if (startsWithAscii(buffer, 0, "GIF")) return "image/gif";
  if (startsWithAscii(buffer, 0, "RIFF") && startsWithAscii(buffer, 8, "WEBP")) return "image/webp";
  return undefined;
}

function imageMimeType(path: string, buffer?: Buffer): string | undefined {
  return imageMimeTypeFromExtension(path) ?? (buffer ? imageMimeTypeFromBuffer(buffer) : undefined);
}

function readWithBuiltInRead(params: ReadParams, ctx: ExtensionContext, displayPath: string, toolCallId: string, signal: AbortSignal | undefined, onUpdate: unknown) {
  const originalRead = createReadTool(ctx.cwd);
  return originalRead.execute(
    toolCallId,
    { path: displayPath, offset: params.offset, limit: params.limit },
    signal,
    onUpdate as never,
  );
}

function isProbablyBinary(buffer: Buffer): boolean {
  const length = Math.min(buffer.length, 4096);
  for (let i = 0; i < length; i++) {
    if (buffer[i] === 0) return true;
  }
  return false;
}

function splitFile(content: string): FileParts {
  const eol = content.includes("\r\n") ? "\r\n" : "\n";
  const finalNewline = /\r?\n$/.test(content);
  const body = finalNewline ? content.replace(/\r?\n$/, "") : content;
  const lines = body.length === 0 ? [] : body.split(/\r?\n/);
  return { lines, eol, finalNewline };
}

function joinFile(lines: string[], eol: string, finalNewline: boolean): string {
  if (lines.length === 0) return "";
  return lines.join(eol) + (finalNewline ? eol : "");
}

function fnv1a8(input: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return (hash & 0xff).toString(16).padStart(2, "0").toUpperCase();
}

function lineHash(lines: string[], index: number): string {
  const previous = index > 0 ? lines[index - 1] : "";
  const current = lines[index] ?? "";
  const next = index < lines.length - 1 ? lines[index + 1] : "";
  return fnv1a8(`${previous}\n${current}\n${next}`);
}

function fileHash(content: string): string {
  // Lightweight non-cryptographic full-file hash for stale/undo metadata.
  let hash = 0x811c9dc5;
  for (let i = 0; i < content.length; i++) {
    hash ^= content.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0").toUpperCase();
}

function formatAnchor(lines: string[], index: number, width: number): string {
  return `${String(index + 1).padStart(width, " ")}${ANCHOR_SEP}${lineHash(lines, index)}`;
}

function formatAnchoredLine(lines: string[], index: number, width: number): string {
  return `${formatAnchor(lines, index, width)}${CONTENT_SEP}${lines[index] ?? ""}`;
}

function boundedLineNumber(value: unknown, fallback: number): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.max(1, Math.floor(value));
}

function byteLength(value: string): number {
  return Buffer.byteLength(value, "utf8");
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)}KB`;
  const mb = kb / 1024;
  return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)}MB`;
}

function formatReadOutput(displayPath: string, content: string, params: ReadParams): { text: string; details: Record<string, unknown> } {
  const { lines } = splitFile(content);
  const offset = boundedLineNumber(params.offset, 1);
  const requestedLimit = boundedLineNumber(params.limit, DEFAULT_LIMIT);
  const limit = Math.min(DEFAULT_LIMIT, requestedLimit);
  const startIndex = Math.min(lines.length, offset - 1);
  const endIndex = Math.min(lines.length, startIndex + limit);
  const selected = lines.slice(startIndex, endIndex);

  if (params.raw) {
    const raw = selected.join("\n");
    return {
      text: raw,
      details: {
        path: displayPath,
        package: PACKAGE,
        raw: true,
        totalLines: lines.length,
        shownStartLine: selected.length > 0 ? startIndex + 1 : 0,
        shownEndLine: selected.length > 0 ? endIndex : 0,
        truncated: endIndex < lines.length,
      },
    };
  }

  const header = [
    `pioc-hashline read: ${displayPath}`,
    `Copy LINE${ANCHOR_SEP}HASH anchors into edit ranges. Send literal replacement lines without prefixes.`,
    ``,
  ];
  const width = String(Math.max(endIndex, 1)).length;
  const output = [...header];
  let outputBytes = byteLength(output.join("\n"));
  let truncated = false;

  for (let i = startIndex; i < endIndex; i++) {
    const row = formatAnchoredLine(lines, i, width);
    const rowBytes = byteLength(row) + 1;
    if (outputBytes + rowBytes > MAX_OUTPUT_BYTES) {
      truncated = true;
      break;
    }
    output.push(row);
    outputBytes += rowBytes;
  }

  if (endIndex < lines.length) truncated = true;
  if (truncated) {
    output.push(``, `[truncated: shown up to ${formatSize(MAX_OUTPUT_BYTES)} / ${DEFAULT_LIMIT} lines. Re-read with offset/limit for a narrower range.]`);
  }

  return {
    text: output.join("\n"),
    details: {
      path: displayPath,
      package: PACKAGE,
      totalLines: lines.length,
      shownStartLine: output.length > header.length ? startIndex + 1 : 0,
      shownEndLine: output.length > header.length ? Math.min(endIndex, lines.length) : 0,
      truncated,
    },
  };
}

function parseAnchor(anchor: string): { line: number; hash: string } {
  const trimmed = anchor.trim();
  const match = trimmed.match(/^(\d+)#([0-9a-fA-F]{2})$/);
  if (!match) {
    throw new Error(`Invalid hashline anchor "${anchor}". Expected LINE#HASH, for example 42#A4.`);
  }
  return { line: Number(match[1]), hash: match[2].toUpperCase() };
}

function resolveAnchor(lines: string[], anchor: string): number {
  const parsed = parseAnchor(anchor);
  const index = parsed.line - 1;
  if (index < 0 || index >= lines.length) {
    throw new Error(`Hashline anchor ${anchor} points outside the current file. Re-read the file and retry.`);
  }

  const actualHash = lineHash(lines, index);
  if (actualHash !== parsed.hash) {
    throw new Error(`Stale hashline anchor ${anchor}; current line ${parsed.line} hash is ${actualHash}. Re-read the file and retry.`);
  }

  return index;
}

function validateReplacementLines(lines: string[], editIndex: number) {
  for (const line of lines) {
    if (/^\s*\d+#[0-9a-fA-F]{2}│/.test(line)) {
      throw new Error(`Edit ${editIndex + 1} replacement contains hashline read prefixes. Send literal file content only.`);
    }
    if (/^[+-]\s*\d*(?:#[0-9a-fA-F]{2})?\s*│/.test(line)) {
      throw new Error(`Edit ${editIndex + 1} replacement appears to contain diff prefixes. Send literal file content only.`);
    }
  }
}

function resolveEdits(lines: string[], edits: EditEntry[]): ResolvedEdit[] {
  if (!Array.isArray(edits) || edits.length === 0) {
    throw new Error("edit requires at least one edit entry.");
  }

  const resolved = edits.map((edit, index) => {
    if (!Array.isArray(edit.range) || edit.range.length !== 2) {
      throw new Error(`Edit ${index + 1} range must be [startAnchor, endAnchor].`);
    }
    if (!Array.isArray(edit.lines)) {
      throw new Error(`Edit ${index + 1} lines must be an array of strings.`);
    }
    validateReplacementLines(edit.lines, index);

    const start = resolveAnchor(lines, edit.range[0] ?? "");
    const end = resolveAnchor(lines, edit.range[1] ?? "");
    if (end < start) {
      throw new Error(`Edit ${index + 1} end anchor is before its start anchor.`);
    }
    return { edit, index, start, end };
  });

  const sorted = [...resolved].sort((a, b) => a.start - b.start);
  for (let i = 1; i < sorted.length; i++) {
    if (sorted[i].start <= sorted[i - 1].end) {
      throw new Error(`Edits ${sorted[i - 1].index + 1} and ${sorted[i].index + 1} overlap. Merge overlapping edits into one entry.`);
    }
  }

  return sorted;
}

function applyResolvedEdits(originalLines: string[], resolved: ResolvedEdit[]): string[] {
  const nextLines = [...originalLines];
  for (const item of [...resolved].reverse()) {
    nextLines.splice(item.start, item.end - item.start + 1, ...item.edit.lines);
  }
  return nextLines;
}

function changedWindow(resolved: ResolvedEdit[], nextLines: string[]): { start: number; end: number } {
  let delta = 0;
  let start = Number.POSITIVE_INFINITY;
  let end = 0;

  for (const item of resolved) {
    const newStart = item.start + delta;
    const newEnd = Math.max(newStart, newStart + item.edit.lines.length - 1);
    start = Math.min(start, Math.max(0, newStart - 3));
    end = Math.max(end, Math.min(nextLines.length - 1, newEnd + 3));
    delta += item.edit.lines.length - (item.end - item.start + 1);
  }

  if (!Number.isFinite(start)) start = 0;
  if (nextLines.length === 0) return { start: 0, end: -1 };
  return { start: Math.max(0, start), end: Math.min(nextLines.length - 1, end) };
}

function buildEditResponse(displayPath: string, original: string, next: string, resolved: ResolvedEdit[]): string {
  const originalLineCount = splitFile(original).lines.length;
  const nextLines = splitFile(next).lines;
  const removed = resolved.reduce((sum, item) => sum + item.end - item.start + 1, 0);
  const added = resolved.reduce((sum, item) => sum + item.edit.lines.length, 0);
  const output = [
    `Applied ${resolved.length} hashline edit(s) to ${displayPath}.`,
    `Lines: ${originalLineCount} → ${nextLines.length} (${added} added, ${removed} removed).`,
  ];

  const window = changedWindow(resolved, nextLines);
  if (window.end >= window.start) {
    output.push(``, `Fresh anchors near the change:`);
    const width = String(window.end + 1).length;
    for (let i = window.start; i <= window.end; i++) {
      output.push(formatAnchoredLine(nextLines, i, width));
    }
  } else {
    output.push(``, `<file is now empty>`);
  }

  output.push(``, `For distant follow-up edits, run read again to get fresh anchors.`);
  return output.join("\n");
}

async function readTextOrImage(params: ReadParams, ctx: ExtensionContext, toolCallId: string, signal: AbortSignal | undefined, onUpdate: unknown) {
  const displayPath = stripAt(params.path);

  if (imageMimeType(displayPath)) {
    return readWithBuiltInRead(params, ctx, displayPath, toolCallId, signal, onUpdate);
  }

  const absolutePath = resolve(ctx.cwd, displayPath);
  const buffer = await readFile(absolutePath);

  if (imageMimeType(displayPath, buffer) || isProbablyBinary(buffer)) {
    return readWithBuiltInRead(params, ctx, displayPath, toolCallId, signal, onUpdate);
  }

  const formatted = formatReadOutput(displayPath, buffer.toString("utf8"), params);
  return {
    content: [{ type: "text" as const, text: formatted.text }],
    details: formatted.details,
  };
}

async function applyHashlineEdit(params: EditParams, ctx: ExtensionContext, setLastEdit: (edit: LastEdit) => void) {
  const displayPath = stripAt(params.path);
  const absolutePath = resolve(ctx.cwd, displayPath);

  return withFileMutationQueue(absolutePath, async () => {
    const original = await readFile(absolutePath, "utf8");
    const parts = splitFile(original);
    const resolved = resolveEdits(parts.lines, params.edits);
    const nextLines = applyResolvedEdits(parts.lines, resolved);
    const next = joinFile(nextLines, parts.eol, parts.finalNewline);

    if (next === original) {
      return {
        content: [{ type: "text" as const, text: `No changes needed for ${displayPath}.` }],
        details: { path: displayPath, package: PACKAGE, classification: "noop" },
      };
    }

    setLastEdit({ path: displayPath, absolutePath, previousContent: original, previousFileHash: fileHash(original) });
    await writeFile(absolutePath, next, "utf8");

    return {
      content: [{ type: "text" as const, text: buildEditResponse(displayPath, original, next, resolved) }],
      details: {
        path: displayPath,
        package: PACKAGE,
        previousFileHash: fileHash(original),
        fileHash: fileHash(next),
        edits: resolved.map((item) => ({ startLine: item.start + 1, endLine: item.end + 1, addedLines: item.edit.lines.length })),
      },
    };
  });
}

export default function piocHashline(pi: ExtensionAPI) {
  let lastEdit: LastEdit | undefined;

  pi.registerTool({
    name: "read",
    label: "Read",
    description:
      "Read a text file with LINE#HASH anchors for hashline edit. Use raw:true only when you do not plan to edit. Images are returned as attachments.",
    promptSnippet: "Read files with hashline anchors for stale-safe edits.",
    promptGuidelines: [
      "Use read before edit; copy LINE#HASH anchors from read output into edit ranges.",
      "Use read raw:true only when you do not plan to edit that file.",
    ],
    parameters: Type.Object(
      {
        path: Type.String({ description: "Path to read. Leading @ is ignored." }),
        offset: Type.Optional(Type.Number({ description: "1-indexed first line to read." })),
        limit: Type.Optional(Type.Number({ description: `Maximum lines to return, capped at ${DEFAULT_LIMIT}.` })),
        raw: Type.Optional(Type.Boolean({ description: "Return plain text without LINE#HASH anchors." })),
      },
      { additionalProperties: false },
    ),
    async execute(toolCallId, params, signal, onUpdate, ctx) {
      return readTextOrImage(params, ctx, toolCallId, signal, onUpdate);
    },
    renderCall(args, theme) {
      let text = theme.fg("toolTitle", theme.bold("read ")) + theme.fg("accent", args.path ?? "");
      const options = [];
      if (args.offset) options.push(`offset=${args.offset}`);
      if (args.limit) options.push(`limit=${args.limit}`);
      if (args.raw) options.push("raw");
      if (options.length > 0) text += theme.fg("dim", ` (${options.join(", ")})`);
      return new Text(text, 0, 0);
    },
    renderResult(result, { expanded, isPartial }, theme) {
      if (isPartial) return new Text(theme.fg("warning", "Reading..."), 0, 0);
      const image = result.content.find((item) => item?.type === "image") as { mimeType?: string } | undefined;
      if (image) {
        let text = theme.fg("success", "Image loaded");
        if (image.mimeType) text += theme.fg("dim", ` ${image.mimeType}`);
        return new Text(text, 0, 0);
      }

      const first = result.content[0];
      if (first?.type !== "text") return new Text(theme.fg("error", "No readable content"), 0, 0);

      const details = result.details as { path?: string; totalLines?: number; shownStartLine?: number; shownEndLine?: number; truncated?: boolean } | undefined;
      const shown = details?.shownStartLine && details.shownEndLine ? `lines ${details.shownStartLine}-${details.shownEndLine}` : "content";
      let text = theme.fg("success", `${shown}${details?.totalLines ? ` of ${details.totalLines}` : ""}`);
      if (details?.truncated) text += theme.fg("warning", " (truncated)");
      if (details?.path) text += theme.fg("dim", ` ${details.path}`);

      if (expanded) {
        const lines = first.text.split("\n").slice(0, 30);
        text += `\n${theme.fg("dim", lines.join("\n"))}`;
        const lineCount = first.text.split("\n").length;
        if (lineCount > 30) text += `\n${theme.fg("muted", `... ${lineCount - 30} more lines`)}`;
      }
      return new Text(text, 0, 0);
    },
  });

  pi.registerTool({
    name: "edit",
    label: "Edit",
    description:
      "Apply hashline edits to a file. Each edit replaces an inclusive [start,end] LINE#HASH range from read output with literal replacement lines. Use [] to delete.",
    promptSnippet: "Apply hashline anchored edits using ranges from read output.",
    promptGuidelines: [
      "Use edit only after read has returned LINE#HASH anchors for the target lines.",
      "For edit, send { path, edits: [{ range: [startAnchor, endAnchor], lines: [...] }] }; replacement lines must be literal file content with no hashline or diff prefixes.",
      "When changing multiple locations in one file, send non-overlapping edits in a single edit call.",
    ],
    parameters: Type.Object(
      {
        path: Type.String({ description: "Path to edit. Leading @ is ignored." }),
        edits: Type.Array(
          Type.Object(
            {
              range: Type.Array(Type.String(), {
                minItems: 2,
                maxItems: 2,
                description: "Inclusive [start,end] LINE#HASH anchors copied from read output.",
              }),
              lines: Type.Array(Type.String(), { description: "Literal replacement lines. Use [] to delete the range." }),
            },
            { additionalProperties: false },
          ),
          { minItems: 1, description: "Non-overlapping edits to apply atomically." },
        ),
      },
      { additionalProperties: false },
    ),
    async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
      return applyHashlineEdit(params, ctx, (edit) => {
        lastEdit = edit;
      });
    },
    renderCall(args, theme) {
      const count = Array.isArray(args.edits) ? args.edits.length : 0;
      let text = theme.fg("toolTitle", theme.bold("edit ")) + theme.fg("accent", args.path ?? "");
      text += theme.fg("dim", ` (${count} hashline edit${count === 1 ? "" : "s"})`);
      return new Text(text, 0, 0);
    },
    renderResult(result, { expanded, isPartial }, theme) {
      if (isPartial) return new Text(theme.fg("warning", "Editing..."), 0, 0);
      const first = result.content[0];
      const output = first?.type === "text" ? first.text : "";
      let text = theme.fg("success", output.split("\n")[0] || "edit complete");
      if (expanded && output) text += `\n${theme.fg("dim", output)}`;
      return new Text(text, 0, 0);
    },
  });

  pi.registerTool({
    name: "undo",
    label: "Undo Hashline Edit",
    description: "Undo the most recent pioc-hashline edit in this session.",
    promptSnippet: "Undo the most recent pioc-hashline edit.",
    parameters: Type.Object({}, { additionalProperties: false }),
    async execute(_toolCallId, _params, _signal, _onUpdate, _ctx) {
      if (!lastEdit) {
        return { content: [{ type: "text" as const, text: "No pioc-hashline edit is available to undo." }], details: { package: PACKAGE, undone: false } };
      }

      const editToUndo = lastEdit;
      return withFileMutationQueue(editToUndo.absolutePath, async () => {
        await writeFile(editToUndo.absolutePath, editToUndo.previousContent, "utf8");
        lastEdit = undefined;
        return {
          content: [{ type: "text" as const, text: `Restored ${editToUndo.path} to previous fileHash ${editToUndo.previousFileHash}.` }],
          details: { package: PACKAGE, path: editToUndo.path, fileHash: editToUndo.previousFileHash, undone: true },
        };
      });
    },
    renderResult(result, { isPartial }, theme) {
      if (isPartial) return new Text(theme.fg("warning", "Undoing..."), 0, 0);
      const first = result.content[0];
      const output = first?.type === "text" ? first.text : "undo complete";
      return new Text(theme.fg("success", output), 0, 0);
    },
  });
}
