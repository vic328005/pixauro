//! 应用图标由 tools/generate-icons.sh 从正式 SVG 生成并随源码保存。
//! 构建时直接使用这些资源，避免覆盖设计文件生成的渐变、阴影和透明边缘。
fn main() {
    tauri_build::build();
}
