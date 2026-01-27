use std::fs;

use eth_btc_strategy::logging::{FileLogger, RotationConfig};
use uuid::Uuid;

#[test]
fn file_logger_rotates_when_exceeding_size() {
    let dir = std::env::temp_dir().join(format!("eth_btc_logs_{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("strategy.log");

    let mut logger = FileLogger::new(
        path.clone(),
        RotationConfig {
            max_bytes: 10,
            max_files: 2,
        },
    )
    .unwrap();
    logger.write_line("first-line").unwrap();
    logger.write_line("second-line").unwrap();

    let rotated = path.with_extension("log.1");
    assert!(rotated.exists());
}
