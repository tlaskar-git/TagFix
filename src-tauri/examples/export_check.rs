// Dev harness: run the real export pipeline against a sweep folder.
// Usage: cargo run --example export_check -- <sweeps-root> <sweep-dir-name>

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("usage: export_check <root> <dir>");
    let dir = args.next().expect("usage: export_check <root> <dir>");
    match tagfix::export::export_sweep_files(std::path::Path::new(&root), &dir) {
        Ok(pointer) => println!("{}", pointer),
        Err(e) => {
            eprintln!("export failed: {}", e);
            std::process::exit(1);
        }
    }
}
