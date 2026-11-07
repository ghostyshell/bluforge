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
    /// Show linked library versions.
    Libs,
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
            println!("bluforge-core : {}", bluforge_core::version());
            println!("libbluray     : {}", bluforge_core::disc::library_version());
            println!("config dir    : {}", bluforge_core::config_dir().display());
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
