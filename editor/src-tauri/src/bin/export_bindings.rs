//! 不创建窗口、不启动 WebView 的真实 tauri-specta 代码生成入口。
fn main() {
    let path = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/generated/bindings.ts")
        });
    pixauro_editor::export_bindings(&path).expect("无法生成 TypeScript IPC 绑定");
    println!("已生成 {}", path.display());
}
