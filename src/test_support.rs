use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unique_temp_path(scope: &str, name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "feishu-vscode-bridge-{scope}-tests-{name}-{}-{nonce}",
        std::process::id()
    ))
}