use clap::{Parser, Subcommand};

use keepsake_core::{device, keys, AppConfig};

#[derive(Parser)]
#[command(
    name = "keepsake-atelier",
    version,
    about = "Keepsake Atelier - Blu-ray ripping & authoring"
)]
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
        /// With --episodes: print each episode's chapter marks (dry run, no rip).
        #[arg(long)]
        chapters: bool,
        /// First episode number on this disc (overrides the remembered mapping).
        #[arg(long)]
        start_episode: Option<u32>,
    },
    /// Rip a title to MKV/MP4 (remux by default, or `--encode`).
    Rip {
        /// Disc source: mounted volume, BDMV folder, ISO, or /dev/rdiskN.
        source: std::path::PathBuf,
        /// Output .mkv/.mp4 file, or a directory (anything without an
        /// extension, or ending in /). Defaults to the configured folder.
        output: Option<std::path::PathBuf>,
        /// Title index (see `keepsake-atelier disc`). Default: the main title.
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
        /// First episode number on this disc (overrides the remembered mapping).
        #[arg(long)]
        start_episode: Option<u32>,
        /// Retry the whole rip this many times on failure (transient drive/decrypt
        /// hiccups after a replug). Default 2.
        #[arg(long, default_value_t = 2)]
        retries: u32,
    },
    /// Cover art: search and download case/disc covers.
    Covers {
        #[command(subcommand)]
        cmd: CoversCmd,
    },
    /// Author a Blu-ray (BDMV / ISO) from finished MKV or MP4 files.
    Author {
        /// Input files, in playback order (episodes are chaptered).
        #[arg(required = true)]
        inputs: Vec<std::path::PathBuf>,
        /// Output .iso file or BDMV folder.
        #[arg(long)]
        out: std::path::PathBuf,
        /// Volume label.
        #[arg(long)]
        label: Option<String>,
        /// Do not carry chapter marks from the inputs.
        #[arg(long)]
        no_chapters: bool,
        /// Force a chapter every N minutes instead.
        #[arg(long)]
        auto_chapters: Option<u32>,
        /// Split the inputs across discs of at most this many GB (25 or 50).
        /// Whole inputs stay together, so the output is one folder per disc.
        #[arg(long)]
        split: Option<f64>,
    },
    /// AACS helper: read the volume ID from the drive and cache it for libaacs.
    Aacs {
        /// Device node, eg. /dev/disk4.
        device: std::path::PathBuf,
        /// Mounted disc root (used to compute the disc id).
        #[arg(long)]
        disc: std::path::PathBuf,
    },
    /// Eject the disc from the optical drive.
    Eject,
    /// Show linked library versions.
    Libs,
    /// BD+ VM data for libbdplus (needed to rip BD+ discs without MakeMKV).
    Bdplus {
        /// Download and install the public VM data into libbdplus's config dir.
        #[arg(long)]
        install: bool,
    },
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
    /// 1:1 sector copy of a disc to an ISO (keeps AACS/BD+; archival image).
    Clone {
        /// Raw device, eg. /dev/rdisk4.
        source: std::path::PathBuf,
        /// Output .iso file.
        dest: std::path::PathBuf,
    },
    /// DVD-Video: list titles or rip one (uses the bundled dvdvideo demuxer).
    Dvd {
        #[command(subcommand)]
        action: DvdAction,
    },
    /// Verify a BDMV tree: navigation, playlists and clip files.
    Verify {
        /// Mounted disc, disc root, or BDMV folder.
        path: std::path::PathBuf,
    },
    /// Show what is installed and which capability each missing piece blocks.
    Doctor,
    /// Show effective configuration and paths.
    Config,
    /// Parse, edit and save a disc's title menu as a reusable template.
    Menu {
        #[command(subcommand)]
        action: MenuAction,
    },
}

#[derive(Subcommand)]
enum DvdAction {
    /// List the disc's titles, chapters and durations.
    List {
        /// Mounted DVD or a VIDEO_TS directory.
        path: std::path::PathBuf,
    },
    /// Rip one title to MKV (MPEG-2 + AC-3/DTS copied).
    Rip {
        /// Mounted DVD or a VIDEO_TS directory.
        path: std::path::PathBuf,
        /// Title number, 1-based (see `dvd list`).
        #[arg(long)]
        title: u16,
        /// Output .mkv (default: the configured output folder).
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Include DVD subtitles (VobSub).
        #[arg(long)]
        subs: bool,
    },
}

