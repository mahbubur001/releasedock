//! Creates (or opens) a ReleaseDock data folder and adds one plugin product, for local
//! end-to-end checks of the desktop app against a `file://` test repository.
//!
//! `cargo run -p rd-store --example seed_product -- <data dir> <svn url> <working copy> <dev folder> [slug] [name] [current version]`
//! Prints the new product's id.

use rd_model::{Product, ProductType};
use rd_store::{AppPaths, ProductRepository, Store};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!(
            "usage: seed_product <data dir> <svn url> <working copy> <dev folder> [slug] [name] [current version]"
        );
        std::process::exit(2);
    }
    let slug = args.get(4).map_or("sample-plugin", String::as_str);
    let name = args.get(5).map_or("Sample Plugin", String::as_str);
    let paths = AppPaths::new(&args[0]);
    paths.create_all()?;
    let store = Store::open(&paths)?;
    let mut product = Product::new(name, slug, ProductType::Plugin, &args[1], &args[2]);
    product.local_development_path = Some(args[3].clone());
    product.current_version = Some(args.get(6).cloned().unwrap_or_else(|| "1.0.0".into()));
    store.save_product(&product)?;
    println!("{}", product.id);
    Ok(())
}
