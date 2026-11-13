/**
 * Development-only fixture for the Tauri bridge.
 *
 * `npm run dev:mock` opens Vite with `?mock=1`; this module installs a small
 * `window.__TAURI_INTERNALS__` shim so the interface can be laid out, clicked
 * and screenshotted in a plain browser without a disc drive. It is loaded by a
 * dynamic import behind `import.meta.env.DEV`, so it is never in a release
 * bundle.
 */

type Invoke = (cmd: string, args?: Record<string, any>) => Promise<unknown>;

const callbacks = new Map<number, (payload: unknown) => void>();
let nextCallback = 1;

const bytes = Array.from({ length: 24 }, (_, i) =>
  Array.from({ length: 24 }, (_, j) => {
    const hue = 190 + ((i + j) % 40);
    return `<rect x="${j}" y="${i}" width="1.05" height="1.05" fill="hsl(${hue} 70% ${18 + ((i * j) % 34)}%)"/>`;
  }).join(""),
).join("");

function coverArt(letter: string, tone: number): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="300" height="400" viewBox="0 0 300 400">
    <defs>
      <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="hsl(${tone} 70% 32%)"/>
        <stop offset=".55" stop-color="hsl(${tone + 32} 62% 16%)"/>
        <stop offset="1" stop-color="hsl(${tone + 60} 70% 9%)"/>
      </linearGradient>
    </defs>
    <rect width="300" height="400" fill="url(#g)"/>
    <g opacity=".22">${bytes}</g>
    <text x="24" y="352" fill="#eef3ff" font-family="Helvetica, Arial, sans-serif"
      font-size="58" font-weight="700" opacity=".92">${letter}</text>
    <text x="24" y="378" fill="#c9d4ee" font-family="Helvetica, Arial, sans-serif"
      font-size="12" letter-spacing="3">KEEPSAKE</text>
  </svg>`;
  return `data:image/svg+xml;base64,${btoa(unescape(encodeURIComponent(svg)))}`;
}

const drive = {
  bsd_name: "disk4",
  media_name: "NORTHLIGHT S1 D1",
  is_whole: true,
  removable: true,
  size_bytes: 50011819200,
  vendor: "Pioneer",
  product: "BD-RW BDR-XD07",
  revision: "1.03",
  scsi_pdt: 5,
  optical: true,
  interconnect: "USB",
  mount_point: "/Volumes/NORTHLIGHT_S1_D1",
  device_path: "/dev/rdisk4",
  registry_path: "IOService:/AppleUSBSCSITarget",
};

const disc = {
  name: "NORTHLIGHT S1 D1",
  path: "/dev/rdisk4",
  aacs: true,
  aacs_ready: true,
  aacs_error: 0,
  titles: [
    { index: 0, playlist: 800, duration_secs: 3120, chapters: 12, is_main: true, resolution: "2160p", dynamic_range: "HDR10", audio_streams: 4 },
    { index: 1, playlist: 801, duration_secs: 2894, chapters: 11, is_main: false, resolution: "2160p", dynamic_range: "HDR10", audio_streams: 4 },
    { index: 2, playlist: 802, duration_secs: 2971, chapters: 10, is_main: false, resolution: "2160p", dynamic_range: "HDR10", audio_streams: 3 },
    { index: 3, playlist: 803, duration_secs: 3044, chapters: 11, is_main: false, resolution: "2160p", dynamic_range: "HDR10", audio_streams: 3 },
    { index: 4, playlist: 900, duration_secs: 94, chapters: 1, is_main: false, resolution: "1080p", dynamic_range: null, audio_streams: 2 },
    { index: 5, playlist: 901, duration_secs: 41, chapters: 1, is_main: false, resolution: "1080p", dynamic_range: null, audio_streams: 2 },
  ],
  main_title: 0,
  library: "1.5.1",
  video_codec: "HEVC",
  audio_codec: "TrueHD",
  audio_lang: "eng",
};

const config = {
  version: 3,
  output_dir: "/Volumes/atelier/rips",
  output_template: "{title} ({year})/{title} ({year}) - {resolution} {hdr}",
  tv_template: "{series}/Season {season:02}/{series} - S{season:02}E{episode:02} - {episode_title}",
  tv_subs: "all",
  auto_update_keys: true,
  auto_check_updates: true,
  last_update_check: null,
  auto_cover: true,
  hirescovers_dir: "",
  mcp_enabled: true,
  mcp_port: 8848,
};

const streams = [
  { kind: "video", lang: null, coding_type: 0x24, format: 0, rate: 0, pid: 4113 },
  { kind: "audio", lang: "eng", coding_type: 0x83, format: 0, rate: 0, pid: 4352 },
  { kind: "audio", lang: "eng", coding_type: 0x86, format: 0, rate: 0, pid: 4353 },
  { kind: "audio", lang: "fra", coding_type: 0x81, format: 0, rate: 0, pid: 4354 },
  { kind: "subtitle", lang: "eng", coding_type: 0x90, format: 0, rate: 0, pid: 4608 },
  { kind: "subtitle", lang: "eng", coding_type: 0x90, format: 0, rate: 0, pid: 4609 },
];

const coverHits = [
  { source: "hirescovers", id: "hc-1", title: "Northlight (2025)", kind: "cover", format: "png", preview_url: "hc-1", full_url: "hc-1", needs_auth: false, note: "" },
  { source: "hirescovers", id: "hc-2", title: "Northlight - Season 1", kind: "cover", format: "jpg", preview_url: "hc-2", full_url: "hc-2", needs_auth: false, note: "" },
  { source: "tmdb", id: "tm-3", title: "Northlight poster", kind: "poster", format: "jpg", preview_url: "tm-3", full_url: "tm-3", needs_auth: false, note: "" },
  { source: "dvdcover", id: "dc-4", title: "Northlight disc label", kind: "disc", format: "png", preview_url: "dc-4", full_url: "dc-4", needs_auth: false, note: "" },
  { source: "hirescovers", id: "hc-5", title: "Northlight alternate", kind: "cover", format: "png", preview_url: "hc-5", full_url: "hc-5", needs_auth: true, note: "member original" },
  { source: "dvdcover", id: "dc-6", title: "Northlight label B", kind: "disc", format: "png", preview_url: "dc-6", full_url: "dc-6", needs_auth: false, note: "" },
];

const doctor = {
  version: "0.1.0",
  checks: [
    { name: "ffmpeg", ok: true, detail: "/opt/homebrew/bin/ffmpeg", fix: null },
    { name: "ffprobe", ok: true, detail: "/opt/homebrew/bin/ffprobe", fix: null },
    { name: "tsMuxeR", ok: true, detail: "/usr/local/bin/tsMuxeR", fix: null },
    { name: "libbluray", ok: true, detail: "1.5.1", fix: null },
    { name: "libaacs", ok: true, detail: "0.11.1", fix: null },
    { name: "libbdplus", ok: true, detail: "0.2.0", fix: null },
    { name: "newfs_udf", ok: true, detail: "/sbin/newfs_udf", fix: null },
    { name: "hdiutil", ok: true, detail: "/usr/bin/hdiutil", fix: null },
    { name: "growisofs", ok: false, detail: "not found", fix: "brew install dvd+rw-tools" },
  ],
  capabilities: [
    { name: "Blu-ray read and decrypt", ok: true, detail: "libbluray 1.5.1, native AACS and BD+" },
    { name: "Remux and encode", ok: true, detail: "ffmpeg 7.1 with VideoToolbox" },
    { name: "BDMV authoring", ok: true, detail: "UDF 2.50 ISO and folder output" },
    { name: "BD-J title menus", ok: true, detail: "verified with libbluray" },
    { name: "Burning", ok: false, detail: "native hdiutil writer, no growisofs" },
  ],
  errors: [],
  ok: true,
};

const invoke: Invoke = async (cmd, args = {}) => {
  switch (cmd) {
    case "snapshot":
      return {
        version: "0.1.0",
        devices: [drive],
        optical: [drive],
        keys: { path: "/Users/you/Library/Application Support/keepsake-atelier/KEYDB.cfg", exists: true, entries: 128433, bytes: 24117248, modified: 0 },
      };
    case "changelog":
      return { version: "0.2.0", text: "## Unreleased\n\nNothing yet.\n" };
    case "config_get":
      return config;
    case "config_set":
    case "reveal":
    case "install_update":
    case "cancel_rip":
    case "queue_clear":
    case "queue_resume":
      return null;
    case "open_disc":
      return disc;
    case "dvd_info":
      throw new Error("No DVD-Video structure on this disc");
    case "disc_streams":
      return streams;
    case "key_entry":
      return {
        id: "0x8A4C1F90DE22B77C",
        name: "NORTHLIGHT S1 D1",
        media_key: "0x…",
        volume_id: "0x…",
        vuk: "0x9C2A…",
        unit_keys: ["0x11…", "0x2F…", "0x7B…", "0xA0…"],
        note: null,
      };
    case "queue_list":
      return [
        { id: 1, title: 1, source: "/dev/rdisk4", output: "/Volumes/atelier/rips/Northlight/Season 01/Northlight - S01E01 - First Light.mkv", state: "done", percent: 100, message: "wrote 24.1 GB", dest: "/Volumes/atelier/rips/Northlight/Season 01/Northlight - S01E01 - First Light.mkv" },
        { id: 2, title: 2, source: "/dev/rdisk4", output: "/Volumes/atelier/rips/Northlight/Season 01/Northlight - S01E02 - The Long Dark.mkv", state: "queued", percent: 0, message: "waiting", dest: null },
      ];
    case "template_placeholders":
      return [
        ["title", "Movie or disc title"],
        ["year", "Release year"],
        ["resolution", "2160p, 1080p"],
        ["hdr", "HDR10, DV"],
        ["video_codec", "HEVC, AVC"],
        ["ext", "Container extension"],
      ];
    case "preview_output":
      return `${args.vars?.output_dir ?? "/rips"}/${args.vars?.title ?? "Disc"} (${args.vars?.year ?? "2025"})/${args.vars?.title ?? "Disc"} (${args.vars?.year ?? "2025"}) - ${args.vars?.resolution ?? "2160p"} ${args.vars?.hdr ?? "HDR10"}.${args.vars?.ext ?? "mkv"}`;
    case "tmdb_lookup":
      return { title: "Northlight", year: "2025", tmdb_id: 424242 };
    case "check_update":
      return { version: null, notes: null };
    case "mcp_status":
      return { enabled: true, port: 8848 };
    case "doctor":
      return doctor;
    case "verify":
      return { path: args.path, first_play: 0, top_menu: 1, titles: [{ index: 0, playlist: 800, duration_secs: 3120, chapters: 12, errors: [] }], errors: [], warnings: [], ok: true };
    case "tv_plan":
      return {
        series: "Northlight",
        season: 1,
        disc: 1,
        tmdb_id: 424242,
        poster: null,
        items: [
          { title_index: 0, start_secs: 0, duration_secs: 3120, episode: { season: 1, number: 1, title: "First Light", still: null } },
          { title_index: 1, start_secs: 0, duration_secs: 2894, episode: { season: 1, number: 2, title: "The Long Dark", still: null } },
          { title_index: 2, start_secs: 0, duration_secs: 2971, episode: { season: 1, number: 3, title: "Signal", still: null } },
          { title_index: 3, start_secs: 0, duration_secs: 3044, episode: { season: 1, number: 4, title: "The Hollow", still: null } },
        ],
      };
    case "covers_search":
      return coverHits;
    case "cover_thumb": {
      const hit = coverHits.find((h) => h.preview_url === args.url);
      const tone = (coverHits.indexOf(hit ?? coverHits[0]) * 47 + 190) % 360;
      return coverArt((hit?.title ?? "K").slice(0, 1), tone);
    }
    case "save_covers":
      return (args.items ?? []).map((i: { role: string }) => ({
        role: i.role,
        path: `${args.destDir}/${args.stem}-${i.role}.png`,
        original: true,
        watermarked: false,
        width: 2000,
        height: 1600,
      }));
    case "queue_add":
      return 3;
    case "keys_update":
      return { path: config.output_dir, exists: true, entries: 128433, bytes: 24117248, modified: 0 };
    case "keys_search":
      return [];
    case "keys_native":
      return { key: { id: "0x8A4C", name: "NORTHLIGHT S1 D1", vid: "0x…", media_key: "0x…", vuk: "0x…" }, added: true };
    case "bdplus_install":
      return "/Users/you/Library/Application Support/keepsake-atelier/bdplus";
    case "menu_list":
      return [
        { name: "severance-s2", path: "/menus/severance-s2.json", label: "NORTHLIGHT S1", titles: 4 },
        { name: "reference-disc", path: "/menus/reference-disc.json", label: "Reference", titles: 6 },
      ];
    case "menu_parse":
    case "menu_load":
      return {
        version: 1,
        label: "NORTHLIGHT S1 D1",
        navigation: {
          first_playback: { kind: "bdj", name: "00000.jar" },
          top_menu: { kind: "bdj", name: "00000.jar" },
          titles: [
            { kind: "hdmv", object: 0 },
            { kind: "hdmv", object: 1 },
          ],
        },
        titles: disc.titles.slice(0, 4).map((t) => ({
          index: t.index,
          playlist: t.playlist,
          duration_secs: t.duration_secs,
          chapters: t.chapters,
          name: `Episode ${t.index + 1}`,
          main: t.is_main,
          chapter_names: Array.from({ length: t.chapters }, (_, i) => `Chapter ${i + 1}`),
          resolution: t.resolution,
          audio_streams: t.audio_streams,
        })),
      };
    case "menu_save":
      return "/menus/severance-s2.json";
    case "menu_remove":
      return "removed";
    case "menu_apply":
      return { version: 1, label: "applied", titles: [] };
    default:
      // Event plumbing and the file dialogs are no-ops in the mock.
      if (cmd.startsWith("plugin:")) return 1;
      throw new Error(`mock: no handler for ${cmd}`);
  }
};

export function installMock() {
  const w = window as any;
  w.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback(cb: (payload: unknown) => void) {
      const id = nextCallback++;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback(id: number) {
      callbacks.delete(id);
    },
    unregisterListener(id: number) {
      callbacks.delete(id);
    },
    convertFileSrc(filePath: string) {
      return filePath;
    },
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener() {
      /* the mock never emits events */
    },
  };
  // A tiny window hook so Playwright can drive the mock without a real backend.
  w.__KEEPSAKE_MOCK__ = { callbacks };
}
