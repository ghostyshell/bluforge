//! `BDJO` (BD-J object) writing.
//!
//! A BD-J title is a small binary object in `BDMV/BDJO/xxxxx.bdjo` that names the
//! JAR to load (`base_dir`), the initial class, the accessible playlists and
//! the file-access mask. The index refers to it by its five-digit name.
//!
//! The layout follows libbluray's `bdjo_parse.c`, which is the only open
//! implementation available here. Bytes that the parser skips (the application
//! descriptor tag and the reserved address table) are written with the values
//! the spec uses, but only the fields libbluray reads are verified by tests.
//! This is an authoring aid: keep `--menu` opt-in because a real player may be
//! stricter than libbluray.

use std::path::PathBuf;

use crate::CoreError;

/// The prebuilt generic menu Xlet, from `KEEPSAKE_MENU_JAR`, the app resources,
/// the dev asset directory, or the data directory.
pub fn jar_path() -> Option<PathBuf> {
    if let Some(p) = crate::env_compat("MENU_JAR") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    crate::binaries::helper("00000.jar")
}

/// One BD-J application in the application management table.
#[derive(Debug, Clone)]
pub struct App {
    /// `00000` - the JAR is `BDMV/JAR/00000.jar`.
    pub jar: String,
    /// Java class to instantiate, e.g. `Menu`.
    pub initial_class: String,
    /// Human-readable application name.
    pub name: String,
    /// Accessible playlists; empty with `access_to_all` exposes every playlist.
    pub playlists: Vec<u16>,
    /// Allow the app to read/write the whole virtual file system (".").
    pub access_all_files: bool,
}

impl App {
    pub fn new(initial_class: impl Into<String>) -> Self {
        Self {
            jar: "00000".into(),
            initial_class: initial_class.into(),
            name: "Keepsake Atelier".into(),
            playlists: Vec::new(),
            access_all_files: true,
        }
    }

    fn five(&self) -> [u8; 5] {
        let mut out = [b'0'; 5];
        for (i, b) in self.jar.as_bytes().iter().take(5).enumerate() {
            out[i] = *b;
        }
        out
    }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// A length-prefixed application string: `len: u8`, bytes, padded to a word.
fn put_app_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(255);
    out.push(len as u8);
    out.extend_from_slice(&bytes[..len]);
    if len % 2 == 0 {
        out.push(0);
    }
}

/// Serialize one application entry.
fn app_entry(app: &App) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(1); // control code: autostart
    out.push(1); // type: BD-J application
    put_u32(&mut out, 0); // organization id
    put_u16(&mut out, 0); // application id

    // Application descriptor: tag "APPd", length, reserved.
    out.extend_from_slice(b"APPd");
    put_u32(&mut out, 2);
    put_u16(&mut out, 0);

    // One BD-ROM profile: number 1, version 1.0.0, byte aligned.
    out.push(0x10); // num_profile = 1, padding
    out.push(0x00);
    put_u16(&mut out, 1); // profile number (BD-ROM profile 1)
    out.extend_from_slice(&[1, 0, 0, 0]); // major, minor, micro, reserved

    out.push(0); // priority
    out.push(0x40); // binding = 1 (title bound), visibility = 0

    // Application names: one entry, lang + length + UTF-8.
    let name = app.name.as_bytes();
    let name = &name[..name.len().min(255)];
    let mut names = Vec::new();
    names.extend_from_slice(b"eng");
    names.push(name.len() as u8);
    names.extend_from_slice(name);
    put_u16(&mut out, names.len() as u16);
    out.extend_from_slice(&names);
    if names.len() % 2 == 1 {
        out.push(0);
    }

    put_app_string(&mut out, ""); // icon locator
    put_u16(&mut out, 0); // icon flags
    put_app_string(&mut out, &app.jar); // base dir
    put_app_string(&mut out, ""); // classpath extension
    put_app_string(&mut out, &app.initial_class);

    // Parameters: none. An even (zero) length is followed by one pad byte.
    out.push(0);
    out.push(0);

    out
}

