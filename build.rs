use std::{
    env, fs,
    path::{Path, PathBuf},
};

const ICON_SIZE: u32 = 32;
const MOUTH_PATH: &str = "M10 13a2 2 0 1 0 4 0h2a4 4 0 1 1-8 0h2z";

fn main() {
    println!("cargo:rerun-if-changed=assets/bear-smile-svgrepo-com.svg");

    let source = include_str!("assets/bear-smile-svgrepo-com.svg");
    assert!(
        source.contains(MOUTH_PATH),
        "bear SVG mouth path changed; update the tray icon transform"
    );
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));

    write_icon(source, &out_dir.join("happy-tray-icon.rgba"));

    let unhappy_source = source.replace(
        MOUTH_PATH,
        &format!(r#""/><path transform="rotate(180 12 14)" d="{MOUTH_PATH}"#),
    );
    write_icon(&unhappy_source, &out_dir.join("unhappy-tray-icon.rgba"));
}

fn write_icon(svg: &str, path: &Path) {
    let rgba = render_rgba(svg);
    fs::write(path, rgba).expect("failed to write generated tray icon");
}

fn render_rgba(svg: &str) -> Vec<u8> {
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &resvg::usvg::Options::default())
        .expect("failed to parse tray icon SVG");
    let source_size = tree.size().to_int_size();
    let scale = ICON_SIZE as f32 / source_size.width() as f32;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(ICON_SIZE, ICON_SIZE).expect("failed to create icon pixmap");

    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.data().to_vec()
}
