import type { Message } from "./types";
import { formatTime } from "./ui-utils";

function escapeXml(value: string) {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function attachmentLines(message: Message) {
  return message.attachments.map((attachment) => (
    `- Attachment: ${attachment.original_name} (${attachment.mime_type}, ${attachment.size_bytes} bytes)`
  ));
}

function artifactLines(message: Message) {
  return message.artifacts.map((artifact) => (
    `- Artifact: ${artifact.title || artifact.kind} (${artifact.kind})`
  ));
}

export function messageShareLink(message: Message) {
  const base = `${window.location.origin}${window.location.pathname}`;
  return `${base}#/message/${message.id}`;
}

export function messageToMarkdown(message: Message, surfaceLabel: string) {
  const lines = [
    `### ${message.sender_name} - ${formatTime(message.created_at)}`,
    "",
    `Surface: ${surfaceLabel}`,
    "",
    message.body.trim() || "_Empty message_",
  ];
  const attachments = attachmentLines(message);
  const artifacts = artifactLines(message);
  if (attachments.length > 0 || artifacts.length > 0) {
    lines.push("", "Resources:", ...attachments, ...artifacts);
  }
  return lines.join("\n");
}

export function messagesToMarkdown(messages: Message[], surfaceLabel: string) {
  return messages.map((message) => messageToMarkdown(message, surfaceLabel)).join("\n\n---\n\n");
}

function wrapLine(line: string, maxChars: number) {
  if (line.length <= maxChars) return [line];
  const result: string[] = [];
  let remaining = line;
  while (remaining.length > maxChars) {
    result.push(remaining.slice(0, maxChars));
    remaining = remaining.slice(maxChars);
  }
  if (remaining) result.push(remaining);
  return result;
}

function plainShareLines(messages: Message[], surfaceLabel: string) {
  const lines = [`LocalSlock share - ${surfaceLabel}`, ""];
  for (const message of messages) {
    lines.push(`${message.sender_name} - ${formatTime(message.created_at)}`);
    lines.push(...(message.body.trim() || "Empty message").split("\n"));
    for (const attachment of attachmentLines(message)) lines.push(attachment);
    for (const artifact of artifactLines(message)) lines.push(artifact);
    lines.push("");
  }
  return lines.flatMap((line) => wrapLine(line, 92));
}

export function downloadMessagesAsSvg(messages: Message[], surfaceLabel: string) {
  const lines = plainShareLines(messages, surfaceLabel);
  const width = 1100;
  const lineHeight = 24;
  const padding = 36;
  const height = Math.max(220, padding * 2 + lines.length * lineHeight);
  const text = lines.map((line, index) => (
    `<text x="${padding}" y="${padding + 24 + index * lineHeight}">${escapeXml(line)}</text>`
  )).join("\n");
  const svg = [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">`,
    '<rect width="100%" height="100%" rx="24" fill="#fffaf0"/>',
    '<rect x="18" y="18" width="1064" height="' + (height - 36) + '" rx="18" fill="#ffffff" stroke="#f0d88b"/>',
    '<style>text{font-family:-apple-system,BlinkMacSystemFont,"SF Pro Text",sans-serif;font-size:17px;fill:#202124;white-space:pre}</style>',
    text,
    "</svg>",
  ].join("\n");
  const blob = new Blob([svg], { type: "image/svg+xml;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `localslock-share-${Date.now()}.svg`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}
