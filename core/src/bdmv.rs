//! Multi-title BDMV writing.
//!
//! tsMuxeR authors one title per disc. For an episode set we let it author each
//! input into its own temporary BDMV, then merge them here: renumber the
//! playlists/clips, write an `index.bdmv` with one entry per title and a
//! `MovieObject.bdmv` with one `PlayPL` object per title plus first-play and
//! top-menu objects.
//!
//! The layouts follow libbluray's parsers (`bdnav/index_parse.c`,
//! `hdmv/mobj_parse.c`, `hdmv/hdmv_vm.c`): 12-byte index entries and 12-byte
//! navigation commands. This is an authoring path, so it is covered by unit
//! tests for the bytes and by libbluray opening the merged tree, not by a
//! hardware player.

use std::path::{Path, PathBuf};

use crate::CoreError;

/// HDMV index entry: object type 1 (HDMV), access type 0, playback type 0
/// (movie), movie object id at bytes 6-7.
fn index_entry(object_id: u16) -> [u8; 12] {
    let mut e = [0u8; 12];
    e[0] = 0x40; // object_type=1, access_type=0
    e[6] = (object_id >> 8) as u8;
    e[7] = object_id as u8;
    e
}

/// BD-J index entry: object type 2, playback type 3 (interactive), and the
/// five-digit BDJO name at bytes 6-10.
fn index_entry_bdj(name: &str) -> [u8; 12] {
    let mut e = [0u8; 12];
    e[0] = 0x80; // object_type=2 (BD-J), access_type=0
    e[4] = 0xC0; // playback_type=3 (interactive)
    let b = name.as_bytes();
    for (i, slot) in e[6..11].iter_mut().enumerate() {
        *slot = *b.get(i).unwrap_or(&b'0');
    }
    e
}

/// The common header: version, app info, then the index body at 0x4e.
fn wrap_index(body: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(0x4e + 4 + body.len());
    out.extend_from_slice(b"INDX0200");
    out.extend_from_slice(&0x4eu32.to_be_bytes()); // IndexesStartAddress
    out.extend_from_slice(&0u32.to_be_bytes()); // extension data
    out.resize(0x28, 0); // reserved
    out.extend_from_slice(&34u32.to_be_bytes()); // AppInfoLength
                                                 // AppInfo: reserved(1) output_mode_preference(1) content_exist(1)
                                                 // reserved(1) dynamic_range(4) video_format(4) frame_rate(4), then
                                                 // 32 bytes of user data. 2D output, SDR, 1080p, 29.97 Hz.
    let mut info = [0u8; 34];
    info[0] = 0x40; // output_mode_preference = 2D (bit 14)
    info[1] = 0x64; // video_format 6, frame_rate 4
    out.extend_from_slice(&info);
    debug_assert_eq!(out.len(), 0x4e);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// `index.bdmv`: first playback, top menu, and one title per entry.
pub fn write_index(first_play: u16, top_menu: u16, title_objects: &[u16]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&index_entry(first_play));
    body.extend_from_slice(&index_entry(top_menu));
    body.extend_from_slice(&(title_objects.len() as u16).to_be_bytes());
    for id in title_objects {
        body.extend_from_slice(&index_entry(*id));
    }
    wrap_index(body)
}

/// `index.bdmv` whose first play and top menu are the BD-J menu `name`, with
/// one HDMV title object per entry.
pub fn write_index_bdj(name: &str, title_objects: &[u16]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&index_entry_bdj(name));
    body.extend_from_slice(&index_entry_bdj(name));
    body.extend_from_slice(&(title_objects.len() as u16).to_be_bytes());
    for id in title_objects {
        body.extend_from_slice(&index_entry(*id));
    }
    wrap_index(body)
}

/// `PlayPL playlist` command (branch/play, two immediate operands).
fn cmd_play_pl(playlist: u32) -> [u8; 12] {
    let mut c = [0u8; 12];
    c[0] = 0x42; // op_cnt=2, group=BRANCH, sub=PLAY
    c[1] = 0xC0; // imm1, imm2, branch_opt=INSN_PLAY_PL
    c[4..8].copy_from_slice(&playlist.to_be_bytes());
    c
}

/// `Jump title 0` command (branch/jump, one immediate operand).
fn cmd_jump_title(title: u32) -> [u8; 12] {
    let mut c = [0u8; 12];
    c[0] = 0x21; // op_cnt=1, group=BRANCH, sub=JUMP
    c[1] = 0x81; // imm1, branch_opt=INSN_JUMP_TITLE
    c[4..8].copy_from_slice(&title.to_be_bytes());
    c
}

