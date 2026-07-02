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
}

export interface ChannelsConfig {
  email: EmailConfig;
}

export interface Config {
  version: number;
  capture: CaptureConfig;
  channels: ChannelsConfig;
}

export interface CaptureRow {
  id: number;
  taken_at: string;
  path: string;
  monitor: string;
  width: number;
  height: number;
  status: string;
}

export type RunState = "running" | "paused";

export interface Status {
  state: RunState;
  interval_seconds: number;
  total_captures: number;
}

export type SinkKind = "email";

export type CoreEvent =
  | { type: "capture_taken"; data: CaptureRow }
  | { type: "capture_failed"; data: { message: string } }
  | { type: "state_changed"; data: { state: RunState } }
  | { type: "config_changed"; data: Config }
  | { type: "delivery_succeeded"; data: { sink: SinkKind; count: number } }
  | { type: "delivery_failed"; data: { sink: SinkKind; message: string } };

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
};
