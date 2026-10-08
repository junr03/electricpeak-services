fn main() {
    if let Err(error) = rawbackup_status::run() {
        eprintln!("rawbackup status collection failed: {error}");
        std::process::exit(1);
    }
}