#[derive(Subcommand)]
enum MenuAction {
    /// Parse a disc menu: index.bdmv navigation plus the playable titles.
    Parse {
        /// Mounted volume or BDMV folder (an ISO must be mounted first).
        source: std::path::PathBuf,
        /// Save the parsed document as a template with this name.
        #[arg(long)]
        save: Option<String>,
    },
    /// List saved menu templates.
    List,
    /// Show a saved menu template (or a JSON document path).
    Show {
        /// Template name, or a path to a `.json` document.
        template: String,
    },
    /// Edit a saved template in place.
    Edit {
        /// Template name, or a path to a `.json` document.
        template: String,
        /// Title index to edit (as shown by `menu show`).
        #[arg(long)]
        title: u32,
        /// New display name for that title.
        #[arg(long)]
        name: Option<String>,
        /// Mark that title as the main feature (clears the flag on the others).
        #[arg(long)]
        main: bool,
        /// Chapter label as `N:Text`, 1-based, repeatable.
        #[arg(long = "chapter")]
        chapters: Vec<String>,
    },
    /// Apply a saved template to a different disc.
    Apply {
        /// Template name, or a path to a `.json` document.
        template: String,
        /// Mounted volume or BDMV folder of the target disc.
        target: std::path::PathBuf,
        /// Save the result as a new template.
        #[arg(long)]
        save: Option<String>,
    },
    /// Delete a saved template.
    Remove {
        /// Template name.
        template: String,
    },
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
    Inspect { file: std::path::PathBuf },
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
    /// Add or replace a disc key (retires MakeMKV for that disc).
    Add {
        /// Disc id: 40 hex chars (SHA-1 of AACS/Unit_Key_RO.inf).
        #[arg(long)]
        disc: Option<String>,
        /// Derive the disc id from a mounted disc / BDMV folder instead.
        #[arg(long)]
        from: Option<std::path::PathBuf>,
        /// Volume-unique key (hex). Alone this decrypts an AACS1 disc.
        #[arg(long)]
        vuk: Option<String>,
        /// Unit key (hex), repeatable. Needed for AACS2/UHD discs.
        #[arg(long = "unit-key")]
        unit_keys: Vec<String>,
        /// Media key (hex), optional.
        #[arg(long = "media-key")]
        media_key: Option<String>,
        /// Volume id (hex), optional.
        #[arg(long = "volume-id")]
        volume_id: Option<String>,
        /// Human name for the entry.
        #[arg(long)]
        name: Option<String>,
    },
    /// Derive a key from the inserted disc: read the volume id from the drive
    /// and recover the key from the disc's MKB. No MakeMKV, no host certificate,
    /// no existing KEYDB entry.
    Native {
        /// Mounted disc / volume (default: the mounted optical volume).
        #[arg(long)]
        from: Option<std::path::PathBuf>,
        /// Print the derived key instead of storing it.
        #[arg(long)]
        print: bool,
    },
    /// Recover the key via MakeMKV's discatt.dat (VID) plus offline MKB
    /// derivation. Bypasses the host certificate entirely.
    FromMakemkv {
        #[arg(long)]
        from: Option<std::path::PathBuf>,
        #[arg(long)]
        print: bool,
    },
    /// List known discs.
    List {
        /// Maximum rows to print.
        #[arg(long, default_value_t = 40)]
        limit: usize,
        /// Only entries that carry a key.
        #[arg(long)]
        keyed: bool,
        /// Filter by name or disc id (case-insensitive).
        #[arg(long)]
        search: Option<String>,
    },
    /// Remove a disc's entry.
    Remove {
        #[arg(long)]
        disc: Option<String>,
        #[arg(long)]
        from: Option<std::path::PathBuf>,
    },
    /// Show the entry for one disc.
    Show {
        #[arg(long)]
        disc: Option<String>,
        #[arg(long)]
        from: Option<std::path::PathBuf>,
    },
    /// Merge another KEYDB.cfg into the local one.
    Import { file: std::path::PathBuf },
}

/// A `/dev/rdiskN` node goes stale when the drive re-enumerates (macOS moved it
/// from disk5 to disk4 and back during one session). If the requested node is
/// gone, use the optical drive that is present now.
fn resolve_source(p: &std::path::Path) -> std::path::PathBuf {
    if p.exists() {
        return p.to_path_buf();
    }
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !(name.starts_with("rdisk") || name.starts_with("disk")) {
        return p.to_path_buf();
    }
    if let Some(d) = keepsake_core::device::list_optical().into_iter().next() {
        let path = d
            .device_path
            .clone()
            .unwrap_or_else(|| format!("/dev/r{}", d.bsd_name));
        if std::path::Path::new(&path).exists() {
            eprintln!(
                "note: {} is gone; the drive re-enumerated, using {path}",
                p.display()
            );
            return std::path::PathBuf::from(path);
        }
    }
    p.to_path_buf()
}

fn nav_label(t: &keepsake_core::menu::NavTarget) -> String {
    use keepsake_core::menu::NavType;
    let kind = match t.kind {
        NavType::Hdmv => "HDMV",
        NavType::Bdj => "BD-J",
        NavType::None => "none",
        NavType::Unknown => "unknown",
    };
    match (&t.name, t.object) {
        (Some(name), _) => format!("{kind} {name}"),
        (_, Some(object)) => format!("{kind} object {object}"),
        _ => kind.to_string(),
    }
}

