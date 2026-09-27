//! Machine header ("host banner") coloring for the multi-machine sidebar: a lolcat-style
//! per-character gradient, optionally drifting over time (`ui.sidebar.host`).

use ratatui::style::Color;

use crate::app::state::Palette;
use crate::config::{HostBannerAnimation, HostBannerGradient, SidebarHostConfig};

const MIN_LUMA: f32 = 0.45;
const MAX_LUMA: f32 = 1.00;
const FREQ: f32 = 0.30;
/// Animation ticks per second while `animation = "animated"`.
pub(super) const TICKS_PER_SECOND: u64 = 10;

pub(super) fn animation_tick(host: &SidebarHostConfig) -> u32 {
    match host.animation {
        HostBannerAnimation::Static => 0,
        HostBannerAnimation::Animated => {
            (crate::terminal::state::unix_now_ms() * TICKS_PER_SECOND / 1000) as u32
        }
    }
}

fn speed(host: &SidebarHostConfig) -> f32 {
    match host.animation {
        HostBannerAnimation::Static => 0.0,
        HostBannerAnimation::Animated => host.speed.drift(),
    }
}

fn rgb_components(color: Color, fallback: (u8, u8, u8)) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => fallback,
    }
}

fn base_color(gradient: HostBannerGradient, palette: &Palette) -> Option<(u8, u8, u8)> {
    let color = match gradient {
        HostBannerGradient::Rainbow => return None,
        HostBannerGradient::Accent => palette.accent,
        HostBannerGradient::Cool => palette.blue,
        HostBannerGradient::Warm => palette.peach,
        HostBannerGradient::Muted => palette.overlay1,
    };
    Some(rgb_components(color, (205, 214, 244)))
}

/// Deterministic color for the `index`th label character at `tick`.
pub(super) fn color(host: &SidebarHostConfig, palette: &Palette, index: usize, tick: u32) -> Color {
    let phase = FREQ * index as f32 + speed(host) * tick as f32;
    let luma =
        |offset: f32| MIN_LUMA + ((phase + offset).sin() * 0.5 + 0.5) * (MAX_LUMA - MIN_LUMA);
    match base_color(host.gradient, palette) {
        Some((r, g, b)) => {
            let factor = luma(0.0);
            let scale = |c: u8| (c as f32 * factor).round().clamp(0.0, 255.0) as u8;
            Color::Rgb(scale(r), scale(g), scale(b))
        }
        None => {
            let channel = |offset: f32| (luma(offset) * 255.0).round().clamp(0.0, 255.0) as u8;
            Color::Rgb(
                channel(0.0),
                channel(std::f32::consts::TAU / 3.0),
                channel(2.0 * std::f32::consts::TAU / 3.0),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_banner_is_a_stable_spatial_gradient() {
        let host = SidebarHostConfig {
            animation: HostBannerAnimation::Static,
            ..SidebarHostConfig::default()
        };
        let palette = Palette::catppuccin();
        assert_eq!(animation_tick(&host), 0);
        assert_ne!(color(&host, &palette, 0, 0), color(&host, &palette, 5, 0));
        assert_eq!(color(&host, &palette, 3, 0), color(&host, &palette, 3, 999));
    }

    #[test]
    fn animated_banner_drifts_with_tick_and_keeps_legible_luma() {
        let host = SidebarHostConfig::default();
        let palette = Palette::catppuccin();
        assert_ne!(color(&host, &palette, 0, 0), color(&host, &palette, 0, 40));
        for index in 0..32 {
            let Color::Rgb(r, g, b) = color(&host, &palette, index, 7) else {
                panic!("rgb");
            };
            assert!(r.max(g).max(b) >= (MIN_LUMA * 255.0) as u8);
        }
    }
}
