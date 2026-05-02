import { useMemo, useState, type KeyboardEvent, type RefObject } from "react";
import {
  filterMentionAgents,
  getMentionState,
  insertAgentMention,
  type MentionState,
} from "../mentions";
import { Agent } from "../types";

type UseMentionPickerArgs = {
  agents: Agent[];
  value: string;
  setValue: (value: string) => void;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
};

export function useMentionPicker({ agents, value, setValue, textareaRef }: UseMentionPickerArgs) {
  const [mentionState, setMentionState] = useState<MentionState | null>(null);
  const [mentionIndex, setMentionIndex] = useState(0);
  const mentionCandidates = useMemo(() => {
    return mentionState ? filterMentionAgents(agents, mentionState.query) : [];
  }, [agents, mentionState]);

  function focusComposer(cursor?: number) {
    window.requestAnimationFrame(() => {
      textareaRef.current?.focus();
      if (cursor !== undefined) {
        textareaRef.current?.setSelectionRange(cursor, cursor);
      }
    });
  }

  function refreshMentionState(text: string, cursor: number) {
    setMentionState(getMentionState(text, cursor));
    setMentionIndex(0);
  }

  function closeMentionPicker() {
    setMentionState(null);
  }

  function chooseMention(agent: Agent) {
    if (!mentionState) return;
    const { nextText, nextCursor } = insertAgentMention(value, mentionState, agent.handle);
    setValue(nextText);
    closeMentionPicker();
    focusComposer(nextCursor);
  }

  function handleMentionKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (!mentionState || mentionCandidates.length === 0) return false;

    if (event.key === "ArrowDown") {
      event.preventDefault();
      setMentionIndex((current) => (current + 1) % mentionCandidates.length);
      return true;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setMentionIndex((current) => (current - 1 + mentionCandidates.length) % mentionCandidates.length);
      return true;
    }
    if (event.key === "Enter" || event.key === "Tab") {
      event.preventDefault();
      chooseMention(mentionCandidates[mentionIndex] ?? mentionCandidates[0]);
      return true;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      closeMentionPicker();
      return true;
    }

    return false;
  }

  return {
    mentionState,
    mentionIndex,
    mentionCandidates,
    refreshMentionState,
    chooseMention,
    handleMentionKeyDown,
    closeMentionPicker,
    focusComposer,
  };
}
