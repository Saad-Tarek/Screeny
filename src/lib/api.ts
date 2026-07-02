import { invoke } from "@tauri-apps/api/core";

export interface CaptureConfig {
  interval_seconds: number;
  format: "png" | "jpeg";
  jpeg_quality: number;
  retention_days: number | null;
  start_on_launch: boolean;
}

export interface Config {
  version: number;
  capture: CaptureConfig;
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

export type CoreEvent =
  | { type: "capture_taken"; data: CaptureRow }
  | { type: "capture_failed"; data: { message: string } }
  | { type: "state_changed"; data: { state: RunState } }
  | { type: "config_changed"; data: Config };

export const api = {
  getConfig: () => invoke<Config>("get_config"),
  setConfig: (config: Config) => invoke<Config>("set_config", { config }),
  getStatus: () => invoke<Status>("get_status"),
  setRunState: (running: boolean) => invoke<RunState>("set_run_state", { running }),
  captureNow: () => invoke<void>("capture_now"),
  listCaptures: (limit?: number, beforeId?: number) =>
    invoke<CaptureRow[]>("list_captures", { limit, beforeId }),
};
