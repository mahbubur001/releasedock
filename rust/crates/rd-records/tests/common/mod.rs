#![allow(dead_code)]

use rd_model::{Product, ProductType, ReleaseMode};

/// Swift's `makeProduct(type:slug:url:workingCopy:mode:automation:)` from `CoreTests.swift`.
pub fn make_product(
    product_type: ProductType,
    slug: &str,
    url: Option<&str>,
    working_copy: &str,
    mode: ReleaseMode,
    automation: bool,
) -> Product {
    let name = match product_type {
        ProductType::Plugin => "Sample Plugin",
        ProductType::Theme => "Sample Theme",
    };
    let host = match product_type {
        ProductType::Plugin => "plugins",
        ProductType::Theme => "themes",
    };
    let url = url.map_or_else(|| format!("https://{host}.svn.wordpress.org/{slug}"), str::to_owned);
    let mut product = Product::new(name, slug, product_type, &url, working_copy);
    product.release_mode = mode;
    product.automation_enabled = automation;
    product
}

/// `makeProduct()` with every default.
pub fn default_product() -> Product {
    make_product(
        ProductType::Plugin,
        "sample-plugin",
        None,
        "/tmp/wc",
        ReleaseMode::Assisted,
        false,
    )
}
