//! How see-through the main window's background is.
//!
//! Omarchy offers opaque, transparent and glassy windows (`omarchy hyprland
//! window transparency`), and opaque, glassy and clear bars. Spotifast follows
//! the window choice by default, or keeps one of its own: the window's
//! backgrounds take an alpha, the words and icons over them stay solid, and
//! Hyprland blurs what shows through when its blur is on (Omarchy's Glassy
//! turns it on).

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use egui::Color32;
use serde::{Deserialize, Serialize};

use crate::i18n::{Locale, gettext};
use crate::theme::Palette;

/// The window background the listener chose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    /// Omarchy's window transparency where Omarchy is set up, else opaque.
    #[default]
    Auto,
    Opaque,
    Transparent,
    Glassy,
    Clear,
}

/// How the window is drawn this frame, once [`Backdrop::Auto`] is resolved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    #[default]
    Opaque,
    /// A hint of what is behind, as Omarchy's transparent terminals.
    Transparent,
    /// Frosted: well see-through, blurred by Hyprland.
    Glassy,
    /// Mostly see-through, unblurred, like Omarchy's clear bar.
    Clear,
}

impl Backdrop {
    pub const ALL: [Backdrop; 5] = [
        Self::Auto,
        Self::Opaque,
        Self::Transparent,
        Self::Glassy,
        Self::Clear,
    ];

    /// The label in the picker. Auto names Omarchy where it is followed.
    pub fn label(self, locale: Locale, omarchy: bool) -> Cow<'static, str> {
        match self {
            Self::Auto if omarchy => gettext(locale, "Follow Omarchy"),
            Self::Auto => gettext(locale, "Automatic"),
            Self::Opaque => gettext(locale, "Opaque"),
            Self::Transparent => gettext(locale, "Transparent"),
            Self::Glassy => gettext(locale, "Glassy"),
            Self::Clear => gettext(locale, "Clear"),
        }
    }

    /// What to draw, given Omarchy's window choice where it has one.
    pub fn resolve(self, desktop: Option<Look>) -> Look {
        match self {
            Self::Auto => desktop.unwrap_or_default(),
            Self::Opaque => Look::Opaque,
            Self::Transparent => Look::Transparent,
            Self::Glassy => Look::Glassy,
            Self::Clear => Look::Clear,
        }
    }
}

/// The alpha each layer of the window keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Alphas {
    /// The page behind everything.
    window: f32,
    /// The sidebar, side panels, top bar and player bar.
    panel: f32,
    /// Cards, fields, hovers and selections over them.
    surface: f32,
    /// Menus, tooltips and dialogs, which have to stay readable over
    /// whatever is under them.
    overlay: f32,
}

impl Look {
    pub fn label(self, locale: Locale) -> Cow<'static, str> {
        match self {
            Self::Opaque => gettext(locale, "Opaque"),
            Self::Transparent => gettext(locale, "Transparent"),
            Self::Glassy => gettext(locale, "Glassy"),
            Self::Clear => gettext(locale, "Clear"),
        }
    }

    pub fn is_opaque(self) -> bool {
        self == Self::Opaque
    }

    fn alphas(self) -> Alphas {
        match self {
            Self::Opaque => Alphas {
                window: 1.0,
                panel: 1.0,
                surface: 1.0,
                overlay: 1.0,
            },
            // Omarchy's transparent terminals sit at 0.96 to 0.985.
            Self::Transparent => Alphas {
                window: 0.88,
                panel: 0.91,
                surface: 0.94,
                overlay: 0.99,
            },
            // Omarchy's glassy terminals are 0.65.
            Self::Glassy => Alphas {
                window: 0.62,
                panel: 0.70,
                surface: 0.78,
                overlay: 0.97,
            },
            Self::Clear => Alphas {
                window: 0.26,
                panel: 0.36,
                surface: 0.55,
                overlay: 0.95,
            },
        }
    }

    /// The palette with this look's see-through backgrounds. Words, icons,
    /// the accent and outlines are left solid, so text stays crisp. An
    /// opaque look returns the palette exactly as it was.
    pub fn apply(self, mut palette: Palette) -> Palette {
        if self.is_opaque() {
            return palette;
        }
        let alphas = self.alphas();
        palette.window = fade(palette.window, alphas.window);
        palette.panel = fade(palette.panel, alphas.panel);
        for surface in [
            &mut palette.surface,
            &mut palette.surface_hover,
            &mut palette.surface_active,
        ] {
            *surface = fade(*surface, alphas.surface);
        }
        palette.overlay = fade(palette.overlay, alphas.overlay);
        palette
    }
}

/// `color` with its alpha scaled by `alpha`, keeping its hue.
pub fn fade(color: Color32, alpha: f32) -> Color32 {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (f32::from(a) * alpha).round() as u8)
}

/// `color` without any see-through, for words and icons drawn in a
/// background's colour (a play glyph on its disc).
pub fn solid(color: Color32) -> Color32 {
    if color.a() == 255 {
        return color;
    }
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    Color32::from_rgb(r, g, b)
}

const UNKNOWN: u8 = 0;

fn encode(look: Option<Look>) -> u8 {
    match look {
        None => UNKNOWN,
        Some(Look::Opaque) => 1,
        Some(Look::Transparent) => 2,
        Some(Look::Glassy) => 3,
        Some(Look::Clear) => 4,
    }
}

fn decode(value: u8) -> Option<Look> {
    match value {
        1 => Some(Look::Opaque),
        2 => Some(Look::Transparent),
        3 => Some(Look::Glassy),
        4 => Some(Look::Clear),
        _ => None,
    }
}