/// Build `xxxxx.bdjo` for a single menu application.
pub fn write(app: &App) -> Result<Vec<u8>, CoreError> {
    if app.jar.len() > 5 || !app.jar.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CoreError::Disc(format!(
            "BD-J jar name must be five digits, got {:?}",
            app.jar
        )));
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"BDJO");
    out.extend_from_slice(b"0200");
    out.extend_from_slice(&[0u8; 40]); // address table

    // Terminal info: length, 5-byte default font ("*****" = none), flags, pad.
    put_u32(&mut out, 10);
    out.extend_from_slice(b"*****");
    out.push(0x03); // havi config 0, menu call + title search allowed
    out.extend_from_slice(&[0u8; 4]);

    // Application cache: one JAR, language "*.*".
    put_u32(&mut out, 14);
    out.push(1); // num_item
    out.push(0); // padding
    out.push(1); // type: JAR
    out.extend_from_slice(&app.five());
    out.extend_from_slice(b"*.*");
    out.extend_from_slice(&[0u8; 3]);

    // Accessible playlists. `access_to_all_flag` is set so the menu may start
    // any playlist; the explicit list still names the ones we authored.
    // `autostart_first_playlist_flag` stays 0 because the application
    // autostarts instead.
    put_u32(&mut out, (4 + app.playlists.len() * 6) as u32);
    let mut header = 1u32 << 20;
    header |= (app.playlists.len() as u32) << 21;
    put_u32(&mut out, header);
    for pl in &app.playlists {
        let mut name = [b'0'; 5];
        let s = format!("{:05}", pl);
        name.copy_from_slice(&s.as_bytes()[..5]);
        out.extend_from_slice(&name);
        out.push(0);
    }

    // Application management table.
    let entry = app_entry(app);
    put_u32(&mut out, (2 + entry.len()) as u32);
    out.push(1); // num_app
    out.push(0); // padding
    out.extend_from_slice(&entry);

    // Key interest table: no keys captured from the player.
    put_u32(&mut out, 0);

    // File access info: "." gives the application the whole VFS.
    let path = if app.access_all_files { "." } else { "" };
    put_u16(&mut out, path.len() as u16);
    out.extend_from_slice(path.as_bytes());

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be32(b: &[u8], o: usize) -> u32 {
        u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
    }

    #[test]
    fn header_and_sections_are_walkable() {
        let mut app = App::new("Menu");
        app.playlists = vec![0, 1];
        let data = write(&app).unwrap();
        assert_eq!(&data[0..8], b"BDJO0200");
        let mut p = 8 + 40;
        // Terminal info
        assert_eq!(be32(&data, p), 10);
        p += 4 + 10;
        // App cache
        assert_eq!(be32(&data, p), 14);
        p += 4 + 14;
        // Accessible playlists: 4-byte header + two 6-byte entries
        assert_eq!(be32(&data, p), 4 + 12);
        p += 4;
        assert_eq!(be32(&data, p) >> 21 & 0x7ff, 2);
        assert_eq!((be32(&data, p) >> 20) & 1, 1);
        p += 4;
        assert_eq!(&data[p..p + 5], b"00000");
        p += 6;
        assert_eq!(&data[p..p + 5], b"00001");
        p += 6;
        // App management table (one 70-byte entry)
        let table_len = be32(&data, p) as usize;
        assert_eq!(table_len, 2 + 70);
        assert_eq!(data.len(), p + 4 + table_len + 4 + 3); // + key interest + file access
        p += 4;
        assert_eq!(data[p], 1);
        p += 2;
        // App entry
        assert_eq!(data[p], 1); // autostart
        assert_eq!(data[p + 1], 1); // BD-J
        assert_eq!(&data[p + 8..p + 12], b"APPd");
        p += 18; // control .. descriptor reserved, up to num_profile
        assert_eq!(data[p] >> 4, 1); // num_profile
        p += 2 + 6 + 1 + 1;
        // Names block
        let name_len = u16::from_be_bytes([data[p], data[p + 1]]) as usize;
        assert_eq!(name_len, 4 + "Keepsake Atelier".len());
        p += 2 + name_len + (name_len % 2);
        // icon locator (empty, padded), flags, base dir, classpath, class
        assert_eq!(data[p], 0);
        p += 2;
        assert_eq!(&data[p..p + 2], &[0, 0]);
        p += 2;
        assert_eq!(data[p], 5);
        assert_eq!(&data[p + 1..p + 6], b"00000");
        p += 6;
        assert_eq!(data[p], 0);
        p += 2;
        assert_eq!(data[p], 4);
        assert_eq!(&data[p + 1..p + 5], b"Menu");
        p += 6;
        // params + key interest + file access
        assert_eq!(&data[p..p + 2], &[0, 0]);
        p += 2;
        assert_eq!(be32(&data, p), 0);
        p += 4;
        assert_eq!(u16::from_be_bytes([data[p], data[p + 1]]), 1);
        assert_eq!(data[p + 2], b'.');
        assert_eq!(p + 3, data.len());
    }

    #[test]
    fn rejects_a_bad_jar_name() {
        let mut app = App::new("Menu");
        app.jar = "menu".into();
        assert!(write(&app).is_err());
    }

    #[test]
    fn explicit_playlist_headers() {
        let mut app = App::new("Menu");
        app.playlists = vec![3];
        let data = write(&app).unwrap();
        let p = 8 + 40 + 4 + 10 + 4 + 14;
        let header = be32(&data, p + 4);
        assert_eq!(header >> 21 & 0x7ff, 1);
        assert_eq!(&data[p + 8..p + 13], b"00003");
    }
}
