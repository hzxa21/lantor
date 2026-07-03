import { useEffect, useState } from "react";
import { Image, Lock, Monitor, Moon, ShieldCheck, Sun, Type } from "lucide-react";
import type { WebAuthStatus } from "../apiClient";
import { Modal } from "./Modal";

export type ThemePreference = "auto" | "light" | "dark";
export type ChatTextSize = "compact" | "default" | "large" | "xlarge";

type SettingsModalProps = {
  open: boolean;
  themePreference: ThemePreference;
  chatTextSize: ChatTextSize;
  showImageThumbnails: boolean;
  webAuth: WebAuthStatus | null;
  webPinSaving: boolean;
  webPinError: string | null;
  onThemePreferenceChange: (value: ThemePreference) => void;
  onChatTextSizeChange: (value: ChatTextSize) => void;
  onShowImageThumbnailsChange: (value: boolean) => void;
  onWebPinSubmit: (currentPin: string, nextPin: string) => Promise<boolean>;
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

export function SettingsModal({
  open,
  themePreference,
  chatTextSize,
  showImageThumbnails,
  webAuth,
  webPinSaving,
  webPinError,
  onThemePreferenceChange,
  onChatTextSizeChange,
  onShowImageThumbnailsChange,
  onWebPinSubmit,
  onClose,
}: SettingsModalProps) {
  const [currentPin, setCurrentPin] = useState("");
  const [nextPin, setNextPin] = useState("");
  const [confirmPin, setConfirmPin] = useState("");
  const pinConfigured = Boolean(webAuth?.required);
  const pinMismatch = nextPin.length === 6 && confirmPin.length === 6 && nextPin !== confirmPin;
  const canSubmitPin = nextPin.length === 6
    && confirmPin.length === 6
    && nextPin === confirmPin
    && (!pinConfigured || currentPin.length === 6)
    && !webPinSaving;

  useEffect(() => {
    if (!open) {
      setCurrentPin("");
      setNextPin("");
      setConfirmPin("");
    }
  }, [open]);

  function normalizePin(value: string) {
    return value.replace(/\D/g, "").slice(0, 6);
  }

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
        <fieldset className="settings-fieldset settings-web-pin-fieldset">
          <legend>Web access PIN</legend>
          <div className="settings-pin-status">
            {pinConfigured ? <ShieldCheck size={17} /> : <Lock size={17} />}
            <span>
              <strong>{pinConfigured ? "PIN enabled" : "No PIN set"}</strong>
              <small>{pinConfigured ? "Browser access requires the 6-digit PIN." : "Set a 6-digit PIN before exposing the web UI."}</small>
            </span>
          </div>
          <div className="settings-pin-grid">
            {pinConfigured && (
              <label>
                <span>Current PIN</span>
                <input
                  type="password"
                  value={currentPin}
                  inputMode="numeric"
                  pattern="[0-9]*"
                  autoComplete="current-password"
                  maxLength={6}
                  onChange={(event) => setCurrentPin(normalizePin(event.currentTarget.value))}
                />
              </label>
            )}
            <label>
              <span>{pinConfigured ? "New PIN" : "PIN"}</span>
              <input
                type="password"
                value={nextPin}
                inputMode="numeric"
                pattern="[0-9]*"
                autoComplete="new-password"
                maxLength={6}
                onChange={(event) => setNextPin(normalizePin(event.currentTarget.value))}
              />
            </label>
            <label>
              <span>Confirm PIN</span>
              <input
                type="password"
                value={confirmPin}
                inputMode="numeric"
                pattern="[0-9]*"
                autoComplete="new-password"
                maxLength={6}
                onChange={(event) => setConfirmPin(normalizePin(event.currentTarget.value))}
              />
            </label>
          </div>
          {pinMismatch && <p className="settings-field-error">PIN confirmation does not match.</p>}
          {webPinError && <p className="settings-field-error">{webPinError}</p>}
          {webAuth?.locked && webAuth.unlockCommand && (
            <pre className="settings-pin-command">{webAuth.unlockCommand}</pre>
          )}
          <button
            type="button"
            className="settings-pin-submit"
            disabled={!canSubmitPin}
            onClick={async () => {
              const saved = await onWebPinSubmit(currentPin, nextPin);
              if (saved) {
                setCurrentPin("");
                setNextPin("");
                setConfirmPin("");
              }
            }}
          >
            {webPinSaving ? "Saving..." : pinConfigured ? "Change PIN" : "Set PIN"}
          </button>
        </fieldset>
      </section>
    </Modal>
  );
}
