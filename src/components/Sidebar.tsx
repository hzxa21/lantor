import {
  ChevronDown,
  Circle,
  Hash,
  LayoutList,
  MessageSquare,
  Plus,
  Search,
  Settings,
  Sparkles,
  Square,
} from "lucide-react";
import {
  Agent,
  AgentRun,
  Bootstrap,
  Channel,
  Message,
  SearchResult,
} from "../types";

type SidebarProps = {
  data: Bootstrap;
  channel: Channel | null;
  rootMessages: Message[];
  followedThreads: number;
  searchQuery: string;
  searchResults: SearchResult[];
  setSearchQuery: (value: string) => void;
  openSearchResult: (result: SearchResult) => void;
  openCreateChannelModal: () => void;
  openChannelSettingsModal: () => void;
  selectChannel: (channelId: string) => void;
  openCreateAgentModal: () => void;
  openAgentDetail: (agent: Agent) => void;
  activeRunFor: (agentId: string) => AgentRun | null;
  startAgent: (agent: Agent) => void;
  stopAgent: (run: AgentRun) => void;
};

export function Sidebar({
  data,
  channel,
  rootMessages,
  followedThreads,
  searchQuery,
  searchResults,
  setSearchQuery,
  openSearchResult,
  openCreateChannelModal,
  openChannelSettingsModal,
  selectChannel,
  openCreateAgentModal,
  openAgentDetail,
  activeRunFor,
  startAgent,
  stopAgent,
}: SidebarProps) {
  return (
    <aside className="sidebar">
      <section className="workspace">
        <div className="workspace-switch">LocalSlock</div>
      </section>

      <section className="quick-actions">
        <label className="search-box">
          <Search size={18} />
          <input
            value={searchQuery}
            onChange={(event) => setSearchQuery(event.target.value)}
            placeholder="Search messages, agents, tasks…"
          />
        </label>
        <div className="stat-row">
          <span><MessageSquare size={14} /> {followedThreads}/{rootMessages.length} threads</span>
          <span><LayoutList size={14} /> {data.tasks.length} tasks</span>
          <span><Sparkles size={14} /> {data.agents.length} agents</span>
        </div>
        {searchQuery.trim() && (
          <div className="search-results">
            {searchResults.length === 0 && <span>No local results</span>}
            {searchResults.map((result) => (
              <button key={`${result.kind}-${result.id}`} onClick={() => openSearchResult(result)}>
                <strong>{result.kind}</strong>
                <span>{result.title}</span>
                <small>{result.detail}</small>
              </button>
            ))}
          </div>
        )}
      </section>

      <section className="channel-block">
        <div className="section-title">
          <span><ChevronDown size={14} /> Channels {data.channels.length}</span>
          <div className="section-actions">
            {channel && <button onClick={openChannelSettingsModal} title="Channel settings"><Settings size={16} /></button>}
            <button onClick={openCreateChannelModal} title="Create channel"><Plus size={18} /></button>
          </div>
        </div>
        {data.channels.map((item) => (
          <button
            key={item.id}
            className={`channel ${item.id === channel?.id ? "selected" : ""}`}
            onClick={() => selectChannel(item.id)}
          >
            <Hash size={17} /> {item.name}
            {item.unread_count > 0 && <strong>{item.unread_count}</strong>}
          </button>
        ))}
        {data.channels.length === 0 && (
          <div className="empty-mini">Create a channel to start chatting.</div>
        )}
      </section>

      <section className="agent-list">
        <div className="section-title">
          <span><ChevronDown size={14} /> Agents {data.agents.length}</span>
          <button onClick={openCreateAgentModal} title="Add agent"><Plus size={18} /></button>
        </div>
        {data.agents.map((agent) => {
          const run = activeRunFor(agent.id);
          return (
            <div className="agent-card" key={agent.id}>
              <button className="agent" onClick={() => openAgentDetail(agent)}>
                <div className="avatar">{agent.avatar || "A"}</div>
                <div>
                  <strong>{agent.display_name}</strong>
                  <span>@{agent.handle} · {agent.runtime} · {agent.status}</span>
                </div>
                <Circle className={`dot ${agent.status}`} size={10} />
              </button>
              <div className="agent-runtime-actions">
                {run ? (
                  <button className="runtime-stop" onClick={() => stopAgent(run)}>
                    <Square size={14} /> Stop
                  </button>
                ) : (
                  <button className="runtime-start" onClick={() => startAgent(agent)}>
                    <Sparkles size={14} /> Start
                  </button>
                )}
              </div>
            </div>
          );
        })}
        {data.agents.length === 0 && (
          <div className="empty-mini">Add a local agent profile from the plus button.</div>
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
