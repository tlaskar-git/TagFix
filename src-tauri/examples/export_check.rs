// Dev harness: run the real export pipeline against a sweep folder.
// Usage: cargo run --example export_check -- <sweeps-root> <sweep-dir-name>
//
// Targets come from the settings.json beside the exe, so a run here writes
// the same target copies a real Export would.

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("usage: export_check <root> <dir>");
    let dir = args.next().expect("usage: export_check <root> <dir>");
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let settings = tagfix::settings::load(&exe_dir);
    match tagfix::export::export_sweep_files(
        std::path::Path::new(&root),
        &dir,
        &settings.targets,
    ) {
        Ok(result) => {
            println!("{}", result.pointer);
            for target_dir in result.target_dirs {
                println!("target copy: {}", target_dir);
            }
        }
        Err(e) => {
            eprintln!("export failed: {}", e);
            std::process::exit(1);
        }
    }
}
