import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type BlockDevice = {
  bsd_name: string;
  media_name: string | null;
  is_whole: boolean;
  removable: boolean;
  size_bytes: number;
  vendor: string | null;
  product: string | null;
  revision: string | null;
  scsi_pdt: number | null;
  optical: boolean;
  interconnect: string | null;
  mount_point: string | null;
  registry_path: string;
};

export type KeyDbStatus = {
  path: string;
  exists: boolean;
  entries: number;
  bytes: number;
  modified: number | null;
};

export type AppConfig = {
  version: number;
  output_dir: string;
  auto_update_keys: boolean;
  auto_check_updates: boolean;
  last_update_check: number | null;
};

export type Title = {
  index: number;
  playlist: number;
  duration_secs: number;
  chapters: number;
  is_main: boolean;
  resolution: string | null;
  dynamic_range: string | null;
  audio_streams: number;
};

export type DiscInfo = {
  name: string;
  path: string;
  aacs: boolean;
  aacs_ready: boolean;
  aacs_error: number;
  titles: Title[];
  main_title: number | null;
  library: string;
};

export type Snapshot = {
  version: string;
  devices: BlockDevice[];
  optical: BlockDevice[];
  keys: KeyDbStatus | null;
};

export type RipProgress = {
  out_time_ms: number;
  total_ms: number;
  frames: number;
  fps: number;
  speed: string;
  done: boolean;
  percent: number;
  message: string;
};

export type EncodeSettings = {
  codec: "x264" | "x265" | "hevc_videotoolbox" | "h264_videotoolbox";
  quality: number;
  preset: string | null;
  ten_bit: boolean;
  aac_audio: boolean;
};

export type RipMode =
  | { kind: "remux"; container: "mkv" | "mp4" }
  | { kind: "encode"; container: "mkv" | "mp4"; settings: EncodeSettings };

export type RipRequest = {
  source: string;
  keydb: string | null;
  title: number;
  output: string;
  mode: RipMode;
};

export type UpdateInfo = { version: string | null; notes: string | null };

export const api = {
  snapshot: () => invoke<Snapshot>("snapshot"),
  keysStatus: () => invoke<KeyDbStatus>("keys_status"),
  keysUpdate: () => invoke<KeyDbStatus>("keys_update"),
  config: () => invoke<AppConfig>("config_get"),
  setConfig: (config: AppConfig) => invoke<void>("config_set", { config }),
  openDisc: (path: string) => invoke<DiscInfo>("open_disc", { path }),
  startRip: (request: RipRequest) => invoke<void>("start_rip", { request }),
  cancelRip: () => invoke<void>("cancel_rip"),
  onProgress: (cb: (p: RipProgress) => void): Promise<UnlistenFn> =>
    listen<RipProgress>("rip://progress", (e) => cb(e.payload)),
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  reveal: (path: string) => invoke<void>("reveal", { path }),
};
