import { Bell, Check, Clock3, X } from "lucide-react";
import type { Channel, Reminder } from "../types";
import { formatTime } from "../ui-utils";
import { Modal } from "./Modal";

type ReminderModalProps = {
  open: boolean;
  reminders: Reminder[];
  channels: Channel[];
  currentChannel: Channel | null;
  title: string;
  note: string;
  dueAt: string;
  recurrence: string;
  includeThread: boolean;
  activeThreadId: string | null;
  onTitleChange: (value: string) => void;
  onNoteChange: (value: string) => void;
  onDueAtChange: (value: string) => void;
  onRecurrenceChange: (value: string) => void;
  onIncludeThreadChange: (value: boolean) => void;
  onCreate: () => void;
  onSnooze: (reminder: Reminder, minutes: number) => void;
  onComplete: (reminder: Reminder) => void;
  onCancelReminder: (reminder: Reminder) => void;
  onClose: () => void;
};

function channelLabel(channels: Channel[], reminder: Reminder) {
  if (reminder.channel_name) return `#${reminder.channel_name}`;
  const channel = channels.find((item) => item.id === reminder.channel_id);
  if (!channel) return "No channel";
  return channel.kind === "dm" ? "Direct message" : `#${channel.name}`;
}

function dueLabel(reminder: Reminder) {
  const due = new Date(reminder.due_at);
  if (Number.isNaN(due.getTime())) return formatTime(reminder.due_at);
  const delta = due.getTime() - Date.now();
  const abs = Math.abs(delta);
  const minutes = Math.max(1, Math.round(abs / 60_000));
  const suffix = delta < 0 ? "ago" : "from now";
  if (minutes < 60) return `${minutes}m ${suffix}`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours}h ${suffix}`;
  return due.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

export function ReminderModal({
  open,
  reminders,
  channels,
  currentChannel,
  title,
  note,
  dueAt,
  recurrence,
  includeThread,
  activeThreadId,
  onTitleChange,
  onNoteChange,
  onDueAtChange,
  onRecurrenceChange,
  onIncludeThreadChange,
  onCreate,
  onSnooze,
  onComplete,
  onCancelReminder,
  onClose,
}: ReminderModalProps) {
  const scheduled = reminders.filter((reminder) => reminder.status === "scheduled");
  const fired = reminders.filter((reminder) => reminder.status === "fired");
  const canCreate = title.trim() && dueAt.trim();

  return (
    <Modal open={open} title="Reminders" onClose={onClose} width={720}>
      <div className="reminder-modal">
        <section className="reminder-create">
          <div className="reminder-create-head">
            <Bell size={18} />
            <div>
              <strong>New reminder</strong>
              <span>{currentChannel ? `Anchored to ${currentChannel.kind === "dm" ? "this DM" : `#${currentChannel.name}`}` : "No active channel"}</span>
            </div>
          </div>
          <input
            value={title}
            onChange={(event) => onTitleChange(event.target.value)}
            placeholder="What should LocalSlock remind you about?"
          />
          <textarea
            value={note}
            onChange={(event) => onNoteChange(event.target.value)}
            placeholder="Optional note"
          />
          <div className="reminder-create-grid">
            <label>
              Due
              <input type="datetime-local" value={dueAt} onChange={(event) => onDueAtChange(event.target.value)} />
            </label>
            <label>
              Repeat
              <select value={recurrence} onChange={(event) => onRecurrenceChange(event.target.value)}>
                <option value="none">No repeat</option>
                <option value="daily">Daily</option>
                <option value="weekly">Weekly</option>
              </select>
            </label>
          </div>
          <label className={`reminder-thread-toggle ${!activeThreadId ? "disabled" : ""}`}>
            <input
              type="checkbox"
              checked={includeThread && Boolean(activeThreadId)}
              disabled={!activeThreadId}
              onChange={(event) => onIncludeThreadChange(event.target.checked)}
            />
            Anchor to current thread
          </label>
          <button className="primary" disabled={!canCreate} onClick={onCreate}>
            Create reminder
          </button>
        </section>

        <section className="reminder-list">
          {fired.length > 0 && (
            <div className="reminder-group">
              <h4>Due now</h4>
              {fired.map((reminder) => (
                <article key={reminder.id} className="reminder-row fired">
                  <Clock3 size={16} />
                  <div>
                    <strong>{reminder.title}</strong>
                    <span>{channelLabel(channels, reminder)} · {dueLabel(reminder)}</span>
                    {reminder.note && <p>{reminder.note}</p>}
                  </div>
                  <button onClick={() => onSnooze(reminder, 10)}>10m</button>
                  <button onClick={() => onComplete(reminder)}><Check size={15} /></button>
                </article>
              ))}
            </div>
          )}

          <div className="reminder-group">
            <h4>Scheduled</h4>
            {scheduled.length === 0 && <p className="empty-mini">No scheduled reminders.</p>}
            {scheduled.map((reminder) => (
              <article key={reminder.id} className="reminder-row">
                <Clock3 size={16} />
                <div>
                  <strong>{reminder.title}</strong>
                  <span>{channelLabel(channels, reminder)} · {dueLabel(reminder)}{reminder.recurrence !== "none" ? ` · ${reminder.recurrence}` : ""}</span>
                  {reminder.note && <p>{reminder.note}</p>}
                </div>
                <button onClick={() => onSnooze(reminder, 60)}>+1h</button>
                <button className="danger" onClick={() => onCancelReminder(reminder)}><X size={15} /></button>
              </article>
            ))}
          </div>
        </section>
      </div>
    </Modal>
  );
}
