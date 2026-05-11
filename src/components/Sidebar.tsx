import {
  ChevronDown,
  Circle,
  Hash,
  Inbox,
  Bookmark,
  Plus,
  Search,
  X,
} from "lucide-react";
import { useState, type PointerEvent as ReactPointerEvent } from "react";
import {
  Agent,
  Bootstrap,
  Channel,
  SavedMessage,
} from "../types";
import { AgentAvatar } from "./AgentAvatar";
import { firstLines, formatTime } from "../ui-utils";

type SidebarProps = {
  data: Bootstrap;
  channel: Channel | null;
  channelAlertIds: Set<string>;
  inboxUnreadCount: number;
  openSearch: () => void;
  openInbox: () => void;
  openSavedMessage: (item: SavedMessage) => void;
  openCreateChannelModal: () => void;
  selectChannel: (channelId: string) => void;
  openCreateAgentModal: () => void;
  openDmWithAgent: (agent: Agent) => void;
  onMobileClose?: () => void;
  onResizeStart: (event: ReactPointerEvent<HTMLButtonElement>) => void;
};

export function Sidebar({
  data,
  channel,
  channelAlertIds,
  inboxUnreadCount,
  openSearch,
  openInbox,
  openSavedMessage,
  openCreateChannelModal,
  selectChannel,
  openCreateAgentModal,
  openDmWithAgent,
  onMobileClose,
  onResizeStart,
}: SidebarProps) {
  const [collapsedSections, setCollapsedSections] = useState({ saved: false, channels: false, dms: false });
  const normalChannels = data.channels.filter((item) => item.kind !== "dm");
  const dmChannels = data.channels.filter((item) => item.kind === "dm");
  const toggleSection = (section: "saved" | "channels" | "dms") => {
    setCollapsedSections((current) => ({ ...current, [section]: !current[section] }));
  };

  return (
    <aside className="sidebar">
      <button
        className="sidebar-resize-handle"
        aria-label="Resize sidebar"
        onPointerDown={onResizeStart}
      />
      <section className="workspace">
        <div className="workspace-switch">LocalSlock</div>
        <button
          type="button"
          className="mobile-sidebar-close"
          aria-label="Close navigation"
          onClick={onMobileClose}
        >
          <X size={18} />
        </button>
      </section>

      <section className="quick-actions">
        <button className="search-trigger" onClick={openSearch}>
          <Search size={18} />
          <span>Search</span>
          <kbd>⌘K</kbd>
        </button>
        <button
          className={`sidebar-nav-trigger ${inboxUnreadCount ? "has-unread" : ""}`}
          onClick={openInbox}
        >
          <Inbox size={18} />
          <span>Inbox</span>
          {inboxUnreadCount > 0 && <strong>{inboxUnreadCount}</strong>}
        </button>
      </section>

      {data.saved_messages.length > 0 && (
        <section className={`saved-list ${collapsedSections.saved ? "collapsed" : ""}`}>
          <div className="section-title">
            <div className="section-label">
              <button
                className={`section-collapse ${collapsedSections.saved ? "collapsed" : ""}`}
                onClick={() => toggleSection("saved")}
                aria-expanded={!collapsedSections.saved}
                title={collapsedSections.saved ? "Show saved messages" : "Hide saved messages"}
              >
                <ChevronDown size={14} />
              </button>
              <span>Saved</span>
            </div>
          </div>
          {!collapsedSections.saved && data.saved_messages.slice(0, 8).map((item) => (
            <button
              key={item.id}
              type="button"
              className="saved-message"
              title={item.body}
              onClick={() => openSavedMessage(item)}
            >
              <Bookmark size={15} />
              <div>
                <strong>{firstLines(item.body, 1) || "Saved message"}</strong>
                <span>
                  #{item.channel_name} · {item.thread_root_id ? "thread" : "channel"} · {formatTime(item.message_created_at)}
                </span>
              </div>
            </button>
          ))}
        </section>
      )}

      <section className={`channel-block ${collapsedSections.channels ? "collapsed" : ""}`}>
        <div className="section-title">
          <div className="section-label">
            <button
              className={`section-collapse ${collapsedSections.channels ? "collapsed" : ""}`}
              onClick={() => toggleSection("channels")}
              aria-expanded={!collapsedSections.channels}
              title={collapsedSections.channels ? "Show channels" : "Hide channels"}
            >
              <ChevronDown size={14} />
            </button>
            <span>Channels</span>
          </div>
          <div className="section-actions">
            <button onClick={openCreateChannelModal} title="Create channel"><Plus size={18} /></button>
          </div>
        </div>
        {!collapsedSections.channels && normalChannels.map((item) => {
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
        {!collapsedSections.channels && normalChannels.length === 0 && (
          <div className="empty-mini">Create a channel to start chatting.</div>
        )}
      </section>

      <section className={`dm-list ${collapsedSections.dms ? "collapsed" : ""}`}>
        <div className="section-title">
          <div className="section-label">
            <button
              className={`section-collapse ${collapsedSections.dms ? "collapsed" : ""}`}
              onClick={() => toggleSection("dms")}
              aria-expanded={!collapsedSections.dms}
              title={collapsedSections.dms ? "Show direct messages" : "Hide direct messages"}
            >
              <ChevronDown size={14} />
            </button>
            <span>Direct Messages</span>
          </div>
          <button onClick={openCreateAgentModal} title="Add agent"><Plus size={18} /></button>
        </div>
        {!collapsedSections.dms && data.agents.map((agent) => {
          const item = dmChannels.find((candidate) => candidate.dm_agent_id === agent.id) ?? null;
          const badge = item
            ? item.unread_count > 0
              ? String(item.unread_count)
              : channelAlertIds.has(item.id)
                ? "new"
                : ""
            : "";
          return (
            <div
              key={agent.id}
              className={`dm-row ${item?.id === channel?.id ? "selected" : ""} ${badge ? "has-unread" : ""}`}
            >
              <div className="dm-avatar-shell" aria-hidden="true">
                <AgentAvatar agent={agent} size="sm" />
              </div>
              <button
                type="button"
                className="dm"
                onClick={() => item ? selectChannel(item.id) : openDmWithAgent(agent)}
              >
                <div>
                  <strong>{agent.display_name}</strong>
                  <span>@{agent.handle} · {agent.status}</span>
                </div>
                <Circle className={`dot ${agent.status}`} size={10} />
                {badge && <strong>{badge}</strong>}
              </button>
            </div>
          );
        })}
        {!collapsedSections.dms && data.agents.length === 0 && (
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
