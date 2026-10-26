//! 作品查询和图层合并共用的 RGBA 合成。

use crate::{Color, FrameId, Layer, Palette};

/// 作品显示和向下合并使用相同的合成规则，避免两条路径产生不同颜色。
pub(crate) fn composite_layers(
    palette: &Palette,
    layers: &[Layer],
    frame: FrameId,
    position: (i32, i32),
) -> Color {
    let mut rgb = [0.0_f64; 3];
    let mut alpha = 0.0_f64;
    for layer in layers {
        if !layer.properties().visible {
            continue;
        }
        let Some(color) = layer
            .pixel_at(frame, position)
            .and_then(|id| palette.color(id))
        else {
            continue;
        };
        let a = f64::from(color.alpha) * f64::from(layer.properties().opacity) / 65025.0;
        for (out, channel) in rgb.iter_mut().zip([color.red, color.green, color.blue]) {
            *out = f64::from(channel) * a + *out * (1.0 - a);
        }
        alpha = a + alpha * (1.0 - a);
    }
    if alpha == 0.0 {
        return Color::rgba(0, 0, 0, 0);
    }
    Color::rgba(
        (rgb[0] / alpha).round() as u8,
        (rgb[1] / alpha).round() as u8,
        (rgb[2] / alpha).round() as u8,
        (alpha * 255.0).round() as u8,
    )
}
