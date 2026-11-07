use clap::{Parser, Subcommand};

use bluforge_core::{device, keys, AppConfig};

#[derive(Parser)]
#[command(name = "bluforge", version, about = "BluForge — Blu-ray ripping & authoring")]
struct Cli {
    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List block devices and flag optical drives/discs.
    Devices {
        /// Include every block device, not just optical.
        #[arg(long)]
        all: bool,
    },
    /// Manage the AACS key database (KEYDB.cfg).
    Keys {
        #[command(subcommand)]
        action: KeysAction,
    },
    /// Inspect a Blu-ray disc, folder or ISO: titles, streams, AACS state.
    Disc {
        /// Path to a mounted disc, BDMV folder, or ISO.
        path: std::path::PathBuf,
        /// Also dump every title's streams (codec, language, pid).
        #[arg(long)]
        streams: bool,
        /// Also dump clip boundaries per title (episode splits on TV discs).
        #[arg(long)]
        clips: bool,
        /// Detect TV/anime episodes (clip boundaries + TMDB titles) and show the plan.
        #[arg(long)]
        episodes: bool,
        /// Override the series name used to look episodes up.
        #[arg(long)]
        series: Option<String>,
    },
    /// Rip a title to MKV/MP4 (remux by default, or `--encode`).
    Rip {
        /// Disc source: mounted volume, BDMV folder, ISO, or /dev/rdiskN.
        source: std::path::PathBuf,
        /// Output .mkv/.mp4 file, or a directory (anything without an
        /// extension, or ending in /). Defaults to the configured folder.
        output: Option<std::path::PathBuf>,
        /// Title index (see `bluforge disc`). Default: the main title.
        #[arg(long)]
        title: Option<u32>,
        /// Re-encode video instead of lossless remux.
        #[arg(long)]
        encode: bool,
        /// Encoder: x265 | x264 | hevc_videotoolbox | h264_videotoolbox.
        #[arg(long, default_value = "x265")]
        codec: String,
        /// Quality (CRF / VideoToolbox q).
        #[arg(long, default_value_t = 20)]
        quality: u32,
        /// x264/x265 preset.
        #[arg(long, default_value = "medium")]
        preset: String,
        /// Container: mkv | mp4.
        #[arg(long, default_value = "mkv")]
        container: String,
        /// 10-bit (software encoders only).
        #[arg(long)]
        ten_bit: bool,
        /// Don't add ChapterDB chapter names.
        #[arg(long)]
        no_chapters: bool,
        /// Don't fetch/save a TMDB poster.
        #[arg(long)]
        no_poster: bool,
        /// Movie title for the metadata lookups (default: the disc name).
        #[arg(long)]
        movie: Option<String>,
        /// Audio tracks to keep: all | main | english.
        #[arg(long, default_value = "all")]
        audio: String,
        /// Subtitle tracks to keep: all | english | none.
        #[arg(long, default_value = "all")]
        subs: String,
        /// Don't download cover art for the disc.
        #[arg(long)]
        no_covers: bool,
        /// Rip as a TV/anime disc: one file per episode, SxxExx named from TMDB.
        #[arg(long)]
        tv: bool,
        /// Series name for episode lookups (default: derived from the disc label).
        #[arg(long)]
        series: Option<String>,
        /// Only these episodes, eg. "1-3" or "1,2,5".
        #[arg(long)]
        episodes: Option<String>,
        /// TV subtitles: all (default) or signs (signs & songs only).
        #[arg(long, default_value = "all")]
        tv_subs: String,
    },
    /// Cover art: search and download case/disc covers.
    Covers {
        #[command(subcommand)]
        cmd: CoversCmd,
    },
    /// Eject the disc from the optical drive.
    Eject,
    /// Show linked library versions.
    Libs,
    /// Export a decrypted ISO (or `--folder`): all AACS/BD+ removed.
    Iso {
        /// Disc source: mounted volume, BDMV folder, ISO, or /dev/rdiskN.
        source: std::path::PathBuf,
        /// Output `.iso` file, or directory with `--folder`.
        dest: std::path::PathBuf,
        /// Write a plain decrypted folder instead of an image.
        #[arg(long)]
        folder: bool,
        /// Volume label (defaults to the disc name).
        #[arg(long)]
        label: Option<String>,
    },
    /// Show effective configuration and paths.
    Config,
}

