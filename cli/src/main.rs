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
    },
    /// Rip a title to MKV/MP4 (remux by default, or `--encode`).
    Rip {
        /// Disc source: mounted volume, BDMV folder, ISO, or /dev/rdiskN.
        source: std::path::PathBuf,
        /// Output file (.mkv or .mp4), or a directory. Defaults to ~/Downloads.
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
    },
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
        Cmd::Disc { path } => {
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
            };
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
