import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import {
  AtSign,
  Bot,
  CheckCircle2,
  ChevronDown,
  Circle,
  Hash,
  LayoutList,
  MessageSquare,
  Plus,
  Search,
  Send,
  Settings,
  Sparkles,
  Square,
  Users,
  X,
} from "lucide-react";
import "./styles.css";

type Agent = {
  id: string;
  handle: string;
  display_name: string;
  role: string;
  status: string;
  runtime: string;
  model: string;
  avatar: string;
  description: string;
};

type Channel = {
  id: string;
  name: string;
  description: string;
  unread_count: number;
};

type Message = {
  id: string;
  channel_id: string;
  thread_root_id: string | null;
  sender_name: string;
  sender_role: string;
  body: string;
  is_task: boolean;
  task_number: number | null;
  task_status: string | null;
  created_at: string;
};

type Task = {
  id: string;
  number: number;
  title: string;
  status: string;
  channel_name: string;
  assignee_name: string | null;
};

type Bootstrap = {
  db_url: string;
  channels: Channel[];
  agents: Agent[];
  messages: Message[];
  tasks: Task[];
};

function formatTime(value: string) {
  return new Intl.DateTimeFormat("en", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function firstLines(text: string, lines = 8) {
  const split = text.trim().split("\n");
  return split.slice(0, lines).join("\n") + (split.length > lines ? "\n..." : "");
}

function App() {
  const [data, setData] = useState<Bootstrap | null>(null);
  const [activeChannelId, setActiveChannelId] = useState<string>("");
  const [activeThreadId, setActiveThreadId] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"chat" | "tasks">("chat");
  const [draft, setDraft] = useState("");
  const [newChannel, setNewChannel] = useState("");
  const [newAgentHandle, setNewAgentHandle] = useState("");
  const [newAgentModel, setNewAgentModel] = useState("gpt-5.5");
  const [newAgentRuntime, setNewAgentRuntime] = useState("codex");

  async function refresh() {
    const next = await invoke<Bootstrap>("bootstrap");
    setData(next);
    setActiveChannelId((prev) => {
      if (next.channels.some((item) => item.id === prev)) return prev;
      return next.channels[0]?.id || "";
    });
    setActiveThreadId((prev) => {
      if (next.messages.some((item) => item.id === prev)) return prev;
      return next.messages.find((m) => !m.thread_root_id)?.id || null;
    });
  }

  useEffect(() => {
    refresh().catch((err) => console.error(err));
  }, []);

  const channel = data?.channels.find((c) => c.id === activeChannelId) ?? data?.channels[0];
  const rootMessages = useMemo(() => {
    if (!data || !channel) return [];
    return data.messages.filter((m) => m.channel_id === channel.id && !m.thread_root_id);
  }, [data, channel]);
  const activeRoot = rootMessages.find((m) => m.id === activeThreadId) ?? rootMessages[0] ?? null;
  const replies = useMemo(() => {
    if (!data || !activeRoot) return [];
    return data.messages.filter((m) => m.thread_root_id === activeRoot.id);
  }, [data, activeRoot]);

  async function createChannel() {
    const name = newChannel.trim().replace(/^#/, "");
    if (!name) return;
    await invoke("create_channel", { name });
    setNewChannel("");
    await refresh();
  }

  async function createAgent() {
    const handle = newAgentHandle.trim().replace(/^@/, "");
    if (!handle) return;
    await invoke("create_agent", {
      handle,
      displayName: handle,
      runtime: newAgentRuntime,
      model: newAgentModel,
    });
    setNewAgentHandle("");
    await refresh();
  }

  async function sendMessage(asTask = false) {
    if (!channel || !draft.trim()) return;
    await invoke("send_message", {
      channelId: channel.id,
      threadRootId: activeThreadId,
      body: draft.trim(),
      asTask,
    });
    setDraft("");
    await refresh();
  }

  if (!data) {
    return <div className="boot">Opening LocalSlock...</div>;
  }

  return (
    <main className="app theme-liquid">
      <aside className="sidebar">
        <section className="workspace">
          <button className="workspace-switch">
            King's Landing <ChevronDown size={16} />
          </button>
        </section>

        <nav className="rail">
          <button className="rail-item active"><MessageSquare size={18} /></button>
          <button className="rail-item"><Users size={18} /></button>
        </nav>

        <section className="quick-actions">
          <button><Search size={18} /> Search <span>⌘K</span></button>
          <button><MessageSquare size={18} /> Threads <strong>43</strong></button>
          <button><LayoutList size={18} /> Tasks</button>
          <button><Sparkles size={18} /> Saved</button>
        </section>

        <section className="channel-block">
          <div className="section-title">
            <span><ChevronDown size={14} /> Channels {data.channels.length}</span>
            <button onClick={createChannel}><Plus size={18} /></button>
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
              onClick={() => {
                setActiveChannelId(item.id);
                const first = data.messages.find((m) => m.channel_id === item.id && !m.thread_root_id);
                setActiveThreadId(first?.id ?? null);
              }}
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
          <div className="section-title"><span><ChevronDown size={14} /> Agents {data.agents.length}</span></div>
          <div className="agent-form">
            <input
              value={newAgentHandle}
              onChange={(event) => setNewAgentHandle(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") createAgent();
              }}
              placeholder="@agent"
            />
            <select value={newAgentRuntime} onChange={(event) => setNewAgentRuntime(event.target.value)}>
              <option value="codex">Codex</option>
              <option value="claude">Claude</option>
              <option value="kimi">Kimi</option>
            </select>
            <input
              value={newAgentModel}
              onChange={(event) => setNewAgentModel(event.target.value)}
              placeholder="model"
            />
            <button onClick={createAgent}><Plus size={16} /> Add agent</button>
          </div>
          {data.agents.map((agent) => (
            <div className="agent" key={agent.id}>
              <div className="avatar">{agent.avatar || "A"}</div>
              <div>
                <strong>{agent.display_name}</strong>
                <span>{agent.description || agent.runtime}</span>
              </div>
              <Circle className={`dot ${agent.status}`} size={10} />
            </div>
          ))}
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

      <section className="conversation">
        <header className="topbar">
          <div className="channel-title">
            <span className="hash-card"><Hash /></span>
            <div>
              <h1>{channel?.name || "No channel"}</h1>
              <p>{channel?.description || "Create a channel from the sidebar"}</p>
            </div>
          </div>
          <div className="top-actions">
            <button className="style-pill"><Sparkles size={16} /> Liquid Glass</button>
            <button><Square size={16} /></button>
            <button><Settings size={16} /></button>
            <button><Users size={16} /> {data.agents.length + 1}</button>
          </div>
        </header>

        <div className="tabs">
          <button className={activeTab === "chat" ? "active" : ""} onClick={() => setActiveTab("chat")}>
            <MessageSquare size={16} /> Chat
          </button>
          <button className={activeTab === "tasks" ? "active" : ""} onClick={() => setActiveTab("tasks")}>
            <LayoutList size={16} /> Tasks
          </button>
        </div>

        {activeTab === "chat" ? (
          <div className="message-list">
            {channel ? (
              <div className="beginning">Beginning of messages</div>
            ) : (
              <div className="empty-state">
                <Hash size={34} />
                <h2>No channels yet</h2>
                <p>Create a channel in the left sidebar, then send messages or tasks.</p>
              </div>
            )}
            {rootMessages.map((message) => (
              <article
                key={message.id}
                className={`message-card ${message.id === activeRoot?.id ? "focused" : ""}`}
                onClick={() => setActiveThreadId(message.id)}
              >
                <div className="avatar">{message.sender_name.slice(0, 1)}</div>
                <div className="message-body">
                  <div className="meta">
                    <strong>{message.sender_name}</strong>
                    <span>{message.sender_role}</span>
                    <time>{formatTime(message.created_at)}</time>
                    {message.task_number && <mark><CheckCircle2 size={14} /> #{message.task_number}</mark>}
                  </div>
                  <p>{firstLines(message.body)}</p>
                  <button className="reply-pill"><MessageSquare size={15} /> Open thread</button>
                </div>
              </article>
            ))}
          </div>
        ) : (
          <div className="task-board">
            {data.tasks.length === 0 && (
              <div className="empty-state">
                <LayoutList size={34} />
                <h2>No tasks yet</h2>
                <p>Use “Send Task” in a channel to create the first local task.</p>
              </div>
            )}
            {data.tasks.map((task) => (
              <article className="task-card" key={task.id}>
                <span>#{task.number}</span>
                <h3>{task.title}</h3>
                <p>{task.channel_name} · {task.assignee_name || "unassigned"}</p>
                <strong>{task.status}</strong>
              </article>
            ))}
          </div>
        )}

        <footer className="composer">
          <textarea
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            disabled={!channel}
            placeholder={channel ? `Message #${channel.name}` : "Create a channel before messaging"}
          />
          <div className="composer-actions">
            <button className="icon"><AtSign size={18} /></button>
            <label><input type="checkbox" /> As Task</label>
            <button className="send" disabled={!channel} onClick={() => sendMessage(false)}>Send <Send size={15} /></button>
            <button className="task-send" disabled={!channel} onClick={() => sendMessage(true)}>Send Task</button>
          </div>
        </footer>
      </section>

      <aside className="thread">
        <header>
          <div>
            <h2>Thread <span>{channel ? `- #${channel.name}` : "- no channel"}</span></h2>
            <p>{activeRoot ? `Root ${activeRoot.id.slice(0, 8)}` : "No thread selected"}</p>
          </div>
          <button><X size={18} /></button>
        </header>

        <section className="theme-candidates">
          <h3>Style Direction</h3>
          <button className="active">
            <span>Liquid Glass</span>
            <small>Single macOS-style direction for the local desktop app.</small>
          </button>
        </section>

        {activeRoot && (
          <article className="thread-root">
            <div className="meta">
              <strong>{activeRoot.sender_name}</strong>
              <time>{formatTime(activeRoot.created_at)}</time>
            </div>
            <p>{activeRoot.body}</p>
          </article>
        )}

        <section className="reply-list">
          {!activeRoot && (
            <div className="empty-state compact">
              <MessageSquare size={28} />
              <h2>No thread selected</h2>
              <p>Select a root message after you create one.</p>
            </div>
          )}
          {replies.map((reply) => (
            <article key={reply.id}>
              <div className="avatar tiny">{reply.sender_name.slice(0, 1)}</div>
              <div>
                <div className="meta">
                  <strong>{reply.sender_name}</strong>
                  <time>{formatTime(reply.created_at)}</time>
                </div>
                <p>{reply.body}</p>
              </div>
            </article>
          ))}
        </section>

        <section className="db-card">
          <Bot size={18} />
          <div>
            <strong>Postgres State</strong>
            <span>{data.db_url.replace(/:[^:@/]+@/, ":***@")}</span>
          </div>
        </section>
      </aside>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
