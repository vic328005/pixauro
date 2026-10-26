use pixauro_engine::{
    GifExportOptions, SpriteImportProgress,
    assets::{self, AnimationRecipe, AssetOptions, BackgroundMode, LocateAxis, SliceLayout},
};
use std::{
    collections::HashMap,
    ffi::OsString,
    fs::File,
    io::Read,
    ops::ControlFlow,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const HELP: &str = "Pixauro CLI — PNG 像素化与动画资源处理

用法：
  pixauro-cli pixelate INPUT.png -o OUTPUT.png [选项]
  pixauro-cli animate INPUT.png -o OUTPUT_DIRECTORY [选项]

共同选项：
  --config FILE          JSON 配置：pixelate 为 AssetOptions，animate 为 AnimationRecipe
  --size WIDTHxHEIGHT    输出尺寸上限 / 动画画布尺寸，默认 64x64
  --pixel-size N         像素网格长边上限，默认 64
  --colors N             每帧 RGB 色数，默认 16
  --working-size N       求解工作输入长边，默认 1024
  --iterations N         每帧迭代预算，默认 500
  --background MODE      none | source | pixel | both，默认 pixel
  --edge N               原图去背边缘宽度，默认 0
  --save-config FILE     保存本次最终配置，便于重复执行
  -h, --help             显示帮助

animate 专用选项：
  --rows N --columns N   每行一个动画、从左到右的规则网格，默认 1x1
  --margin N --gap N     原图像素单位的外边距与间距，默认 0
  --fps N               设置所有动画的 FPS，默认 10
  --auto-locate AXIS     自动定位切片：both | x | y
  --no-pixelate          保留裁切像素；请设置足够大的画布
  --name NAME           导出名称，默认输入文件名
  --gif                 在输出目录为每个动画另存循环 GIF 预览
  --gif-scale N         GIF 最近邻放大倍率，1～16，默认 1（需要 --gif）

命令行参数覆盖配置；精确切片、逐帧偏移、动画名和帧率使用 JSON 配置。
输出为 RGBA PNG，或 pixauro-spritesheet v1 PNG + JSON。
进度和提示写入 stderr；成功路径写入 stdout。退出码：0 成功，1 失败，130 取消。
";

type Error = Box<dyn std::error::Error>;
struct Args {
    command: String,
    input: PathBuf,
    output: PathBuf,
    flags: HashMap<String, OsString>,
}
impl Args {
    fn parse() -> Result<Option<Self>, Error> {
        let mut args = std::env::args_os().skip(1);
        let Some(command) = args.next() else {
            print!("{HELP}");
            return Ok(None);
        };
        if command == "--help" || command == "-h" {
            print!("{HELP}");
            return Ok(None);
        }
        let command = command.into_string().map_err(|_| "命令必须为 UTF-8")?;
        if command != "pixelate" && command != "animate" {
            return Err(format!("未知命令：{command}").into());
        }
        let mut flags = HashMap::new();
        let mut input = None;
        let mut positional = false;
        while let Some(arg) = args.next() {
            if !positional && (arg == "--help" || arg == "-h") {
                print!("{HELP}");
                return Ok(None);
            }
            if !positional && arg == "--" {
                positional = true;
                continue;
            }
            if !positional && arg.to_string_lossy().starts_with('-') {
                let key = arg.into_string().map_err(|_| "选项必须为 UTF-8")?;
                let key = if key == "-o" {
                    "--output".to_owned()
                } else {
                    key
                };
                let common = [
                    "--output",
                    "--config",
                    "--size",
                    "--pixel-size",
                    "--colors",
                    "--working-size",
                    "--iterations",
                    "--background",
                    "--edge",
                    "--save-config",
                ];
                let animation = [
                    "--rows",
                    "--columns",
                    "--margin",
                    "--gap",
                    "--fps",
                    "--auto-locate",
                    "--no-pixelate",
                    "--name",
                    "--gif",
                    "--gif-scale",
                ];
                if !common.contains(&key.as_str())
                    && !(command == "animate" && animation.contains(&key.as_str()))
                {
                    return Err(format!("不支持的选项：{key}").into());
                }
                let value = if key == "--no-pixelate" || key == "--gif" {
                    OsString::from("true")
                } else {
                    args.next().ok_or_else(|| format!("{key} 缺少值"))?
                };
                if flags.insert(key.clone(), value).is_some() {
                    return Err(format!("重复选项：{key}").into());
                }
            } else if input.replace(PathBuf::from(arg)).is_some() {
                return Err("只支持一个输入文件".into());
            }
        }
        let input = input.ok_or("缺少输入 PNG")?;
        let output = flags
            .remove("--output")
            .map(PathBuf::from)
            .ok_or("缺少 -o 输出路径")?;
        Ok(Some(Self {
            command,
            input,
            output,
            flags,
        }))
    }
    fn text(&self, key: &str) -> Result<Option<&str>, Error> {
        self.flags
            .get(key)
            .map(|value| {
                value
                    .to_str()
                    .ok_or_else(|| format!("{key} 必须为 UTF-8").into())
            })
            .transpose()
    }
    fn number<T: std::str::FromStr>(&self, key: &str) -> Result<Option<T>, Error> {
        self.text(key)?
            .map(|v| v.parse().map_err(|_| format!("{key} 数值无效：{v}").into()))
            .transpose()
    }
    fn configure(&self, options: &mut AssetOptions) -> Result<(), Error> {
        if let Some(size) = self.text("--size")? {
            let (w, h) = size.split_once('x').ok_or("--size 必须为 WIDTHxHEIGHT")?;
            options.canvas_width = w.parse()?;
            options.canvas_height = h.parse()?;
        }
        if self.flags.contains_key("--no-pixelate") {
            options.pixelation = None;
        }
        let has_pixel_flags = ["--pixel-size", "--colors", "--working-size", "--iterations"]
            .iter()
            .any(|k| self.flags.contains_key(*k));
        if has_pixel_flags && options.pixelation.is_none() {
            return Err("关闭像素化时不能指定像素化参数".into());
        }
        if let Some(p) = &mut options.pixelation {
            if let Some(v) = self.number("--pixel-size")? {
                p.pixel_size = v;
            }
            if let Some(v) = self.number("--colors")? {
                p.colors = v;
            }
            if let Some(v) = self.number("--working-size")? {
                p.working_size = v;
            }
            if let Some(v) = self.number("--iterations")? {
                p.max_iterations = v;
            }
        }
        if let Some(mode) = self.text("--background")? {
            options.background = match mode {
                "none" => BackgroundMode::None,
                "source" => BackgroundMode::Source,
                "pixel" => BackgroundMode::Pixel,
                "both" => BackgroundMode::Both,
                _ => return Err("--background 必须为 none、source、pixel 或 both".into()),
            };
        }
        if let Some(edge) = self.number("--edge")? {
            options.source_background_edge = edge;
        }
        Ok(())
    }
    fn read_config<T: serde::de::DeserializeOwned + Default>(&self) -> Result<T, Error> {
        let Some(path) = self.flags.get("--config") else {
            return Ok(T::default());
        };
        let mut bytes = Vec::new();
        File::open(path)?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("配置文件最大 16 MiB".into());
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
    fn save_config(&self, value: &impl serde::Serialize) -> Result<(), Error> {
        if let Some(path) = self.flags.get("--save-config") {
            let bytes = serde_json::to_vec_pretty(value)?;
            // 配方是输入资产的一部分；保存时不覆盖既有文件。
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            file.write_all(&bytes)?;
        }
        Ok(())
    }
}
fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
fn run() -> Result<ExitCode, Error> {
    let Some(args) = Args::parse()? else {
        return Ok(ExitCode::SUCCESS);
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    let mut previous = None;
    let mut progress = |p: SpriteImportProgress| {
        if cancelled.load(Ordering::Relaxed) {
            return ControlFlow::Break(());
        }
        if previous != Some((p.completed, p.total, p.active)) {
            eprintln!("已处理 {}/{} 帧，运行中 {}", p.completed, p.total, p.active);
            previous = Some((p.completed, p.total, p.active));
        }
        ControlFlow::Continue(())
    };
    if args.command == "pixelate" {
        let mut options: AssetOptions = args.read_config()?;
        args.configure(&mut options)?;
        // 原图不得被误覆盖；其他现有目标遵循引擎的原子替换约定。
        if args.output.exists()
            && std::fs::canonicalize(&args.input)? == std::fs::canonicalize(&args.output)?
        {
            return Err("输出路径不能是输入原图".into());
        }
        let Some(image) = assets::pixelate(&args.input, &options, &mut progress)? else {
            return Ok(ExitCode::from(130));
        };
        if cancelled.load(Ordering::Relaxed) {
            return Ok(ExitCode::from(130));
        }
        args.save_config(&options)?;
        std::fs::create_dir_all(parent(&args.output))?;
        image.export(&args.output)?;
        for warning in &image.warnings {
            eprintln!("提示：{warning}");
        }
        println!("{}", args.output.display());
    } else {
        let mut recipe: AnimationRecipe = args.read_config()?;
        args.configure(&mut recipe.options)?;
        let grid_flags = ["--rows", "--columns", "--margin", "--gap"]
            .iter()
            .any(|k| args.flags.contains_key(*k));
        if grid_flags {
            let SliceLayout::Grid { grid } = &mut recipe.layout else {
                return Err("精确切片配置不能同时使用网格参数".into());
            };
            if let Some(v) = args.number("--rows")? {
                grid.rows = v;
            }
            if let Some(v) = args.number("--columns")? {
                grid.columns = v;
            }
            if let Some(v) = args.number("--margin")? {
                grid.margin = v;
            }
            if let Some(v) = args.number("--gap")? {
                grid.gap = v;
            }
        }
        if let Some(axis) = args.text("--auto-locate")? {
            recipe.auto_locate = Some(match axis {
                "both" => LocateAxis::Both,
                "x" => LocateAxis::X,
                "y" => LocateAxis::Y,
                _ => return Err("定位轴必须为 both、x 或 y".into()),
            });
        }
        if let Some(fps) = args.number::<f64>("--fps")? {
            if recipe.groups.is_empty() {
                let rows = match &recipe.layout {
                    SliceLayout::Grid { grid } => usize::from(grid.rows),
                    SliceLayout::Slices { slices } => slices
                        .iter()
                        .map(|s| usize::from(s.row) + 1)
                        .max()
                        .unwrap_or(0),
                };
                if rows > 256 {
                    return Err("最多支持 256 个动画分组".into());
                }
                recipe.groups = (0..rows)
                    .map(|i| assets::ImportGroup {
                        name: format!("Animation {}", i + 1),
                        fps,
                    })
                    .collect();
            } else {
                for group in &mut recipe.groups {
                    group.fps = fps;
                }
            }
        }
        let gif = args.flags.contains_key("--gif");
        if !gif && args.flags.contains_key("--gif-scale") {
            return Err("--gif-scale 需要同时指定 --gif".into());
        }
        let gif_options = GifExportOptions {
            scale: args.number("--gif-scale")?.unwrap_or(1),
            ..GifExportOptions::default()
        };
        if gif {
            gif_options.validate(recipe.options.canvas_width, recipe.options.canvas_height, 1)?;
        }
        let name = args.text("--name")?.unwrap_or_else(|| {
            args.input
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("animation")
        });
        let Some(asset) = assets::animate(&args.input, &recipe, &mut progress)? else {
            return Ok(ExitCode::from(130));
        };
        if cancelled.load(Ordering::Relaxed) {
            return Ok(ExitCode::from(130));
        }
        args.save_config(&recipe)?;
        std::fs::create_dir_all(&args.output)?;
        if gif {
            for animation in asset.animations() {
                gif_options.validate(
                    recipe.options.canvas_width,
                    recipe.options.canvas_height,
                    animation.frames.len(),
                )?;
            }
        }
        let path = asset.export(name, &args.output)?;
        for warning in &asset.warnings {
            eprintln!("提示：{warning}");
        }
        println!("{}", path.display());
        if gif {
            let paths = asset
                .export_gifs(parent(&path), gif_options)
                .map_err(|error| {
                    format!(
                        "PNG + JSON 已保存到 {}，GIF 导出失败：{error}",
                        path.display()
                    )
                })?;
            for path in paths {
                println!("{}", path.display());
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("错误：{error}");
            ExitCode::FAILURE
        }
    }
}
