import {
  ChevronDown,
  Circle,
  Hash,
  LayoutList,
  MessageSquare,
  Plus,
  Save,
  Search,
  Settings,
  Sparkles,
  Square,
  Trash2,
  Users,
  X,
} from "lucide-react";
import {
  Agent,
  AgentForm,
  AgentRun,
  Bootstrap,
  Channel,
  Message,
  RUNTIME_PRESETS,
  SearchResult,
} from "../types";
import { firstLines } from "../ui-utils";

type SidebarProps = {
  data: Bootstrap;
  channel: Channel | null;
  rootMessages: Message[];
  followedThreads: number;
  searchQuery: string;
  searchResults: SearchResult[];
  newChannel: string;
  channelNameDraft: string;
  channelDescriptionDraft: string;
  channelMemberIds: Set<string>;
  agentDraft: AgentForm;
  editingAgentId: string | null;
  agentEdit: AgentForm;
  draftPresetCommand: string;
  editPresetCommand: string;
  setSearchQuery: (value: string) => void;
  openSearchResult: (result: SearchResult) => void;
  setNewChannel: (value: string) => void;
  createChannel: () => void;
  selectChannel: (channelId: string) => void;
  setChannelNameDraft: (value: string) => void;
  setChannelDescriptionDraft: (value: string) => void;
  saveChannel: () => void;
  deleteChannel: () => void;
  setChannelMember: (agentId: string, member: boolean) => void;
  setAgentDraft: (value: AgentForm) => void;
  updateDraftRuntime: (runtime: string) => void;
  applyDraftPreset: () => void;
  createAgent: () => void;
  activeRunFor: (agentId: string) => AgentRun | null;
  startAgent: (agent: Agent) => void;
  stopAgent: (run: AgentRun) => void;
  deleteAgent: (agent: Agent) => void;
  startEditAgent: (agent: Agent) => void;
  setAgentEdit: (value: AgentForm) => void;
  updateEditRuntime: (runtime: string) => void;
  applyEditPreset: () => void;
  saveAgent: () => void;
  cancelEditAgent: () => void;
};

