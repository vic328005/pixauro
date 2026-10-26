<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="editor/src/assets/pixauro-mark.svg">
    <img src="editor/src/assets/pixauro-mark-light.svg" width="54" height="54" alt="Pixauro">
  </picture>
</p>

# Pixauro

**用于像素画和精灵动画的桌面编辑器。**

[English](README.md) · 简体中文 · [日本語](README.ja.md)

从空白画布开始画，也可以把一张 PNG 整理成可编辑的动画帧。Pixauro 把绘画、调色板、精灵切片和图片像素化放在同一个工作区，还提供 CLI，方便在脚本中处理素材。

项目处于早期开发阶段，项目文件格式和 API 可能发生不兼容的变化。编辑器界面目前主要使用英文，README 提供中、英、日三种语言。

## 可以做什么

- **绘制与修改。** 使用画笔、橡皮擦、吸管、魔棒选区和 cel 移动工具，支持撤销、重做和历史记录浏览。cel 是某一帧、某一图层上的图像单元。
- **管理调色板。** 编辑整个项目共用的 RGBA 颜色，从 PNG 提取颜色，或将调色板导出为 PNG。修改一个颜色条目，使用它的像素会一起更新。
- **制作逐帧动画。** 一个项目可以包含多个动画，分别设置帧率；支持图层、洋葱皮预览，以及跨帧共享像素的链接 cel。
- **整理已有素材。** 将 PNG 切成多帧，调整裁切区域和偏移，降低分辨率和色数，预览去背结果后再导入。也可以直接保留裁切后的原始像素。
- **导出资源。** 将单帧导出为 PNG，或将动画导出为精灵图和 JSON 描述。CLI 还可以导出循环 GIF 预览。
- **调整工作区。** 移动和缩放面板，开关像素网格，切换深浅两种透明棋盘格。

像素化会逐帧独立处理。去背和切片自动定位的结果可能需要手动修正，这些功能不识别角色，也不保证帧间一致性。

## 从源码运行

需要先安装：

- 当前稳定版 Rust 工具链，包含 Cargo。
- Node.js 和 npm。前端支持的 Node 版本为 `^20.19.0 || ^22.13.0 || >=24`。
- 对应操作系统的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。

克隆仓库后，在仓库根目录执行：

```sh
npm --prefix editor ci
npm --prefix editor run desktop
```

`desktop` 会同时启动前端开发服务器和原生应用。直接在浏览器里打开前端，无法使用原生文件操作和编辑命令。

为当前平台构建桌面安装包：

```sh
npm --prefix editor run desktop:build
```

### 第一个项目

1. 新建项目，设置画布尺寸。
2. 选择调色板颜色开始画，或者把 PNG 拖到画布上，打开导入窗口。
3. 在时间轴中添加帧。导入精灵图时，每一行会成为一个动画；确认切片和播放效果后再导入。
4. 将项目保存为 JSON，保留图层、动画、调色板和编辑历史。需要图片或动画资源时，使用 **Export** 菜单。

## 命令行

CLI 与编辑器共用资源处理流程，无需启动桌面应用。下面的命令在仓库根目录执行，请将 `input.png` 和 `sheet.png` 换成自己的文件。

将图片像素化，尺寸上限为 64 × 64，最多使用 16 种 RGB 颜色：

```sh
cargo run --locked --release -p pixauro_cli -- pixelate input.png -o output.png --size 64x64 --colors 16 --background none
```

将一张两行、四列的精灵图拆成两个动画，保留裁切后的像素，并导出精灵图和 GIF 预览：

```sh
cargo run --locked --release -p pixauro_cli -- animate sheet.png -o output --rows 2 --columns 4 --size 64x64 --fps 10 --no-pixelate --background none --gif
```

使用 `--no-pixelate` 时，请将画布设得足够大，以容纳每个裁切区域。

需要指定动画名称、分别设置帧率或精确裁切区域时，可以使用 JSON 配置：

```sh
cargo run --locked --release -p pixauro_cli -- animate sheet.png -o output --config cli/examples/animation.json
cargo run --locked --release -p pixauro_cli -- --help
```

仓库提供了[图片像素化](cli/examples/pixelate.json)、[网格动画](cli/examples/animation.json)和[精确切片与偏移](cli/examples/slices.json)三份示例。命令行参数会覆盖配置；`--save-config recipe.json` 可将最终参数保存到新文件。进度和提示写入 stderr，输出文件路径写入 stdout。

## 文件格式

| 格式              | 用途                                                                               |
| ----------------- | ---------------------------------------------------------------------------------- |
| 项目 JSON         | 保存可编辑的作品、调色板、图层、动画和编辑历史。                                   |
| PNG               | 图片输入；单帧、调色板和精灵图输出。                                               |
| 精灵图 PNG + JSON | 导出的帧及其矩形区域、动画名称、帧顺序和 FPS，使用 `pixauro-spritesheet` v1 格式。 |
| GIF               | 通过 CLI 的 `--gif` 选项导出的循环动画预览。                                       |

项目 JSON 和精灵图 JSON 用途不同。需要继续编辑时，请保留项目文件。精灵图描述使用 Pixauro 自定义结构，接入其他工具时可能需要转换。

## 参与开发

Pixauro 使用 Rust 管理作品数据和处理图片，以 Tauri 2 作为桌面宿主，界面使用 React 和 TypeScript。

| 目录                    | 职责                                               |
| ----------------------- | -------------------------------------------------- |
| `engine/`               | 作品数据、编辑历史、图片读写和共用的资源处理流程。 |
| `editor/`               | React 界面，以及 `src-tauri/` 中的桌面宿主。       |
| `cli/`                  | 命令行像素化和动画导出。                           |
| `pixelart-abstraction/` | 支持透明度的像素化算法。                           |

修改前请阅读 [AGENTS.md](AGENTS.md)，涉及界面时也请阅读 [DESIGN.md](DESIGN.md)。这两份文档目前使用中文。在仓库根目录运行：

```sh
sh tools/check.sh
```

该脚本检查 Rust 格式和 Clippy、生成的 IPC 绑定、ESLint、Prettier、TypeScript 和前端构建，不运行自动化测试。功能变化需要在桌面应用或 CLI 中手动验证。

欢迎提交问题、小范围修复和文档改进。反馈问题时，请附上操作系统、构建所用的提交版本、复现步骤，以及方便分享的最小输入文件。较大的改动建议先开 issue 讨论范围。

## 算法说明

像素化模块根据 Gerstner 等人的 _Pixelated Image Abstraction with Integrated User Constraints_ 独立实现，并加入了透明度处理。实现选择及与论文的差异见[算法说明](pixelart-abstraction/docs/algorithm.md)。本地扩展未经过与论文方法相同的验证。

## 许可证

Pixauro 使用 [MIT 许可证](LICENSE)。`pixelart-abstraction` 模块保留[自己的 MIT 版权声明](pixelart-abstraction/LICENSE)。