#[derive(Subcommand)]
enum CoversCmd {
    /// List the available sources.
    Sources,
    /// Search for covers.
    Search {
        /// Title to search for.
        query: String,
        /// tmdb | dvdcover | hirescovers | all.
        #[arg(long, default_value = "all")]
        source: String,
        #[arg(long, default_value_t = 12)]
        limit: usize,
    },
    /// Download one cover by source and id (see `covers search`).
    Get {
        source: String,
        id: String,
        /// Output file, or a directory. Default: the current directory.
        #[arg(short, long)]
        out: Option<std::path::PathBuf>,
        /// Movie title, used to re-resolve the id.
        #[arg(long)]
        title: Option<String>,
        /// Save as a rip sidecar: cover | poster | disc (needs --stem).
        #[arg(long)]
        role: Option<String>,
        /// Rip file stem to name the sidecar after.
        #[arg(long)]
        stem: Option<String>,
    },
    /// Show what a cover file really is (original vs watermarked preview).
    Inspect {
        file: std::path::PathBuf,
    },
    /// Log in to HiResCovers with the configured credentials.
    Login,
    /// Store a HiResCovers member cookie pasted from a browser, and verify it.
    Cookie {
        /// `name=value; name=value` exactly as the browser shows it.
        value: String,
    },
}


#[derive(Subcommand)]
enum KeysAction {
    /// Show local key database status.
    Status,
    /// Download the latest key database.
    Update,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let code = match run(&cli) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };
    std::process::exit(code);
}

