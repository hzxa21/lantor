import { Agent } from "./types";

export type MentionState = {
  query: string;
  start: number;
  end: number;
};

export function getMentionState(text: string, cursor: number): MentionState | null {
  const beforeCursor = text.slice(0, cursor);
  const match = beforeCursor.match(/(^|\s)@([A-Za-z0-9_-]*)$/);
  if (!match) return null;
  const query = match[2] ?? "";
  return {
    query,
    start: cursor - query.length - 1,
    end: cursor,
  };
}

export function insertAgentMention(text: string, state: MentionState, handle: string) {
  const insertion = `@${handle} `;
  const nextText = `${text.slice(0, state.start)}${insertion}${text.slice(state.end)}`;
  const nextCursor = state.start + insertion.length;
  return { nextText, nextCursor };
}

export function filterMentionAgents(agents: Agent[], query: string) {
  const lowered = query.toLowerCase();
  return agents
    .filter((agent) => {
      const haystack = `${agent.handle} ${agent.display_name} ${agent.runtime} ${agent.model}`.toLowerCase();
      return haystack.includes(lowered);
    })
    .slice(0, 6);
}

export function mentionedAgentsForBody(body: string, agents: Agent[]) {
  return agents.filter((agent) => {
    const pattern = new RegExp(`(^|\\s)@${escapeRegExp(agent.handle)}(?=$|\\s|[.,;:!?])`);
    return pattern.test(body);
  });
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
