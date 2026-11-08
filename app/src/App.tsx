import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type AppConfig,
  type DiscInfo,
  type EncodeSettings,
  type AudioSel,
  type CoverHit,
  type IsoProgress,
  type MovieHit,
  type RipMode,
  type RipProgress,
  type Snapshot,
  type SubSel,
  type Title,
} from "./api";

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
function sanitize(s: string): string {
  return s.replace(/[\/\\:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim() || "disc";
}

export default function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [disc, setDisc] = useState<DiscInfo | null>(null);
  const [discPath, setDiscPath] = useState("");
  const [title, setTitle] = useState<number | null>(null);
  const [container, setContainer] = useState<"mkv" | "mp4">("mkv");
  const [lossless, setLossless] = useState(true);
  const [enc, setEnc] = useState<EncodeSettings>({
    codec: "x265", quality: 20, preset: "medium", ten_bit: true, aac_audio: false,
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
  const [startEp, setStartEp] = useState<number | "">("");
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
    })();
    const un = api.onProgress((p) => {
      setProgress(p);
      if (p.done) { setBusy(null); setToast("Rip complete"); }
    });
    const unErr = api.onRipError((msg) => {
      setBusy(null);
      setError(msg);
    });
    return () => { un.then((f) => f()); unErr.then((f) => f()); };
  }, [refresh]);

  const openDisc = async (p: string) => {
    setError(null); setBusy("reading disc");
    try {
      const info = await api.openDisc(p);
      setDisc(info);
      setTitle(info.main_title ?? info.titles[0]?.index ?? null);
      setMovie(info.name || null);
      const q = info.name || "";
      setTabs((t) => ({
        cover: { ...t.cover, query: q },
        poster: { ...t.poster, query: q },
        disc: { ...t.disc, query: q },
      }));
    } catch (e) { setDisc(null); setError(String(e)); }
    finally { setBusy(null); }
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

  const startRip = async () => {
    // A TV rip does not need a title: the episode plan picks the title.
    if (!discPath || (!tvPlan && title == null)) return;
    // Decryption must read the raw device; a mounted optical volume returns
    // still-encrypted bytes.
    const dev = optical[0];
    const onOptical = !!dev && (discPath === dev.mount_point || discPath === `/dev/r${dev.bsd_name}`);
    const src = onOptical ? `/dev/r${dev.bsd_name}` : discPath;
    setError(null); setBusy("ripping"); setProgress({ out_time_ms: 0, total_ms: 0, frames: 0, fps: 0, speed: "", done: false, percent: 0, message: "starting", dest: null });
    try {
      await api.startRip({
        source: src,
        keydb: snap?.keys?.path ?? null,
        title: title ?? 0,
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
        episodes: tvPlan ? Object.entries(tvSel).filter(([, v]) => v).map(([k]) => k).join(",") : null,
        start_episode: tvPlan && startEp !== "" ? Number(startEp) : null,
      });
    } catch (e) { setError(String(e)); setBusy(null); }
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

  const cancel = async () => { try { await api.cancelRip(); } catch { /* ignore */ } setBusy(null); };

  useEffect(() => {
    const us = [
      api.onIsoProgress((p) => setIsoProgress(p)),
      api.onIsoDone(() => { setIsoBusy(false); setToast("Decrypted ISO written"); }),
      api.onIsoError((e) => { setIsoBusy(false); setError(e); }),
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

  const keys = snap?.keys;
  const optical = snap?.optical ?? [];

  return (
    <div className="app">
      <header className="header">
        <div className="brand"><span className="logo">◲</span> BluForge</div>
        <span className="ver">v{snap?.version ?? "…"}</span>
        <div className="spacer" />
        {update && <span className="pill warn"><span className="dot" /> update {update}</span>}
        <button className="ghost sm" onClick={async () => { try { await api.installUpdate(); } catch { /* ignore */ } }}>Check updates</button>
      </header>

      <div className="body">
        <main className="main">
          {/* Drive / disc */}
          <section className="card">
            <div className="row between">
              <div>
                <h2>Drive</h2>
                {optical.length === 0
                  ? <div className="sub">No optical drive detected</div>
                  : optical.map((d) => (
                      <div key={d.bsd_name} className="row" style={{ gap: 8, alignItems: "center" }}>
                        <span className="sub">
                          {d.vendor || d.product || "Optical drive"} · {d.bsd_name}
                          {d.interconnect ? ` · ${d.interconnect}` : ""}
                        </span>
                        <button className="ghost sm" onClick={() => setDiscPath(d.device_path ?? d.mount_point ?? "")}>
                          use
                        </button>
                      </div>
                    ))}
              </div>
              <div className="row" style={{ gap: 6 }}>
                <button className="ghost sm" disabled={optical.length === 0}
                  onClick={async () => { try { setToast(await api.eject()); } catch (e) { setError(String(e)); } }}>
                  Eject
                </button>
                <button className="ghost sm" onClick={refresh}>Rescan</button>
              </div>
            </div>

            {optical.length === 0 && (
              <p className="hint" style={{ marginTop: 10 }}>
                Attach a Blu-ray drive (ideally directly to a USB port, not a hub) and press <b>Rescan</b>.
                You can also point BluForge at a folder or ISO below.
              </p>
            )}

            <div className="row" style={{ marginTop: 12 }}>
              <input className="grow" value={discPath} onChange={(e) => setDiscPath(e.target.value)} placeholder="/Volumes/DISC_NAME, or an .iso path" />
              <button onClick={() => openDisc(discPath)} disabled={!discPath || !!busy}>Read disc</button>
            </div>

            {disc && (
              <div className="row wrap" style={{ marginTop: 12, gap: 8 }}>
                <span className="pill good"><span className="dot" />{disc.name || "Blu-ray"}</span>
                <span className="pill">libbluray {disc.library}</span>
                <span className={`pill ${disc.aacs ? (disc.aacs_ready ? "warn" : "bad") : "good"}`}>
                  <span className="dot" />{disc.aacs ? (disc.aacs_ready ? "AACS (decryptable)" : `AACS error ${disc.aacs_error}`) : "no AACS"}
                </span>
                <span className="pill">{disc.titles.length} titles</span>
              </div>
            )}
          </section>

          {/* Titles */}
          {disc && (
            <section className="card">
              <h2>Titles</h2>
              <div className="sub" style={{ marginBottom: 10 }}>Select what to rip. The main feature is preselected.</div>
              <div className="titles">
                {disc.titles.map((t: Title) => (
                  <div key={t.index} className={`title ${title === t.index ? "on" : ""}`} onClick={() => setTitle(t.index)}>
                    <span className="idx">#{t.index}</span>
                    <div className="grow">
                      <div className="row" style={{ gap: 8 }}>
                        <b>Playlist {String(t.playlist).padStart(5, "0")}</b>
                        {t.is_main && <span className="badge">main</span>}
                        {t.dynamic_range && <span className="badge hdr">{t.dynamic_range}</span>}
                      </div>
                      <div className="meta">
                        {fmtDuration(t.duration_secs)} · {t.resolution ?? "?"} · {t.chapters} chapters · audio {t.audio_streams}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </section>
          )}

          {/* Output */}
          <section className="card">
            <h2>Output</h2>
            <div className="modes" style={{ marginTop: 10 }}>
              <button type="button" className={`mode ${lossless ? "on" : ""}`}
                aria-pressed={lossless} onClick={() => setLossless(true)}>
                <span className="t">Remux (lossless)</span>
                <span className="d">Bit-for-bit copy of the original streams. Fast.</span>
              </button>
              <button type="button" className={`mode ${!lossless ? "on" : ""}`}
                aria-pressed={!lossless} onClick={() => setLossless(false)}>
                <span className="t">Encode</span>
                <span className="d">Re-encode video (CPU x264/x265 or GPU) to shrink.</span>
              </button>
            </div>

            <div className="row" style={{ marginTop: 14 }}>
              <span className="sub">Container</span>
              <div className="seg" role="group" aria-label="Container">
                <button aria-pressed={container === "mkv"} className={container === "mkv" ? "on" : ""} onClick={() => setContainer("mkv")}>MKV</button>
                <button aria-pressed={container === "mp4"} className={container === "mp4" ? "on" : ""} onClick={() => setContainer("mp4")}>MP4</button>
              </div>
            </div>

            {!lossless && (
              <div className="grid3" style={{ marginTop: 14 }}>
                <label className="field">Encoder
                  <select value={enc.codec} onChange={(e) => setEnc({ ...enc, codec: e.target.value as EncodeSettings["codec"] })}>
                    {CODECS.map((c) => <option key={c.id} value={c.id}>{c.label}</option>)}
                  </select>
                </label>
                <label className="field">Quality (CRF / q)
                  <input type="range" min={12} max={32} value={enc.quality} onChange={(e) => setEnc({ ...enc, quality: +e.target.value })} />
                  <span className="sub">{enc.quality}</span>
                </label>
                <label className="field">Preset
                  <select value={enc.preset ?? "medium"} onChange={(e) => setEnc({ ...enc, preset: e.target.value })} disabled={enc.codec.includes("videotoolbox")}>
                    {["ultrafast", "fast", "medium", "slow", "slower"].map((p) => <option key={p} value={p}>{p}</option>)}
                  </select>
                </label>
              </div>
            )}
            {!lossless && (
              <div className="row wrap" style={{ marginTop: 10, gap: 16 }}>
                <label className="check"><input type="checkbox" checked={enc.ten_bit} disabled={enc.codec.includes("videotoolbox")} onChange={(e) => setEnc({ ...enc, ten_bit: e.target.checked })} /> 10-bit</label>
                <label className="check"><input type="checkbox" checked={enc.aac_audio} onChange={(e) => setEnc({ ...enc, aac_audio: e.target.checked })} /> Re-encode audio to AAC</label>
              </div>
            )}

            <div className="row wrap" style={{ marginTop: 12, gap: 16 }}>
              <label className="field">Audio tracks
                <select value={audio} onChange={(e) => setAudio(e.target.value as AudioSel)}>
                  <option value="all">All tracks</option>
                  <option value="main">Default track only</option>
                  <option value="english">English only</option>
                </select>
              </label>
              <label className="field">Subtitles
                <select value={subs} onChange={(e) => setSubs(e.target.value as SubSel)}>
                  <option value="all">All</option>
                  <option value="english">English only</option>
                  <option value="none">None</option>
                </select>
              </label>
            </div>

            <div className="row wrap" style={{ marginTop: 12, gap: 16 }}>
              <label className="check">
                <input type="checkbox" checked={chapters} onChange={(e) => setChapters(e.target.checked)} />
                Chapter names (ChapterDB)
              </label>
              <label className="check">
                <input type="checkbox" checked={poster} onChange={(e) => setPoster(e.target.checked)} />
                TMDB poster
              </label>
              <label className="check">
                <input type="checkbox" checked={covers} onChange={(e) => setCovers(e.target.checked)} />
                Cover art next to the rip
              </label>
            </div>

            <label className="field" style={{ marginTop: 12 }}>Movie title for lookups
              <input value={movie ?? ""} placeholder={disc?.name ?? "movie title"}
                onChange={(e) => setMovie(e.target.value)} />
            </label>

            <div className="field" style={{ marginTop: 12 }}>
              <span className="sub">Output file</span>
              <div className="mono truncate" title={preview}>{preview || "—"}</div>
            </div>
          </section>

          {/* TV / anime */}
          <section className="card">
            <div className="row between">
              <h2>TV / anime</h2>
              <span className="sub">one file per episode, SxxExx + title from TMDB</span>
            </div>
            <div className="row" style={{ marginTop: 10, gap: 8 }}>
              <button onClick={detectEpisodes} disabled={tvBusy}>
                {tvBusy ? "Detecting…" : "Detect episodes"}
              </button>
              {tvPlan && (
                <>
                  <span className="grow mono truncate">
                    {tvPlan.series} · season {tvPlan.season}
                    {tvPlan.disc ? ` · disc ${tvPlan.disc}` : ""} · {tvPlan.items.length} eps
                  </span>
                  <button className="ghost sm"
                    onClick={() => setTvSel(Object.fromEntries(tvPlan.items.map((i) => [i.episode.number, true])))}>All</button>
                  <button className="ghost sm" onClick={() => setTvSel({})}>None</button>
                </>
              )}
            </div>
            {tvError && <p className="err" style={{ marginTop: 8 }}>⚠ {tvError}</p>}
            {tvPlan && (
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
            )}
            <div className="row wrap" style={{ marginTop: 10, gap: 12, alignItems: "flex-end" }}>
              <label className="field" style={{ maxWidth: 130 }}>Start episode
                <input className="mono" type="number" min={1} value={startEp} placeholder="auto"
                  onChange={(e) => setStartEp(e.target.value === "" ? "" : Number(e.target.value))} />
              </label>
              <label className="field" style={{ flex: 1, minWidth: 220 }}>Subtitles on TV rips
                <select value={cfg?.tv_subs ?? "all"}
                  onChange={(e) => setCfg((c) => (c ? { ...c, tv_subs: e.target.value } : c))}>
                  <option value="all">Keep every track (signs default)</option>
                  <option value="signs">Signs &amp; songs only (English audio)</option>
                </select>
              </label>
            </div>
            <p className="hint" style={{ marginTop: 10 }}>
              Split on the disc's clip boundaries. Subtitle default is the signs &amp; songs track
              (probed by event count), full English if there is no English audio. Episodes land at
              the TV template below.
            </p>
          </section>

          {/* Covers */}
          <section className="card">
            <div className="row between">
              <h2>Covers</h2>
              <span className="sub">pick each image, then save beside the rip</span>
            </div>

            <div className="tabs" style={{ marginTop: 10 }}>
              {(["cover", "poster", "disc"] as CoverRole[]).map((r) => (
                <button key={r} className={coverTab === r ? "on" : ""} onClick={() => setCoverTab(r)}>
                  {ROLE_LABEL[r]}
                  {tabs[r].selected && <span className="tick"> ✓</span>}
                </button>
              ))}
              <div className="spacer" />
              <button className="ghost sm" onClick={autoFillAll} disabled={!tabs[coverTab].query.trim()}>
                Auto pick
              </button>
            </div>

            <div className="row" style={{ marginTop: 10, gap: 8 }}>
              <input className="grow" value={tabs[coverTab].query} placeholder="Movie title"
                onChange={(e) => patchTab(coverTab, { query: e.target.value })}
                onKeyDown={(e) => { if (e.key === "Enter") searchRole(coverTab); }} />
              <select value={tabs[coverTab].source}
                onChange={(e) => patchTab(coverTab, { source: e.target.value })}>
                <option value="all">All sources</option>
                <option value="hirescovers">HiResCovers</option>
                <option value="dvdcover">dvdcover.com</option>
                <option value="tmdb">TMDB poster</option>
              </select>
              <button onClick={() => searchRole(coverTab)} disabled={tabs[coverTab].busy || !tabs[coverTab].query.trim()}>
                {tabs[coverTab].busy ? "…" : "Search"}
              </button>
            </div>

            {activeHits.length > 0 && (
              <div className="covers">
                {activeHits.map((h) => {
                  const sel = tabs[coverTab].selected;
                  const on = !!sel && sel.source === h.source && sel.id === h.id;
                  return (
                    <div key={`${h.source}-${h.id}`} className={`cover ${on ? "sel" : ""}`}
                      title={h.title} onClick={() => patchTab(coverTab, { selected: h })}>
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
              <p className="hint" style={{ marginTop: 10 }}>
                {ROLE_LABEL[coverTab]}: search and click a result to choose it. HiResCovers
                originals need a member cookie; without one the public preview is used.
              </p>
            )}

            <div className="row between" style={{ marginTop: 12, gap: 8 }}>
              <span className="sub mono truncate" title={`${coverTarget.dir}/${coverTarget.stem}`}>
                → {coverTarget.dir ? `${coverTarget.dir.split("/").pop()}/${coverTarget.stem}` : "…"}
              </span>
              <button className="primary" disabled={chosen.length === 0 || savingCovers} onClick={saveChosen}>
                {savingCovers ? "Saving…" : `Save ${chosen.length || ""} ${chosen.length === 1 ? "file" : "files"}`.trim()}
              </button>
            </div>
          </section>

          {error && <div className="err" role="alert">⚠ {error}</div>}
        </main>

        {/* Side panel: keys, action, progress */}
        <aside className="side">
          <h1>Rip</h1>
          <p className="sub" style={{ marginTop: 0 }}>First iteration — ripping first.</p>

          <section className="card" style={{ marginTop: 16 }}>
            <div className="row between">
              <h2>Decryption keys</h2>
              <button className="ghost sm" disabled={busy === "keys"}
                onClick={async () => { setBusy("keys"); try { await api.keysUpdate(); await refresh(); setToast("Keys updated"); } catch (e) { setError(String(e)); } finally { setBusy(null); } }}>
                Update
              </button>
            </div>
            {keys?.exists
              ? <div className="sub">{keys.entries.toLocaleString()} entries · {fmtBytes(keys.bytes)}</div>
              : <div className="sub">Not downloaded yet</div>}
          </section>

          <section className="card" style={{ marginTop: 16 }}>
            <label className="field">Output folder
              <input className="mono" value={cfg?.output_dir ?? ""} onChange={(e) => cfg && setCfg({ ...cfg, output_dir: e.target.value })}
                onBlur={async () => { if (cfg) { await api.setConfig(cfg); setToast("Saved"); } }} />
            </label>
            <details className="group" open>
              <summary>Output templates</summary>
            <label className="field" style={{ marginTop: 10 }}>Output template
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

            <label className="field" style={{ marginTop: 12 }}>TV / anime template
              <input className="mono" value={cfg?.tv_template ?? ""}
                placeholder="{series}/Season {season:02}/{series} - S{season:02}E{episode:02} - {episode_title}"
                onChange={(e) => cfg && setCfg({ ...cfg, tv_template: e.target.value })}
                onBlur={async () => { if (cfg) { await api.setConfig(cfg); setToast("Saved"); } }} />
            </label>
            <div className="row wrap" style={{ gap: 6, marginTop: 8 }}>
              {["{series}", "{season:02}", "{episode:02}", "{episode_title}", "{episode_title_clean}", "{disc}"].map((p) => (
                <button key={p} className="ghost sm" title="insert into the TV template"
                  onClick={() => cfg && setCfg({ ...cfg, tv_template: cfg.tv_template + p })}>{p}</button>
              ))}
            </div>

            </details>

            <label className="check" style={{ marginTop: 10 }}>
              <input type="checkbox" checked={cfg?.auto_update_keys ?? true}
                onChange={async (e) => { if (cfg) { const n = { ...cfg, auto_update_keys: e.target.checked }; setCfg(n); await api.setConfig(n); } }} />
              Update keys on launch
            </label>
            <label className="check" style={{ marginTop: 8 }}>
              <input type="checkbox" checked={cfg?.auto_check_updates ?? true}
                onChange={async (e) => { if (cfg) { const n = { ...cfg, auto_check_updates: e.target.checked }; setCfg(n); await api.setConfig(n); } }} />
              Check for updates on launch
            </label>
          </section>

          <section className="card" style={{ marginTop: 16 }}>
            <div className="row between">
              <h2>MCP server</h2>
              <span className={`pill ${mcp?.port ? "good" : ""}`}>
                <span className="dot" />{mcp?.port ? `port ${mcp.port}` : "off"}
              </span>
            </div>
            <label className="check" style={{ marginTop: 10 }}>
              <input type="checkbox" checked={cfg?.mcp_enabled ?? false}
                onChange={(e) => void saveCfg({ mcp_enabled: e.target.checked })} />
              Let an agent drive BluForge over MCP
            </label>
            <label className="field" style={{ marginTop: 10 }}>Port
              <input className="mono" type="number" min={1024} max={65535}
                value={cfg?.mcp_port ?? 8848}
                onChange={(e) => cfg && setCfg({ ...cfg, mcp_port: Number(e.target.value) || 8848 })}
                onBlur={() => cfg && void saveCfg({ mcp_port: cfg.mcp_port })} />
            </label>
            <p className="hint" style={{ marginTop: 8 }}>
              Localhost only. Endpoint <span className="mono">http://127.0.0.1:{cfg?.mcp_port ?? 8848}/mcp</span> -
              disc, TV plan, rip, ISO export, covers, keys, eject and config as tools.
            </p>
          </section>

          <div style={{ marginTop: 18 }}>
            {!busy
              ? <button className="primary" style={{ width: "100%", justifyContent: "center" }} disabled={!discPath || (!tvPlan && title == null)} onClick={startRip}>
                  Start rip
                </button>
              : <button style={{ width: "100%", justifyContent: "center" }} onClick={cancel}>
                  <span className="spin" /> {busy}… (cancel)
                </button>}
          </div>

          {progress && (
            <div style={{ marginTop: 16 }}>
              <div className="progress" role="progressbar" aria-label="Rip progress"
                aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(progress.percent)}>
                <i style={{ width: `${progress.percent}%` }} />
              </div>
              <div className="stats" style={{ marginTop: 8 }} aria-live="polite">
                <span>{progress.percent.toFixed(1)}%</span>
                <span>{progress.fps.toFixed(0)} fps</span>
                <span>{(progress.speed || "").trim()}</span>
              </div>
              {progress.done && <button className="ghost sm" style={{ marginTop: 8 }} onClick={() => api.reveal(progress.dest ?? preview)}>Reveal file</button>}
            </div>
          )}

          <section className="panel" style={{ marginTop: 16 }}>
            <div className="row" style={{ justifyContent: "space-between", alignItems: "center" }}>
              <strong>Decrypted ISO</strong>
              <span className="hint">AACS/BD+ removed, plays anywhere</span>
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
              <button className="ghost" style={{ flex: 1, justifyContent: "center" }}
                disabled={!discPath || isoBusy || !!busy} onClick={() => exportIso("iso")}>
                Export .iso
              </button>
              <button className="ghost" style={{ flex: 1, justifyContent: "center" }}
                disabled={!discPath || isoBusy || !!busy} onClick={() => exportIso("folder")}>
                Export folder
              </button>
            </div>
            {isoProgress && (
              <div style={{ marginTop: 12 }}>
                <div className="progress" role="progressbar" aria-label="ISO export progress"
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
          </section>

          <p className="hint" style={{ marginTop: 18 }}>
            Keys: KEYDB.cfg is stored at <span className="mono">{keys?.path ?? "…"}</span> and refreshed automatically.
          </p>
        </aside>
      </div>

      {toast && <div className="toast" role="status" aria-live="polite">{toast}</div>}
    </div>
  );
}
