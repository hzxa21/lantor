import { Image, Monitor, Moon, Sun, Type } from "lucide-react";
import { Modal } from "./Modal";
import { usePerfSnapshot, type PerfCause, type PerfPhaseMetrics } from "../perf";

export type ThemePreference = "auto" | "light" | "dark";
export type ChatTextSize = "compact" | "default" | "large" | "xlarge";
export type FontPreset = "space-grotesk" | "system";

type SettingsModalProps = {
  open: boolean;
  themePreference: ThemePreference;
  chatTextSize: ChatTextSize;
  fontPreset: FontPreset;
  showImageThumbnails: boolean;
  onThemePreferenceChange: (value: ThemePreference) => void;
  onChatTextSizeChange: (value: ChatTextSize) => void;
  onFontPresetChange: (value: FontPreset) => void;
  onShowImageThumbnailsChange: (value: boolean) => void;
  onClose: () => void;
};

const THEME_OPTIONS: Array<{
  value: ThemePreference;
  label: string;
  detail: string;
  icon: typeof Monitor;
}> = [
  { value: "auto", label: "Auto", detail: "Follow system", icon: Monitor },
  { value: "light", label: "Light", detail: "Bright surfaces", icon: Sun },
  { value: "dark", label: "Dark", detail: "Dim surfaces", icon: Moon },
];

const CHAT_TEXT_SIZE_OPTIONS: Array<{
  value: ChatTextSize;
  label: string;
  detail: string;
}> = [
  { value: "compact", label: "Small", detail: "Compact UI" },
  { value: "default", label: "Default", detail: "Current scale" },
  { value: "large", label: "Large", detail: "More readable" },
  { value: "xlarge", label: "Extra", detail: "Largest" },
];

const FONT_PRESET_OPTIONS: Array<{
  value: FontPreset;
  label: string;
  detail: string;
}> = [
  { value: "system", label: "System", detail: "Original · native fonts" },
  { value: "space-grotesk", label: "Space Grotesk", detail: "New · Space Mono code" },
];

const PERF_CAUSES: PerfCause[] = ["full-refresh", "message-upsert", "activity-flush"];

function formatMs(value?: number) {
  if (value === undefined) return "n/a";
  if (value >= 100) return `${Math.round(value)} ms`;
  return `${value.toFixed(1)} ms`;
}

function formatBytes(value?: number) {
  if (value === undefined) return "n/a";
  if (value >= 1024 * 1024) return `${(value / 1024 / 1024).toFixed(2)} MB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KB`;
  return `${value} B`;
}

function phaseSummary(phases: PerfPhaseMetrics) {
  return [
    `B ${formatMs(phases.backendMs)}`,
    `T ${formatMs(phases.transportMs)}`,
    `P ${formatMs(phases.parseApplyMs ?? phases.applyMs)}`,
    `R ${formatMs(phases.commitMs)}`,
  ].join(" · ");
}

export function SettingsModal({
  open,
  themePreference,
  chatTextSize,
  fontPreset,
  showImageThumbnails,
  onThemePreferenceChange,
  onChatTextSizeChange,
  onFontPresetChange,
  onShowImageThumbnailsChange,
  onClose,
}: SettingsModalProps) {
  const perf = usePerfSnapshot();
  const latestSample = perf.samples[0] ?? null;
  return (
    <Modal open={open} title="Settings" onClose={onClose} width={560}>
      <section className="settings-panel">
        <div className="settings-section-head">
          <h4>Appearance</h4>
          <p>Device-local preferences for this Lantor app.</p>
        </div>
        <fieldset className="settings-fieldset">
          <legend>Theme</legend>
          <div className="theme-choice-grid">
            {THEME_OPTIONS.map((option) => {
              const Icon = option.icon;
              return (
                <button
                  type="button"
                  key={option.value}
                  className={themePreference === option.value ? "selected" : ""}
                  aria-pressed={themePreference === option.value}
                  onClick={() => onThemePreferenceChange(option.value)}
                >
                  <Icon size={18} />
                  <span>
                    <strong>{option.label}</strong>
                    <small>{option.detail}</small>
                  </span>
                </button>
              );
            })}
          </div>
        </fieldset>
        <fieldset className="settings-fieldset">
          <legend>Text size</legend>
          <div className="chat-text-size-grid">
            {CHAT_TEXT_SIZE_OPTIONS.map((option) => (
              <button
                type="button"
                key={option.value}
                className={chatTextSize === option.value ? "selected" : ""}
                aria-pressed={chatTextSize === option.value}
                onClick={() => onChatTextSizeChange(option.value)}
              >
                <Type size={17} />
                <span>
                  <strong>{option.label}</strong>
                  <small>{option.detail}</small>
                </span>
              </button>
            ))}
          </div>
          <p className="settings-hint">Applies across messages, inputs, panels, and modals. Use Command +/- or Ctrl +/- to adjust without opening Settings. Command/Ctrl+0 resets.</p>
        </fieldset>
        <fieldset className="settings-fieldset">
          <legend>Font</legend>
          <div className="theme-choice-grid font-preset-grid">
            {FONT_PRESET_OPTIONS.map((option) => (
              <button
                type="button"
                key={option.value}
                className={fontPreset === option.value ? "selected" : ""}
                aria-pressed={fontPreset === option.value}
                onClick={() => onFontPresetChange(option.value)}
              >
                <Type size={17} />
                <span>
                  <strong>{option.label}</strong>
                  <small>{option.detail}</small>
                </span>
              </button>
            ))}
          </div>
          <p className="settings-hint">Space Grotesk is the new app typeface (with Space Mono for code). System uses your platform&rsquo;s native fonts.</p>
        </fieldset>
        <fieldset className="settings-fieldset settings-attachments-fieldset">
          <legend>Attachments</legend>
          <label className="settings-toggle-row">
            <span className="settings-toggle-copy">
              <Image size={17} />
              <span>
                <strong>Show image thumbnails</strong>
                <small>Display uploaded images inline before opening them.</small>
              </span>
            </span>
            <input
              type="checkbox"
              checked={showImageThumbnails}
              onChange={(event) => onShowImageThumbnailsChange(event.currentTarget.checked)}
            />
          </label>
          <p className="settings-hint">When disabled, images appear as compact attachment rows and still open in preview when clicked.</p>
        </fieldset>
        <fieldset className="settings-fieldset settings-perf-fieldset">
          <legend>Performance</legend>
          <div className="settings-perf-grid">
            {PERF_CAUSES.map((cause) => {
              const item = perf.summary[cause];
              return (
                <div className="settings-perf-card" key={cause}>
                  <strong>{cause}</strong>
                  <small>{item.count} samples</small>
                  <span>{phaseSummary(item.p50)}</span>
                  <span>p95 {phaseSummary(item.p95)}</span>
                </div>
              );
            })}
          </div>
          {latestSample ? (
            <div className="settings-perf-latest">
              <span>Latest</span>
              <strong>{latestSample.cause}</strong>
              <small>
                {phaseSummary(latestSample.phases)} · payload {formatBytes(latestSample.transportPayloadBytes ?? latestSample.payloadBytes)}
              </small>
            </div>
          ) : (
            <p className="settings-hint">No samples yet. Run a refresh or wait for activity updates.</p>
          )}
          <p className="settings-hint">Recent samples are also available at window.__LANTOR_PERF__. Enable production sampling with ?lantorPerf or localStorage lantor:perf=1. Web payload bytes are decoded response bytes; Tauri transport includes IPC deserialization.</p>
        </fieldset>
      </section>
    </Modal>
  );
}