/// `Break` command: stop this movie object.
fn cmd_break() -> [u8; 12] {
    let mut c = [0u8; 12];
    c[1] = 0x02; // group=BRANCH, sub=GOTO, branch_opt=INSN_BREAK
    c
}

fn push_object(out: &mut Vec<u8>, cmds: &[[u8; 12]]) {
    out.extend_from_slice(&0x8000u16.to_be_bytes()); // flags
    out.extend_from_slice(&(cmds.len() as u16).to_be_bytes());
    for c in cmds {
        out.extend_from_slice(c);
    }
}

/// `MovieObject.bdmv` for `titles` titles.
///
/// Object `i` plays playlist `i`; object `titles` is first playback and
/// `titles + 1` is the top menu (both jump to title 0 - there is no drawn menu).
pub fn write_movie_objects(titles: usize) -> Vec<u8> {
    let mut objects = Vec::new();
    for i in 0..titles {
        push_object(&mut objects, &[cmd_play_pl(i as u32), cmd_break()]);
    }
    push_object(&mut objects, &[cmd_jump_title(0), cmd_break()]); // first play
    push_object(&mut objects, &[cmd_jump_title(0), cmd_break()]); // top menu

    let mut out = Vec::with_capacity(0x2c + objects.len());
    out.extend_from_slice(b"MOBJ0200");
    out.extend_from_slice(&0u32.to_be_bytes()); // extension data
    out.resize(0x28, 0);
    // data_len covers everything after this field: reserved u32 + count u16 + objects
    out.extend_from_slice(&((6 + objects.len()) as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // reserved
    out.extend_from_slice(&((titles + 2) as u16).to_be_bytes());
    out.extend_from_slice(&objects);
    out
}

/// Replace every occurrence of a 5-digit clip name inside a playlist.
pub fn rename_clip(data: &[u8], old: &[u8; 5], new: &[u8; 5]) -> Vec<u8> {
    let mut out = data.to_vec();
    let mut i = 0;
    while i + 5 <= out.len() {
        if &out[i..i + 5] == old {
            out[i..i + 5].copy_from_slice(new);
        }
        i += 1;
    }
    out
}

fn five(n: usize) -> [u8; 5] {
    let s = format!("{n:05}");
    let b = s.as_bytes();
    [b[0], b[1], b[2], b[3], b[4]]
}

/// An authored BD-J title menu.
#[derive(Debug, Clone)]
pub struct MenuSpec {
    /// Heading drawn at the top of the menu.
    pub heading: String,
    /// Disc label drawn under the heading.
    pub label: String,
    /// One row per title: display text and playlist number.
    pub entries: Vec<(String, u32)>,
    /// Prebuilt generic menu Xlet (`00000.jar`).
    pub jar: PathBuf,
    /// Optional background image, copied to `BDMV/JAR/menu.png`.
    pub background: Option<PathBuf>,
}

impl MenuSpec {
    /// `menu.txt` consumed by the Xlet at runtime.
    pub fn text(&self) -> String {
        let clean = |s: &str| s.replace('|', "-").replace('\n', " ");
        let mut out = format!("{}|{}\n", clean(&self.heading), clean(&self.label));
        for (name, playlist) in &self.entries {
            out.push_str(&format!("{}|{}\n", clean(name), playlist));
        }
        if self.background.is_some() {
            out.push_str("bg=menu.png\n");
        }
        out
    }
}

/// Write the BD-J menu files and rewrite `index.bdmv`/`MovieObject.bdmv` so the
/// menu is first play and top menu. `titles` HDMV objects play playlists
/// `00000..`; the menu itself starts them through `bd://`.
pub fn install_menu(root: &Path, menu: &MenuSpec, titles: usize) -> Result<(), CoreError> {
    let bdmv = root.join("BDMV");
    let backup = bdmv.join("BACKUP");
    for d in ["BDJO", "JAR"] {
        std::fs::create_dir_all(bdmv.join(d))?;
        std::fs::create_dir_all(backup.join(d))?;
    }
    if !menu.jar.is_file() {
        return Err(CoreError::Disc(format!(
            "menu JAR not found: {} (run scripts/build-menu.sh)",
            menu.jar.display()
        )));
    }

    let app = crate::bdjo::App::new("Menu");
    let bdjo = crate::bdjo::write(&app)?;
    std::fs::write(bdmv.join("BDJO/00000.bdjo"), &bdjo)?;
    std::fs::write(backup.join("BDJO/00000.bdjo"), &bdjo)?;

    for dir in [bdmv.join("JAR"), backup.join("JAR")] {
        std::fs::copy(&menu.jar, dir.join("00000.jar"))?;
        std::fs::write(dir.join("menu.txt"), menu.text())?;
        if let Some(bg) = &menu.background {
            std::fs::copy(bg, dir.join("menu.png"))?;
        }
    }

    let objects: Vec<u16> = (0..titles as u16).collect();
    let index = write_index_bdj("00000", &objects);
    let movie = write_movie_objects(titles);
    std::fs::write(bdmv.join("index.bdmv"), &index)?;
    std::fs::write(bdmv.join("MovieObject.bdmv"), &movie)?;
    std::fs::write(backup.join("index.bdmv"), &index)?;
    std::fs::write(backup.join("MovieObject.bdmv"), &movie)?;
    Ok(())
}

/// Copy a staged disc root (`BDMV` plus `CERTIFICATE`) to `dest`.
pub fn export_tree(src: &Path, dest: &Path) -> Result<(), CoreError> {
    std::fs::create_dir_all(dest)?;
    copy_tree(&src.join("BDMV"), &dest.join("BDMV"))?;
    copy_tree(&src.join("CERTIFICATE"), &dest.join("CERTIFICATE"))?;
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), CoreError> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(&path, dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Merge per-title BDMV trees into one disc at `dest`.
///
/// `parts[i]` is a disc root (the directory containing `BDMV`, as tsMuxeR
/// writes it). Playlists and clips are renumbered to `i`; index and movie
/// objects are generated. When `menu` is given, a BD-J menu is installed as
/// first play and top menu.
pub fn merge<F>(
    parts: &[PathBuf],
    dest: &Path,
    menu: Option<&MenuSpec>,
    mut progress: F,
) -> Result<(), CoreError>
where
    F: FnMut(f64),
{
    if parts.is_empty() {
        return Err(CoreError::Disc("nothing to author".into()));
    }
    let dest_bdmv = dest.join("BDMV");
    for d in [
        "AUXDATA", "BACKUP", "BDJO", "CLIPINF", "JAR", "META", "PLAYLIST", "STREAM",
    ] {
        std::fs::create_dir_all(dest_bdmv.join(d))?;
    }

    for (i, part) in parts.iter().enumerate() {
        let src = part.join("BDMV");
        let name = five(i);

        // One playlist per input (tsMuxeR names it 00000).
        let playlist = src.join("PLAYLIST").join("00000.mpls");
        let data = std::fs::read(&playlist).map_err(|e| {
            CoreError::Disc(format!(
                "{}: {e} (tsMuxeR did not write a playlist)",
                playlist.display()
            ))
        })?;

        // Renumber every clip of this part and patch the playlist references.
        let mut clips: Vec<PathBuf> = std::fs::read_dir(src.join("STREAM"))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "m2ts"))
            .collect();
        clips.sort();
        let mut patched = data;
        for (j, clip) in clips.iter().enumerate() {
            let old = clip
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.as_bytes().get(0..5))
                .map(|b| [b[0], b[1], b[2], b[3], b[4]])
                .ok_or_else(|| CoreError::Disc(format!("odd clip name {}", clip.display())))?;
            // Clip names stay unique across the merged disc.
            let new = five(i * 100 + j);
            patched = rename_clip(&patched, &old, &new);
            std::fs::copy(
                clip,
                dest_bdmv
                    .join("STREAM")
                    .join(format!("{}.m2ts", String::from_utf8_lossy(&new))),
            )?;
            let clpi = src
                .join("CLIPINF")
                .join(format!("{}.clpi", String::from_utf8_lossy(&old)));
            if clpi.is_file() {
                std::fs::copy(
                    &clpi,
                    dest_bdmv
                        .join("CLIPINF")
                        .join(format!("{}.clpi", String::from_utf8_lossy(&new))),
                )?;
            }
        }
        std::fs::write(
            dest_bdmv
                .join("PLAYLIST")
                .join(format!("{}.mpls", String::from_utf8_lossy(&name))),
            patched,
        )?;
        progress((i + 1) as f64 / parts.len() as f64 * 90.0);
    }

    // One title per input, each with its own movie object.
    let title_objects: Vec<u16> = (0..parts.len() as u16).collect();
    let first_play = parts.len() as u16;
    let top_menu = parts.len() as u16 + 1;
    let index = write_index(first_play, top_menu, &title_objects);
    let objects = write_movie_objects(parts.len());
    std::fs::write(dest_bdmv.join("index.bdmv"), &index)?;
    std::fs::write(dest_bdmv.join("MovieObject.bdmv"), &objects)?;
    // Players fall back to BACKUP when the primary files are unreadable.
    let backup = dest_bdmv.join("BACKUP");
    std::fs::write(backup.join("index.bdmv"), &index)?;
    std::fs::write(backup.join("MovieObject.bdmv"), &objects)?;

    // Carry the first disc's metadata and any certificate folder.
    copy_tree(&parts[0].join("BDMV").join("META"), &dest_bdmv.join("META"))?;
    copy_tree(&parts[0].join("CERTIFICATE"), &dest.join("CERTIFICATE"))?;

    if let Some(menu) = menu {
        install_menu(dest, menu, parts.len())?;
    }
    progress(100.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be16(b: &[u8], o: usize) -> u16 {
        u16::from_be_bytes([b[o], b[o + 1]])
    }

    #[test]
    fn index_round_trips_through_the_parser() {
        let data = write_index(3, 4, &[0, 1, 2]);
        let nav = crate::menu::parse_index(&data).expect("parsed");
        assert_eq!(nav.first_playback.object, Some(3));
        assert_eq!(nav.top_menu.object, Some(4));
        assert_eq!(nav.titles.len(), 3);
        assert_eq!(nav.titles[0].object, Some(0));
        assert_eq!(nav.titles[2].object, Some(2));
        assert_eq!(nav.titles[0].kind, crate::menu::NavType::Hdmv);
    }

    #[test]
    fn movie_objects_are_playpl_plus_break() {
        let data = write_movie_objects(3);
        assert_eq!(&data[0..8], b"MOBJ0200");
        assert_eq!(be16(&data, 0x30) as usize, 3 + 2); // title objects + first play + top menu
        let objects_len =
            u32::from_be_bytes([data[0x28], data[0x29], data[0x2a], data[0x2b]]) as usize;
        assert_eq!(objects_len, data.len() - 0x2c);

        // Object 0: PlayPL playlist 0, then Break.
        let obj = 0x32;
        assert_eq!(be16(&data, obj), 0x8000);
        assert_eq!(be16(&data, obj + 2), 2);
        let cmd = obj + 4;
        assert_eq!(data[cmd], 0x42);
        assert_eq!(data[cmd + 1], 0xC0);
        assert_eq!(&data[cmd + 4..cmd + 8], &0u32.to_be_bytes());
        let brk = cmd + 12;
        assert_eq!(data[brk + 1], 0x02);

        // Object 1 plays playlist 1 (each object is 4 + 2*12 bytes).
        let obj1 = obj + 28;
        assert_eq!(&data[obj1 + 4 + 4..obj1 + 4 + 8], &1u32.to_be_bytes());
        let first_play = obj + 3 * 28;
        assert_eq!(data[first_play + 4], 0x21); // Jump title
        assert_eq!(data[first_play + 5], 0x81);
    }

    #[test]
    fn installing_a_menu_makes_it_first_play() {
        let dir = std::env::temp_dir().join(format!("keepsake-bdmv-menu-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("BDMV/PLAYLIST")).unwrap();
        std::fs::write(dir.join("BDMV/PLAYLIST/00000.mpls"), b"MPLS").unwrap();
        std::fs::write(dir.join("BDMV/PLAYLIST/00001.mpls"), b"MPLS").unwrap();
        let jar = dir.join("menu.jar");
        std::fs::write(&jar, b"PK\x03\x04").unwrap();

        let menu = MenuSpec {
            heading: "Keepsake Atelier".into(),
            label: "TEST".into(),
            entries: vec![("One".into(), 0), ("Two".into(), 1)],
            jar,
            background: None,
        };
        install_menu(&dir, &menu, 2).unwrap();

        let index = std::fs::read(dir.join("BDMV/index.bdmv")).unwrap();
        let nav = crate::menu::parse_index(&index).expect("parses");
        assert_eq!(nav.first_playback.kind, crate::menu::NavType::Bdj);
        assert_eq!(nav.first_playback.name.as_deref(), Some("00000"));
        assert_eq!(nav.top_menu.kind, crate::menu::NavType::Bdj);
        assert_eq!(nav.titles.len(), 2);
        assert_eq!(nav.titles[0].kind, crate::menu::NavType::Hdmv);
        assert!(dir.join("BDMV/BDJO/00000.bdjo").is_file());
        assert!(dir.join("BDMV/JAR/00000.jar").is_file());
        assert!(dir.join("BDMV/BACKUP/BDJO/00000.bdjo").is_file());
        assert!(dir.join("BDMV/BACKUP/JAR/00000.jar").is_file());
        let text = std::fs::read_to_string(dir.join("BDMV/JAR/menu.txt")).unwrap();
        assert!(text.starts_with("Keepsake Atelier|TEST\n"));
        assert!(text.contains("Two|1\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renames_every_clip_reference() {
        let mut data = vec![0u8; 32];
        data[3..8].copy_from_slice(b"00000");
        data[20..25].copy_from_slice(b"00000");
        let out = rename_clip(&data, b"00000", b"00007");
        assert_eq!(&out[3..8], b"00007");
        assert_eq!(&out[20..25], b"00007");
    }
}
