use std::path::{Path, PathBuf};

pub(crate) fn directory(resource_root: &Path, relative: &str) -> PathBuf {
    let packaged = resource_root.join(relative);
    // Release binaries must neither embed the build machine's checkout path nor
    // load resources from it when an installed resource is missing.
    #[cfg(debug_assertions)]
    if !packaged.is_dir() {
        return Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(relative);
    }
    packaged
}

#[cfg(test)]
mod tests {
    use super::directory;
    #[cfg(debug_assertions)]
    use std::path::Path;

    #[test]
    fn existing_packaged_directory_wins() {
        let current = std::env::current_dir().unwrap();
        assert_eq!(directory(&current, "."), current.join("."));
    }

    #[test]
    fn missing_resource_uses_only_the_compiled_build_policy() {
        let absent_root =
            std::env::temp_dir().join(format!("omicsops-absent-resources-{}", std::process::id()));
        assert!(!absent_root.exists());
        let relative = "browser-extension";
        let actual = directory(&absent_root, relative);
        #[cfg(debug_assertions)]
        assert_eq!(
            actual,
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join(relative)
        );
        #[cfg(not(debug_assertions))]
        assert_eq!(actual, absent_root.join(relative));
    }
}
