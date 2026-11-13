import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import {
  api,
  type AppConfig,
  type DiscInfo,
  type DoctorReport,
  type DvdInfo,
  type DvdProgress,
  type EncodeSettings,
  type AudioSel,
  type BurnProgress,
  type CloneProgress,
  type CoverHit,
  type IsoProgress,
  type KeyEntry,
  type MovieHit,
  type QueueJob,
  type RipMode,
  type RipProgress,
  type Snapshot,
  type StreamInfo,
  type SubSel,
  type Title,
  type VerifyReport,
} from "./api";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Menus } from "./Menus";
import logo from "./assets/logo.png";

type CoverRole = "cover" | "poster" | "disc";

type TabState = {
  query: string;
  source: string;
  hits: CoverHit[];
  selected: CoverHit | null;
  busy: boolean;
};

const ROLE_LABEL: Record<CoverRole, string> = {
  cover: "Cover",
  poster: "Poster",
  disc: "Disc label",
};

/// Which result best fits a role: a disc label for disc, a front cover for
/// cover (never a disc), and anything for a poster.
function fitsRole(role: CoverRole, h: CoverHit): boolean {
  if (role === "disc") return h.kind === "disc";
  if (role === "poster") return h.source === "tmdb" || h.kind === "cover";
  return h.kind === "cover";
}

const CODECS = [
  { id: "x265", label: "x265 (HEVC, CPU)" },
  { id: "x264", label: "x264 (AVC, CPU)" },
  { id: "hevc_videotoolbox", label: "HEVC (GPU · VideoToolbox)" },
  { id: "h264_videotoolbox", label: "H.264 (GPU · VideoToolbox)" },
] as const;

