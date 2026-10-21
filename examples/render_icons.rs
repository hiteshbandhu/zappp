//! Renders the brand SVGs into the PNGs the app ships:
//!   cargo run --example render_icons
//! Writes brand/AppIcon.iconset/* (turned into AppIcon.icns by
//! scripts/bundle.sh) and assets/menubar@2x.png.

use std::{fs, path::Path};

use resvg::{tiny_skia, usvg};

fn render(svg: &Path, size: u32, out: &Path) {
    let data = fs::read(svg).expect("read svg");
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).expect("parse svg");
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("pixmap");
    let scale = size as f32 / tree.size().width();
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.save_png(out).expect("write png");
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let iconset = root.join("brand/AppIcon.iconset");
    fs::create_dir_all(&iconset).unwrap();
    for base in [16, 32, 128, 256, 512] {
        render(&root.join("brand/icon.svg"), base, &iconset.join(format!("icon_{base}x{base}.png")));
        render(&root.join("brand/icon.svg"), base * 2, &iconset.join(format!("icon_{base}x{base}@2x.png")));
    }
    render(&root.join("brand/icon.svg"), 1024, &root.join("brand/icon-1024.png"));
    render(&root.join("brand/menubar.svg"), 36, &root.join("assets/menubar@2x.png"));
    render(&root.join("brand/menubar-recording.svg"), 36, &root.join("assets/menubar-recording@2x.png"));
    println!("rendered icons");
}
