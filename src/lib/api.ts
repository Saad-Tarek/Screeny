import { invoke } from "@tauri-apps/api/core";

export interface CaptureConfig {
  interval_seconds: number;
  format: "png" | "jpeg";
  jpeg_quality: number;
  retention_days: number | null;
  start_on_launch: boolean;
}

export type SmtpSecurity = "ssl" | "starttls";

export interface EmailConfig {
  enabled: boolean;
  smtp_host: string;
  smtp_port: number;
  security: SmtpSecurity;
  username: string;
  from: string;
  to: string;
  batch_size: number;
  content: ContentMode;
}

export interface TelegramConfig {
  enabled: boolean;
  chat_id: string;
  content: ContentMode;
}

export interface ChannelsConfig {
  email: EmailConfig;
  telegram: TelegramConfig;
}

export type ContentMode = "image" | "analysis" | "both";
export type LlmBackendKind = "ollama" | "lmstudio" | "custom";

export interface LlmConfig {
  enabled: boolean;
  backend: LlmBackendKind;
  base_url: string;
  model: string;
  prompt_override: string | null;
}

export interface Config {
  version: number;
  capture: CaptureConfig;
  channels: ChannelsConfig;
  llm: LlmConfig;
  onboarding_complete: boolean;
}

export interface CaptureRow {
  id: number;
  taken_at: string;
  path: string;
  monitor: string;
  width: number;
  height: number;
  status: string;
  description: string | null;
  delivery_summary: string | null;
}

export type RunState = "running" | "paused";

export interface Status {
  state: RunState;
  interval_seconds: number;
  total_captures: number;
}

export type SinkKind = "email" | "telegram";

export interface DiscoveredChat {
  id: number;
  label: string;
}

export type CoreEvent =
  | { type: "capture_taken"; data: CaptureRow }
  | { type: "capture_failed"; data: { message: string } }
  | { type: "state_changed"; data: { state: RunState } }
  | { type: "config_changed"; data: Config }
  | {
      type: "delivery_succeeded";
      data: { sink: SinkKind; count: number; capture_ids: number[] };
    }
  | {
      type: "delivery_failed";
      data: { sink: SinkKind; message: string; capture_ids: number[] };
    }
  | { type: "analysis_completed"; data: { capture_id: number; description: string } }
  | { type: "analysis_failed"; data: { capture_id: number; message: string } }
  | { type: "analysis_skipped"; data: { capture_id: number } };

export interface DetectResult {
  ollama: string[] | null;
  lmstudio: string[] | null;
}

export interface PullProgressEvent {
  model: string;
  status: string;
  total: number | null;
  completed: number | null;
}

export const api = {
  getConfig: () => invoke<Config>("get_config"),
  setConfig: (config: Config) => invoke<Config>("set_config", { config }),
  getStatus: () => invoke<Status>("get_status"),
  setRunState: (running: boolean) => invoke<RunState>("set_run_state", { running }),
  captureNow: () => invoke<void>("capture_now"),
  listCaptures: (limit?: number, beforeId?: number) =>
    invoke<CaptureRow[]>("list_captures", { limit, beforeId }),
  setEmailPassword: (password: string) =>
    invoke<void>("set_email_password", { password }),
  emailPasswordSet: () => invoke<boolean>("email_password_set"),
  testEmail: (config: Config) => invoke<void>("test_email", { config }),
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<void>("set_autostart", { enabled }),
  detectBackends: () => invoke<DetectResult>("detect_backends"),
  listModels: (config: Config) => invoke<string[]>("list_models", { config }),
  pullModel: (model: string) => invoke<void>("pull_model", { model }),
  searchCaptures: (query: string, limit?: number) =>
    invoke<CaptureRow[]>("search_captures", { query, limit }),
  setLlmApiKey: (key: string) => invoke<void>("set_llm_api_key", { key }),
  llmApiKeySet: () => invoke<boolean>("llm_api_key_set"),
  setTelegramToken: (token: string) => invoke<void>("set_telegram_token", { token }),
  telegramTokenSet: () => invoke<boolean>("telegram_token_set"),
  testTelegram: (config: Config) => invoke<void>("test_telegram", { config }),
  telegramDiscoverChats: () => invoke<DiscoveredChat[]>("telegram_discover_chats"),
};

/** Model suggestions for the onboarding wizard and settings (Ollama tags). */
export const RECOMMENDED_MODELS = [
  {
    tag: "moondream",
    label: "Moondream 2 (~1.7 GB)",
    blurb: "Smallest and fastest. Good descriptions, basic OCR. Fine for 8 GB RAM machines.",
  },
  {
    tag: "qwen2.5vl:3b",
    label: "Qwen 2.5 VL 3B (~3.2 GB)",
    blurb: "Balanced quality and speed with better OCR than Moondream.",
  },
  {
    tag: "qwen2.5vl:7b",
    label: "Qwen 2.5 VL 7B (~6 GB)",
    blurb: "Best OCR quality of the three. Needs a stronger machine (16 GB RAM / GPU).",
  },
];
