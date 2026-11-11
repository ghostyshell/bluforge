//! Portable UDF 2.50 image creation.
//!
//! macOS uses `newfs_udf` plus `hdiutil` in [`crate::udf`], which produces the
//! metadata partition that BD-RE expects. Linux and Windows have no such
//! writer, so this module builds a mastered UDF 2.50 image with `hadris-cd`.
//!
//! The output is a UDF-only, Type-1 image. It is the BD-R profile: a metadata
//! partition is not written, because the writer does not implement one. BD-RE
//! media still wants the macOS path, but BD-R and most players accept this.

use std::path::Path;

use hadris_cd::{FileTree, OpticalImageOptions, OpticalImageWriter};
use hadris_udf::UdfRevision;

use crate::CoreError;

/// Force the portable writer even where `newfs_udf` exists.
pub fn forced() -> bool {
    std::env::var_os("KEEPSAKE_PORTABLE_UDF").is_some()
}

/// A volume label that fits UDF's 30-character dstring.
pub fn label(s: &str) -> String {
    let upper = crate::udf::label(s);
    upper.chars().take(30).collect()
}

/// Package `tree` (a staged disc root containing `BDMV/`) as a UDF 2.50 image.
///
/// File data is streamed from disk, so a 50 GB tree does not need 50 GB of
/// memory. The destination is truncated and rewritten.
pub fn create_from_tree(tree: &Path, dest: &Path, volume: &str) -> Result<(), CoreError> {
    if !tree.is_dir() {
        return Err(CoreError::Disc(format!(
            "disc tree is not a directory: {}",
            tree.display()
        )));
    }
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CoreError::Disc(format!("create {}: {e}", parent.display())))?;
        }
    }

    let file_tree = FileTree::from_fs(tree)
        .map_err(|e| CoreError::Disc(format!("read {}: {e}", tree.display())))?;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(dest)
        .map_err(|e| CoreError::Disc(format!("create {}: {e}", dest.display())))?;

    let mut options = OpticalImageOptions::default()
        .volume_id(label(volume))
        .udf_only();
    options.udf.revision = UdfRevision::V2_50;

    OpticalImageWriter::new(file, options)
        .finish(file_tree)
        .map_err(|e| CoreError::Disc(format!("UDF 2.50 image: {e}")))?;

    let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        return Err(CoreError::Disc(
            "the UDF writer produced an empty image".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_udf_image_from_a_tree() {
        let root = std::env::temp_dir().join(format!("keepsake-udf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("BDMV/PLAYLIST")).unwrap();
        std::fs::write(root.join("BDMV/index.bdmv"), vec![0u8; 4096]).unwrap();
        std::fs::write(root.join("BDMV/PLAYLIST/00000.mpls"), vec![1u8; 2048]).unwrap();

        let dest = std::env::temp_dir().join(format!("keepsake-udf-{}.iso", std::process::id()));
        create_from_tree(&root, &dest, "KEEPSAKE TEST").unwrap();
        let size = std::fs::metadata(&dest).unwrap().len();
        assert!(size >= 2048 * 32, "image too small: {size}");

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn clamps_the_volume_label() {
        assert!(label(&"x".repeat(80)).chars().count() <= 30);
    }
}