fn run(cli: &Cli) -> Result<(), bluforge_core::CoreError> {
    match &cli.cmd {
        Cmd::Devices { all } => {
            let devs = if *all {
                device::list()
            } else {
                device::list_optical()
            };
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&devs)
                        .map_err(|e| bluforge_core::CoreError::Device(e.to_string()))?
                );
            } else if devs.is_empty() {
                println!("No optical devices found.");
                println!("Attach a Blu-ray/DVD drive and retry. All block devices:");
                for d in device::list() {
                    println!("  {}  {}  {}", d.bsd_name, d.display(), device::human_bytes(d.size_bytes));
                }
            } else {
                for d in &devs {
                    println!(
                        "{}  {}  {}  [{}]",
                        d.bsd_name,
                        d.display(),
                        device::human_bytes(d.size_bytes),
                        d.interconnect.clone().unwrap_or_else(|| "?".into())
                    );
                    if let Some(mp) = &d.mount_point {
                        println!("    mounted at {mp}");
                    }
                }
            }
        }
        Cmd::Keys { action } => match action {
            KeysAction::Status => print_keys(cli.json, keys::status())?,
            KeysAction::Update => {
                let st = keys::update()?;
                print_keys(cli.json, st)?;
            }
        },
        Cmd::Disc {
            path,
            streams,
            clips,
            episodes,
            series,
        } => {
            let disc = bluforge_core::Disc::open(path, Some(&keys::keydb_path()))?;
            let info = disc.info()?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&info)
                        .map_err(|e| bluforge_core::CoreError::Disc(e.to_string()))?
                );
            } else {
                println!("disc      : {}", if info.name.is_empty() { "(unnamed)" } else { &info.name });
                println!("libbluray : {}", info.library);
                println!("AACS      : {} (ready: {})", info.aacs, info.aacs_ready);
                println!("titles    : {}", info.titles.len());
                if *episodes {
                    let dbg = bluforge_core::tv::parse_label(&info.name).0;
                    println!("tmdb tv id : {:?}", bluforge_core::tv::tmdb_tv_id(&dbg));
                    println!("tmdb season: {:?}", bluforge_core::tv::tmdb_season(63926, 1).map(|e| e.len()));
                    match bluforge_core::tv::lookup_series(&dbg, 1) {
                        Ok(Some(sr)) => println!(
                            "lookup    : {} (tmdb {:?}) eps={} poster={}",
                            sr.name,
                            sr.tmdb_id,
                            sr.episodes.len(),
                            sr.poster.is_some()
                        ),
                        Ok(None) => println!("lookup    : nothing for {dbg:?}"),
                        Err(e) => println!("lookup    : ERROR {e}"),
                    }
                    match bluforge_core::tv::plan(&disc, &info, series.as_deref()) {
                        Ok(p) => {
                            println!(
                                "series    : {} - season {} (disc {:?}, tmdb {:?}, mal {:?})",
                                p.series, p.season, p.disc, p.tmdb_id, p.mal_id
                            );
                            println!("poster    : {:?}", p.poster);
                            println!("episodes  : {}", p.items.len());
                            for it in &p.items {
                                let m = it.duration_secs as u64 / 60;
                                let s2 = it.duration_secs as u64 % 60;
                                println!(
                                    "  S{:02}E{:02}  t#{} +{:>7.1}s  {:>2}:{:02}  {}  still={}",
                                    it.episode.season,
                                    it.episode.number,
                                    it.title_index,
                                    it.start_secs,
                                    m,
                                    s2,
                                    it.episode.title,
                                    it.episode.still.is_some()
                                );
                            }
                        }
                        Err(e) => println!("episodes  : ERROR {e}"),
                    }
                }
                if *streams || *clips {
                    for t in &info.titles {
                        if *clips {
                            let cl = disc.clips(t.index).unwrap_or_default();
                            println!(
                                "  #{} clips: {} [{}]",
                                t.index,
                                cl.len(),
                                cl.iter()
                                    .map(|c| format!("{:.1}", c.duration_secs))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                        }
                        if *streams {
                            for s in disc.streams(t.index).unwrap_or_default() {
                                println!(
                                    "  #{} stream {} pid={} lang={:?} coding={} fmt={}",
                                    t.index, s.kind, s.pid, s.lang, s.coding_type, s.format
                                );
                            }
                        }
                    }
                }
                for t in &info.titles {
                    let h = t.duration_secs as u64 / 3600;
                    let m = (t.duration_secs as u64 % 3600) / 60;
                    println!(
                        "  #{:<3} playlist {:<5} {:02}:{:02}:{:02}  {}  {}  audio:{} {}",
                        t.index,
                        t.playlist,
                        h,
                        m,
                        t.duration_secs as u64 % 60,
                        t.resolution.clone().unwrap_or_else(|| "?".into()),
                        t.dynamic_range.clone().unwrap_or_else(|| "SDR".into()),
                        t.audio_streams,
                        if t.is_main { "[main]" } else { "" }
                    );
                }
            }
        }
        Cmd::Eject => match bluforge_core::device::eject() {
            Ok(m) => println!("{m}"),
            Err(e) => return Err(e),
        },
        Cmd::Libs => {
            bluforge_core::Disc::init_env();
            let (aacs, bdplus) = bluforge_core::Disc::decryption_backend();
            println!("bluforge-core : {}", bluforge_core::version());
            println!("libbluray     : {}", bluforge_core::disc::library_version());
            println!("AACS backend  : {}", aacs.as_deref().unwrap_or("(default)"));
            println!("BD+ backend   : {}", bdplus.as_deref().unwrap_or("(default)"));
            println!("config dir    : {}", bluforge_core::config_dir().display());
        }
        Cmd::Iso {
            source,
            dest,
            folder,
            label,
        } => {
            use bluforge_core::{IsoFormat, IsoProgress, IsoRequest};
            let req = IsoRequest {
                source: source.clone(),
                keydb: Some(keys::keydb_path()),
                dest: dest.clone(),
                format: if *folder { IsoFormat::Folder } else { IsoFormat::Iso },
                label: label.clone(),
            };
            let mut last = String::new();
            bluforge_core::iso::run(&req, |p: IsoProgress| {
                let line = format!("[{}] {:.0}% {}", p.stage, p.percent, p.message);
                if line != last {
                    last = line.clone();
                    eprintln!("{line}");
                }
            })?;
            eprintln!("wrote {}", dest.display());
        }
        Cmd::Rip {
            source,
            output,
            title,
            encode,
            codec,
            quality,
            preset,
            container,
            ten_bit,
            no_chapters,
            no_poster,
            movie,
            audio,
            subs,
            no_covers,
            tv,
            series,
            episodes,
            tv_subs,
        } => {
            use bluforge_core::{Container, EncodeSettings, RipMode, RipProgress, RipRequest, VideoCodec};
            let container_kind = if container.eq_ignore_ascii_case("mp4") {
                Container::Mp4
            } else {
                Container::Mkv
            };
            let mode = if *encode {
                let codec = match codec.as_str() {
                    "x264" => VideoCodec::X264,
                    "hevc_videotoolbox" => VideoCodec::HevcVideotoolbox,
                    "h264_videotoolbox" => VideoCodec::H264Videotoolbox,
                    _ => VideoCodec::X265,
                };
                RipMode::Encode {
                    container: container_kind,
                    settings: EncodeSettings {
                        codec,
                        quality: *quality,
                        preset: Some(preset.clone()),
                        ten_bit: *ten_bit,
                        aac_audio: false,
                    },
                }
            } else {
                RipMode::Remux { container: container_kind }
            };
            let req = RipRequest {
                source: source.clone(),
                keydb: Some(keys::keydb_path()),
                title: *title,
                output: output.clone(),
                mode,
                chapters: !*no_chapters,
                poster: !*no_poster,
                movie: movie.clone(),
                audio: match audio.as_str() {
                    "main" => bluforge_core::rip::AudioSel::Main,
                    "english" | "eng" => bluforge_core::rip::AudioSel::English,
                    _ => bluforge_core::rip::AudioSel::All,
                },
                subs: match subs.as_str() {
                    "english" | "eng" => bluforge_core::rip::SubSel::English,
                    "none" => bluforge_core::rip::SubSel::None,
                    _ => bluforge_core::rip::SubSel::All,
                },
                covers: !*no_covers,
                tv: *tv,
                series: series.clone(),
                episodes: episodes.clone(),
            };
            if tv_subs != "all" {
                let mut c = AppConfig::load();
                c.tv_subs = tv_subs.clone();
                c.save()?;
            }
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let mut last = -1i64;
            let mut dest = String::new();
            bluforge_core::rip::run(&req, &cancel, |p: RipProgress| {
                if let Some(d) = p.dest {
                    dest = d.display().to_string();
                }
                let pct = p.percent as i64;
                if pct != last {
                    last = pct;
                    eprint!("\r  {pct:>3}%  {:.0} fps  {}          ", p.fps, p.speed.trim());
                }
            })?;
            eprintln!("\nwrote {dest}");
        }
        Cmd::Covers { cmd } => {
            use bluforge_core::covers::{self, CoverSource};
            let cfg = AppConfig::load();
            match cmd {
                CoversCmd::Sources => {
                    for s in CoverSource::all() {
                        println!("{}", s.as_str());
                    }
                }
                CoversCmd::Search {
                    query,
                    source,
                    limit,
                } => {
                    let sources: Vec<CoverSource> = if source == "all" {
                        CoverSource::all().to_vec()
                    } else {
                        vec![CoverSource::parse(source).ok_or_else(|| {
                            bluforge_core::CoreError::Config(format!("unknown source '{source}'"))
                        })?]
                    };
                    let mut rows = Vec::new();
                    for s in sources {
                        match covers::search(s, query, *limit, &cfg.hirescovers_dir) {
                            Ok(mut r) => rows.append(&mut r),
                            Err(e) => eprintln!("{e}"),
                        }
                    }
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&rows)
                                .map_err(|e| bluforge_core::CoreError::Config(e.to_string()))?
                        );
                    } else if rows.is_empty() {
                        eprintln!("no covers found");
                    } else {
                        for (i, h) in rows.iter().enumerate() {
                            println!(
                                "{i:>3}  {:<12} {:<6} {:<10} {}   ({} {})",
                                h.source, h.kind, h.format, h.title, h.source, h.id
                            );
                        }
                    }
                }
                CoversCmd::Get {
                    source,
                    id,
                    out,
                    title,
                    role,
                    stem,
                } => {
                    let s = CoverSource::parse(source).ok_or_else(|| {
                        bluforge_core::CoreError::Config(format!("unknown source '{source}'"))
                    })?;
                    let hit = match covers::by_id(s, id, &cfg.hirescovers_dir)? {
                        Some(h) => h,
                        None => match title {
                            Some(t) => covers::search(s, t, 50, &cfg.hirescovers_dir)?
                                .into_iter()
                                .find(|h| h.id == *id)
                                .ok_or_else(|| {
                                    bluforge_core::CoreError::Config(format!("no cover {source}:{id}"))
                                })?,
                            None => {
                                return Err(bluforge_core::CoreError::Config(format!(
                                    "no cover {source}:{id}"
                                )))
                            }
                        },
                    };
                    let name = match (role, stem) {
                        (Some(r), Some(st)) => covers::role_filename(r, st),
                        _ => format!("{} {}.jpg", hit.kind, covers::sanitize(&hit.title)),
                    };
                    let dest = match out {
                        Some(p) if p.is_dir() => p.join(name),
                        Some(p) => p.clone(),
                        None => std::path::PathBuf::from(name),
                    };
                    let saved = covers::download(&hit, &dest, covers::cookie(&cfg).as_deref())?;
                    eprintln!(
                        "wrote {} ({}x{}, {:.1} MB) - {}",
                        saved.path.display(),
                        saved.info.width,
                        saved.info.height,
                        saved.bytes as f64 / 1e6,
                        if saved.original {
                            "original".to_string()
                        } else if saved.info.watermarked {
                            format!("WATERMARKED PREVIEW: {}", saved.info.reasons.join("; "))
                        } else {
                            format!("downscaled derivative: {}", saved.info.reasons.join("; "))
                        }
                    );
                }
                CoversCmd::Login => {
                    let user = cfg
                        .hirescovers_user
                        .clone()
                        .ok_or_else(|| bluforge_core::CoreError::Config("no hirescovers_user".into()))?;
                    let pass = cfg
                        .hirescovers_pass
                        .clone()
                        .ok_or_else(|| bluforge_core::CoreError::Config("no hirescovers_pass".into()))?;
                    let cookie = covers::login_hirescovers(&user, &pass)?;
                    if covers::verify_cookie(&cookie).unwrap_or(false) {
                        eprintln!("login ok: secure_image.php returned an original");
                    } else {
                        eprintln!("login did not unlock originals (check the credentials)");
                    }
                    let mut c = cfg.clone();
                    c.hirescovers_cookie = Some(cookie);
                    c.save()?;
                    eprintln!("hirescovers cookie stored");
                }
                CoversCmd::Inspect { file } => {
                    let info = covers::inspect_file(file)?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&info)
                                .map_err(|e| bluforge_core::CoreError::Config(e.to_string()))?
                        );
                    } else {
                        println!("file    : {}", info.path.display());
                        println!("format  : {} {}x{}", info.format, info.width, info.height);
                        println!("bytes   : {}", info.bytes);
                        println!("dpi     : {:?}", info.dpi);
                        println!("creator : {:?}", info.creator);
                        println!(
                            "markers : exif={} icc={} adobe={}",
                            info.has_exif, info.has_icc, info.has_adobe
                        );
                        println!(
                            "verdict : {}",
                            if info.watermarked {
                                "watermarked preview (server re-encode)"
                            } else if info.derivative {
                                "downscaled derivative (free-tier resize)"
                            } else {
                                "looks like the original upload"
                            }
                        );
                        for r in &info.reasons {
                            println!("  - {r}");
                        }
                    }
                }
                CoversCmd::Cookie { value } => {
                    let cookie = value.trim().to_string();
                    if covers::verify_cookie(&cookie).unwrap_or(false) {
                        eprintln!("cookie works: secure_image.php returned an original");
                    } else {
                        eprintln!("WARNING: cookie did not unlock an original");
                    }
                    let mut c = cfg.clone();
                    c.hirescovers_cookie = Some(cookie);
                    c.save()?;
                    eprintln!("hirescovers cookie stored");
                }
            }
        }
        Cmd::Config => {
            let cfg = AppConfig::load();
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&cfg)
                        .map_err(|e| bluforge_core::CoreError::Config(e.to_string()))?
                );
            } else {
                println!("config dir : {}", bluforge_core::config_dir().display());
                println!("output dir : {}", cfg.output_dir.display());
                println!("auto keys  : {}", cfg.auto_update_keys);
                println!("auto update: {}", cfg.auto_check_updates);
            }
        }
    }
    Ok(())
}

fn print_keys(json: bool, st: keys::KeyDbStatus) -> Result<(), bluforge_core::CoreError> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&st)
                .map_err(|e| bluforge_core::CoreError::Keys(e.to_string()))?
        );
    } else {
        println!("path    : {}", st.path.display());
        println!("present : {}", st.exists);
        println!("entries : {}", st.entries);
        println!("bytes   : {}", st.bytes);
    }
    Ok(())
}
