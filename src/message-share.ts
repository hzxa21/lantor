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

export function messageShareLink(message: Message, baseUrl?: string | null) {
  const base = (baseUrl && baseUrl.trim()) || window.location.origin;
  const url = new URL(base, window.location.href);
  const current = new URL(window.location.href);
  const token = current.searchParams.get("token");
  if (token && url.origin === current.origin) {
    url.searchParams.set("token", token);
  }
  url.hash = `/message/${message.id}`;
  return url.toString();
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

function shareSvg(messages: Message[], surfaceLabel: string) {
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
  return { svg, width, height };
}

function downloadBlob(blob: Blob, fileName: string) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function loadImage(url: string) {
  return new Promise<HTMLImageElement>((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = reject;
    image.src = url;
  });
}

function canvasToPngBlob(canvas: HTMLCanvasElement) {
  return new Promise<Blob>((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob);
      else reject(new Error("Canvas export failed"));
    }, "image/png");
  });
}

export async function downloadMessagesAsImage(messages: Message[], surfaceLabel: string) {
  if (messages.length === 0) return;
  const { svg, width, height } = shareSvg(messages, surfaceLabel);
  const svgBlob = new Blob([svg], { type: "image/svg+xml;charset=utf-8" });
  const fileBase = `localslock-share-${Date.now()}`;
  const svgUrl = URL.createObjectURL(svgBlob);
  try {
    if (height > 12_000) throw new Error("Share image too tall for PNG export");
    const image = await loadImage(svgUrl);
    const scale = Math.min(2, Math.max(1, window.devicePixelRatio || 1));
    const canvas = document.createElement("canvas");
    canvas.width = Math.ceil(width * scale);
    canvas.height = Math.ceil(height * scale);
    const context = canvas.getContext("2d");
    if (!context) throw new Error("Canvas unavailable");
    context.scale(scale, scale);
    context.drawImage(image, 0, 0, width, height);
    const pngBlob = await canvasToPngBlob(canvas);
    downloadBlob(pngBlob, `${fileBase}.png`);
  } catch (err) {
    console.warn("PNG share export failed; falling back to SVG", err);
    downloadBlob(svgBlob, `${fileBase}.svg`);
  } finally {
    URL.revokeObjectURL(svgUrl);
  }
}
