import { Agent, Channel } from "./types";

export type MentionState = {
  query: string;
  start: number;
  end: number;
};

export type MentionKind = "agent" | "channel";

export type TokenMentionState = MentionState & {
  kind: MentionKind;
};

function isMentionBoundary(text: string, markerIndex: number) {
  if (markerIndex === 0) return true;
  const previous = text[markerIndex - 1];
  return !/[A-Za-z0-9_-]/.test(previous);
}

function getTokenMentionState(text: string, cursor: number, marker: "@" | "#"): MentionState | null {
  const beforeCursor = text.slice(0, cursor);
  const match = beforeCursor.match(new RegExp(`\\${marker}([A-Za-z0-9_-]*)$`));
  if (!match || match.index === undefined) return null;
  if (!isMentionBoundary(beforeCursor, match.index)) return null;
  const query = match[1] ?? "";
  return {
    query,
    start: match.index,
    end: cursor,
  };
}

export function getMentionState(text: string, cursor: number): MentionState | null {
  return getTokenMentionState(text, cursor, "@");
}

export function getChannelMentionState(text: string, cursor: number): MentionState | null {
  return getTokenMentionState(text, cursor, "#");
}

export function insertAgentMention(text: string, state: MentionState, handle: string) {
  const insertion = `@${handle} `;
  const nextText = `${text.slice(0, state.start)}${insertion}${text.slice(state.end)}`;
  const nextCursor = state.start + insertion.length;
  return { nextText, nextCursor };
}

export function insertChannelMention(text: string, state: MentionState, name: string) {
  const insertion = `#${name} `;
  const nextText = `${text.slice(0, state.start)}${insertion}${text.slice(state.end)}`;
  const nextCursor = state.start + insertion.length;
  return { nextText, nextCursor };
}

export function filterMentionAgents(agents: Agent[], query: string) {
  const lowered = query.toLowerCase();
  return agents
    .filter((agent) => {
      const haystack =
        `${agent.handle} ${agent.display_name} ${agent.role} ${agent.description} ${agent.runtime} ${agent.model}`.toLowerCase();
      return haystack.includes(lowered);
    })
    .slice(0, 6);
}

export function filterMentionChannels(channels: Channel[], query: string) {
  const lowered = query.toLowerCase();
  return channels
    .filter((channel) => {
      if (channel.kind === "dm") return false;
      const haystack = `${channel.name} ${channel.description}`.toLowerCase();
      return haystack.includes(lowered);
    })
    .sort((left, right) => {
      const leftName = left.name.toLowerCase();
      const rightName = right.name.toLowerCase();
      const leftStarts = leftName.startsWith(lowered);
      const rightStarts = rightName.startsWith(lowered);
      if (leftStarts !== rightStarts) return leftStarts ? -1 : 1;
      return left.name.localeCompare(right.name);
    })
    .slice(0, 6);
}

export function mentionableAgentsForChannel(channel: Channel | null, agents: Agent[], channelAgents: Agent[]) {
  if (!channel) return [];
  if (channel.kind === "dm") {
    const dmAgent = channel.dm_agent_id ? agents.find((agent) => agent.id === channel.dm_agent_id) ?? null : null;
    return dmAgent ? [dmAgent] : channelAgents;
  }
  return channelAgents;
}

export function mentionedAgentsForBody(body: string, agents: Agent[]) {
  return agents.filter((agent) => {
    const pattern = new RegExp(`(^|[^A-Za-z0-9_-])@${escapeRegExp(agent.handle)}(?=$|\\s|[.,;:!?])`);
    return pattern.test(body);
  });
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
