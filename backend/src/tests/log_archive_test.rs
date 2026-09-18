use std::{
    fs,
    io::Cursor,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::handlers::log_archive::build_log_archive;

#[test]
fn log_archive_contains_only_configured_application_logs() {
    let directory = std::env::temp_dir().join(format!(
        "stellar-log-archive-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&directory).unwrap();
    let log_file = directory.join("stellar.log");
    fs::write(&log_file, "current log").unwrap();
    fs::write(directory.join("stellar.20260917"), "rotated log").unwrap();
    fs::write(directory.join("stellar.db"), "must not be archived").unwrap();
    fs::write(directory.join("other.20260917"), "must not be archived").unwrap();

    let bytes = build_log_archive(&log_file)
        .unwrap()
        .expect("archive with matching logs");
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();

    assert_eq!(archive.len(), 2);
    assert!(archive.by_name("stellar.log").is_ok());
    assert!(archive.by_name("stellar.20260917").is_ok());
    assert!(archive.by_name("stellar.db").is_err());
    assert!(archive.by_name("other.20260917").is_err());

    // An empty log directory yields no archive instead of an error.
    let empty_dir = directory.join("empty");
    fs::create_dir_all(&empty_dir).unwrap();
    assert!(
        build_log_archive(&empty_dir.join("stellar.log"))
            .unwrap()
            .is_none()
    );

    fs::remove_dir_all(directory).unwrap();
}
