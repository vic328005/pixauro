# 仓库协作约定

- 使用简体中文回答，Git 提交说明使用简体中文。
- 修改文件前，先读取从仓库根目录到目标文件所在目录沿途的 `AGENTS.md`；子目录规则仅适用于其范围，冲突时以更具体的规则为准。不要假定所有子目录指令已自动加载。
- `AGENTS.md` 只保留影响开发决策的约束与必要入口，不重复源码、操作教程或变更记录；只有确有独立约束时才新增下层文件。

## 实现原则

- 不要过度工程化。优先简单、直接的实现；不为假设中的需求增加泛型、trait、通用接口或额外封装。少量具体函数和适度重复可以接受，抽象必须有当前需求支撑。
- 文件名体现具体业务或功能，避免 `components`、`types`、`utils`、`state` 等笼统名称；语言或工具约定的入口和配置文件除外。改名同步更新引用，不额外拆层或改变行为。
- 当前允许破坏性修改，不为旧数据或接口保留兼容层、迁移逻辑；除非用户明确要求。

## 验证

- 当前不新增、维护或运行自动化测试，除非用户明确要求恢复；功能由用户手动运行验证，不能把静态检查通过说成功能验证通过。
- Rust 修改须运行 `cargo fmt`、`cargo clippy` 和必要编译检查。
- TypeScript / React 使用 ESLint 检查正确性、Prettier 检查格式。提交前实际通过零警告 lint 和格式检查；保持版本兼容，不忽略工具链兼容警告。
- 生成文件不手改，也不由 ESLint / Prettier 重写；明确排除，通过真实代码生成及结果比对验证。

- 图片、模型、音视频和字体直接存入 Git，不使用 Git LFS；二进制属性见 [.gitattributes](.gitattributes)。不要提交 LFS 指针文件。

# Pixauro 开发约定

## 职责边界

- workspace 包含 `engine/`、`editor/src-tauri/`、`cli/`、`pixelart-abstraction/`，共享锁文件和构建目录。engine 不依赖桌面框架；桌面与 CLI 共用资源流水线，CLI 不创建临时编辑器项目。
- Rust Engine 是正式作品、历史和修改状态的唯一权威；前端只保存只读快照、选择与临时交互状态。播放、布局和预览不进入作品历史。
- Rust DTO 是 IPC 类型的唯一来源；所有 ID 和 revision 用字符串跨 IPC。操作绑定开始时的会话、版本及目标，Rust 拒绝过期请求，提交时不得悄悄换目标。
- 界面修改先读 [DESIGN.md](DESIGN.md)，设计变量以 [tokens.css](editor/src/tokens.css) 为准。

## 编辑与数据

- 正式编辑先在临时作品执行和校验，成功后一次提交；无变化不入历史、不截断重做分支。删除或撤销不回退 ID 分配器，不复用身份。
- 每个帧和图层只归属一个动画，每个动画至少一层一帧；图层操作不影响其他动画。FPS 属于整个动画，保留小数，不下放为帧时长。
- 链接 cel 共用图像但偏移独立：改像素影响整组，移动只影响当前 cel。锁定保护由 engine 执行；全局调色板修改仍影响全部动画。
- 透明颜色引用与未绘制像素不同；半透明笔触直接替换颜色。画布外像素可以保留，导出时裁切；临时网格、选区和预览不得进入导出。
- 切换编辑目标、文件或历史操作前先提交笔画和移动等草稿。项目切换与关闭遵循保存/不保存/取消；文件操作失败保留原作品，成功后才更新会话和保存状态。
- 文件使用统一原子写入入口；多文件导出不是跨文件事务，失败须报告已保留的产物，不能声称整体原子。

## 资源处理

- 从原图裁切后求解，不用预览缩略图替代原图；透明度参与求解，隐藏 RGB 不参与可见颜色统计，颜色和 alpha 使用同一最终网格。
- 导入先处理临时数据，完成校验后一次提交作品和历史；失败或取消不留下部分导入，重做不依赖源文件。取消要等待运行任务退出并忽略迟到结果，异步完成顺序不能改变帧顺序。
- 修改求解输入使旧预览和手工修正失效；保留原始结果供恢复。背景或定位不确定时保留原结果并提示，不宣称语义识别或跨帧一致性。
- `pixelart-abstraction` 保留 [MIT 许可](pixelart-abstraction/LICENSE)；[上游算法说明](pixelart-abstraction/docs/algorithm.md) 不代表本地 RGBA 扩展经过相同验证。图片 IO 和全透明输入处理由 engine 负责。
- CLI 参数修改同步帮助和 `cli/examples/`；进度与警告写 stderr，成功路径写 stdout，不覆盖输入原图。

## 命令与生成文件

以下命令在仓库根目录执行：

- 安装与启动：`npm --prefix editor ci`、`npm --prefix editor run desktop`。普通浏览器没有原生 IPC，不能代替桌面验证。
- 完整检查：`sh tools/check.sh`，包含 Rust 格式与 Clippy、绑定生成比对、ESLint、Prettier、类型检查和前端构建，不运行自动化测试。
- 前端单独检查：`npm --prefix editor run lint`、`npm --prefix editor run format:check`；统一格式用 `npm --prefix editor run format`。桌面打包用 `npm --prefix editor run desktop:build`。
- IPC 修改后执行 `cargo run --locked -p pixauro_editor --bin export-bindings` 并提交生成结果。`editor/src/generated/bindings.ts` 不手改、不格式化；Specta 相关依赖保持兼容并锁定版本。
- 图标源为 `editor/src-tauri/icons/pixauro-app-icon.svg`，修改后执行 `sh tools/generate-icons.sh`，源文件和生成图标一起提交。