export function Sidebar({
  data,
  channel,
  rootMessages,
  followedThreads,
  searchQuery,
  searchResults,
  newChannel,
  channelNameDraft,
  channelDescriptionDraft,
  channelMemberIds,
  agentDraft,
  editingAgentId,
  agentEdit,
  draftPresetCommand,
  editPresetCommand,
  setSearchQuery,
  openSearchResult,
  setNewChannel,
  createChannel,
  selectChannel,
  setChannelNameDraft,
  setChannelDescriptionDraft,
  saveChannel,
  deleteChannel,
  setChannelMember,
  setAgentDraft,
  updateDraftRuntime,
  applyDraftPreset,
  createAgent,
  activeRunFor,
  startAgent,
  stopAgent,
  deleteAgent,
  startEditAgent,
  setAgentEdit,
  updateEditRuntime,
  applyEditPreset,
  saveAgent,
  cancelEditAgent,
}: SidebarProps) {
  return (
    <aside className="sidebar">
      <section className="workspace">
        <button className="workspace-switch">
          LocalSlock <ChevronDown size={16} />
        </button>
      </section>

      <nav className="rail">
        <button className="rail-item active"><MessageSquare size={18} /></button>
        <button className="rail-item"><Users size={18} /></button>
      </nav>

      <section className="quick-actions">
        <label className="search-box">
          <Search size={18} />
          <input
            value={searchQuery}
            onChange={(event) => setSearchQuery(event.target.value)}
            placeholder="Search local state"
          />
        </label>
        <button><MessageSquare size={18} /> Threads <strong>{followedThreads}/{rootMessages.length}</strong></button>
        <button><LayoutList size={18} /> Tasks <strong>{data.tasks.length}</strong></button>
        <button><Sparkles size={18} /> Agents <strong>{data.agents.length}</strong></button>
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
          <button onClick={createChannel} title="Create channel"><Plus size={18} /></button>
        </div>
        <div className="new-channel">
          <input
            value={newChannel}
            onChange={(event) => setNewChannel(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") createChannel();
            }}
            placeholder="new-channel"
          />
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
        {channel && (
          <div className="management-card">
            <h4>Channel Settings</h4>
            <input
              value={channelNameDraft}
              onChange={(event) => setChannelNameDraft(event.target.value)}
              placeholder="channel-name"
            />
            <textarea
              value={channelDescriptionDraft}
              onChange={(event) => setChannelDescriptionDraft(event.target.value)}
              placeholder="Channel description"
            />
            <div className="inline-actions">
              <button onClick={saveChannel}><Save size={15} /> Save</button>
              <button className="danger" onClick={deleteChannel}><Trash2 size={15} /> Delete</button>
            </div>
            <div className="member-editor">
              <strong>Agent members</strong>
              {data.agents.length === 0 && <span>No agents yet.</span>}
              {data.agents.map((agent) => (
                <label key={agent.id}>
                  <input
                    type="checkbox"
                    checked={channelMemberIds.has(agent.id)}
                    onChange={(event) => setChannelMember(agent.id, event.target.checked)}
                  />
                  @{agent.handle}
                </label>
              ))}
            </div>
          </div>
        )}
      </section>

      <section className="agent-list">
        <div className="section-title"><span><ChevronDown size={14} /> Agents {data.agents.length}</span></div>
        <div className="agent-form">
          <input
            value={agentDraft.handle}
            onChange={(event) => setAgentDraft({ ...agentDraft, handle: event.target.value })}
            onKeyDown={(event) => {
              if (event.key === "Enter") createAgent();
            }}
            placeholder="@agent"
          />
          <input
            value={agentDraft.displayName}
            onChange={(event) => setAgentDraft({ ...agentDraft, displayName: event.target.value })}
            placeholder="display name"
          />
          <select
            value={agentDraft.runtime}
            onChange={(event) => updateDraftRuntime(event.target.value)}
          >
            <option value="codex">Codex</option>
            <option value="claude">Claude</option>
            <option value="kimi">Kimi</option>
            <option value="custom">Custom</option>
          </select>
          <input
            value={agentDraft.model}
            onChange={(event) => setAgentDraft({ ...agentDraft, model: event.target.value })}
            placeholder="model"
          />
          <div className="preset-panel">
            <div>
              <strong>{RUNTIME_PRESETS[agentDraft.runtime]?.label ?? "Custom"} preset</strong>
              <span>
                {draftPresetCommand
                  ? "Generate an editable launch command with the LocalSlock event protocol."
                  : "Custom runtime uses the command exactly as written."}
              </span>
            </div>
            {draftPresetCommand && <pre>{firstLines(draftPresetCommand, 6)}</pre>}
            <button disabled={!draftPresetCommand} onClick={applyDraftPreset}>
              <Sparkles size={14} /> Apply preset
            </button>
          </div>
          <textarea
            value={agentDraft.launchCommand}
            onChange={(event) => setAgentDraft({ ...agentDraft, launchCommand: event.target.value })}
            placeholder="launch command; empty uses a placeholder runtime"
          />
          <input
            value={agentDraft.workingDirectory}
            onChange={(event) => setAgentDraft({ ...agentDraft, workingDirectory: event.target.value })}
            placeholder="working directory"
          />
          <button onClick={createAgent}><Plus size={16} /> Add agent</button>
        </div>
        {data.agents.map((agent) => {
          const run = activeRunFor(agent.id);
          return (
            <div className="agent-card" key={agent.id}>
              <button className="agent" onClick={() => startEditAgent(agent)}>
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
                <button className="icon-danger" onClick={() => deleteAgent(agent)} title="Delete agent">
                  <Trash2 size={15} />
                </button>
              </div>
            </div>
          );
        })}
        {editingAgentId && (
          <div className="management-card">
            <h4>Edit Agent</h4>
            <input
              value={agentEdit.handle}
              onChange={(event) => setAgentEdit({ ...agentEdit, handle: event.target.value })}
              placeholder="@agent"
            />
            <input
              value={agentEdit.displayName}
              onChange={(event) => setAgentEdit({ ...agentEdit, displayName: event.target.value })}
              placeholder="display name"
            />
            <select
              value={agentEdit.runtime}
              onChange={(event) => updateEditRuntime(event.target.value)}
            >
              <option value="codex">Codex</option>
              <option value="claude">Claude</option>
              <option value="kimi">Kimi</option>
              <option value="custom">Custom</option>
            </select>
            <input
              value={agentEdit.model}
              onChange={(event) => setAgentEdit({ ...agentEdit, model: event.target.value })}
              placeholder="model"
            />
            <div className="preset-panel">
              <div>
                <strong>{RUNTIME_PRESETS[agentEdit.runtime]?.label ?? "Custom"} preset</strong>
                <span>
                  {editPresetCommand
                    ? "Regenerate the command from current handle/model/runtime."
                    : "Custom runtime uses the command exactly as written."}
                </span>
              </div>
              {editPresetCommand && <pre>{firstLines(editPresetCommand, 6)}</pre>}
              <button disabled={!editPresetCommand} onClick={applyEditPreset}>
                <Sparkles size={14} /> Apply preset
              </button>
            </div>
            <textarea
              value={agentEdit.launchCommand}
              onChange={(event) => setAgentEdit({ ...agentEdit, launchCommand: event.target.value })}
              placeholder="launch command; empty uses a placeholder runtime"
            />
            <input
              value={agentEdit.workingDirectory}
              onChange={(event) => setAgentEdit({ ...agentEdit, workingDirectory: event.target.value })}
              placeholder="working directory"
            />
            <textarea
              value={agentEdit.description}
              onChange={(event) => setAgentEdit({ ...agentEdit, description: event.target.value })}
              placeholder="Agent notes"
            />
            <div className="inline-actions">
              <button onClick={saveAgent}><Save size={15} /> Save</button>
              <button onClick={cancelEditAgent}><X size={15} /> Cancel</button>
            </div>
          </div>
        )}
        {data.agents.length === 0 && (
          <div className="empty-mini">Add a local agent profile first.</div>
        )}
      </section>

      <section className="profile">
        <div className="avatar human">D</div>
        <div>
          <strong>Dylan</strong>
          <span>local owner</span>
        </div>
        <button><Settings size={18} /></button>
      </section>
    </aside>
  );
}
