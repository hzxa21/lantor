import {
  ChevronDown,
  Circle,
  Hash,
  MessageSquare,
  Plus,
  Search,
  Settings,
} from "lucide-react";
import { type PointerEvent as ReactPointerEvent } from "react";
import {
  Agent,
  Bootstrap,
  Channel,
} from "../types";

type SidebarProps = {
  data: Bootstrap;
  channel: Channel | null;
  channelAlertIds: Set<string>;
  threadUnreadCounts: Record<string, number>;
  openSearch: () => void;
  openThreadBrowser: () => void;
  openCreateChannelModal: () => void;
  openChannelSettingsModal: () => void;
  selectChannel: (channelId: string) => void;
  openCreateAgentModal: () => void;
  openAgentDetail: (agent: Agent) => void;
  openDmWithAgent: (agent: Agent) => void;
  onResizeStart: (event: ReactPointerEvent<HTMLButtonElement>) => void;
};

export function Sidebar({
  data,
  channel,
  channelAlertIds,
  threadUnreadCounts,
  openSearch,
  openThreadBrowser,
  openCreateChannelModal,
  openChannelSettingsModal,
  selectChannel,
  openCreateAgentModal,
  openAgentDetail,
  openDmWithAgent,
  onResizeStart,
}: SidebarProps) {
  const normalChannels = data.channels.filter((item) => item.kind !== "dm");
  const dmChannels = data.channels.filter((item) => item.kind === "dm");
  const hasThreadUnread = Object.values(threadUnreadCounts).some((count) => count > 0);

  return (
    <aside className="sidebar">
      <button
        className="sidebar-resize-handle"
        aria-label="Resize sidebar"
        onPointerDown={onResizeStart}
      />
      <section className="workspace">
        <div className="workspace-switch">LocalSlock</div>
      </section>

      <section className="quick-actions">
        <button className="search-trigger" onClick={openSearch}>
          <Search size={18} />
          <span>Search</span>
          <kbd>⌘K</kbd>
        </button>
        <button
          className={`sidebar-nav-trigger ${hasThreadUnread ? "has-unread" : ""}`}
          onClick={openThreadBrowser}
        >
          <MessageSquare size={18} />
          <span>Threads</span>
          {hasThreadUnread && <strong>new</strong>}
        </button>
      </section>

      <section className="channel-block">
        <div className="section-title">
          <span><ChevronDown size={14} /> Channels</span>
          <div className="section-actions">
            {channel?.kind !== "dm" && channel && (
              <button onClick={openChannelSettingsModal} title="Channel settings"><Settings size={16} /></button>
            )}
            <button onClick={openCreateChannelModal} title="Create channel"><Plus size={18} /></button>
          </div>
        </div>
        {normalChannels.map((item) => {
          const badge = item.unread_count > 0 ? String(item.unread_count) : channelAlertIds.has(item.id) ? "new" : "";
          return (
            <button
              key={item.id}
              className={`channel ${item.id === channel?.id ? "selected" : ""} ${badge ? "has-unread" : ""}`}
              onClick={() => selectChannel(item.id)}
            >
              <Hash size={17} /> {item.name}
              {badge && <strong>{badge}</strong>}
            </button>
          );
        })}
        {normalChannels.length === 0 && (
          <div className="empty-mini">Create a channel to start chatting.</div>
        )}
      </section>

      <section className="dm-list">
        <div className="section-title">
          <span><ChevronDown size={14} /> Direct Messages</span>
          <button onClick={openCreateAgentModal} title="Add agent"><Plus size={18} /></button>
        </div>
        {data.agents.map((agent) => {
          const item = dmChannels.find((candidate) => candidate.dm_agent_id === agent.id) ?? null;
          const badge = item
            ? item.unread_count > 0
              ? String(item.unread_count)
              : channelAlertIds.has(item.id)
                ? "new"
                : ""
            : "";
          return (
            <button
              key={agent.id}
              className={`dm ${item?.id === channel?.id ? "selected" : ""} ${badge ? "has-unread" : ""}`}
              onClick={() => item ? selectChannel(item.id) : openDmWithAgent(agent)}
            >
              <div
                className="avatar small dm-detail-trigger"
                title={`View @${agent.handle} details`}
                onClick={(event) => {
                  event.stopPropagation();
                  openAgentDetail(agent);
                }}
              >
                {agent.avatar || "A"}
              </div>
              <div>
                <strong>{agent.display_name}</strong>
                <span>@{agent.handle} · {agent.status}</span>
              </div>
              <Circle className={`dot ${agent.status}`} size={10} />
              {badge && <strong>{badge}</strong>}
            </button>
          );
        })}
        {data.agents.length === 0 && (
          <div className="empty-mini">Add an agent to start a direct message.</div>
        )}
      </section>

      <section className="profile">
        <div className="avatar human">D</div>
        <div>
          <strong>Dylan</strong>
          <span>local owner</span>
        </div>
      </section>
    </aside>
  );
}
