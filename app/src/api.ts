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
  device_path: string | null;
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
  output_template: string;
  tv_template: string;
  tv_subs: string;
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
  video_codec: string | null;
  audio_codec: string | null;
  audio_lang: string | null;
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
  dest: string | null;
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
  title: number | null;
  /** null = render the configured output template */
  output: string | null;
  mode: RipMode;
  /** Chapter names + marks (ChapterDB) */
  chapters: boolean;
  /** Fetch and attach a TMDB poster */
  poster: boolean;
  /** Movie title used for the metadata lookups */
  movie: string | null;
  audio: AudioSel;
  subs: SubSel;
  /** Download cover art next to the output */
  covers: boolean;
  /** Rip as TV: one file per episode. */
  tv: boolean;
  series: string | null;
  episodes: string | null;
};

export type CoverHit = {
  source: string;
  id: string;
  title: string;
  kind: string;
  format: string;
  preview_url: string;
  full_url: string;
  needs_auth: boolean;
  note: string;
};

export type AudioSel = "all" | "main" | "english";
export type SubSel = "all" | "english" | "none";

export type PreviewVars = {
  output_dir: string;
  disc: string;
  title: string;
  year: string | null;
  tmdb: number | null;
  resolution: string | null;
  hdr: string | null;
  video_codec: string | null;
  audio_lang: string | null;
  audio_codec: string | null;
  runtime_secs: number;
  chapters: number;
  playlist: number;
  index: number;
  ext: string;
};

export type MovieHit = { title: string; year: string | null; tmdb_id: number };

export type UpdateInfo = { version: string | null; notes: string | null };

export type IsoRequest = {
  source: string;
  keydb: string | null;
  dest: string;
  format: "iso" | "folder";
  label: string | null;
};

export type IsoProgress = {
  stage: string;
  percent: number;
  files: number;
  message: string;
};

export const api = {
  snapshot: () => invoke<Snapshot>("snapshot"),
  keysStatus: () => invoke<KeyDbStatus>("keys_status"),
  keysUpdate: () => invoke<KeyDbStatus>("keys_update"),
  config: () => invoke<AppConfig>("config_get"),
  setConfig: (config: AppConfig) => invoke<void>("config_set", { config }),
  openDisc: (path: string) => invoke<DiscInfo>("open_disc", { path }),
  startRip: (request: RipRequest) => invoke<void>("start_rip", { request }),
  cancelRip: () => invoke<void>("cancel_rip"),
  exportIso: (request: IsoRequest) => invoke<void>("export_iso", { request }),
  onIsoProgress: (cb: (p: IsoProgress) => void): Promise<UnlistenFn> =>
    listen<IsoProgress>("iso://progress", (e) => cb(e.payload)),
  onIsoDone: (cb: () => void): Promise<UnlistenFn> =>
    listen("iso://done", () => cb()),
  onIsoError: (cb: (e: string) => void): Promise<UnlistenFn> =>
    listen<string>("iso://error", (e) => cb(e.payload)),
  onProgress: (cb: (p: RipProgress) => void): Promise<UnlistenFn> =>
    listen<RipProgress>("rip://progress", (e) => cb(e.payload)),
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  eject: () => invoke<string>("eject"),
  tvPlan: (source: string) =>
    invoke<{
      series: string;
      season: number;
      disc: number | null;
      tmdb_id: number | null;
      poster: string | null;
      items: {
        title_index: number;
        start_secs: number;
        duration_secs: number;
        episode: { season: number; number: number; title: string; still: string | null };
      }[];
    }>("tv_plan", { source }),
  reveal: (path: string) => invoke<void>("reveal", { path }),
  previewOutput: (template: string, vars: PreviewVars) =>
    invoke<string>("preview_output", { template, vars }),
  templatePlaceholders: () => invoke<[string, string][]>("template_placeholders"),
  coversSearch: (query: string, source: string, limit: number) =>
    invoke<CoverHit[]>("covers_search", { query, source, limit }),
  coverThumb: (url: string) => invoke<string | null>("cover_thumb", { url }),
  saveCovers: (
    destDir: string,
    stem: string,
    items: { role: string; source: string; id: string; title: string }[],
  ) =>
    invoke<
      { role: string; path: string; original: boolean; watermarked: boolean; width: number; height: number }[]
    >("save_covers", { destDir, stem, items }),
  tmdbLookup: (movie: string, runtimeSecs: number) =>
    invoke<MovieHit | null>("tmdb_lookup", { movie, runtimeSecs }),
};
