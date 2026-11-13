import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type LogEntry = {
  id: number;
  at: number;
  level: "error" | "warn" | "info" | "debug" | string;
  source: string;
  message: string;
  detail: string | null;
};

export type AboutInfo = {
  version: string;
  text: string;
};

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

export type AuthorProgress = {
  percent: number;
  message: string;
  done: boolean;
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
  auto_cover: boolean;
  hirescovers_dir: string;
  hirescovers_user?: string | null;
  hirescovers_pass?: string | null;
  hirescovers_cookie?: string | null;
  mcp_enabled: boolean;
  mcp_port: number;
  auto_rip: boolean;
  auto_rip_mode: string;
  auto_rip_min_secs: number;
  auto_rip_eject: boolean;
  watch_dir?: string | null;
  notify_native: boolean;
  notify_webhook?: string | null;
  prevent_sleep: boolean;
  cinavia: boolean;
  cinavia_codec: string | null;
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
  cinavia: boolean;
  cinavia_codec: string;
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
  /** First episode number on this disc (uneven multi-disc sets). */
  start_episode: number | null;
  cinavia?: boolean;
  cinavia_codec?: string | null;
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

export type NavKind = "none" | "hdmv" | "bdj" | "unknown";

export type NavTarget = {
  kind: NavKind;
  object?: number;
  name?: string;
};

export type NavIndex = {
  first_playback: NavTarget;
  top_menu: NavTarget;
  titles: NavTarget[];
};

export type MenuTitle = {
  index: number;
  playlist: number;
  duration_secs: number;
  chapters: number;
  name?: string;
  main: boolean;
  chapter_names?: string[];
  resolution?: string | null;
  audio_streams?: number | null;
};

export type DiscMenu = {
  version: number;
  label: string;
  disc_id?: string | null;
  source?: string | null;
  applied_from?: string | null;
  note?: string | null;
  navigation?: NavIndex | null;
  titles: MenuTitle[];
};

export type MenuEntry = {
  name: string;
  path: string;
  label: string;
  titles: number;
};

export type NativeKeyResult = {
  key: {
    id: string;
    name: string;
    vid: string;
    media_key: string;
    vuk: string;
  };
  added: boolean;
};

export type StreamInfo = {
  kind: string;
  lang: string | null;
  coding_type: number;
  format: number;
  rate: number;
  pid: number;
};

export type BurnProgress = {
  percent: number;
  message: string;
  done: boolean;
};

export type DvdTitle = {
  number: number;
  chapters: number;
  angles: number;
  title_set: number;
  vts_title: number;
  duration_secs?: number | null;
};

export type DvdInfo = {
  root: string;
  label: string;
  titles: DvdTitle[];
};

export type DvdProgress = {
  percent: number;
  out_time_ms: number;
  total_ms: number;
  message: string;
  done: boolean;
};

export type DoctorCheck = {
  name: string;
  ok: boolean;
  detail: string;
  fix?: string | null;
};

export type DoctorCapability = { name: string; ok: boolean; detail: string };

export type DoctorReport = {
  version: string;
  checks: DoctorCheck[];
  capabilities: DoctorCapability[];
  errors: string[];
  ok: boolean;
};

export type VerifyTitle = {
  index: number;
  object?: number | null;
  playlist?: number | null;
  clips?: string[];
  duration_secs?: number | null;
  chapters?: number | null;
  errors?: string[];
};

export type VerifyReport = {
  path: string;
  first_play: number | null;
  top_menu: number | null;
  titles: VerifyTitle[];
  errors: string[];
  warnings: string[];
  ok: boolean;
};

export type KeyEntry = {
  id: string;
  name: string | null;
  media_key: string | null;
  volume_id: string | null;
  vuk: string | null;
  unit_keys: string[];
  note: string | null;
};

export type CloneProgress = {
  percent: number;
  bytes: number;
  total: number;
  message: string;
  done: boolean;
};

export type QueueJob = {
  id: number;
  title: number | null;
  source: string;
  output: string | null;
  /** queued | running | done | error | cancelled */
  state: string;
  percent: number;
  message: string;
  dest: string | null;
};

export const api = {
  snapshot: () => invoke<Snapshot>("snapshot"),
  changelog: () => invoke<AboutInfo>("changelog"),
  logsTail: (limit?: number, filter?: string | null) =>
    invoke<LogEntry[]>("logs_tail", { limit: limit ?? null, filter: filter ?? null }),
  logsGet: (id: number) => invoke<LogEntry | null>("logs_get", { id }),
  logsClear: () => invoke<void>("logs_clear"),
  logError: (source: string, message: string, detail: string) =>
    invoke<number>("log_error", { source, message, detail }),
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
  onLogFail: (cb: (e: { id: number; message: string }) => void): Promise<UnlistenFn> =>
    listen<{ id: number; message: string }>("logs://fail", (e) => cb(e.payload)),
  onIsoError: (cb: (e: string) => void): Promise<UnlistenFn> =>
    listen<string>("iso://error", (e) => cb(e.payload)),
  onProgress: (cb: (p: RipProgress) => void): Promise<UnlistenFn> =>
    listen<RipProgress>("rip://progress", (e) => cb(e.payload)),
  onRipError: (cb: (msg: string) => void): Promise<UnlistenFn> =>
    listen<string>("rip://error", (e) => cb(e.payload)),
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  eject: () => invoke<string>("eject"),
  mcpStatus: () => invoke<{ enabled: boolean; port: number | null }>("mcp_status"),
  authorStart: (request: {
    inputs: string[];
    dest: string;
    label: string | null;
    chapters: boolean;
    auto_chapter_minutes: number | null;
    split_gb: number | null;
    menu: boolean;
    menu_background: string | null;
    menu_template: string | null;
  }) => invoke<void>("author_start", { request }),
  onAuthorProgress: (cb: (p: AuthorProgress) => void): Promise<UnlistenFn> =>
    listen<AuthorProgress>("author://progress", (e) => cb(e.payload)),
  onAuthorDone: (cb: (path: string) => void): Promise<UnlistenFn> =>
    listen<string>("author://done", (e) => cb(e.payload)),
  onAuthorError: (cb: (msg: string) => void): Promise<UnlistenFn> =>
    listen<string>("author://error", (e) => cb(e.payload)),
  tvPlan: (source: string, start?: number | null) =>
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
    }>("tv_plan", { source, start: start ?? null }),
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
  menuParse: (source: string) => invoke<DiscMenu>("menu_parse", { source }),
  menuList: () => invoke<MenuEntry[]>("menu_list"),
  menuLoad: (template: string) => invoke<DiscMenu>("menu_load", { template }),
  menuSave: (template: string, doc: DiscMenu) =>
    invoke<string>("menu_save", { template, doc }),
  menuApply: (template: string, target: string, save: string | null) =>
    invoke<DiscMenu>("menu_apply", { template, target, save }),
  menuRemove: (template: string) => invoke<string>("menu_remove", { template }),
  keyEntry: (source: string) => invoke<KeyEntry | null>("key_entry", { source }),
  keysSearch: (query: string, limit: number) =>
    invoke<KeyEntry[]>("keys_search", { query, limit }),
  keysRemove: (id: string) => invoke<boolean>("keys_remove", { id }),
  keysImport: (path: string) => invoke<[number, number]>("keys_import", { path }),
  keysSetUnits: (source: string, keys: string[]) =>
    invoke<KeyEntry>("keys_set_units", { source, keys }),
  keysNative: (source: string | null) =>
    invoke<NativeKeyResult>("keys_native", { source }),
  bdplusInstall: () => invoke<string>("bdplus_install"),
  tsmuxerInstall: () => invoke<string>("tsmuxer_install"),
  verify: (path: string) => invoke<VerifyReport>("verify", { path }),
  doctor: () => invoke<DoctorReport>("doctor"),
  burnStart: (request: {
    image: string;
    device: string | null;
    speed: number | null;
    dry_run: boolean;
  }) => invoke<void>("burn_start", { request }),
  onBurnProgress: (cb: (p: BurnProgress) => void): Promise<UnlistenFn> =>
    listen<BurnProgress>("burn://progress", (e) => cb(e.payload)),
  onBurnDone: (cb: (path: string) => void): Promise<UnlistenFn> =>
    listen<string>("burn://done", (e) => cb(e.payload)),
  onBurnError: (cb: (msg: string) => void): Promise<UnlistenFn> =>
    listen<string>("burn://error", (e) => cb(e.payload)),
  dvdInfo: (path: string) => invoke<DvdInfo>("dvd_info", { path }),
  dvdRip: (request: {
    source: string;
    title: number;
    output: string;
    subtitles: boolean;
  }) => invoke<void>("dvd_rip", { request }),
  onDvdProgress: (cb: (p: DvdProgress) => void): Promise<UnlistenFn> =>
    listen<DvdProgress>("dvd://progress", (e) => cb(e.payload)),
  onDvdDone: (cb: (path: string) => void): Promise<UnlistenFn> =>
    listen<string>("dvd://done", (e) => cb(e.payload)),
  onDvdError: (cb: (msg: string) => void): Promise<UnlistenFn> =>
    listen<string>("dvd://error", (e) => cb(e.payload)),
  discStreams: (path: string, title: number) =>
    invoke<StreamInfo[]>("disc_streams", { path, title }),
  queueAdd: (request: RipRequest) => invoke<number>("queue_add", { request }),
  queueList: () => invoke<QueueJob[]>("queue_list"),
  queueCancel: (id: number) => invoke<void>("queue_cancel", { id }),
  queueRemove: (id: number) => invoke<void>("queue_remove", { id }),
  queueClear: () => invoke<void>("queue_clear"),
  queueResume: () => invoke<void>("queue_resume"),
  onQueueChanged: (cb: () => void): Promise<UnlistenFn> =>
    listen("queue://changed", () => cb()),
  onQueueProgress: (
    cb: (p: { id: number; progress: RipProgress }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ id: number; progress: RipProgress }>("queue://progress", (e) =>
      cb(e.payload),
    ),
  cloneStart: (request: { source: string; dest: string }) =>
    invoke<void>("clone_start", { request }),
  onCloneProgress: (cb: (p: CloneProgress) => void): Promise<UnlistenFn> =>
    listen<CloneProgress>("clone://progress", (e) => cb(e.payload)),
  onCloneDone: (
    cb: (p: { bytes: number; dest: string }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ bytes: number; dest: string }>("clone://done", (e) => cb(e.payload)),
  onCloneError: (cb: (msg: string) => void): Promise<UnlistenFn> =>
    listen<string>("clone://error", (e) => cb(e.payload)),
};

export const changelog = () => invoke<AboutInfo>("changelog");