fn menu_clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

fn print_menu(doc: &keepsake_core::menu::DiscMenu) {
    println!("disc      : {}", doc.label);
    if let Some(id) = &doc.disc_id {
        println!("disc id   : 0x{id}");
    }
    if let Some(src) = &doc.source {
        println!("source    : {src}");
    }
    if let Some(applied) = &doc.applied_from {
        println!("applied   : {applied}");
    }
    if let Some(nav) = &doc.navigation {
        println!(
            "first play: {}   top menu: {}   nav titles: {}",
            nav_label(&nav.first_playback),
            nav_label(&nav.top_menu),
            nav.titles.len()
        );
    }
    if let Some(note) = &doc.note {
        println!("note      : {note}");
    }
    println!("titles    : {}", doc.titles.len());
    for t in &doc.titles {
        println!(
            "  #{:<3} playlist {:05}  {:>8}  {:>2} ch  {}{}",
            t.index,
            t.playlist,
            menu_clock(t.duration_secs),
            t.chapters,
            if t.main { "[main] " } else { "" },
            t.name.as_deref().unwrap_or("")
        );
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
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

fn run(cli: &Cli) -> Result<(), keepsake_core::CoreError> {
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
                        .map_err(|e| keepsake_core::CoreError::Device(e.to_string()))?
                );
            } else if devs.is_empty() {
                println!("No optical devices found.");
                println!("Attach a Blu-ray/DVD drive and retry. All block devices:");
                for d in device::list() {
                    println!(
                        "  {}  {}  {}",
                        d.bsd_name,
                        d.display(),
                        device::human_bytes(d.size_bytes)
                    );
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
            KeysAction::Add {
                disc,
                from,
                vuk,
                unit_keys,
                media_key,
                volume_id,
                name,
            } => {
                let id = resolve_disc_id(disc, from)?;
                let entry = keys::KeyEntry {
                    id: id.clone(),
                    name: name.clone(),
                    media_key: media_key.as_deref().and_then(norm_hex),
                    volume_id: volume_id.as_deref().and_then(norm_hex),
                    vuk: vuk.as_deref().and_then(norm_hex),
                    unit_keys: unit_keys.iter().filter_map(|s| norm_hex(s)).collect(),
                    note: Some("keepsake-atelier".into()),
                };
                if entry.vuk.is_none() && entry.unit_keys.is_empty() && entry.media_key.is_none() {
                    return Err(keepsake_core::CoreError::Keys(
                        "nothing to store: give --vuk (and/or --unit-key)".into(),
                    ));
                }
                let replaced = keys::upsert_at(&keys::keydb_path(), &entry)?;
                eprintln!(
                    "{} {} {}",
                    if replaced { "updated" } else { "added" },
                    id,
                    name.clone().unwrap_or_default()
                );
            }
            KeysAction::Native { from, print } => {
                let mount = match from {
                    Some(p) => p.clone(),
                    None => keepsake_core::device::list_optical()
                        .into_iter()
                        .find_map(|d| d.mount_point.map(std::path::PathBuf::from))
                        .ok_or_else(|| {
                            keepsake_core::CoreError::Device(
                                "no mounted optical volume; pass --from <mount>".into(),
                            )
                        })?,
                };
                if *print {
                    let k = keepsake_core::native::derive(&mount)?;
                    println!("disc      : {}", k.name);
                    println!("id        : 0x{}", k.id);
                    println!("volume id : 0x{}", k.vid);
                    println!("media key : 0x{}", k.media_key);
                    println!("vuk       : 0x{}", k.vuk);
                    if k.unit_keys.is_empty() {
                        println!("unit keys : none");
                    } else {
                        println!("unit keys : {}", k.unit_keys.len());
                        for (i, uk) in k.unit_keys.iter().enumerate() {
                            println!("  {:<3} 0x{uk}", i + 1);
                        }
                    }
                } else {
                    let (k, added) = keepsake_core::native::derive_and_store(&mount)?;
                    eprintln!(
                        "{} 0x{} {}",
                        if added { "derived" } else { "already keyed" },
                        k.id,
                        k.name
                    );
                }
            }
            KeysAction::FromMakemkv { from, print } => {
                let mount = match from {
                    Some(p) => p.clone(),
                    None => keepsake_core::device::list_optical()
                        .into_iter()
                        .find_map(|d| d.mount_point.map(std::path::PathBuf::from))
                        .ok_or_else(|| {
                            keepsake_core::CoreError::Device(
                                "no mounted optical volume; pass --from <mount>".into(),
                            )
                        })?,
                };
                if *print {
                    let (k, _dc, _rdk) = keepsake_core::native::from_makemkv(&mount)?;
                    println!("disc      : {}", k.name);
                    println!("id        : 0x{}", k.id);
                    println!("volume id : 0x{}", k.vid);
                    println!("media key : 0x{}", k.media_key);
                    println!("vuk       : 0x{}", k.vuk);
                    if k.unit_keys.is_empty() {
                        println!("unit keys : none");
                    } else {
                        println!("unit keys : {}", k.unit_keys.len());
                        for (i, uk) in k.unit_keys.iter().enumerate() {
                            println!("  {:<3} 0x{uk}", i + 1);
                        }
                    }
                } else {
                    let (k, added) = keepsake_core::native::from_makemkv_and_store(&mount)?;
                    eprintln!(
                        "{} 0x{} {}",
                        if added { "derived" } else { "already keyed" },
                        k.id,
                        k.name
                    );
                }
            }
            KeysAction::Show { disc, from } => {
                let id = match (disc, from) {
                    (Some(d), _) => d
                        .trim()
                        .trim_start_matches("0x")
                        .trim_start_matches("0X")
                        .to_ascii_lowercase(),
                    (None, Some(mount)) => keys::disc_id_for_path(mount).ok_or_else(|| {
                        keepsake_core::CoreError::Disc(format!(
                            "no AACS/Unit_Key_RO.inf under {}",
                            mount.display()
                        ))
                    })?,
                    _ => {
                        return Err(keepsake_core::CoreError::Keys(
                            "pass --disc <id> or --from <mount>".into(),
                        ))
                    }
                };
                let entry = keys::lookup(&id)?
                    .ok_or_else(|| keepsake_core::CoreError::Keys(format!("no entry for {id}")))?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&entry)
                            .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
                    );
                } else {
                    println!("disc id   : 0x{}", entry.id);
                    println!("name      : {}", entry.name.clone().unwrap_or_default());
                    if let Some(v) = &entry.media_key {
                        println!("media key : 0x{v}");
                    }
                    if let Some(v) = &entry.volume_id {
                        println!("volume id : 0x{v}");
                    }
                    println!(
                        "vuk       : {}",
                        entry
                            .vuk
                            .as_deref()
                            .map(|v| format!("0x{v}"))
                            .unwrap_or_else(|| "none".into())
                    );
                    if entry.unit_keys.is_empty() {
                        println!("unit keys : none (AACS1)");
                    } else {
                        println!("unit keys : {}", entry.unit_keys.len());
                        for (i, k) in entry.unit_keys.iter().enumerate() {
                            println!("  {:<2} 0x{}", i + 1, k);
                        }
                    }
                    if let Some(n) = &entry.note {
                        println!("note      : {n}");
                    }
                }
            }
            KeysAction::List {
                limit,
                keyed,
                search,
            } => {
                let all = match search {
                    Some(q) => keys::search_at(&keys::keydb_path(), q, *limit)?,
                    None => keys::list_at(&keys::keydb_path(), Some(*limit))?,
                };
                let mut shown = 0usize;
                for e in all {
                    let has_key = e.vuk.is_some() || !e.unit_keys.is_empty();
                    if *keyed && !has_key {
                        continue;
                    }
                    if shown >= *limit {
                        break;
                    }
                    println!(
                        "{}  {}  {}",
                        e.id,
                        if has_key { "KEY" } else { "   " },
                        e.name.unwrap_or_default()
                    );
                    shown += 1;
                }
                eprintln!("{shown} shown");
            }
            KeysAction::Remove { disc, from } => {
                let id = resolve_disc_id(disc, from)?;
                let gone = keys::remove_at(&keys::keydb_path(), &id)?;
                eprintln!("{} {id}", if gone { "removed" } else { "not found:" });
            }
            KeysAction::Import { file } => {
                let (added, skipped) = keys::import_at(&keys::keydb_path(), file)?;
                eprintln!("imported {added} entries ({skipped} already present)");
            }
        },
        Cmd::Disc {
            path,
            streams,
            clips,
            episodes,
            series,
            chapters,
            start_episode,
        } => {
            let source = resolve_source(path);
            let backend = keepsake_core::Disc::resolve_backend_for(Some(&source));
            let keyed = keepsake_core::Disc::disc_id_for_source(&source)
                .map(|id| keepsake_core::keys::has(&id).unwrap_or(false))
                .unwrap_or(false);
            let disc = keepsake_core::Disc::open(&source, Some(&keys::keydb_path()))?;
            let info = disc.info()?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&info)
                        .map_err(|e| keepsake_core::CoreError::Disc(e.to_string()))?
                );
            } else {
                println!(
                    "disc      : {}",
                    if info.name.is_empty() {
                        "(unnamed)"
                    } else {
                        &info.name
                    }
                );
                println!("libbluray : {}", info.library);
                println!("AACS      : {} (ready: {})", info.aacs, info.aacs_ready);
                println!(
                    "decrypt   : {}{}",
                    backend.label(),
                    if keyed {
                        " [disc key present]"
                    } else {
                        " [no disc key]"
                    }
                );
                println!("titles    : {}", info.titles.len());
                if *episodes {
                    let dbg = keepsake_core::tv::parse_label(&info.name).0;
                    println!("tmdb tv id : {:?}", keepsake_core::tv::tmdb_tv_id(&dbg));
                    println!(
                        "tmdb season: {:?}",
                        keepsake_core::tv::tmdb_season(63926, 1).map(|e| e.len())
                    );
                    match keepsake_core::tv::lookup_series(&dbg, 1) {
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
                    match keepsake_core::tv::plan(&disc, &info, series.as_deref(), *start_episode) {
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
                            if *chapters {
                                let first = p.items.first().map(|i| i.title_index).unwrap_or(0);
                                let anime =
                                    disc.streams(first).unwrap_or_default().iter().any(|s| {
                                        s.kind == "audio" && s.lang.as_deref() == Some("jpn")
                                    });
                                let marks = disc.chapters(first).unwrap_or_default();
                                for it in &p.items {
                                    let mut ch: Vec<keepsake_core::disc::Chapter> = marks
                                        .iter()
                                        .filter(|c| {
                                            c.start_secs >= it.start_secs - 1.0
                                                && c.start_secs
                                                    < it.start_secs + it.duration_secs - 0.5
                                        })
                                        .map(|c| keepsake_core::disc::Chapter {
                                            start_secs: c.start_secs - it.start_secs,
                                            name: c.name.clone(),
                                        })
                                        .collect();
                                    if anime {
                                        keepsake_core::rip::label_anime_chapters(
                                            &mut ch,
                                            it.duration_secs,
                                        );
                                    }
                                    let fmt: Vec<String> = ch
                                        .iter()
                                        .map(|c| {
                                            format!(
                                                "{}:{:02} {}",
                                                (c.start_secs as u64) / 60,
                                                (c.start_secs as u64) % 60,
                                                c.name.clone().unwrap_or_default()
                                            )
                                        })
                                        .collect();
                                    println!(
                                        "  S{:02}E{:02} chapters: {}",
                                        it.episode.season,
                                        it.episode.number,
                                        fmt.join(" | ")
                                    );
                                }
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
        Cmd::Author {
            inputs,
            out,
            label,
            no_chapters,
            auto_chapters,
            split,
        } => {
            let req = keepsake_core::author::AuthorRequest {
                inputs: inputs.clone(),
                dest: out.clone(),
                label: label.clone(),
                chapters: !*no_chapters,
                auto_chapter_minutes: *auto_chapters,
                split_gb: *split,
            };
            let mut last = -1i64;
            let written = keepsake_core::author::run(&req, |p| {
                let pct = p.percent as i64;
                if pct != last {
                    last = pct;
                    eprint!("\r  {pct:>3}%  {}          ", p.message);
                }
            })?;
            eprintln!("\nwrote {}", written.display());
        }
        Cmd::Aacs { device, disc } => {
            // The unit-key file is needed for the disc id and lives on the volume,
            // so read it before unmounting the disc for exclusive access.
            let ukf = std::fs::read(disc.join("AACS").join("Unit_Key_RO.inf")).ok();
            let unmounted = false; // read-only: do not disturb a mounted disc
            let vid = keepsake_core::disc::aacs_volume_id(device);
            match (vid, ukf) {
                (Some(vid), Some(ukf)) => {
                    let hex: String = vid.iter().map(|b| format!("{b:02x}")).collect();
                    println!("volume id : {hex} (unmounted: {unmounted})");
                    let ok = keepsake_core::disc::aacs_cache_volume_id_bytes(&ukf, &vid);
                    println!("cached    : {ok}");
                }
                (Some(vid), None) => {
                    let hex: String = vid.iter().map(|b| format!("{b:02x}")).collect();
                    println!("volume id : {hex}");
                    println!("cached    : false (no unit key file)");
                }
                (None, _) => println!("volume id : unreadable (unmounted: {unmounted})"),
            }
        }
        Cmd::Eject => match keepsake_core::device::eject() {
            Ok(m) => println!("{m}"),
            Err(e) => return Err(e),
        },
        Cmd::Bdplus { install } => {
            if *install {
                let dir = keepsake_core::bdplus::install()?;
                eprintln!("installed BD+ VM data at {}", dir.display());
            } else {
                let dir = keepsake_core::bdplus::config_dir();
                println!("BD+ vm config : {}", dir.display());
                println!(
                    "installed     : {}",
                    if keepsake_core::bdplus::installed() {
                        "yes"
                    } else {
                        "no (run `keepsake-atelier bdplus --install`)"
                    }
                );
            }
        }
        Cmd::Libs => {
            keepsake_core::Disc::init_env();
            let (aacs, bdplus) = keepsake_core::Disc::decryption_backend();
            let chosen = keepsake_core::Disc::resolve_backend_for(None);
            println!("keepsake-core : {}", keepsake_core::version());
            println!("libbluray     : {}", keepsake_core::disc::library_version());
            println!("chosen backend: {}", chosen.label());
            println!("AACS backend  : {}", aacs.as_deref().unwrap_or("(default)"));
            println!(
                "BD+ backend   : {}",
                bdplus.as_deref().unwrap_or("(default)")
            );
            println!("config dir    : {}", keepsake_core::config_dir().display());
        }
        Cmd::Iso {
            source,
            dest,
            folder,
            label,
        } => {
            use keepsake_core::{IsoFormat, IsoProgress, IsoRequest};
            let req = IsoRequest {
                source: resolve_source(source),
                keydb: Some(keys::keydb_path()),
                dest: dest.clone(),
                format: if *folder {
                    IsoFormat::Folder
                } else {
                    IsoFormat::Iso
                },
                label: label.clone(),
            };
            let mut last = String::new();
            keepsake_core::iso::run(&req, |p: IsoProgress| {
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
            start_episode,
            retries,
        } => {
            use keepsake_core::{
                Container, EncodeSettings, RipMode, RipProgress, RipRequest, VideoCodec,
            };
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
                RipMode::Remux {
                    container: container_kind,
                }
            };
            let req = RipRequest {
                source: resolve_source(source),
                keydb: Some(keys::keydb_path()),
                title: *title,
                output: output.clone(),
                mode,
                chapters: !*no_chapters,
                poster: !*no_poster,
                movie: movie.clone(),
                audio: match audio.as_str() {
                    "main" => keepsake_core::rip::AudioSel::Main,
                    "english" | "eng" => keepsake_core::rip::AudioSel::English,
                    _ => keepsake_core::rip::AudioSel::All,
                },
                subs: match subs.as_str() {
                    "english" | "eng" => keepsake_core::rip::SubSel::English,
                    "none" => keepsake_core::rip::SubSel::None,
                    _ => keepsake_core::rip::SubSel::All,
                },
                covers: !*no_covers,
                tv: *tv,
                series: series.clone(),
                episodes: episodes.clone(),
                start_episode: *start_episode,
            };
            if tv_subs != "all" {
                let mut c = AppConfig::load();
                c.tv_subs = tv_subs.clone();
                c.save()?;
            }
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let mut attempt = 0u32;
            loop {
                attempt += 1;
                let mut last = -1i64;
                let mut dest = String::new();
                let res = keepsake_core::rip::run(&req, &cancel, |p: RipProgress| {
                    if let Some(d) = p.dest {
                        dest = d.display().to_string();
                    }
                    let pct = p.percent as i64;
                    if pct != last {
                        last = pct;
                        eprint!(
                            "\r  {pct:>3}%  {:.0} fps  {}          ",
                            p.fps,
                            p.speed.trim()
                        );
                    }
                });
                match res {
                    Ok(()) => {
                        if dest.is_empty() {
                            eprintln!("\nrip finished but reported no output file");
                        } else {
                            eprintln!("\nwrote {dest}");
                        }
                        break;
                    }
                    Err(e) if attempt <= *retries => {
                        eprintln!("\nrip failed ({e}); retry {attempt}/{retries}");
                        std::thread::sleep(std::time::Duration::from_secs(5));
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Cmd::Covers { cmd } => {
            use keepsake_core::covers::{self, CoverSource};
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
                            keepsake_core::CoreError::Config(format!("unknown source '{source}'"))
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
                                .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
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
                        keepsake_core::CoreError::Config(format!("unknown source '{source}'"))
                    })?;
                    let hit = match covers::by_id(s, id, &cfg.hirescovers_dir)? {
                        Some(h) => h,
                        None => match title {
                            Some(t) => covers::search(s, t, 50, &cfg.hirescovers_dir)?
                                .into_iter()
                                .find(|h| h.id == *id)
                                .ok_or_else(|| {
                                    keepsake_core::CoreError::Config(format!(
                                        "no cover {source}:{id}"
                                    ))
                                })?,
                            None => {
                                return Err(keepsake_core::CoreError::Config(format!(
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
                    let user = cfg.hirescovers_user.clone().ok_or_else(|| {
                        keepsake_core::CoreError::Config("no hirescovers_user".into())
                    })?;
                    let pass = cfg.hirescovers_pass.clone().ok_or_else(|| {
                        keepsake_core::CoreError::Config("no hirescovers_pass".into())
                    })?;
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
                                .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
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
        Cmd::Clone { source, dest } => {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            eprintln!("cloning {} -> {}", source.display(), dest.display());
            let written = keepsake_core::clone::run(
                &keepsake_core::CloneRequest {
                    source: source.clone(),
                    dest: dest.clone(),
                },
                &cancel,
                |p| {
                    if p.total > 0 {
                        eprint!("\r  {:>5.1}%  {}", p.percent, p.message);
                    } else {
                        eprint!("\r  {}", p.message);
                    }
                },
            )?;
            eprintln!("\nwrote {} ({} bytes)", dest.display(), written);
        }
        Cmd::Dvd { action } => match action {
            DvdAction::List { path } => {
                let info = keepsake_core::dvd::info(path)?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&info)
                            .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
                    );
                } else {
                    println!("disc      : {}", info.label);
                    println!("root      : {}", info.root);
                    println!("titles    : {}", info.titles.len());
                    for t in &info.titles {
                        println!(
                            "  #{:<2} VTS {:<2} title {:<2}  {:>8}  {:>2} chapters  {} angles",
                            t.number,
                            t.title_set,
                            t.vts_title,
                            menu_clock(t.duration_secs.unwrap_or(0.0)),
                            t.chapters,
                            t.angles
                        );
                    }
                }
            }
            DvdAction::Rip {
                path,
                title,
                out,
                subs,
            } => {
                let info = keepsake_core::dvd::info(path)?;
                let output = match out {
                    Some(p) => p.clone(),
                    None => {
                        let cfg = AppConfig::load();
                        let name = info.label.replace('/', "-");
                        cfg.output_dir
                            .join(&name)
                            .join(format!("{name} - Title {title}.mkv"))
                    }
                };
                let req = keepsake_core::dvd::DvdRipRequest {
                    source: path.clone(),
                    title: *title,
                    output: output.clone(),
                    subtitles: *subs,
                };
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let mut last = -1i64;
                keepsake_core::dvd::rip(&req, &cancel, |p| {
                    let pct = p.percent as i64;
                    if pct != last {
                        last = pct;
                        eprint!("\r  {pct:>3}%  {}", p.message);
                    }
                })?;
                eprintln!("\nwrote {}", output.display());
            }
        },
        Cmd::Verify { path } => {
            let report = keepsake_core::verify::run(path)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
                );
            } else {
                println!("tree      : {}", report.path);
                println!(
                    "first play: {}",
                    report
                        .first_play
                        .map(|o| format!("movie object {o}"))
                        .unwrap_or_else(|| "none".into())
                );
                println!(
                    "top menu  : {}",
                    report
                        .top_menu
                        .map(|o| format!("movie object {o}"))
                        .unwrap_or_else(|| "none".into())
                );
                println!("titles    : {}", report.titles.len());
                for t in &report.titles {
                    println!(
                        "  #{:<3} object {:<3} playlist {:<6} {:<3} clips  {:>8}  {:>2} marks{}",
                        t.index,
                        t.object
                            .map(|o| o.to_string())
                            .unwrap_or_else(|| "-".into()),
                        t.playlist
                            .map(|p| format!("{p:05}"))
                            .unwrap_or_else(|| "-".into()),
                        t.clips.len(),
                        menu_clock(t.duration_secs.unwrap_or(0.0)),
                        t.chapters.unwrap_or(0),
                        if t.errors.is_empty() { "" } else { "  ERRORS" }
                    );
                }
                for w in &report.warnings {
                    println!("warning   : {w}");
                }
                for e in &report.errors {
                    println!("error     : {e}");
                }
                println!("result    : {}", if report.ok { "ok" } else { "FAILED" });
            }
            if !report.ok {
                return Err(keepsake_core::CoreError::Disc(format!(
                    "{} problem(s) in the tree",
                    report.errors.len()
                )));
            }
        }
        Cmd::Doctor => {
            let r = keepsake_core::doctor::run();
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&r)
                        .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
                );
            } else {
                println!("version   : {}", r.version);
                for c in &r.checks {
                    println!(
                        "{:<16} {:<8} {}",
                        c.name,
                        if c.ok { "ok" } else { "MISSING" },
                        c.detail
                    );
                    if !c.ok {
                        if let Some(fix) = &c.fix {
                            println!("{:<16} {:<8} fix: {fix}", "", "");
                        }
                    }
                }
                println!();
                println!("{:<30} {:<8} {}", "capability", "status", "detail");
                for c in &r.capabilities {
                    println!(
                        "{:<30} {:<8} {}",
                        c.name,
                        if c.ok { "ok" } else { "no" },
                        c.detail
                    );
                }
            }
        }
        Cmd::Config => {
            let cfg = AppConfig::load();
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&cfg)
                        .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
                );
            } else {
                println!("config dir : {}", keepsake_core::config_dir().display());
                println!("output dir : {}", cfg.output_dir.display());
                println!("auto keys  : {}", cfg.auto_update_keys);
                println!("auto update: {}", cfg.auto_check_updates);
            }
        }
        Cmd::Menu { action } => match action {
            MenuAction::Parse { source, save } => {
                let doc = keepsake_core::menu::parse(source, Some(&keys::keydb_path()))?;
                if let Some(name) = save {
                    let path =
                        keepsake_core::menu::save(&keepsake_core::menu::store_dir(), name, &doc)?;
                    eprintln!("saved template {}", path.display());
                }
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&doc)
                            .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
                    );
                } else {
                    print_menu(&doc);
                }
            }
            MenuAction::List => {
                let entries = keepsake_core::menu::list(&keepsake_core::menu::store_dir());
                if cli.json {
                    let rows: Vec<_> = entries
                        .iter()
                        .map(|(name, path)| {
                            let doc =
                                keepsake_core::menu::load(&keepsake_core::menu::store_dir(), name)
                                    .ok();
                            serde_json::json!({
                                "name": name,
                                "path": path.display().to_string(),
                                "label": doc.as_ref().map(|d| d.label.clone()).unwrap_or_default(),
                                "titles": doc.as_ref().map(|d| d.titles.len()).unwrap_or(0),
                            })
                        })
                        .collect();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&rows)
                            .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
                    );
                } else if entries.is_empty() {
                    println!(
                        "no saved menus in {}",
                        keepsake_core::menu::store_dir().display()
                    );
                } else {
                    for (name, path) in entries {
                        match keepsake_core::menu::load(&keepsake_core::menu::store_dir(), &name) {
                            Ok(doc) => println!(
                                "{:<28} {:>3} titles  {}  ({})",
                                name,
                                doc.titles.len(),
                                doc.label,
                                path.display()
                            ),
                            Err(e) => println!("{name:<28} (unreadable: {e})"),
                        }
                    }
                }
            }
            MenuAction::Show { template } => {
                let dir = keepsake_core::menu::store_dir();
                let doc = keepsake_core::menu::load(&dir, template)?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&doc)
                            .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
                    );
                } else {
                    print_menu(&doc);
                }
            }
            MenuAction::Edit {
                template,
                title,
                name,
                main,
                chapters,
            } => {
                let dir = keepsake_core::menu::store_dir();
                let mut doc = keepsake_core::menu::load(&dir, template)?;
                let idx = *title;
                if *main {
                    doc.set_main(idx)?;
                }
                if let Some(name) = name {
                    doc.set_name(idx, name)?;
                }
                for spec in chapters {
                    let (n, text) = keepsake_core::menu::parse_chapter_spec(spec)?;
                    doc.set_chapter(idx, n, &text)?;
                }
                let path = keepsake_core::menu::save(&dir, template, &doc)?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&doc)
                            .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
                    );
                } else {
                    eprintln!("updated {}", path.display());
                    print_menu(&doc);
                }
            }
            MenuAction::Apply {
                template,
                target,
                save,
            } => {
                let dir = keepsake_core::menu::store_dir();
                let tpl = keepsake_core::menu::load(&dir, template)?;
                let target_doc = keepsake_core::menu::parse(target, Some(&keys::keydb_path()))?;
                let doc = keepsake_core::menu::apply(&tpl, &target_doc, template);
                if let Some(save) = save {
                    let path = keepsake_core::menu::save(&dir, save, &doc)?;
                    eprintln!("saved template {}", path.display());
                }
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&doc)
                            .map_err(|e| keepsake_core::CoreError::Config(e.to_string()))?
                    );
                } else {
                    print_menu(&doc);
                }
            }
            MenuAction::Remove { template } => {
                let path =
                    keepsake_core::menu::remove(&keepsake_core::menu::store_dir(), template)?;
                println!("removed {}", path.display());
            }
        },
    }
    Ok(())
}

