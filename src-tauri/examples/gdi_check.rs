// Dev harness: exercise the GDI BitBlt capture fallback directly.
// Usage: cargo run --example gdi_check -- <x> <y> <w> <h> <out.png>

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        eprintln!("usage: gdi_check <x> <y> <w> <h> <out.png>");
        std::process::exit(1);
    }
    let x: i32 = args[0].parse().unwrap();
    let y: i32 = args[1].parse().unwrap();
    let w: u32 = args[2].parse().unwrap();
    let h: u32 = args[3].parse().unwrap();
    match tagfix::capture::capture_region_gdi(x, y, w, h, std::path::Path::new(&args[4])) {
        Ok(()) => println!("gdi capture ok"),
        Err(e) => {
            eprintln!("gdi capture failed: {}", e);
            std::process::exit(1);
        }
    }
}
