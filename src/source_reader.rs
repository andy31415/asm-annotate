//! Provides a utility for reading lines from source files with path remapping.

use color_eyre::eyre::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

/// Reads source files, allowing for path remappings.
///
/// This is useful when debug information contains paths that don't directly
/// match the local filesystem, for example, paths from a build server.
#[derive(Debug, Default)]
pub struct SourceReader {
    remappings: Vec<(PathBuf, PathBuf)>,
}

impl SourceReader {
    /// Creates a new `SourceReader` with the given path remappings.
    ///
    /// Remappings are provided as a flat list of strings, alternating
    /// between the old prefix and the new prefix.
    ///
    /// # Arguments
    ///
    /// * `remaps` - A slice of strings representing `OLD_PREFIX NEW_PREFIX` pairs.
    ///   Example: `["/build/path", "/local/path", "/other/build", "/other/local"]`
    ///
    /// # Returns
    ///
    /// A `Result` containing the `SourceReader` or an error if remaps are not balanced.
    pub fn new(remaps: &[String]) -> Result<Self> {
        let mut remappings = Vec::new();
        if !remaps.len().is_multiple_of(2) {
            return Err(color_eyre::eyre::eyre!(
                "Invalid remap arguments: must be pairs of OLD NEW"
            ));
        }
        for i in (0..remaps.len()).step_by(2) {
            remappings.push((PathBuf::from(&remaps[i]), PathBuf::from(&remaps[i + 1])));
        }
        log::debug!("SourceReader created with remappings: {:?}", remappings);
        Ok(Self { remappings })
    }

    // Applies the configured remappings to the given path.
    fn apply_remaps(&self, path: &str) -> PathBuf {
        let original_path = PathBuf::from(path);
        for (old_prefix, new_prefix) in &self.remappings {
            if let Ok(stripped) = original_path.strip_prefix(old_prefix) {
                let remapped = new_prefix.join(stripped);
                log::debug!("Path remapped: {} -> {}", path, remapped.display());
                return remapped;
            }
        }
        original_path
    }

    /// Reads a specific line from a source file, applying path remappings.
    ///
    /// Line numbers are 1-based.
    ///
    /// # Arguments
    ///
    /// * `file_path` - The original path to the source file (before remapping).
    /// * `line_number` - The 1-based line number to read.
    ///
    /// # Returns
    ///
    /// A `Result` containing `Some(line_content)` if the line is read successfully,
    /// `Ok(None)` if the file is not found, the line number is out of bounds, or line_number is 0.
    /// Returns an error if there is an IO issue while reading the file.
    pub fn read_line(&self, file_path: &str, line_number: usize) -> Result<Option<String>> {
        if line_number == 0 {
            return Ok(None); // Line numbers are 1-based
        }
        let remapped_path = self.apply_remaps(file_path);
        let file = match File::open(&remapped_path) {
            Ok(f) => f,
            Err(e) => {
                log::warn!(
                    "Failed to open source file {}: {}",
                    remapped_path.display(),
                    e
                );
                return Ok(None); // Don't error out the whole process if one file is missing
            }
        };
        let reader = BufReader::new(file);
        match reader.lines().nth(line_number - 1) {
            Some(Ok(line)) => Ok(Some(line.trim_end().to_string())),
            Some(Err(e)) => Err(e).wrap_err(format!(
                "Error reading line {} from {}",
                line_number,
                remapped_path.display()
            )),
            None => {
                log::warn!(
                    "Line number {} not found in {}",
                    line_number,
                    remapped_path.display()
                );
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_apply_remaps() -> Result<()> {
        let reader =
            SourceReader::new(&vec!["/old/prefix".to_string(), "/new/prefix".to_string()])?;
        assert_eq!(
            reader.apply_remaps("/old/prefix/file.c"),
            PathBuf::from("/new/prefix/file.c")
        );
        assert_eq!(
            reader.apply_remaps("/other/path/file.c"),
            PathBuf::from("/other/path/file.c")
        );

        let reader2 = SourceReader::new(&vec!["src".to_string(), "dist".to_string()])?;
        assert_eq!(
            reader2.apply_remaps("src/app/main.c"),
            PathBuf::from("dist/app/main.c")
        );
        Ok(())
    }

    #[test]
    fn test_read_line() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("test.txt");
        fs::write(
            &file_path,
            "Line 1
Line 2
Line 3",
        )?;

        let reader = SourceReader::default();
        assert_eq!(
            reader.read_line(file_path.to_str().unwrap(), 1)?.unwrap(),
            "Line 1"
        );
        assert_eq!(
            reader.read_line(file_path.to_str().unwrap(), 2)?.unwrap(),
            "Line 2"
        );
        assert_eq!(
            reader.read_line(file_path.to_str().unwrap(), 3)?.unwrap(),
            "Line 3"
        );
        assert!(reader.read_line(file_path.to_str().unwrap(), 4)?.is_none());
        assert!(reader.read_line(file_path.to_str().unwrap(), 0)?.is_none());

        Ok(())
    }

    #[test]
    fn test_read_line_with_remap() -> Result<()> {
        let dir = tempdir()?;
        let old_dir = dir.path().join("old");
        let new_dir = dir.path().join("new");
        fs::create_dir(&old_dir)?;
        fs::create_dir(&new_dir)?;

        let file_path = new_dir.join("test.txt");
        fs::write(&file_path, "Remapped Line 1")?;

        let reader = SourceReader::new(&vec![
            old_dir.to_str().unwrap().to_string(),
            new_dir.to_str().unwrap().to_string(),
        ])?;

        let old_file_path = old_dir.join("test.txt");
        assert_eq!(
            reader
                .read_line(old_file_path.to_str().unwrap(), 1)?
                .unwrap(),
            "Remapped Line 1"
        );

        Ok(())
    }
}