/// Lowercase hex without a `0x` prefix, if valid.
fn norm_hex(s: &str) -> Option<String> {
    let t = s
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .to_ascii_lowercase();
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(t)
    } else {
        None
    }
}

/// Resolve a disc id from `--disc <id>` or `--from <path>`.
fn resolve_disc_id(
    disc: &Option<String>,
    from: &Option<std::path::PathBuf>,
) -> Result<String, keepsake_core::CoreError> {
    if let Some(d) = disc {
        return match norm_hex(d).filter(|s| s.len() == 40) {
            Some(id) => Ok(id),
            None => Err(keepsake_core::CoreError::Keys(format!("bad disc id '{d}'"))),
        };
    }
    if let Some(p) = from {
        return keepsake_core::keys::disc_id_for_path(p).ok_or_else(|| {
            keepsake_core::CoreError::Keys(format!("no AACS/Unit_Key_RO.inf under {}", p.display()))
        });
    }
    Err(keepsake_core::CoreError::Keys(
        "give --disc <id> or --from <mounted disc>".into(),
    ))
}

fn print_keys(json: bool, st: keys::KeyDbStatus) -> Result<(), keepsake_core::CoreError> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&st)
                .map_err(|e| keepsake_core::CoreError::Keys(e.to_string()))?
        );
    } else {
        println!("path    : {}", st.path.display());
        println!("present : {}", st.exists);
        println!("entries : {}", st.entries);
        println!("bytes   : {}", st.bytes);
    }
    Ok(())
}