function fmtDuration(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = Math.floor(s % 60);
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${m}:${String(sec).padStart(2, "0")}`;
}
function fmtBytes(n: number): string {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let v = n, i = 0;
  while (v >= 1000 && i < u.length - 1) { v /= 1000; i++; }
  return `${v.toFixed(i === 0 ? 0 : 2)} ${u[i]}`;
}
function streamCodec(s: StreamInfo): string {
  const t = s.coding_type;
  const map: Record<number, string> =
    s.kind === "video"
      ? { 0x01: "MPEG-1", 0x02: "MPEG-2", 0x1b: "H.264", 0x24: "HEVC", 0xea: "VC-1" }
      : s.kind === "audio"
        ? { 0x80: "LPCM", 0x81: "AC3", 0x82: "DTS", 0x83: "TrueHD", 0x84: "E-AC3", 0x85: "DTS-HD", 0x86: "DTS-HD MA", 0xa1: "E-AC3", 0xa2: "DTS Express" }
        : { 0x90: "PGS" };
  return map[t] ?? s.kind;
}
function sanitize(s: string): string {
  return s.replace(/[\/\\:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim() || "disc";
}

export default function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [disc, setDisc] = useState<DiscInfo | null>(null);
  const [discPath, setDiscPath] = useState("");
  const [title, setTitle] = useState<number | null>(null);
  const [streams, setStreams] = useState<StreamInfo[]>([]);
  const [queue, setQueue] = useState<QueueJob[]>([]);
  const [entry, setEntry] = useState<KeyEntry | null>(null);
  const [unitsText, setUnitsText] = useState("");
  const [unitsBusy, setUnitsBusy] = useState(false);
  const [keyEpoch, setKeyEpoch] = useState(0);
  const [keyQuery, setKeyQuery] = useState("");
  const [keyHits, setKeyHits] = useState<KeyEntry[]>([]);
  const [keyBusy, setKeyBusy] = useState(false);
  const [container, setContainer] = useState<"mkv" | "mp4">("mkv");
  const [lossless, setLossless] = useState(true);
  const [enc, setEnc] = useState<EncodeSettings>({
    codec: "x265", quality: 20, preset: "medium", ten_bit: true, aac_audio: false, cinavia: false,
    cinavia_codec: "auto",
  });
  const [audio, setAudio] = useState<AudioSel>("all");
  const [subs, setSubs] = useState<SubSel>("all");
  const [chapters, setChapters] = useState(true);
  const [poster, setPoster] = useState(true);
  const [movie, setMovie] = useState<string | null>(null);
  const [placeholders, setPlaceholders] = useState<[string, string][]>([]);
  const [covers, setCovers] = useState(true);
  const [tvPlan, setTvPlan] = useState<Awaited<ReturnType<typeof api.tvPlan>> | null>(null);
  const [tvSel, setTvSel] = useState<Record<number, boolean>>({});
  const [tvBusy, setTvBusy] = useState(false);
  const [tvError, setTvError] = useState<string | null>(null);
  const [mcp, setMcp] = useState<{ enabled: boolean; port: number | null } | null>(null);
  const [doctor, setDoctor] = useState<DoctorReport | null>(null);
  const [dvd, setDvd] = useState<DvdInfo | null>(null);
  const [dvdTitle, setDvdTitle] = useState<number | null>(null);
  const [dvdProgress, setDvdProgress] = useState<DvdProgress | null>(null);
  const [startEp, setStartEp] = useState<number | "">("");
  const [authorFiles, setAuthorFiles] = useState<string[]>([]);
  const [authorOut, setAuthorOut] = useState("");
  const [authorMenu, setAuthorMenu] = useState(false);
  const [authorMenuBg, setAuthorMenuBg] = useState("");
  const [authorSplit, setAuthorSplit] = useState("");
  const [authorBusy, setAuthorBusy] = useState(false);
  const [authorProg, setAuthorProg] = useState<{ percent: number; message: string } | null>(null);
  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const [coverTab, setCoverTab] = useState<CoverRole>("cover");
  const [tabs, setTabs] = useState<Record<CoverRole, TabState>>({
    cover: { query: "", source: "all", hits: [], selected: null, busy: false },
    poster: { query: "", source: "tmdb", hits: [], selected: null, busy: false },
    disc: { query: "", source: "all", hits: [], selected: null, busy: false },
  });
  const [savingCovers, setSavingCovers] = useState(false);
  const [preview, setPreview] = useState("");
  const [progress, setProgress] = useState<RipProgress | null>(null);
  const [isoProgress, setIsoProgress] = useState<IsoProgress | null>(null);
  const [isoBusy, setIsoBusy] = useState(false);
  const [cloneProgress, setCloneProgress] = useState<CloneProgress | null>(null);
  const [burnProgress, setBurnProgress] = useState<BurnProgress | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [update, setUpdate] = useState<string | null>(null);
  const startedKeys = useRef(false);

  // Toasts are transient; nothing else clears them.
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 2600);
    return () => clearTimeout(t);
  }, [toast]);

  // Track list for the selected title (codec + language per stream).
  useEffect(() => {
    let live = true;
    if (title == null || !discPath) {
      setStreams([]);
      return;
    }
    api.discStreams(discPath, title)
      .then((s) => { if (live) setStreams(s); })
      .catch(() => { if (live) setStreams([]); });
    return () => { live = false; };
  }, [title, discPath]);

  const refreshQueue = useCallback(async () => {
    try { setQueue(await api.queueList()); } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    void refreshQueue();
    const us = [
      api.onQueueChanged(() => { void refreshQueue(); }),
      api.onQueueProgress(({ id, progress }) =>
        setQueue((q) =>
          q.map((j) =>
            j.id === id ? { ...j, percent: progress.percent, message: progress.message } : j,
          ),
        ),
      ),
    ];
    return () => { us.forEach((u) => u.then((f) => f())); };
  }, [refreshQueue]);

  // KEYDB entry for the selected disc (VUK and AACS2 unit keys).
  useEffect(() => {
    let live = true;
    if (!discPath) {
      setEntry(null);
      setUnitsText("");
      return;
    }
    api.keyEntry(discPath)
      .then((e) => {
        if (!live) return;
        setEntry(e);
        setUnitsText(e?.unit_keys.join("\n") ?? "");
      })
      .catch(() => { if (live) { setEntry(null); setUnitsText(""); } });
    return () => { live = false; };
  }, [discPath, keyEpoch]);

  const refresh = useCallback(async () => {
    try {
      const s = await api.snapshot();
      setSnap(s);
      // auto-select the optical drive: prefer a mounted volume, else the raw
      // device node (UHD/UDF often won't mount, libbluray can read /dev/rdiskN).
      const dev = s.optical[0];
      if (dev) setDiscPath(dev.mount_point ?? `/dev/r${dev.bsd_name}`);
      else if (s.optical.length === 0) setDiscPath("");
    } catch (e) { setError(String(e)); }
  }, []);

  useEffect(() => {
    (async () => {
      setCfg(await api.config());
      await refresh();
      if (!startedKeys.current) {
        startedKeys.current = true;
        try { const c = await api.config(); if (c.auto_update_keys) await api.keysUpdate(); await refresh(); } catch { /* offline ok */ }
      }
      try { const u = await api.checkUpdate(); setUpdate(u.version); } catch { /* offline ok */ }
      try { setMcp(await api.mcpStatus()); } catch { /* offline ok */ }
      try { setDoctor(await api.doctor()); } catch { /* offline ok */ }
    })();
    const un = api.onProgress((p) => {
      setProgress(p);
      if (p.done) { setBusy(null); setToast("Rip complete"); }
    });
    const unAuthor = [
      api.onAuthorProgress((p) => setAuthorProg({ percent: p.percent, message: p.message })),
      api.onAuthorDone((path) => { setAuthorBusy(false); setToast(`Authored ${path}`); }),
      api.onAuthorError((e) => { setAuthorBusy(false); setError(e); }),
    ];
    const unErr = api.onRipError((msg) => {
      setBusy(null);
      setError(msg);
    });
    return () => {
      un.then((f) => f());
      unErr.then((f) => f());
      unAuthor.forEach((u) => u.then((f) => f()));
    };
  }, [refresh]);

  const openDisc = async (p: string) => {
    setError(null); setBusy("reading disc");
    try {
      const info = await api.openDisc(p);
      setDisc(info);
      setDvd(null);
      setTitle(info.main_title ?? info.titles[0]?.index ?? null);
      setMovie(info.name || null);
      const q = info.name || "";
      setTabs((t) => ({
        cover: { ...t.cover, query: q },
        poster: { ...t.poster, query: q },
        disc: { ...t.disc, query: q },
      }));
    } catch (e) {
      // A DVD-Video disc has no BDMV; try the DVD path before failing.
      try {
        const d = await api.dvdInfo(p);
        setDvd(d);
        setDisc(null);
        setDvdTitle(d.titles[0]?.number ?? null);
        setToast(`DVD-Video: ${d.titles.length} titles`);
      } catch {
        setDisc(null);
        setDvd(null);
        setError(String(e));
      }
    } finally { setBusy(null); }
  };

  const mode: RipMode = lossless
    ? { kind: "remux", container }
    : { kind: "encode", container, settings: enc };

  const titleInfo = useMemo(
    () => disc?.titles.find((x) => x.index === title) ?? null,
    [disc, title],
  );

  // Live preview of the output path, rendered by the same code the ripper uses.
  useEffect(() => {
    api.templatePlaceholders().then(setPlaceholders).catch(() => {});
  }, []);

  useEffect(() => {
    let alive = true;
    if (!cfg || !disc) { setPreview(""); return; }
    (async () => {
      const movieName = (movie ?? disc.name ?? "").trim();
      let hit: MovieHit | null = null;
      const wantsTmdb =
        poster || ["{title", "{year", "{tmdb}"].some((k) => cfg.output_template.includes(k));
      if (wantsTmdb && movieName) {
        try { hit = await api.tmdbLookup(movieName, titleInfo?.duration_secs ?? 0); } catch { hit = null; }
      }
      if (!alive) return;
      try {
        const p = await api.previewOutput(cfg.output_template, {
          output_dir: cfg.output_dir,
          disc: disc.name || "Blu-ray",
          title: hit?.title || movieName || disc.name || "Blu-ray",
          year: hit?.year ?? null,
          tmdb: hit?.tmdb_id ?? null,
          resolution: titleInfo?.resolution ?? null,
          hdr: titleInfo?.dynamic_range ?? null,
          video_codec: disc.video_codec,
          audio_lang: disc.audio_lang,
          audio_codec: disc.audio_codec,
          runtime_secs: titleInfo?.duration_secs ?? 0,
          chapters: titleInfo?.chapters ?? 0,
          playlist: titleInfo?.playlist ?? 0,
          index: title ?? 0,
          ext: container,
        });
        if (alive) setPreview(p);
      } catch { if (alive) setPreview(""); }
    })();
    return () => { alive = false; };
  }, [cfg, disc, title, titleInfo, container, movie, poster]);

  /// Save a config patch and refresh anything the backend restarts from it.
  const saveCfg = async (patch: Partial<NonNullable<typeof cfg>>) => {
    if (!cfg) return;
    const next = { ...cfg, ...patch };
    setCfg(next);
    try {
      await api.setConfig(next);
      setToast("Saved");
      setMcp(await api.mcpStatus());
    } catch (e) { setError(String(e)); }
  };

  // Build the request for one title (or the TV plan). The queue backend runs
  // jobs one at a time, so "Start rip" and the per-title buttons share this.
  const ripRequest = (t: number | null) => {
    if (!discPath || (!tvPlan && t == null)) return null;
    // Decryption must read the raw device; a mounted optical volume returns
    // still-encrypted bytes.
    const dev = optical[0];
    const onOptical = !!dev && (discPath === dev.mount_point || discPath === `/dev/r${dev.bsd_name}`);
    const src = onOptical ? `/dev/r${dev.bsd_name}` : discPath;
    return {
      source: src,
      keydb: snap?.keys?.path ?? null,
      title: t ?? 0,
      output: null, // render the configured output template
      mode,
      chapters,
      poster,
      movie,
      audio,
      subs,
      covers,
      tv: !!tvPlan,
      series: tvPlan?.series ?? null,
      episodes: tvPlan
        ? Object.entries(tvSel).filter(([, v]) => v).map(([k]) => k).join(",")
        : null,
      start_episode: tvPlan && startEp !== "" ? Number(startEp) : null,
      cinavia: enc.cinavia,
      cinavia_codec: enc.cinavia ? enc.cinavia_codec : null,
    };
  };

  const enqueueRip = async (t: number | null = title) => {
    const req = ripRequest(t);
    if (!req) return;
    setError(null);
    try {
      await api.queueAdd(req);
      await refreshQueue();
      setToast(tvPlan ? "episodes queued" : `title ${t ?? "main"} queued`);
    } catch (e) { setError(String(e)); }
  };

  // Queue every real title (skip menus/trailers under a minute). In TV mode the
  // episode plan is one job, so queue that instead.
  const enqueueAll = async () => {
    if (tvPlan) {
      await enqueueRip();
      return;
    }
    if (!disc) return;
    const targets = disc.titles.filter((t) => t.duration_secs >= 60);
    if (targets.length === 0) {
      setToast("no titles at least a minute long");
      return;
    }
    setError(null);
    try {
      for (const t of targets) {
        const req = ripRequest(t.index);
        if (req) await api.queueAdd(req);
      }
      await refreshQueue();
      setToast(`${targets.length} title(s) queued`);
    } catch (e) { setError(String(e)); }
  };

  const patchTab = (role: CoverRole, p: Partial<TabState>) =>
    setTabs((t) => ({ ...t, [role]: { ...t[role], ...p } }));

  const searchRole = async (role: CoverRole, selectBest = false) => {
    const st = tabs[role];
    const q = st.query.trim();
    if (!q) return;
    patchTab(role, { busy: true, hits: [] });
    try {
      const hits = await api.coversSearch(q, st.source, 12);
      patchTab(role, { hits });
      if (selectBest) patchTab(role, { selected: hits.find((h) => fitsRole(role, h)) ?? null });
    } catch (e) { setError(String(e)); }
    finally { patchTab(role, { busy: false }); }
  };

  const autoFillAll = () => {
    for (const role of ["cover", "poster", "disc"] as CoverRole[]) void searchRole(role, true);
  };

  // Load thumbnails for the active tab.
  const activeHits = tabs[coverTab].hits;
  useEffect(() => {
    let alive = true;
    (async () => {
      for (const h of activeHits.slice(0, 18)) {
        if (!alive) return;
        if (thumbs[h.preview_url]) continue;
        try {
          const d = await api.coverThumb(h.preview_url);
          if (d && alive) setThumbs((t) => ({ ...t, [h.preview_url]: d }));
        } catch { /* skip */ }
      }
    })();
    return () => { alive = false; };
  }, [activeHits]);

  // Where the files go: beside the rip's output, else the output folder.
  const coverTarget = useMemo(() => {
    const out = preview || "";
    const slash = out.lastIndexOf("/");
    const dir = slash > 0 ? out.slice(0, slash) : cfg?.output_dir ?? "";
    const base = (slash >= 0 ? out.slice(slash + 1) : out).replace(/\.[^.]+$/, "");
    const stem = base || sanitize(movie || disc?.name || "cover");
    return { dir, stem };
  }, [preview, cfg, movie, disc]);

  const chosen = (["cover", "poster", "disc"] as CoverRole[])
    .map((r) => ({ role: r, hit: tabs[r].selected }))
    .filter((x): x is { role: CoverRole; hit: CoverHit } => !!x.hit);

  const detectEpisodes = async () => {
    // Prefer the raw device (works for UHD/encrypted discs that do not mount);
    // fall back to whatever the drive card selected.
    const dev = optical[0];
    const raw = dev ? `/dev/r${dev.bsd_name}` : "";
    const src = raw || discPath;
    if (!src) { setError("no disc device found"); return; }
    setError(null);
    setTvBusy(true);
    try {
      let p;
      try {
        p = await api.tvPlan(src);
      } catch (e) {
        // raw device blocked (permissions) but the volume is readable
        if (src !== discPath && discPath) p = await api.tvPlan(discPath);
        else throw e;
      }
      setTvPlan(p);
      setTvSel(Object.fromEntries(p.items.map((i) => [i.episode.number, true])));
      setToast(`${p.items.length} episodes detected`);
    } catch (e) { setTvError(String(e)); setError(String(e)); }
    finally { setTvBusy(false); }
  };

  const chooseAuthorFile = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: true,
        filters: [{ name: "Video", extensions: ["mkv", "mp4", "m2ts"] }],
      });
      const files =
        typeof picked === "string" ? [picked] : Array.isArray(picked) ? picked : [];
      if (files.length === 0) return;
      setAuthorFiles(files);
      const stem = files[0].split("/").pop()?.replace(/\.[^.]+$/, "") ?? "disc";
      const base = `${(cfg?.output_dir ?? "").replace(/\/$/, "")}/${stem}/${stem}`;
      setAuthorOut(`${base}.iso`);
    } catch (e) { setError(String(e)); }
  };

  const startAuthor = async () => {
    if (authorFiles.length === 0 || !authorOut) return;
    setError(null); setAuthorBusy(true); setAuthorProg({ percent: 0, message: "starting" });
    try {
      await api.authorStart({
        inputs: authorFiles,
        dest: authorOut,
        label: null,
        chapters: true,
        auto_chapter_minutes: null,
        split_gb: authorSplit.trim() === "" ? null : Number(authorSplit) || null,
        menu: authorMenu,
        menu_background: authorMenuBg.trim() === "" ? null : authorMenuBg.trim(),
        menu_template: null,
      });
    } catch (e) { setError(String(e)); setAuthorBusy(false); }
  };

  const saveChosen = async () => {
    if (chosen.length === 0) return;
    setSavingCovers(true);
    try {
      const saved = await api.saveCovers(
        coverTarget.dir,
        coverTarget.stem,
        chosen.map((c) => ({ role: c.role, source: c.hit.source, id: c.hit.id, title: c.hit.title })),
      );
      const wm = saved.filter((s) => !s.original).length;
      setToast(
        `Saved ${saved.map((s) => s.role).join(", ")} to ${coverTarget.dir.split("/").pop()}` +
          (wm > 0 ? ` (${wm} watermarked preview${wm > 1 ? "s" : ""})` : " (originals)"),
      );
    } catch (e) { setError(String(e)); }
    finally { setSavingCovers(false); }
  };


  useEffect(() => {
    const us = [
      api.onIsoProgress((p) => setIsoProgress(p)),
      api.onIsoDone(() => { setIsoBusy(false); setToast("Decrypted ISO written"); }),
      api.onIsoError((e) => { setIsoBusy(false); setError(e); }),
      api.onCloneProgress((p) => setCloneProgress(p)),
      api.onCloneDone(({ dest }) => { setCloneProgress(null); setToast("Clone written"); void api.reveal(dest); }),
      api.onCloneError((e) => { setCloneProgress(null); setError(e); }),
      api.onBurnProgress((p) => setBurnProgress(p)),
      api.onBurnDone(() => { setBurnProgress(null); setToast("Disc burned"); }),
      api.onBurnError((e) => { setBurnProgress(null); setError(e); }),
    ];
    return () => { us.forEach((u) => u.then((f) => f())); };
  }, []);

  useEffect(() => {
    const us = [
      api.onDvdProgress((p) => setDvdProgress(p)),
      api.onDvdDone((path) => { setDvdProgress(null); setToast("DVD title written"); void api.reveal(path); }),
      api.onDvdError((e) => { setDvdProgress(null); setError(e); }),
    ];
    return () => { us.forEach((u) => u.then((f) => f())); };
  }, []);

  const isoPath = useMemo(() => {
    if (!cfg) return "";
    const name = sanitize(disc?.name || "disc");
    return `${cfg.output_dir.replace(/\/$/, "")}/${name}/${name}.iso`;
  }, [cfg, disc]);

  const exportIso = async (format: "iso" | "folder") => {
    if (!discPath) return;
    setError(null); setIsoBusy(true);
    setIsoProgress({ stage: "starting", percent: 0, files: 0, message: "starting" });
    try {
      await api.exportIso({
        source: discPath,
        keydb: snap?.keys?.path ?? null,
        dest: format === "iso" ? isoPath : isoPath.replace(/\.iso$/, ""),
        format,
        label: disc?.name ?? null,
      });
    } catch (e) { setError(String(e)); setIsoBusy(false); }
  };

  const cloneDisc = async () => {
    if (!discPath) return;
    const dev = optical[0];
    if (!dev) {
      setError("cloning needs the raw optical device (/dev/rdiskN); reconnect the drive");
      return;
    }
    const src = `/dev/r${dev.bsd_name}`;
    const dest = isoPath.replace(/\.iso$/i, " - clone.iso");
    setError(null);
    setCloneProgress({ percent: 0, bytes: 0, total: 0, message: "starting", done: false });
    try {
      await api.cloneStart({ source: src, dest });
    } catch (e) { setError(String(e)); setCloneProgress(null); }
  };

  const verifyTree = async (target: string) => {
    if (!target) return;
    try {
      const r: VerifyReport = await api.verify(target);
      if (r.ok) {
        setToast(`tree ok: ${r.titles.length} title(s)`);
      } else {
        setError(r.errors.slice(0, 5).join("; "));
      }
    } catch (e) { setError(String(e)); }
  };

  const ripDvd = async () => {
    if (!dvd || dvdTitle == null || !cfg) return;
    const name = sanitize(dvd.label || "DVD");
    const output = `${cfg.output_dir.replace(/\/$/, "")}/${name}/${name} - Title ${dvdTitle}.mkv`;
    setError(null);
    setDvdProgress({ percent: 0, out_time_ms: 0, total_ms: 0, message: "starting", done: false });
    try {
      await api.dvdRip({ source: dvd.root, title: dvdTitle, output, subtitles: false });
    } catch (e) { setError(String(e)); setDvdProgress(null); }
  };

  const searchKeys = async () => {
    setKeyBusy(true);
    try {
      setKeyHits(await api.keysSearch(keyQuery, 20));
    } catch (e) { setError(String(e)); } finally { setKeyBusy(false); }
  };

  const importKeys = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        filters: [{ name: "KEYDB", extensions: ["cfg"] }],
      });
      if (typeof picked !== "string") return;
      const [added, skipped] = await api.keysImport(picked);
      await refresh();
      setKeyEpoch((n) => n + 1);
      setToast(`imported ${added} (${skipped} already present)`);
    } catch (e) { setError(String(e)); }
  };

  const removeKey = async (id: string) => {
    try {
      const gone = await api.keysRemove(id);
      setKeyHits((h) => h.filter((e) => e.id !== id));
      setKeyEpoch((n) => n + 1);
      setToast(gone ? "key removed" : "not found");
    } catch (e) { setError(String(e)); }
  };

  const burnIso = async () => {
    const dev = optical[0];
    if (!dev || !isoPath) {
      setError("need a writer drive and a configured output folder");
      return;
    }
    if (!window.confirm(`Write\n${isoPath}\nto the drive (${dev.bsd_name})?\n\nThis erases the disc and cannot be undone.`)) return;
    setError(null);
    setBurnProgress({ percent: 0, message: "starting", done: false });
    try {
      // Native hdiutil burn: it picks the writer itself.
      await api.burnStart({ image: isoPath, device: null, speed: null, dry_run: false });
    } catch (e) { setError(String(e)); setBurnProgress(null); }
  };

  const [section, setSection] = useState<SectionId>("disc");
  const [railCompact, setRailCompact] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState("");
  const [paletteIndex, setPaletteIndex] = useState(0);
  const [helpOpen, setHelpOpen] = useState(false);

  const keys = snap?.keys;
  const optical = snap?.optical ?? [];
  const runningJob = queue.find((j) => j.state === "running") ?? null;

  const goto = (id: SectionId) => () => setSection(id);

  const paletteItems: PaletteItem[] = [
    ...SECTIONS.map((s) => ({
      id: `go-${s.id}`,
      label: s.label,
      hint: s.blurb,
      icon: s.icon,
      kbd: `alt ${s.key}`,
      group: "Go to",
      run: goto(s.id),
    })),
    {
      id: "rescan",
      label: "Rescan drives",
      hint: "Look again for an optical drive",
      icon: "refresh",
      group: "Disc",
      run: () => { void refresh(); setToast("Rescanning"); },
    },
    {
      id: "eject",
      label: "Eject the disc",
      hint: "Open the tray",
      icon: "eject",
      group: "Disc",
      run: () => { void api.eject().then(setToast).catch((e) => setError(String(e))); },
    },
    {
      id: "read",
      label: "Read the disc",
      hint: discPath || "No path selected",
      icon: "disc",
      group: "Disc",
      run: () => { if (discPath) void openDisc(discPath); },
    },
    {
      id: "queue-all",
      label: "Queue every title",
      hint: "Skip menus and trailers under a minute",
      icon: "layers",
      group: "Rip",
      run: () => { void enqueueAll(); },
    },
    {
      id: "episodes",
      label: "Detect episodes",
      hint: "Split the disc into a season",
      icon: "layers",
      group: "Rip",
      run: () => { void detectEpisodes(); },
    },
    {
      id: "iso",
      label: "Export a decrypted ISO",
      hint: "AACS and BD+ removed",
      icon: "disc",
      group: "Master",
      run: () => { void exportIso("iso"); },
    },
    {
      id: "clone",
      label: "Clone the disc 1:1",
      hint: "Raw sector copy with encryption intact",
      icon: "copy",
      group: "Master",
      run: () => { void cloneDisc(); },
    },
    {
      id: "keys",
      label: "Update decryption keys",
      hint: "Refresh KEYDB.cfg",
      icon: "key",
      group: "Maintenance",
      run: () => {
        void (async () => {
          try {
            await api.keysUpdate();
            await refresh();
            setKeyEpoch((n) => n + 1);
            setToast("Keys updated");
          } catch (e) { setError(String(e)); }
        })();
      },
    },
    {
      id: "doctor",
      label: "Re-run the system check",
      hint: "Probe native tools and capabilities",
      icon: "system",
      group: "Maintenance",
      run: () => { void api.doctor().then(setDoctor).catch((e) => setError(String(e))); },
    },
    {
      id: "shortcuts",
      label: "Keyboard shortcuts",
      hint: "Every key the workshop listens for",
      icon: "menu",
      kbd: "?",
      group: "Maintenance",
      run: () => { setHelpOpen(true); },
    },
    {
      id: "update",
      label: "Check for updates",
      hint: "Ask the release feed for a newer build",
      icon: "refresh",
      group: "Maintenance",
      run: () => { void checkForUpdate(); },
    },
    {
      id: "discord",
      label: "Open Discord",
      hint: "Questions, logs and builds",
      icon: "chat",
      group: "Support",
      run: () => { void openUrl("https://discord.gg/SUaUrVGj5f").catch(() => {}); },
    },
    {
      id: "kofi",
      label: "Open Ko-fi",
      hint: "Keep the mirrors running",
      icon: "heart",
      group: "Support",
      run: () => { void openUrl("https://ko-fi.com/ghosty99").catch(() => {}); },
    },
  ];

  const paletteFiltered = paletteItems.filter((i) =>
    `${i.label} ${i.hint} ${i.group}`.toLowerCase().includes(paletteQuery.trim().toLowerCase()),
  );
  const paletteActive = Math.min(paletteIndex, Math.max(0, paletteFiltered.length - 1));

  const runPalette = (index: number) => {
    const item = paletteFiltered[index];
    if (!item) return;
    setPaletteOpen(false);
    setPaletteQuery("");
    setPaletteIndex(0);
    item.run();
  };

  const openPalette = () => {
    setPaletteQuery("");
    setPaletteIndex(0);
    setPaletteOpen(true);
  };

  const checkForUpdate = async () => {
    try {
      const u = await api.checkUpdate();
      setUpdate(u.version);
      setToast(u.version ? `Update ${u.version} is ready` : "You are up to date");
    } catch (e) { setError(String(e)); }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((open) => !open);
        setPaletteQuery("");
        setPaletteIndex(0);
      } else if (e.key === "Escape") {
        setPaletteOpen(false);
        setHelpOpen(false);
      } else if (e.key === "?" && !isTyping(e.target)) {
        e.preventDefault();
        setHelpOpen((open) => !open);
      } else if (e.altKey && /^[1-8]$/.test(e.key)) {
        e.preventDefault();
        setSection(SECTIONS[Number(e.key) - 1].id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Automation: opt-in auto-rip for a newly inserted disc. Polls the drive list
  // so it works with the same backend the CLI watcher uses.
  const autoSeen = useRef<string | null>(null);
  const autoBusy = useRef(false);
  useEffect(() => {
    if (!cfg?.auto_rip) return;
    let alive = true;
    const tick = async () => {
      if (!alive || autoBusy.current) return;
      try {
        const s = await api.snapshot();
        const dev = s.optical[0];
        if (!dev || (!dev.mount_point && !dev.media_name)) {
          autoSeen.current = null;
          return;
        }
        const path = dev.device_path ?? dev.mount_point;
        if (!path) { autoSeen.current = null; return; }
        // The device node survives an eject, so key on the media instead.
        const key = `${path}|${dev.media_name ?? ""}|${dev.size_bytes}`;
        if (autoSeen.current === key) return;
        autoSeen.current = key;
        autoBusy.current = true;
        // Let the volume settle before opening it.
        await new Promise((resolve) => setTimeout(resolve, 6000));
        if (!alive) return;
        const info = await api.openDisc(path);
        setDisc(info);
        setDiscPath(path);
        setTitle(info.main_title ?? info.titles[0]?.index ?? null);
        setMovie(info.name);
        const floor = Math.max(60, cfg.auto_rip_min_secs || 600);
        const picks = cfg.auto_rip_mode === "all"
          ? info.titles.filter((t) => t.duration_secs >= floor).map((t) => t.index)
          : (() => {
              const marked = info.titles.find((t) => t.index === info.main_title)
                ?? info.titles.find((t) => t.is_main)
                ?? [...info.titles].sort((a, b) => b.duration_secs - a.duration_secs)[0];
              return marked ? [marked.index] : [];
            })();
        if (picks.length === 0) { setToast("auto-rip: no title long enough"); return; }
        for (const t of picks) {
          await api.queueAdd({
            source: path,
            keydb: s.keys?.path ?? null,
            title: t,
            output: null,
            mode, chapters, poster, movie: info.name, audio, subs, covers,
            tv: false, series: null, episodes: null, start_episode: null,
          });
        }
        await refreshQueue();
        setToast(`auto-ripped ${picks.length} title(s) from ${info.name}`);
      } catch {
        // The drive is still mounting or has no readable disc; try again later.
      } finally {
        autoBusy.current = false;
      }
    };
    void tick();
    const id = setInterval(() => void tick(), 5000);
    return () => { alive = false; clearInterval(id); };
  }, [cfg?.auto_rip, cfg?.auto_rip_mode, cfg?.auto_rip_min_secs, mode, chapters, poster, audio, subs, covers, refreshQueue]);

  // Eject once the queue drains when the operator asked for it.
  const hadQueue = useRef(false);
  useEffect(() => {
    if (queue.length > 0) { hadQueue.current = true; return; }
    if (hadQueue.current && cfg?.auto_rip_eject) {
      hadQueue.current = false;
      void api.eject().then((m) => setToast(m)).catch(() => {});
    }
  }, [queue.length, cfg?.auto_rip_eject]);

  const page = PAGE_META[section];
  const checksOk = doctor?.checks.filter((c) => c.ok).length ?? 0;
  const checksTotal = doctor?.checks.length ?? 0;
  const activePercent = runningJob
    ? runningJob.percent
    : progress && !progress.done
      ? progress.percent
      : null;

  return (
    <div className="app">
      <a className="skip-link" href="#main">Skip to the workspace</a>
      <div className="backdrop" aria-hidden="true">
        <span className="orb orb-a" />
        <span className="orb orb-b" />
        <span className="orb orb-c" />
        <span className="grid" />
      </div>

      <header className="topbar">
        <div className="brand">
          <span className="brand-mark"><img className="logo" src={logo} alt="" /></span>
          <span className="brand-text">
            <strong>Keepsake Atelier</strong>
            <span>optical workshop</span>
          </span>
        </div>

        <div className="topbar-status">
          {optical.length > 0 ? (
            <span className="chip live">
              <span className="dot" />
              {optical[0].vendor || optical[0].product || "Optical drive"} · {optical[0].bsd_name}
            </span>
          ) : (
            <span className="chip"><span className="dot" /> no drive</span>
          )}
          {disc && (
            <span className="chip">
              {disc.name || "Blu-ray"} · {disc.titles.length} titles
            </span>
          )}
          {disc && (
            <span className={`chip ${disc.aacs ? (disc.aacs_ready ? "warn" : "bad") : "good"}`}>
              <span className="dot" />
              {disc.aacs ? (disc.aacs_ready ? "AACS ready" : "AACS locked") : "unencrypted"}
            </span>
          )}
          {runningJob && (
            <span className="chip warn">
              <span className="dot" /> ripping · {Math.round(runningJob.percent)}%
            </span>
          )}
        </div>

        <div className="spacer" />

        <button className="cmd-trigger" onClick={openPalette} aria-label="Open the command palette">
          <Icon name="search" size={14} />
          <span>Jump or run</span>
          <kbd>{IS_MAC ? "⌘K" : "Ctrl K"}</kbd>
        </button>
        {update ? (
          <button className="primary sm" onClick={async () => { try { await api.installUpdate(); } catch { /* offline */ } }}>
            <Icon name="refresh" size={14} /> Update to {update}
          </button>
        ) : (
          <button className="ghost sm" onClick={() => void checkForUpdate()}>
            Check updates
          </button>
        )}
      </header>

      <div className="shell">
        <nav className={`rail ${railCompact ? "compact" : ""}`} aria-label="Workspace sections">
          <button className="rail-toggle" onClick={() => setRailCompact((v) => !v)}
            aria-expanded={!railCompact}
            aria-label={railCompact ? "Expand navigation" : "Collapse navigation"}>
            <Icon name={railCompact ? "chevronsRight" : "chevronsLeft"} size={15} />
          </button>
          <div className="rail-list">
            {SECTIONS.map((s) => (
              <button key={s.id} className="rail-item" onClick={goto(s.id)}
                aria-current={section === s.id ? "page" : undefined}
                aria-label={`${s.label}. ${s.blurb}`}
                title={`${s.label} (alt ${s.key})`}>
                <span className="rail-icon"><Icon name={s.icon} size={18} /></span>
                <span className="rail-label">
                  <span>{s.label}</span>
                  <small>{s.blurb}</small>
                </span>
                {s.id === "disc" && optical.length > 0 && <span className="rail-dot" aria-hidden="true" />}
              </button>
            ))}
          </div>
          <div className="rail-foot">
            <span className="rail-kv"><span>version</span><b>v{snap?.version ?? "…"}</b></span>
            <span className="rail-kv"><span>keys</span><b>{keys?.exists ? keys.entries.toLocaleString() : "none"}</b></span>
            <span className="rail-kv"><span>mcp</span><b>{mcp?.port ? `:${mcp.port}` : "off"}</b></span>
          </div>
        </nav>

        <main className="workspace" id="main" tabIndex={-1}>
          <div className="page" key={section}>
            <header className="page-head">
              <div className="page-head-text">
                <p className="eyebrow">{page.eyebrow}</p>
                <h1>{page.title}</h1>
                <p className="lede">{page.lede}</p>
              </div>
              <button className="ghost sm" onClick={() => setPaletteOpen(true)}>
                <Icon name="search" size={14} /> Commands
              </button>
            </header>

            {error && (
              <div className="err" role="alert">
                <span aria-hidden="true">⚠</span>
                <span className="grow">{error}</span>
                <button className="ghost sm" onClick={() => setError(null)}>Dismiss</button>
              </div>
            )}

            {section === "disc" && (
              <>
                <section className="card">
                  <div className="panel-head">
                    <div>
                      <h2>Drive</h2>
                      <p className="sub">
                        {optical.length > 0
                          ? `${optical.length} optical device${optical.length > 1 ? "s" : ""} online`
                          : "Waiting for an optical drive"}
                      </p>
                    </div>
                    <div className="row" style={{ gap: 7 }}>
                      <button className="ghost sm" disabled={!discPath}
                        title="Check index, MovieObject, playlists and clip files"
                        onClick={() => void verifyTree(discPath)}>
                        Verify
                      </button>
                      <button className="ghost sm" disabled={optical.length === 0}
                        onClick={async () => { try { setToast(await api.eject()); } catch (e) { setError(String(e)); } }}>
                        <Icon name="eject" size={14} /> Eject
                      </button>
                      <button className="ghost sm" onClick={refresh}>
                        <Icon name="refresh" size={14} /> Rescan
                      </button>
                    </div>
                  </div>

                  {optical.length === 0 ? (
                    <div className="empty-state">
                      <span className="empty-ico"><Icon name="disc" size={24} /></span>
                      <div>
                        <b>No optical drive detected</b>
                        <p className="hint">
                          Attach a Blu-ray drive, ideally straight to a USB port rather than a hub,
                          then rescan. You can also open an ISO or a folder below.
                        </p>
                      </div>
                    </div>
                  ) : (
                    <div className="drive-list">
                      {optical.map((d) => {
                        const target = d.device_path ?? d.mount_point ?? "";
                        const on = discPath === target || discPath === d.mount_point || discPath === `/dev/r${d.bsd_name}`;
                        return (
                          <button key={d.bsd_name} className={`drive ${on ? "on" : ""}`}
                            onClick={() => setDiscPath(target)} aria-pressed={on}>
                            <span className="drive-ico"><Icon name="disc" size={20} /></span>
                            <span className="grow">
                              <b>{d.vendor || d.product || "Optical drive"}</b>
                              <span className="sub mono">
                                {d.bsd_name}{d.interconnect ? ` · ${d.interconnect}` : ""}
                              </span>
                            </span>
                            <span className="chip">{on ? "selected" : "use"}</span>
                          </button>
                        );
                      })}
                    </div>
                  )}

                  <div className="path-row">
                    <input className="grow" value={discPath}
                      onChange={(e) => setDiscPath(e.target.value)}
                      placeholder="/Volumes/DISC_NAME or an .iso path"
                      aria-label="Disc path" />
                    <button className="primary" onClick={() => void openDisc(discPath)}
                      disabled={!discPath || !!busy}>
                      {busy === "reading disc" ? <><span className="spin" /> Reading</> : "Read disc"}
                    </button>
                  </div>

                  {disc && (
                    <div className="chip-row" style={{ marginTop: 14 }}>
                      <span className="pill good"><span className="dot" />{disc.name || "Blu-ray"}</span>
                      <span className="pill">libbluray {disc.library}</span>
                      <span className={`pill ${disc.aacs ? (disc.aacs_ready ? "warn" : "bad") : "good"}`}>
                        <span className="dot" />
                        {disc.aacs
                          ? (disc.aacs_ready ? "AACS, decryptable" : `AACS error ${disc.aacs_error}`)
                          : "no AACS"}
                      </span>
                      <span className="pill">{disc.titles.length} titles</span>
                    </div>
                  )}
                </section>

                {!disc && !dvd && (
                  <section className="card">
                    <div className="panel-head">
                      <div>
                        <h2>Start here</h2>
                        <p className="sub">Three steps. No wizard, no account.</p>
                      </div>
                    </div>
                    <div className="steps">
                      <div className="step">
                        <span className="step-n">01</span>
                        <b>Read</b>
                        <p>Point at the drive, an ISO or a BDMV folder. Titles, playlists and tracks appear here.</p>
                      </div>
                      <div className="step">
                        <span className="step-n">02</span>
                        <b>Choose</b>
                        <p>Pick the main feature, a TV plan or a single title, then set remux or encode.</p>
                      </div>
                      <div className="step">
                        <span className="step-n">03</span>
                        <b>Write</b>
                        <p>Queue the rip, then author a disc or export a decrypted ISO from the Author page.</p>
                      </div>
                    </div>
                  </section>
                )}

                {disc && (
                  <section className="card">
                    <div className="panel-head">
                      <div>
                        <h2>Titles</h2>
                        <p className="sub">The main feature is preselected. Add any title to the queue.</p>
                      </div>
                      <button className="ghost sm" disabled={!!runningJob}
                        title="Queue every title of at least one minute"
                        onClick={() => void enqueueAll()}>
                        <Icon name="layers" size={14} /> Queue all
                      </button>
                    </div>

                    <div className="titles">
                      {disc.titles.map((t: Title) => (
                        <div key={t.index} className={`title ${title === t.index ? "on" : ""}`}
                          role="button" tabIndex={0} aria-pressed={title === t.index}
                          onClick={() => setTitle(t.index)}
                          onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setTitle(t.index); } }}>
                          <span className="idx">#{t.index}</span>
                          <div className="grow">
                            <div className="row wrap" style={{ gap: 7 }}>
                              <b>Playlist {String(t.playlist).padStart(5, "0")}</b>
                              {t.is_main && <span className="badge warm">main</span>}
                              {t.dynamic_range && <span className="badge hdr">{t.dynamic_range}</span>}
                              {queue.some((j) => j.title === t.index && (j.state === "queued" || j.state === "running")) && (
                                <span className="badge">queued</span>
                              )}
                            </div>
                            <div className="meta">
                              {fmtDuration(t.duration_secs)} · {t.resolution ?? "?"} · {t.chapters} chapters · audio {t.audio_streams}
                            </div>
                          </div>
                          <button className="ghost sm" title="Add this title to the queue" aria-label={`Queue title ${t.index}`}
                            onClick={(e) => { e.stopPropagation(); void enqueueRip(t.index); }}>
                            +
                          </button>
                        </div>
                      ))}
                    </div>

                    {title != null && streams.length > 0 && (
                      <div style={{ marginTop: 16 }}>
                        <div className="sub" style={{ marginBottom: 8 }}>Track list for title #{title}</div>
                        <div style={{ display: "grid", gap: 5 }}>
                          {streams.map((s, i) => (
                            <div key={i} className="row" style={{ gap: 9, fontSize: 12 }}>
                              <span className="badge">{s.kind}</span>
                              <span className="mono" style={{ width: 38 }}>{s.lang ?? "und"}</span>
                              <span>{streamCodec(s)}</span>
                              <span className="sub mono">pid {s.pid}</span>
                            </div>
                          ))}
                        </div>
                      </div>
                    )}
                  </section>
                )}

                {dvd && (
                  <section className="card">
                    <div className="panel-head">
                      <div>
                        <h2>DVD-Video</h2>
                        <p className="sub">Copied as-is: MPEG-2 video, AC-3 and DTS audio. No re-encode.</p>
                      </div>
                      <span className="pill">{dvd.label}</span>
                    </div>
                    <div className="titles" style={{ maxHeight: 280 }}>
                      {dvd.titles.map((t) => (
                        <div key={t.number} className={`title ${dvdTitle === t.number ? "on" : ""}`}
                          role="button" tabIndex={0} aria-pressed={dvdTitle === t.number}
                          onClick={() => setDvdTitle(t.number)}
                          onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setDvdTitle(t.number); } }}>
                          <span className="idx">#{t.number}</span>
                          <div className="grow">
                            <div className="row wrap" style={{ gap: 8 }}>
                              <b>Title {t.number}</b>
                              <span className="badge">VTS {t.title_set}</span>
                              {t.angles > 1 && <span className="badge">{t.angles} angles</span>}
                            </div>
                            <div className="meta">
                              {fmtDuration(t.duration_secs ?? 0)} · {t.chapters} chapters
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                    <div className="row wrap" style={{ marginTop: 14, gap: 8 }}>
                      <button className="primary" disabled={dvdTitle == null || !!dvdProgress || !!runningJob}
                        onClick={() => void ripDvd()}>
                        {dvdProgress ? "Ripping…" : `Rip title ${dvdTitle ?? ""}`}
                      </button>
                      {dvdProgress && (
                        <button className="ghost" onClick={() => void api.cancelRip()}>Cancel</button>
                      )}
                    </div>
                    {dvdProgress && (
                      <div style={{ marginTop: 12 }}>
                        <div className="progress live" role="progressbar" aria-label="DVD rip progress"
                          aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(dvdProgress.percent)}>
                          <i style={{ width: `${dvdProgress.percent}%` }} />
                        </div>
                        <div className="stats" style={{ marginTop: 8 }} aria-live="polite">
                          <span>{dvdProgress.percent.toFixed(0)}%</span>
                          <span>{dvdProgress.message}</span>
                        </div>
                      </div>
                    )}
                  </section>
                )}
              </>
            )}

            {section === "output" && (
              <section className="card">
                <div className="panel-head">
                  <div>
                    <h2>Method</h2>
                    <p className="sub">Remux keeps the original streams. Encode rewrites the video to save space.</p>
                  </div>
                </div>

                <div className="modes">
                  <button type="button" className={`mode ${lossless ? "on" : ""}`}
                    aria-pressed={lossless} onClick={() => setLossless(true)}>
                    <span className="t">Remux · lossless</span>
                    <span className="d">Bit-for-bit copy of the original streams. Fast and safe.</span>
                  </button>
                  <button type="button" className={`mode ${!lossless ? "on" : ""}`}
                    aria-pressed={!lossless} onClick={() => setLossless(false)}>
                    <span className="t">Encode</span>
                    <span className="d">Re-encode with CPU x264/x265 or the GPU encoder.</span>
                  </button>
                </div>

                <div className="row" style={{ marginTop: 16 }}>
                  <span className="sub">Container</span>
                  <div className="seg" role="group" aria-label="Container">
                    <button aria-pressed={container === "mkv"} className={container === "mkv" ? "on" : ""}
                      onClick={() => setContainer("mkv")}>MKV</button>
                    <button aria-pressed={container === "mp4"} className={container === "mp4" ? "on" : ""}
                      onClick={() => setContainer("mp4")}>MP4</button>
                  </div>
                </div>

                {!lossless && (
                  <>
                    <div className="grid3" style={{ marginTop: 16 }}>
                      <label className="field">Encoder
                        <select value={enc.codec}
                          onChange={(e) => setEnc({ ...enc, codec: e.target.value as EncodeSettings["codec"] })}>
                          {CODECS.map((c) => <option key={c.id} value={c.id}>{c.label}</option>)}
                        </select>
                      </label>
                      <label className="field">Quality (CRF / q)
                        <input type="range" min={12} max={32} value={enc.quality}
                          onChange={(e) => setEnc({ ...enc, quality: +e.target.value })} />
                        <span className="sub mono">{enc.quality}</span>
                      </label>
                      <label className="field">Preset
                        <select value={enc.preset ?? "medium"}
                          onChange={(e) => setEnc({ ...enc, preset: e.target.value })}
                          disabled={enc.codec.includes("videotoolbox")}>
                          {["ultrafast", "fast", "medium", "slow", "slower"].map((p) => (
                            <option key={p} value={p}>{p}</option>
                          ))}
                        </select>
                      </label>
                    </div>
                    <div className="row wrap" style={{ marginTop: 12, gap: 18 }}>
                      <label className="check">
                        <input type="checkbox" checked={enc.ten_bit} disabled={enc.codec.includes("videotoolbox")}
                          onChange={(e) => setEnc({ ...enc, ten_bit: e.target.checked })} />
                        10-bit
                      </label>
                      <label className="check">
                        <input type="checkbox" checked={enc.aac_audio}
                          onChange={(e) => setEnc({ ...enc, aac_audio: e.target.checked })} />
                        Re-encode audio to AAC
                      </label>
                      <label className="check" title="Notch the Verance watermark band and re-encode the audio">
                        <input type="checkbox" checked={enc.cinavia}
                          onChange={(e) => setEnc({ ...enc, cinavia: e.target.checked })} />
                        Strip Cinavia
                      </label>
                      {enc.cinavia && (
                        <label className="field" style={{ minWidth: 170 }}>Stripped audio
                          <select value={enc.cinavia_codec}
                            onChange={(e) => setEnc({ ...enc, cinavia_codec: e.target.value })}>
                            <option value="auto">Same as source</option>
                            <option value="ac3">Dolby Digital</option>
                            <option value="eac3">Dolby Digital Plus</option>
                            <option value="dts">DTS</option>
                            <option value="truehd">Dolby TrueHD</option>
                            <option value="atmos">Dolby Atmos (E-AC-3)</option>
                            <option value="flac">FLAC (lossless)</option>
                            <option value="aac">AAC</option>
                          </select>
                        </label>
                      )}
                    </div>
                  </>
                )}

                <div className="row wrap" style={{ marginTop: 18, gap: 14 }}>
                  <label className="field" style={{ minWidth: 190 }}>Audio tracks
                    <select value={audio} onChange={(e) => setAudio(e.target.value as AudioSel)}>
                      <option value="all">All tracks</option>
                      <option value="main">Default track only</option>
                      <option value="english">English only</option>
                    </select>
                  </label>
                  <label className="field" style={{ minWidth: 190 }}>Subtitles
                    <select value={subs} onChange={(e) => setSubs(e.target.value as SubSel)}>
                      <option value="all">All</option>
                      <option value="english">English only</option>
                      <option value="none">None</option>
                    </select>
                  </label>
                </div>

                <div className="row wrap" style={{ marginTop: 14, gap: 18 }}>
                  <label className="check">
                    <input type="checkbox" checked={chapters} onChange={(e) => setChapters(e.target.checked)} />
                    Chapter names from ChapterDB
                  </label>
                  <label className="check">
                    <input type="checkbox" checked={poster} onChange={(e) => setPoster(e.target.checked)} />
                    TMDB poster
                  </label>
                  <label className="check">
                    <input type="checkbox" checked={covers} onChange={(e) => setCovers(e.target.checked)} />
                    Cover art beside the rip
                  </label>
                </div>

                <label className="field" style={{ marginTop: 16 }}>Title used for lookups
                  <input value={movie ?? ""} placeholder={disc?.name ?? "movie title"}
                    onChange={(e) => setMovie(e.target.value)} />
                </label>

                <div className="field" style={{ marginTop: 14 }}>
                  <span className="sub">Output file</span>
                  <div className="mono truncate" title={preview} style={{ marginTop: 6, color: "var(--text-dim)" }}>
                    {preview || "Read a disc to preview the output path"}
                  </div>
                </div>
              </section>
            )}

            {section === "episodes" && (
              <section className="card">
                <div className="panel-head">
                  <div>
                    <h2>Episode plan</h2>
                    <p className="sub">One file per episode, SxxExx and title from TMDB.</p>
                  </div>
                  <button onClick={() => void detectEpisodes()} disabled={tvBusy}>
                    {tvBusy ? <><span className="spin" /> Detecting</> : "Detect episodes"}
                  </button>
                </div>

                {tvPlan && (
                  <div className="row wrap" style={{ gap: 8, marginBottom: 10 }}>
                    <span className="grow mono truncate">
                      {tvPlan.series} · season {tvPlan.season}
                      {tvPlan.disc ? ` · disc ${tvPlan.disc}` : ""} · {tvPlan.items.length} episodes
                    </span>
                    <button className="ghost sm"
                      onClick={() => setTvSel(Object.fromEntries(tvPlan.items.map((i) => [i.episode.number, true])))}>All</button>
                    <button className="ghost sm" onClick={() => setTvSel({})}>None</button>
                  </div>
                )}

                {tvError && <p className="err" style={{ marginTop: 8, display: "block" }}>⚠ {tvError}</p>}

                {tvPlan ? (
                  <div className="eps" role="group" aria-label="Episodes to rip">
                    {tvPlan.items.map((it) => (
                      <label key={it.episode.number} className="ep">
                        <input type="checkbox" checked={!!tvSel[it.episode.number]}
                          onChange={(e) => setTvSel((s) => ({ ...s, [it.episode.number]: e.target.checked }))} />
                        <span className="mono">
                          S{String(it.episode.season).padStart(2, "0")}E{String(it.episode.number).padStart(2, "0")}
                        </span>
                        <span className="grow truncate">{it.episode.title}</span>
                        <span className="sub mono">{Math.floor(it.duration_secs / 60)}m</span>
                      </label>
                    ))}
                  </div>
                ) : (
                  <div className="empty-state">
                    <span className="empty-ico"><Icon name="layers" size={24} /></span>
                    <div>
                      <b>No plan yet</b>
                      <p className="hint">
                        Read a disc first, then detect episodes. Splitting follows the disc's own clip
                        boundaries, so episodes start where the disc starts them.
                      </p>
                    </div>
                  </div>
                )}

                <div className="row wrap" style={{ marginTop: 16, gap: 14, alignItems: "flex-end" }}>
                  <label className="field" style={{ maxWidth: 140 }}>Start episode
                    <input className="mono" type="number" min={1} value={startEp} placeholder="auto"
                      onChange={(e) => setStartEp(e.target.value === "" ? "" : Number(e.target.value))} />
                  </label>
                  <label className="field" style={{ flex: 1, minWidth: 240 }}>Subtitles on TV rips
                    <select value={cfg?.tv_subs ?? "all"}
                      onChange={(e) => setCfg((c) => (c ? { ...c, tv_subs: e.target.value } : c))}>
                      <option value="all">Keep every track, signs default</option>
                      <option value="signs">Signs and songs only</option>
                    </select>
                  </label>
                </div>
                <p className="hint" style={{ marginTop: 12 }}>
                  Subtitle default is the signs and songs track, probed by event count, with full
                  English when the disc has no English audio. Files land at the TV template on the
                  System page.
                </p>
              </section>
            )}

            {section === "author" && (
              <>
                <section className="card">
                  <div className="panel-head">
                    <div>
                      <h2>Author a disc</h2>
                      <p className="sub">BDMV or ISO from finished files</p>
                    </div>
                    <button className="ghost sm" onClick={() => void chooseAuthorFile()} disabled={authorBusy}>
                      <Icon name="image" size={14} /> Choose files
                    </button>
                  </div>

                  <div className="row" style={{ gap: 10 }}>
                    <span className="grow mono truncate" title={authorFiles.join(", ")}>
                      {authorFiles.length === 0
                        ? "No files selected"
                        : authorFiles.map((f) => f.split("/").pop()).join(", ")}
                    </span>
                  </div>

                  <div className="row wrap" style={{ marginTop: 14, gap: 12, alignItems: "flex-end" }}>
                    <label className="field" style={{ flex: 1, minWidth: 260 }}>Output
                      <input className="mono" value={authorOut} onChange={(e) => setAuthorOut(e.target.value)}
                        placeholder="…/disc.iso or …/disc" />
                    </label>
                    <label className="field" style={{ width: 180 }}>Split each disc at (GB)
                      <input className="mono" inputMode="decimal" value={authorSplit}
                        onChange={(e) => setAuthorSplit(e.target.value)} placeholder="off" />
                    </label>
                  </div>

                  <div className="row wrap" style={{ marginTop: 14, gap: 12, alignItems: "flex-end" }}>
                    <label className="check" style={{ paddingBottom: 8 }}>
                      <input type="checkbox" checked={authorMenu}
                        onChange={(e) => setAuthorMenu(e.target.checked)} />
                      <span>Add a title menu <span className="sub">BD-J, checked with libbluray</span></span>
                    </label>
                    <label className="field grow" style={{ minWidth: 240 }}>Menu background
                      <input className="mono" value={authorMenuBg} disabled={!authorMenu}
                        onChange={(e) => setAuthorMenuBg(e.target.value)}
                        placeholder="PNG image, 1920x1080" />
                    </label>
                    <button className="ghost" disabled={!authorMenu}
                      onClick={async () => {
                        const { open } = await import("@tauri-apps/plugin-dialog");
                        const picked = await open({ multiple: false, filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg"] }] });
                        if (typeof picked === "string") setAuthorMenuBg(picked);
                      }}>Choose image</button>
                  </div>

                  <div className="row between wrap" style={{ marginTop: 16, alignItems: "flex-end", gap: 12 }}>
                    <p className="hint" style={{ maxWidth: 460 }}>
                      One or several inputs write a UDF 2.50 ISO with the native tooling. Several
                      inputs become one title each, with generated index and navigation. Chapter marks
                      are kept. The optional title menu is real BD-J and checks out with libbluray;
                      hardware playback still needs a test.
                    </p>
                    <div className="row" style={{ gap: 8 }}>
                      <button className="ghost" disabled={!authorOut || authorBusy}
                        onClick={() => void verifyTree(authorOut)}>Verify output</button>
                      <button className="primary" disabled={authorFiles.length === 0 || !authorOut || authorBusy}
                        onClick={() => void startAuthor()}>
                        {authorBusy ? "Authoring…" : "Author disc"}
                      </button>
                    </div>
                  </div>

                  {authorProg && (
                    <div style={{ marginTop: 14 }}>
                      <div className="progress live" role="progressbar" aria-label="Authoring progress"
                        aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(authorProg.percent)}>
                        <i style={{ width: `${authorProg.percent}%` }} />
                      </div>
                      <div className="stats" style={{ marginTop: 8 }} aria-live="polite">
                        <span>{authorProg.percent.toFixed(0)}%</span>
                        <span>{authorProg.message}</span>
                      </div>
                    </div>
                  )}
                </section>

                <section className="card">
                  <div className="panel-head">
                    <div>
                      <h2>Master, clone and burn</h2>
                      <p className="sub">Export plays anywhere. Clone keeps encryption for archival.</p>
                    </div>
                  </div>

                  <div className="row wrap" style={{ gap: 8 }}>
                    <button className="ghost grow" disabled={!discPath || isoBusy || !!busy || !!runningJob || !!cloneProgress}
                      onClick={() => void exportIso("iso")}>Export ISO</button>
                    <button className="ghost grow" disabled={!discPath || isoBusy || !!busy || !!runningJob || !!cloneProgress}
                      onClick={() => void exportIso("folder")}>Export folder</button>
                    <button className="ghost grow" disabled={!discPath || !optical[0] || isoBusy || !!busy || !!runningJob || !!cloneProgress}
                      title="Bit-for-bit sector copy. Keeps AACS and BD+ (archival image)."
                      onClick={() => void cloneDisc()}>Clone 1:1</button>
                  </div>
                  <p className="hint" style={{ marginTop: 8 }}>
                    Export ISO and folder are decrypted and playable. Clone 1:1 copies the raw sectors
                    with encryption intact.
                  </p>

                  {cloneProgress && (
                    <div style={{ marginTop: 14 }}>
                      <div className="progress live" role="progressbar" aria-label="Clone progress"
                        aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(cloneProgress.percent)}>
                        <i style={{ width: `${cloneProgress.percent}%` }} />
                      </div>
                      <div className="stats" style={{ marginTop: 8 }} aria-live="polite">
                        <span>{cloneProgress.percent.toFixed(0)}%</span>
                        <span>{cloneProgress.message}</span>
                        <button className="ghost sm" onClick={() => void api.cancelRip()}>cancel</button>
                      </div>
                    </div>
                  )}
                  {isoProgress && (
                    <div style={{ marginTop: 14 }}>
                      <div className="progress live" role="progressbar" aria-label="ISO export progress"
                        aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(isoProgress.percent)}>
                        <i style={{ width: `${isoProgress.percent}%` }} />
                      </div>
                      <div className="stats" style={{ marginTop: 8 }} aria-live="polite">
                        <span>{isoProgress.percent.toFixed(0)}%</span>
                        <span>{isoProgress.message}</span>
                        {isoProgress.files > 0 && <span>{isoProgress.files} files</span>}
                      </div>
                      {isoProgress.stage === "done" && (
                        <button className="ghost sm" style={{ marginTop: 8 }} onClick={() => api.reveal(isoPath)}>Reveal ISO</button>
                      )}
                    </div>
                  )}

                  <div className="row between wrap" style={{ marginTop: 18, gap: 10 }}>
                    <div className="grow" style={{ minWidth: 220 }}>
                      <div className="sub">Target image</div>
                      <div className="mono truncate" title={isoPath}>{isoPath.split("/").pop() || "…"}</div>
                    </div>
                    <button className="ghost" disabled={!optical[0] || isoBusy || !!busy || !!runningJob || !!cloneProgress || !!burnProgress}
                      title={optical[0] ? "Write the exported ISO to the drive. This erases the disc." : "Connect a writer drive first"}
                      onClick={() => void burnIso()}>
                      <Icon name="flame" size={14} /> Burn ISO to disc
                    </button>
                  </div>
                  <p className="hint" style={{ marginTop: 8 }}>
                    Burning uses the native writer and picks the drive itself. It erases the disc and
                    cannot be undone.
                  </p>

                  {burnProgress && (
                    <div style={{ marginTop: 14 }}>
                      <div className="progress live" role="progressbar" aria-label="Burn progress"
                        aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(burnProgress.percent)}>
                        <i style={{ width: `${burnProgress.percent}%` }} />
                      </div>
                      <div className="stats" style={{ marginTop: 8 }} aria-live="polite">
                        <span>{burnProgress.percent.toFixed(0)}%</span>
                        <span>{burnProgress.message}</span>
                        <button className="ghost sm" onClick={() => void api.cancelRip()}>cancel</button>
                      </div>
                    </div>
                  )}
                </section>
              </>
            )}

            {section === "artwork" && (
              <section className="card">
                <div className="panel-head">
                  <div>
                    <h2>Artwork</h2>
                    <p className="sub">Pick each image, then save it beside the rip.</p>
                  </div>
                  <button className="ghost sm" onClick={autoFillAll} disabled={!tabs[coverTab].query.trim()}>
                    Auto pick
                  </button>
                </div>

                <div className="tabs">
                  {(["cover", "poster", "disc"] as CoverRole[]).map((r) => (
                    <button key={r} className={coverTab === r ? "on" : ""} onClick={() => setCoverTab(r)}>
                      {ROLE_LABEL[r]}
                      {tabs[r].selected && <span className="tick"> ✓</span>}
                    </button>
                  ))}
                  <div className="spacer" />
                </div>

                <div className="row wrap" style={{ marginTop: 12, gap: 8 }}>
                  <input className="grow" style={{ minWidth: 180 }} value={tabs[coverTab].query} placeholder="Movie title"
                    onChange={(e) => patchTab(coverTab, { query: e.target.value })}
                    onKeyDown={(e) => { if (e.key === "Enter") searchRole(coverTab); }} />
                  <select style={{ width: 170 }} value={tabs[coverTab].source}
                    onChange={(e) => patchTab(coverTab, { source: e.target.value })}>
                    <option value="all">All sources</option>
                    <option value="hirescovers">HiResCovers</option>
                    <option value="dvdcover">dvdcover.com</option>
                    <option value="tmdb">TMDB poster</option>
                  </select>
                  <button onClick={() => searchRole(coverTab)} disabled={tabs[coverTab].busy || !tabs[coverTab].query.trim()}>
                    {tabs[coverTab].busy ? <><span className="spin" /> Searching</> : "Search"}
                  </button>
                </div>

                {activeHits.length > 0 && (
                  <div className="covers">
                    {activeHits.map((h) => {
                      const sel = tabs[coverTab].selected;
                      const on = !!sel && sel.source === h.source && sel.id === h.id;
                      return (
                        <div key={`${h.source}-${h.id}`} className={`cover ${on ? "sel" : ""}`}
                          title={h.title} onClick={() => patchTab(coverTab, { selected: h })}
                          role="button" tabIndex={0} aria-pressed={on}
                          onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); patchTab(coverTab, { selected: h }); } }}>
                          <div className="thumb">
                            {thumbs[h.preview_url]
                              ? <img src={thumbs[h.preview_url]} alt={h.title} loading="lazy" />
                              : <span className="spin" />}
                          </div>
                          <div className="cmeta">
                            <div className="truncate">{h.title}</div>
                            <div className="sub">{h.source} · {h.format} · {h.kind}</div>
                          </div>
                          {on && <div className="pick">selected for {ROLE_LABEL[coverTab]}</div>}
                        </div>
                      );
                    })}
                  </div>
                )}

                {activeHits.length === 0 && !tabs[coverTab].busy && (
                  <div className="empty-state" style={{ marginTop: 14 }}>
                    <span className="empty-ico"><Icon name="image" size={24} /></span>
                    <div>
                      <b>{ROLE_LABEL[coverTab]}</b>
                      <p className="hint">
                        Search and click a result to choose it. HiResCovers originals need a member
                        cookie; without one the public preview is used.
                      </p>
                    </div>
                  </div>
                )}

                <div className="row between wrap" style={{ marginTop: 16, gap: 10 }}>
                  <span className="sub mono truncate" title={`${coverTarget.dir}/${coverTarget.stem}`}>
                    → {coverTarget.dir ? `${coverTarget.dir.split("/").pop()}/${coverTarget.stem}` : "…"}
                  </span>
                  <button className="primary" disabled={chosen.length === 0 || savingCovers} onClick={saveChosen}>
                    {savingCovers ? "Saving…" : `Save ${chosen.length || ""} ${chosen.length === 1 ? "file" : "files"}`.trim()}
                  </button>
                </div>
              </section>
            )}

            {section === "menus" && (
              <Menus source={discPath} busy={!!runningJob} onError={setError} onToast={setToast} />
            )}

            {section === "keys" && (
              <section className="card">
                <div className="panel-head">
                  <div>
                    <h2>Key store</h2>
                    <p className="sub">
                      {keys?.exists
                        ? `${keys.entries.toLocaleString()} entries · ${fmtBytes(keys.bytes)}`
                        : "KEYDB.cfg is not downloaded yet"}
                    </p>
                  </div>
                  <button className="ghost sm" disabled={busy === "keys"}
                    onClick={async () => {
                      setBusy("keys");
                      try {
                        await api.keysUpdate();
                        await refresh();
                        setKeyEpoch((n) => n + 1);
                        setToast("Keys updated");
                      } catch (e) { setError(String(e)); } finally { setBusy(null); }
                    }}>
                    {busy === "keys" ? <><span className="spin" /> Updating</> : "Update keys"}
                  </button>
                </div>

                <div className="row wrap" style={{ gap: 8 }}>
                  <button className="ghost" disabled={!discPath || !!busy || !!runningJob}
                    onClick={async () => {
                      setBusy("derive");
                      try {
                        const r = await api.keysNative(discPath || null);
                        await refresh();
                        setKeyEpoch((n) => n + 1);
                        setToast(`${r.added ? "Derived" : "Already keyed"}: ${r.key.name}`);
                      } catch (e) { setError(String(e)); } finally { setBusy(null); }
                    }}>
                    {busy === "derive" ? "Deriving…" : "Derive key for this disc"}
                  </button>
                  <button className="ghost" disabled={!!busy}
                    onClick={async () => {
                      setBusy("bdplus");
                      try { setToast(`BD+ VM installed at ${await api.bdplusInstall()}`); }
                      catch (e) { setError(String(e)); } finally { setBusy(null); }
                    }}>
                    {busy === "bdplus" ? "Installing…" : "Install BD+ VM"}
                  </button>
                </div>

                <p className="hint" style={{ marginTop: 12 }}>
                  Native: the VID comes from the drive firmware and the media key from the disc's own
                  MKB. No external ripper and no host certificate.
                </p>

                <div className="mini-grid" style={{ marginTop: 16 }}>
                  <div className="mini">
                    <span>disc key</span>
                    <b>{entry ? (entry.vuk ? "VUK present" : "VUK missing") : "no entry"}</b>
                  </div>
                  <div className="mini">
                    <span>unit keys</span>
                    <b>{entry ? entry.unit_keys.length : 0}</b>
                  </div>
                </div>

                {entry && (
                  <details className="group" style={{ marginTop: 14 }}>
                    <summary>UHD / AACS2 unit keys</summary>
                    <p className="hint" style={{ marginTop: 10 }}>
                      AACS2 discs decrypt with a unit-key table instead of a VUK alone. Paste one hex
                      key per line, in index order, then save.
                    </p>
                    <textarea className="mono" rows={4} value={unitsText}
                      onChange={(e) => setUnitsText(e.target.value)}
                      placeholder={"0x…\n0x…"}
                      style={{ marginTop: 8 }}
                      aria-label="Unit keys, one per line" />
                    <button className="ghost sm" style={{ marginTop: 8 }} disabled={unitsBusy}
                      onClick={async () => {
                        if (!discPath) return;
                        setUnitsBusy(true);
                        try {
                          const units = unitsText.split(/\s+/).map((k) => k.trim()).filter(Boolean);
                          const e = await api.keysSetUnits(discPath, units);
                          setEntry(e);
                          setUnitsText(e.unit_keys.join("\n"));
                          setToast(`saved ${e.unit_keys.length} unit key(s)`);
                        } catch (err) { setError(String(err)); } finally { setUnitsBusy(false); }
                      }}>
                      {unitsBusy ? "Saving…" : "Save unit keys"}
                    </button>
                  </details>
                )}

                <details className="group" style={{ marginTop: 14 }}>
                  <summary>Search and import</summary>
                  <div className="row" style={{ gap: 6, marginTop: 10 }}>
                    <input className="grow" value={keyQuery} placeholder="name or disc id"
                      onChange={(e) => setKeyQuery(e.target.value)}
                      onKeyDown={(e) => { if (e.key === "Enter") void searchKeys(); }} />
                    <button className="ghost sm" onClick={() => void searchKeys()} disabled={keyBusy}>
                      {keyBusy ? "…" : "Search"}
                    </button>
                    <button className="ghost sm" onClick={() => void importKeys()}>Import</button>
                  </div>
                  {keyHits.length > 0 ? (
                    <div style={{ display: "grid", gap: 6, marginTop: 10 }}>
                      {keyHits.map((e) => (
                        <div key={e.id} className="row" style={{ gap: 8 }}>
                          <span className="mono sub" style={{ width: 76 }} title={e.id}>{e.id.slice(0, 8)}…</span>
                          <span className="grow truncate" title={e.name ?? ""}>{e.name ?? ""}</span>
                          <span className="badge">{e.vuk ? "V" : ""}{e.unit_keys.length ? `U${e.unit_keys.length}` : ""}</span>
                          <button className="ghost sm" onClick={() => void removeKey(e.id)}>remove</button>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="sub" style={{ marginTop: 8 }}>
                      Search the database, first 20 shown, or import another KEYDB.cfg.
                    </div>
                  )}
                </details>
              </section>
            )}

            {section === "system" && (
              <>
                <section className="card">
                  <div className="panel-head">
                    <div>
                      <h2>Automation</h2>
                      <p className="sub">Rip on insertion, notify when a job ends, and keep the machine awake.</p>
                    </div>
                  </div>
                  <label className="row" style={{ gap: 8, alignItems: "center" }}>
                    <input type="checkbox" checked={!!cfg?.auto_rip}
                      onChange={(e) => cfg && void saveCfg({ auto_rip: e.target.checked })} />
                    <span>Auto-rip a disc when it is inserted</span>
                  </label>
                  <div className="row wrap" style={{ gap: 12, marginTop: 10 }}>
                    <label className="field" style={{ minWidth: 160 }}>Titles
                      <select value={cfg?.auto_rip_mode ?? "main"}
                        onChange={(e) => cfg && void saveCfg({ auto_rip_mode: e.target.value })}>
                        <option value="main">Main feature</option>
                        <option value="all">All long titles</option>
                      </select>
                    </label>
                    <label className="field" style={{ minWidth: 160 }}>Minimum length (s)
                      <input type="number" min={60} step={60} value={cfg?.auto_rip_min_secs ?? 600}
                        onChange={(e) => cfg && void saveCfg({ auto_rip_min_secs: Number(e.target.value) || 600 })} />
                    </label>
                  </div>
                  <label className="row" style={{ gap: 8, alignItems: "center", marginTop: 10 }}>
                    <input type="checkbox" checked={!!cfg?.auto_rip_eject}
                      onChange={(e) => cfg && void saveCfg({ auto_rip_eject: e.target.checked })} />
                    <span>Eject when the queue finishes</span>
                  </label>
                  <label className="row" style={{ gap: 8, alignItems: "center", marginTop: 6 }}>
                    <input type="checkbox" checked={!!cfg?.notify_native}
                      onChange={(e) => cfg && void saveCfg({ notify_native: e.target.checked })} />
                    <span>Desktop notification when a rip ends</span>
                  </label>
                  <label className="row" style={{ gap: 8, alignItems: "center", marginTop: 6 }}>
                    <input type="checkbox" checked={!!cfg?.prevent_sleep}
                      onChange={(e) => cfg && void saveCfg({ prevent_sleep: e.target.checked })} />
                    <span>Keep the machine awake while ripping</span>
                  </label>
                  <label className="row" style={{ gap: 8, alignItems: "center", marginTop: 6 }}>
                    <input type="checkbox" checked={!!cfg?.cinavia}
                      onChange={(e) => cfg && void saveCfg({ cinavia: e.target.checked })} />
                    <span>Strip Cinavia from automatic rips</span>
                  </label>
                  {cfg?.cinavia && (
                    <label className="field" style={{ maxWidth: 260, marginTop: 6 }}>Automatic strip audio
                      <select value={cfg.cinavia_codec ?? "auto"}
                        onChange={(e) => void saveCfg({ cinavia_codec: e.target.value })}>
                        <option value="auto">Same as source</option>
                        <option value="ac3">Dolby Digital</option>
                        <option value="eac3">Dolby Digital Plus</option>
                        <option value="dts">DTS</option>
                        <option value="truehd">Dolby TrueHD</option>
                        <option value="atmos">Dolby Atmos (E-AC-3)</option>
                        <option value="flac">FLAC (lossless)</option>
                        <option value="aac">AAC</option>
                      </select>
                    </label>
                  )}
                  <label className="field" style={{ marginTop: 10 }}>Notification webhook
                    <input className="mono" placeholder="https://hooks.slack.com/..." value={cfg?.notify_webhook ?? ""}
                      onChange={(e) => cfg && void saveCfg({ notify_webhook: e.target.value || null })} />
                  </label>
                  <p className="sub" style={{ marginTop: 8 }}>
                    The watcher also handles a folder: <code>keepsake-atelier watch</code> reads
                    <code> watch_dir</code> from the config.
                  </p>
                </section>

                <section className="card">
                  <div className="panel-head">
                    <div>
                      <h2>Output</h2>
                      <p className="sub">Where rips land and how they are named.</p>
                    </div>
                  </div>
                  <label className="field">Output folder
                    <input className="mono" value={cfg?.output_dir ?? ""}
                      onChange={(e) => cfg && setCfg({ ...cfg, output_dir: e.target.value })}
                      onBlur={async () => { if (cfg) { await api.setConfig(cfg); setToast("Saved"); } }} />
                  </label>

                  <details className="group" open>
                    <summary>Output templates</summary>
                    <label className="field" style={{ marginTop: 12 }}>Movie template
                      <input className="mono" value={cfg?.output_template ?? ""} placeholder="{title} ({year})"
                        onChange={(e) => cfg && setCfg({ ...cfg, output_template: e.target.value })}
                        onBlur={async () => { if (cfg) { await api.setConfig(cfg); setToast("Saved"); } }} />
                    </label>
                    <div className="row wrap" style={{ gap: 6, marginTop: 8 }}>
                      {placeholders.map(([k, d]) => (
                        <button key={k} className="ghost sm" title={d}
                          onClick={async () => {
                            if (!cfg) return;
                            const next = { ...cfg, output_template: cfg.output_template + "{" + k + "}" };
                            setCfg(next);
                            await api.setConfig(next);
                          }}>
                          {"{" + k + "}"}
                        </button>
                      ))}
                    </div>

                    <label className="field" style={{ marginTop: 14 }}>TV / anime template
                      <input className="mono" value={cfg?.tv_template ?? ""}
                        placeholder="{series}/Season {season:02}/{series} - S{season:02}E{episode:02} - {episode_title}"
                        onChange={(e) => cfg && setCfg({ ...cfg, tv_template: e.target.value })}
                        onBlur={async () => { if (cfg) { await api.setConfig(cfg); setToast("Saved"); } }} />
                    </label>
                    <div className="row wrap" style={{ gap: 6, marginTop: 8 }}>
                      {["{series}", "{season:02}", "{episode:02}", "{episode_title}", "{episode_title_clean}", "{disc}"].map((p) => (
                        <button key={p} className="ghost sm" title="Insert into the TV template"
                          onClick={() => cfg && setCfg({ ...cfg, tv_template: cfg.tv_template + p })}>{p}</button>
                      ))}
                    </div>
                  </details>

                  <label className="check" style={{ marginTop: 14 }}>
                    <input type="checkbox" checked={cfg?.auto_update_keys ?? true}
                      onChange={async (e) => { if (cfg) { const n = { ...cfg, auto_update_keys: e.target.checked }; setCfg(n); await api.setConfig(n); } }} />
                    Update keys on launch
                  </label>
                  <label className="check" style={{ marginTop: 9 }}>
                    <input type="checkbox" checked={cfg?.auto_check_updates ?? true}
                      onChange={async (e) => { if (cfg) { const n = { ...cfg, auto_check_updates: e.target.checked }; setCfg(n); await api.setConfig(n); } }} />
                    Check for updates on launch
                  </label>
                </section>

                <section className="card">
                  <div className="panel-head">
                    <div>
                      <h2>MCP server</h2>
                      <p className="sub">Let an agent drive the workshop over a local endpoint.</p>
                    </div>
                    <span className={`pill ${mcp?.port ? "good" : ""}`}>
                      <span className="dot" />{mcp?.port ? `port ${mcp.port}` : "off"}
                    </span>
                  </div>
                  <label className="check">
                    <input type="checkbox" checked={cfg?.mcp_enabled ?? false}
                      onChange={(e) => void saveCfg({ mcp_enabled: e.target.checked })} />
                    Enable the MCP endpoint
                  </label>
                  <label className="field" style={{ marginTop: 12 }}>Port
                    <input className="mono" type="number" min={1024} max={65535}
                      value={cfg?.mcp_port ?? 8848}
                      onChange={(e) => cfg && setCfg({ ...cfg, mcp_port: Number(e.target.value) || 8848 })}
                      onBlur={() => cfg && void saveCfg({ mcp_port: cfg.mcp_port })} />
                  </label>
                  <p className="hint" style={{ marginTop: 10 }}>
                    Localhost only. Endpoint <span className="mono">http://127.0.0.1:{cfg?.mcp_port ?? 8848}/mcp</span>.
                    Disc, TV plan, rip, ISO export, covers, keys, menus, authoring, verify and config
                    are exposed as tools.
                  </p>
                </section>

                <section className="card">
                  <div className="panel-head">
                    <div>
                      <h2>System check</h2>
                      <p className="sub">
                        {doctor ? `${checksOk} of ${checksTotal} tools present` : "Probing native tools…"}
                      </p>
                    </div>
                    <button className="ghost sm" onClick={async () => { try { setDoctor(await api.doctor()); } catch (e) { setError(String(e)); } }}>
                      <Icon name="refresh" size={14} /> Re-run
                    </button>
                  </div>
                  {doctor ? (
                    <div style={{ display: "grid", gap: 10 }}>
                      {doctor.capabilities.map((c) => (
                        <div key={c.name} className="row" style={{ gap: 10, alignItems: "flex-start" }}>
                          <span className={`pill ${c.ok ? "good" : "warn"}`} style={{ flex: "0 0 auto" }}>
                            <span className="dot" />{c.ok ? "ok" : "no"}
                          </span>
                          <div className="grow">
                            <div style={{ fontSize: 13 }}>{c.name}</div>
                            <div className="sub">{c.detail}</div>
                          </div>
                        </div>
                      ))}
                      <details className="group">
                        <summary>Installed tools</summary>
                        <div style={{ display: "grid", gap: 8, marginTop: 10 }}>
                          {doctor.checks.map((c) => (
                            <div key={c.name} style={{ fontSize: 12 }}>
                              <span className={`pill ${c.ok ? "good" : "warn"}`} style={{ marginRight: 7 }}>
                                <span className="dot" />{c.ok ? "ok" : "missing"}
                              </span>
                              <b>{c.name}</b> <span className="sub mono">{c.detail}</span>
                              {!c.ok && c.name === "tsMuxeR" && (
                                <button className="ghost sm" style={{ marginLeft: 7 }}
                                  onClick={async () => {
                                    try {
                                      const p = await api.tsmuxerInstall();
                                      setDoctor(await api.doctor());
                                      setToast(`tsMuxeR installed at ${p}`);
                                    } catch (e) { setError(String(e)); }
                                  }}>
                                  Install
                                </button>
                              )}
                              {!c.ok && c.fix && <div className="sub" style={{ marginLeft: 4 }}>fix: {c.fix}</div>}
                            </div>
                          ))}
                        </div>
                      </details>
                    </div>
                  ) : (
                    <div className="sub">checking…</div>
                  )}
                </section>

                <section className="card">
                  <div className="panel-head">
                    <div>
                      <h2>Support</h2>
                      <p className="sub">MIT licensed, no paid tier, no telemetry.</p>
                    </div>
                  </div>
                  <p className="sub">
                    Questions and logs are welcome in Discord. Ko-fi and ETH keep the mirrors and the
                    workshop running.
                  </p>
                  <div className="row wrap" style={{ gap: 8, marginTop: 14 }}>
                    <button className="ghost" onClick={() => void openUrl("https://discord.gg/SUaUrVGj5f").catch(() => {})}>
                      <Icon name="chat" size={14} /> Discord
                    </button>
                    <button className="ghost" onClick={() => void openUrl("https://ko-fi.com/ghosty99").catch(() => {})}>
                      <Icon name="heart" size={14} /> Ko-fi
                    </button>
                    <button className="ghost" onClick={() => void openUrl("https://etherscan.io/address/0xbBef115aD853236a350EaF04777a2AdB2e7c0317").catch(() => {})}>
                      <Icon name="key" size={14} /> ETH
                    </button>
                  </div>
                </section>

                <p className="hint">
                  KEYDB.cfg lives at <span className="mono">{keys?.path ?? "…"}</span> and refreshes on
                  launch when that setting is on.
                </p>
              </>
            )}
          </div>
        </main>

        <aside className="inspector" aria-label="Session">
          <section className="cta">
            <div className="cta-glow" aria-hidden="true" />
            <div className="row between" style={{ position: "relative" }}>
              <span className="eyebrow">session</span>
              <span className={`chip ${runningJob ? "warn" : discPath ? "live" : ""}`}>
                <span className="dot" />
                {runningJob ? "working" : discPath ? "ready" : "idle"}
              </span>
            </div>
            <div className="cta-main">
              <div className="grow">
                <h2>
                  {runningJob
                    ? `Ripping title ${runningJob.title ?? "main"}`
                    : disc
                      ? disc.name || "Blu-ray"
                      : "No disc loaded"}
                </h2>
                <p className="lede">
                  {runningJob
                    ? runningJob.message || `${runningJob.percent.toFixed(0)}% complete`
                    : disc
                      ? `${tvPlan ? `${Object.values(tvSel).filter(Boolean).length} episodes` : `Title ${title ?? "main"}`} · ${lossless ? "remux" : "encode"} · ${container.toUpperCase()}`
                      : discPath
                        ? "Drive selected. Read the disc to load its titles."
                        : "Read a disc or point the app at an ISO to begin."}
                </p>
              </div>
              {activePercent != null && (
                <div className="ring" style={{ "--p": activePercent } as CSSProperties}
                  role="img" aria-label={`${Math.round(activePercent)} percent complete`}>
                  <span>{Math.round(activePercent)}%</span>
                </div>
              )}
            </div>
            <div className="cta-actions">
              {runningJob ? (
                <button className="danger" onClick={() => void api.queueCancel(runningJob.id)}>
                  <span className="spin" /> Cancel the rip
                </button>
              ) : (
                <button className="primary" disabled={!discPath || (!tvPlan && title == null)}
                  onClick={() => void enqueueRip()}>
                  <Icon name="play" size={15} /> Start rip
                </button>
              )}
              <button className="ghost" disabled={!!runningJob || (!tvPlan && !disc)}
                onClick={() => void enqueueAll()}>
                <Icon name="layers" size={14} /> Queue all titles
              </button>
            </div>

            <div className="mini-grid" style={{ marginTop: 16, position: "relative" }}>
              <div className="mini"><span>mode</span><b>{lossless ? "remux" : "encode"}</b></div>
              <div className="mini"><span>container</span><b>{container}</b></div>
              <div className="mini"><span>titles</span><b>{disc?.titles.length ?? 0}</b></div>
              <div className="mini"><span>queued</span><b>{queue.filter((j) => j.state === "queued" || j.state === "running").length}</b></div>
            </div>
          </section>

          {progress && (
            <div className="insp-card">
              <div className="insp-title">
                <h3>Rip progress</h3>
                <span className="mono sub">{progress.percent.toFixed(1)}%</span>
              </div>
              <div className="progress live" role="progressbar" aria-label="Rip progress"
                aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(progress.percent)}>
                <i style={{ width: `${progress.percent}%` }} />
              </div>
              <div className="stats" style={{ marginTop: 9 }} aria-live="polite">
                <span>{progress.fps.toFixed(0)} fps</span>
                <span>{(progress.speed || "").trim()}</span>
              </div>
              {progress.done && (
                <button className="ghost sm" style={{ marginTop: 10 }} onClick={() => api.reveal(progress.dest ?? preview)}>
                  Reveal file
                </button>
              )}
            </div>
          )}

          {queue.length > 0 && (
            <div className="insp-card">
              <div className="insp-title">
                <h3>Queue</h3>
                <div className="row" style={{ gap: 5 }}>
                  {!runningJob && queue.some((j) => j.state === "queued") && (
                    <button className="ghost sm" onClick={async () => { await api.queueResume(); await refreshQueue(); }}>
                      Resume
                    </button>
                  )}
                  <button className="ghost sm" onClick={async () => { await api.queueClear(); await refreshQueue(); }}>
                    Clear
                  </button>
                </div>
              </div>
              {queue.map((j) => (
                <div key={j.id} className="queue-item">
                  <span className={`pill ${j.state === "error" ? "bad" : j.state === "done" ? "good" : j.state === "running" ? "warn" : ""}`}>
                    <span className="dot" />{j.state}
                  </span>
                  <div className="grow" style={{ minWidth: 0 }}>
                    <div className="sub truncate">title {j.title ?? "main"} · {j.percent.toFixed(0)}%</div>
                    <div className="sub truncate" title={j.message}>{j.message}</div>
                  </div>
                  {j.state === "running" || j.state === "queued" ? (
                    <button className="ghost sm" onClick={async () => { await api.queueCancel(j.id); await refreshQueue(); }}>
                      cancel
                    </button>
                  ) : (
                    <button className="ghost sm" onClick={async () => { await api.queueRemove(j.id); await refreshQueue(); }}>
                      remove
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}

          <div className="insp-card">
            <div className="insp-title"><h3>Next up</h3></div>
            <div className="mini-grid">
              <div className="mini"><span>audio</span><b>{audio}</b></div>
              <div className="mini"><span>subtitles</span><b>{subs}</b></div>
              <div className="mini"><span>chapters</span><b>{chapters ? "on" : "off"}</b></div>
              <div className="mini"><span>poster</span><b>{poster ? "on" : "off"}</b></div>
            </div>
            <button className="ghost sm" style={{ marginTop: 12, width: "100%" }} onClick={() => setSection("output")}>
              Edit rip settings
            </button>
          </div>
        </aside>
      </div>

      <footer className="statusbar">
        <span>keepsake atelier <b>v{snap?.version ?? "…"}</b></span>
        <span>·</span>
        <span>{optical.length > 0 ? <><b>{optical[0].bsd_name}</b> online</> : "no drive"}</span>
        <span className="spacer" />
        <span>keys <b>{keys?.exists ? keys.entries.toLocaleString() : "none"}</b></span>
        <span>·</span>
        <span className={doctor && checksOk === checksTotal ? "ok" : "warn"}>
          {doctor ? `${checksOk}/${checksTotal} tools` : "checking"}
        </span>
        <span>·</span>
        <span>mcp <b>{mcp?.port ? `:${mcp.port}` : "off"}</b></span>
        <span>·</span>
        <button className="status-link" onClick={() => setHelpOpen(true)} aria-label="Keyboard shortcuts">? shortcuts</button>
      </footer>

      {paletteOpen && (
        <div className="palette-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget) setPaletteOpen(false); }}>
          <div className="palette" role="dialog" aria-modal="true" aria-label="Command palette">
            <div className="palette-input">
              <Icon name="search" size={16} />
              <input autoFocus value={paletteQuery}
                onChange={(e) => { setPaletteQuery(e.target.value); setPaletteIndex(0); }}
                onKeyDown={(e) => {
                  if (e.key === "ArrowDown") {
                    e.preventDefault();
                    setPaletteIndex(Math.min(paletteActive + 1, paletteFiltered.length - 1));
                  } else if (e.key === "ArrowUp") {
                    e.preventDefault();
                    setPaletteIndex(Math.max(paletteActive - 1, 0));
                  } else if (e.key === "Enter") {
                    e.preventDefault();
                    runPalette(paletteActive);
                  }
                }}
                placeholder="Jump to a section or run an action"
                aria-label="Search commands" />
              <kbd>esc</kbd>
            </div>
            <div className="palette-list" role="listbox" aria-label="Commands">
              {paletteFiltered.map((item, i) => (
                <div key={item.id}>
                  {(i === 0 || item.group !== paletteFiltered[i - 1].group) && (
                    <div className="palette-group">{item.group}</div>
                  )}
                  <button className={`palette-item ${i === paletteActive ? "on" : ""}`}
                    role="option" aria-selected={i === paletteActive}
                    onMouseEnter={() => setPaletteIndex(i)}
                    onClick={() => runPalette(i)}>
                    <span className="palette-ico"><Icon name={item.icon} size={15} /></span>
                    <span className="grow">
                      <span className="palette-label">{item.label}</span>
                      <span className="palette-hint">{item.hint}</span>
                    </span>
                    {item.kbd && <kbd>{item.kbd}</kbd>}
                  </button>
                </div>
              ))}
              {paletteFiltered.length === 0 && (
                <p className="hint" style={{ padding: 16 }}>No commands match that.</p>
              )}
            </div>
          </div>
        </div>
      )}

      {helpOpen && (
        <div className="sheet-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget) setHelpOpen(false); }}>
          <div className="sheet" role="dialog" aria-modal="true" aria-label="Keyboard shortcuts">
            <div className="sheet-head">
              <div>
                <p className="eyebrow">reference</p>
                <h2>Keyboard shortcuts</h2>
              </div>
              <button className="ghost sm" onClick={() => setHelpOpen(false)}>Close</button>
            </div>
            <div className="kbd-grid">
              {[
                [IS_MAC ? "⌘ K" : "Ctrl K", "Open the command palette"],
                ["Alt 1 to 8", "Jump to a section"],
                ["Tab", "Move between controls"],
                ["Enter or Space", "Choose a title or artwork"],
                ["Esc", "Close a dialog"],
                ["?", "This sheet"],
              ].map(([key, what]) => (
                <div key={key} className="kbd-row">
                  <kbd>{key}</kbd>
                  <span>{what}</span>
                </div>
              ))}
            </div>
            <p className="hint" style={{ marginTop: 14 }}>
              Burning and authoring still ask for confirmation. Nothing starts from a stray key.
            </p>
          </div>
        </div>
      )}

      {toast && <div className="toast" role="status" aria-live="polite">{toast}</div>}
    </div>
  );
}

/* ---------- shell data and small presentational pieces ---------- */

type SectionId = "disc" | "output" | "episodes" | "author" | "artwork" | "menus" | "keys" | "system";

type PaletteItem = {
  id: string;
  label: string;
  hint: string;
  icon: string;
  kbd?: string;
  group: string;
  run: () => void;
};

const IS_MAC = typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");

const SECTIONS: { id: SectionId; label: string; blurb: string; icon: string; key: string }[] = [
  { id: "disc", label: "Disc", blurb: "Read the drive", icon: "disc", key: "1" },
  { id: "output", label: "Rip settings", blurb: "Remux or encode", icon: "sliders", key: "2" },
  { id: "episodes", label: "Episodes", blurb: "Season splitting", icon: "layers", key: "3" },
  { id: "author", label: "Author", blurb: "Build and burn", icon: "flame", key: "4" },
  { id: "artwork", label: "Artwork", blurb: "Covers and posters", icon: "image", key: "5" },
  { id: "menus", label: "Menus", blurb: "Templates", icon: "menu", key: "6" },
  { id: "keys", label: "Keys", blurb: "AACS and BD+", icon: "key", key: "7" },
  { id: "system", label: "System", blurb: "Paths and tools", icon: "system", key: "8" },
];

const PAGE_META: Record<SectionId, { eyebrow: string; title: string; lede: string }> = {
  disc: {
    eyebrow: "01 / source",
    title: "The disc",
    lede: "Read the drive, pick what plays, and check the tracks before anything is written.",
  },
  output: {
    eyebrow: "02 / output",
    title: "Rip settings",
    lede: "Choose a lossless remux or an encode, then name the file. These settings apply to the next job in the queue.",
  },
  episodes: {
    eyebrow: "03 / series",
    title: "Episodes",
    lede: "One file per episode, split on the disc's own clip boundaries and named from TMDB.",
  },
  author: {
    eyebrow: "04 / master",
    title: "Author and burn",
    lede: "Build a fresh BDMV or ISO, export a decrypted master, or write an image back to a disc.",
  },
  artwork: {
    eyebrow: "05 / artwork",
    title: "Artwork",
    lede: "Find a cover, a poster and a disc label, then save them beside the rip.",
  },
  menus: {
    eyebrow: "06 / menus",
    title: "Disc menus",
    lede: "Read the navigation table, rename titles and chapters, and reuse the result on another disc.",
  },
  keys: {
    eyebrow: "07 / keys",
    title: "Decryption keys",
    lede: "AACS and BD+ material, recovered natively from the drive and the disc. No external ripper.",
  },
  system: {
    eyebrow: "08 / system",
    title: "System",
    lede: "Where files go, which tools are present, and how the workshop is wired.",
  },
};

const ICON_PATHS: Record<string, string[]> = {
  disc: ["M21 12a9 9 0 1 1-9-9 9 9 0 0 1 9 9Z", "M15 12a3 3 0 1 1-3-3 3 3 0 0 1 3 3Z"],
  sliders: [
    "M4 21v-7", "M4 10V3", "M12 21v-9", "M12 8V3", "M20 21v-5", "M20 12V3",
    "M1 14h6", "M9 8h6", "M17 16h6",
  ],
  layers: ["M12 2 2 7l10 5 10-5-10-5Z", "M2 17l10 5 10-5", "M2 12l10 5 10-5"],
  flame: [
    "M12 22c4.4 0 7-2.8 7-6.6 0-4.4-3.7-6.4-4.8-10.4-1 3-2.9 4-3 6.2 0-1.2-.9-2.2-1.9-3.2C7.8 10.2 5 12.7 5 15.4 5 19.2 7.6 22 12 22Z",
  ],
  image: ["M3 3h18v18H3z", "M8.5 8.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3Z", "M21 15l-5-5L5 21"],
  menu: ["M4 6h16", "M4 12h16", "M4 18h10"],
  key: ["M15 7a4 4 0 1 1-3.2 6.4L4 21H2v-3l9.6-9.6A4 4 0 0 1 15 7Z", "M17 9h.01"],
  system: [
    "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z",
    "M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.9.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.9.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.9V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z",
  ],
  search: ["M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16Z", "m21 21-4.3-4.3"],
  refresh: ["M21 12a9 9 0 1 1-3-6.7", "M21 3v6h-6"],
  eject: ["M5 17h14", "M12 4 5 12h14L12 4Z"],
  play: ["m7 4 13 8-13 8V4Z"],
  copy: ["M9 9h11v11H9z", "M5 15H4V4h11v1"],
  chat: ["M21 15a2 2 0 0 1-2 2H8l-5 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2Z"],
  heart: ["M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1-1.1a5.5 5.5 0 0 0-7.8 7.8L12 21l8.8-8.6a5.5 5.5 0 0 0 0-7.8Z"],
  chevronsLeft: ["m15 18-6-6 6-6"],
  chevronsRight: ["m9 18 6-6-6-6"],
};

function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || target.isContentEditable;
}

function Icon({ name, size = 16 }: { name: string; size?: number }) {
  const paths = ICON_PATHS[name] ?? ICON_PATHS.disc;
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round"
      aria-hidden="true" focusable="false">
      {paths.map((d, i) => <path key={i} d={d} />)}
    </svg>
  );
}
