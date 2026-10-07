pub mod detector;

use std::path::{Path, PathBuf};

pub fn project_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

pub fn project_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_owned()
    } else {
        project_dir().join(path)
    }
}

pub fn default_runtime_path() -> PathBuf {
    project_path(if cfg!(target_os = "macos") {
        "models/libonnxruntime.dylib"
    } else if cfg!(target_os = "windows") {
        "models/onnxruntime.dll"
    } else {
        "models/libonnxruntime.so"
    })
}
