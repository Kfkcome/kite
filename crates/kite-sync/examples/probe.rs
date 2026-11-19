//! Read-only check: resolves the token and looks for the kite-sync gist.
//! `cargo run -p kite-sync --example probe`
#![allow(clippy::print_stdout)]

fn main() {
    match kite_sync::token() {
        Err(e) => println!("token: {e}"),
        Ok((token, source)) => {
            println!("token from {source:?}");
            match kite_sync::Gist::new(token).fetch() {
                Ok(None) => println!("no kite-sync gist yet"),
                Ok(Some(b)) => println!(
                    "gist: hash {} from {} at {}",
                    b.hash, b.device, b.updated_at
                ),
                Err(e) => println!("fetch: {e}"),
            }
        }
    }
}