/// Omarchy's window transparency, kept current off the interface thread.
///
/// `omarchy hyprland window transparency` keeps its choice as toggle files
/// and runs no theme hook, so a thread looks at them once a second (two
/// `stat` calls) and repaints when they change.
#[derive(Default)]
pub struct DesktopLook {
    state: Option<Arc<AtomicU8>>,
}

impl DesktopLook {
    /// Omarchy's current window look, or `None` without Omarchy.
    pub fn current(&mut self, ctx: &egui::Context) -> Option<Look> {
        let state = self
            .state
            .get_or_insert_with(|| start(ctx.clone()))
            .load(Ordering::Relaxed);
        decode(state)
    }
}

#[cfg(all(target_os = "linux", not(test)))]
fn start(ctx: egui::Context) -> Arc<AtomicU8> {
    let state = Arc::new(AtomicU8::new(encode(omarchy::window_look())));
    let shared = Arc::downgrade(&state);
    let spawned = std::thread::Builder::new()
        .name("omarchy-windows".into())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let Some(state) = shared.upgrade() else {
                    return;
                };
                let look = encode(omarchy::window_look());
                if state.swap(look, Ordering::Relaxed) != look {
                    ctx.request_repaint();
                }
            }
        });
    if let Err(error) = spawned {
        log::warn!("unable to follow Omarchy's window transparency: {error}");
    }
    state
}

#[cfg(not(all(target_os = "linux", not(test))))]
fn start(_ctx: egui::Context) -> Arc<AtomicU8> {
    Arc::new(AtomicU8::new(UNKNOWN))
}

/// Reading Omarchy's choices from its state directory.
#[cfg(target_os = "linux")]
pub mod omarchy {
    use std::path::PathBuf;

    use super::Look;

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    fn state_home() -> Option<PathBuf> {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| home().map(|home| home.join(".local/state")))
    }

    /// Whether Omarchy is set up for this user.
    pub fn present() -> bool {
        home().is_some_and(|home| home.join(".config/omarchy").is_dir())
            && state_home().is_some_and(|state| state.join("omarchy").is_dir())
    }

    /// The look `omarchy hyprland window transparency` chose, or `None`
    /// without Omarchy. Transparent is Omarchy's default: neither toggle.
    pub fn window_look() -> Option<Look> {
        if !present() {
            return None;
        }
        let toggles = state_home()?.join("omarchy/toggles/hypr");
        Some(look_from(
            toggles.join("window-opaque.lua").is_file(),
            toggles.join("window-glassy.lua").is_file(),
        ))
    }

    pub(super) fn look_from(opaque: bool, glassy: bool) -> Look {
        if opaque {
            Look::Opaque
        } else if glassy {
            Look::Glassy
        } else {
            Look::Transparent
        }
    }

    /// The name of Omarchy's current theme, as its menu shows it.
    pub fn theme_name() -> Option<String> {
        let path = state_home()?.join("omarchy/current/theme.name");
        let name = std::fs::read_to_string(path).ok()?;
        let name = name.trim();
        if name.is_empty() || name.len() > 64 {
            return None;
        }
        Some(
            name.split(['-', '_', ' '])
                .filter(|word| !word.is_empty())
                .map(|word| {
                    let mut chars = word.chars();
                    chars.next().map_or_else(String::new, |first| {
                        first.to_uppercase().chain(chars).collect()
                    })
                })
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opaque_look_leaves_the_palette_alone() {
        for palette in [Palette::dark(), Palette::light()] {
            assert_eq!(Look::Opaque.apply(palette), palette);
        }
    }

    #[test]
    fn see_through_looks_fade_backgrounds_but_not_words() {
        let palette = Palette::dark();
        let mut last = 255;
        for look in [Look::Transparent, Look::Glassy, Look::Clear] {
            let faded = look.apply(palette);
            assert!(faded.window.a() < last, "{look:?}");
            last = faded.window.a();
            // Each layer keeps more than the one under it.
            assert!(faded.panel.a() >= faded.window.a());
            assert!(faded.surface.a() >= faded.panel.a());
            assert!(faded.overlay.a() >= faded.surface.a());
            assert_eq!(faded.text, palette.text);
            assert_eq!(faded.accent, palette.accent);
            // Undoing the alpha loses at most rounding.
            let back = solid(faded.window).to_array();
            for (back, was) in back.iter().zip(palette.window.to_array()) {
                assert!(back.abs_diff(was) <= 2, "{look:?}: {back} vs {was}");
            }
        }
    }

    #[test]
    fn auto_follows_omarchy_and_falls_back_to_opaque() {
        assert_eq!(Backdrop::Auto.resolve(None), Look::Opaque);
        assert_eq!(Backdrop::Auto.resolve(Some(Look::Glassy)), Look::Glassy);
        assert_eq!(Backdrop::Clear.resolve(Some(Look::Glassy)), Look::Clear);
        assert_eq!(Backdrop::Opaque.resolve(Some(Look::Glassy)), Look::Opaque);
    }

    #[test]
    fn omarchy_toggles_map_to_looks() {
        assert_eq!(omarchy::look_from(false, false), Look::Transparent);
        assert_eq!(omarchy::look_from(false, true), Look::Glassy);
        assert_eq!(omarchy::look_from(true, false), Look::Opaque);
        for look in [None, Some(Look::Opaque), Some(Look::Clear)] {
            assert_eq!(decode(encode(look)), look);
        }
    }

    #[test]
    fn settings_keep_the_choice() {
        let text = serde_json::to_string(&Backdrop::Glassy).unwrap();
        assert_eq!(text, "\"glassy\"");
        assert_eq!(
            serde_json::from_str::<Backdrop>("\"clear\"").unwrap(),
            Backdrop::Clear
        );
    }
}
