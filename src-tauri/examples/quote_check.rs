// Dev harness: prove the highlight grab by hand, because no unit test can
// hold a selection in another app.
//
// Usage: cargo run --release --example quote_check
// Then highlight some text in any window within three seconds.

fn main() {
    println!("highlight some text in any window; grabbing in 3 seconds");
    for n in (1..=3).rev() {
        println!("  {}", n);
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    match tagfix::quote::grab_selection() {
        Ok(grabbed) => {
            println!("text ({} chars):", grabbed.text.chars().count());
            println!("{}", grabbed.text);
            match grabbed.html {
                Some(html) => {
                    println!("\nhtml fragment ({} chars):", html.chars().count());
                    println!("{}", html);
                }
                None => println!("\nno CF_HTML on the clipboard"),
            }
            println!("\nthe clipboard should now hold whatever it held before");
        }
        Err(e) => {
            eprintln!("grab failed: {}", e);
            std::process::exit(1);
        }
    }
}
