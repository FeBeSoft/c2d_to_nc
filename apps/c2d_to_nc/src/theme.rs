//! Palettes des deux thèmes (repris de la preview validée : « Sombre Orange » et « Clair MakerCraft »).

use eframe::egui::Color32;

use crate::summary::State;

const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Couleur avec opacité (prémultipliée à la compilation).
const fn rgba(c: u32, a: u8) -> Color32 {
    let a32 = a as u32;
    let r = (((c >> 16) & 0xff) * a32 / 255) as u8;
    let g = (((c >> 8) & 0xff) * a32 / 255) as u8;
    let b = ((c & 0xff) * a32 / 255) as u8;
    Color32::from_rgba_premultiplied(r, g, b, a)
}

/// Couleurs d'un état de tuile.
#[derive(Clone, Copy)]
pub struct StateColors {
    /// bordure et halo
    pub line: Color32,
    /// fond de la tuile
    pub bg: Color32,
    /// dessin de la grosse icône
    pub icon: Color32,
    /// chiffre clé sous le titre
    pub key: Color32,
    /// pastille ✓ ! ✕
    pub badge: Color32,
    /// signe dans la pastille
    pub badge_fg: Color32,
}

pub struct Palette {
    pub dark: bool,
    pub window: Color32,
    pub bar: Color32,
    pub bar_line: Color32,
    pub card: Color32,
    pub card_line: Color32,
    pub pill: Color32,
    pub pill_line: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub weak2: Color32,
    pub icon: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub accent_pill_bg: Color32,
    pub accent_pill_line: Color32,
    pub on_accent: Color32,
    pub violet: Color32,
    pub violet_pill_bg: Color32,
    pub violet_pill_line: Color32,
    pub violet_text: Color32,
    pub drop_line: Color32,
    pub drop_bg: Color32,
    pub drop_glow: Color32,
    pub secondary_line: Color32,
    pub secondary_bg: Color32,
    pub secondary_fg: Color32,
    pub toggle_off: Color32,
    pub knob_off: Color32,
    pub tooltip_bg: Color32,
    pub tooltip_line: Color32,
    /// opacité du halo des tuiles (0 = pas de halo)
    pub halo: f32,
    pub ok: StateColors,
    pub warn: StateColors,
    pub bad: StateColors,
    pub unused: StateColors,
}

impl Palette {
    pub fn state(&self, s: State) -> StateColors {
        match s {
            State::Ok => self.ok,
            State::Warn => self.warn,
            State::Missing => self.bad,
            State::Unused => self.unused,
        }
    }
}

pub const DARK: Palette = Palette {
    dark: true,
    window: rgb(0x151a25),
    bar: rgb(0x11151f),
    bar_line: rgb(0x232a39),
    card: rgb(0x1b2130),
    card_line: rgb(0x262d3d),
    pill: rgb(0x232a3a),
    pill_line: rgb(0x2f3749),
    text: rgb(0xf1f5f9),
    weak: rgb(0x8b95a8),
    weak2: rgb(0x9aa6b8),
    icon: rgb(0xcbd5e1),
    accent: rgb(0xf97316),
    accent_text: rgb(0xfb923c),
    accent_pill_bg: rgba(0xf97316, 31),
    accent_pill_line: rgba(0xf97316, 140),
    on_accent: rgb(0x1a0f05),
    violet: rgb(0xa78bfa),
    violet_pill_bg: rgb(0x232a3a),
    violet_pill_line: rgb(0x2f3749),
    violet_text: rgb(0xa5b4cf),
    drop_line: rgb(0xf97316),
    drop_bg: rgba(0xf97316, 15),
    drop_glow: rgba(0xf97316, 46),
    secondary_line: rgb(0xf97316),
    secondary_bg: rgba(0x000000, 0),
    secondary_fg: rgb(0xfb923c),
    toggle_off: rgb(0x2f3749),
    knob_off: rgb(0xcbd5e1),
    tooltip_bg: rgb(0x0f131c),
    tooltip_line: rgb(0x2f3749),
    halo: 0.30,
    ok: StateColors {
        line: rgb(0x22c55e),
        bg: rgb(0x182029),
        icon: rgb(0x4ade80),
        key: rgb(0x9aa6b8),
        badge: rgb(0x22c55e),
        badge_fg: rgb(0x0b1a10),
    },
    warn: StateColors {
        line: rgb(0xf59e0b),
        bg: rgb(0x211f1c),
        icon: rgb(0xfbbf24),
        key: rgb(0xc9b38a),
        badge: rgb(0xf59e0b),
        badge_fg: rgb(0x1f1405),
    },
    bad: StateColors {
        line: rgb(0xef4444),
        bg: rgb(0x231a20),
        icon: rgb(0xf87171),
        key: rgb(0xf2a5a5),
        badge: rgb(0xef4444),
        badge_fg: rgb(0xffffff),
    },
    unused: StateColors {
        line: rgb(0x2f3749),
        bg: rgb(0x181d28),
        icon: rgb(0x5b6578),
        key: rgb(0x6b7588),
        badge: rgb(0x2f3749),
        badge_fg: rgb(0x8b95a8),
    },
};

pub const LIGHT: Palette = Palette {
    dark: false,
    window: rgb(0xffffff),
    bar: rgb(0xf7f9fc),
    bar_line: rgb(0xe3e8f1),
    card: rgb(0xf7f9fc),
    card_line: rgb(0xe3e8f1),
    pill: rgb(0xffffff),
    pill_line: rgb(0xd5dce8),
    text: rgb(0x0f172a),
    weak: rgb(0x5b6578),
    weak2: rgb(0x5b6578),
    icon: rgb(0x334155),
    accent: rgb(0x2563eb),
    accent_text: rgb(0x1e40af),
    accent_pill_bg: rgb(0xe8efff),
    accent_pill_line: rgb(0xa9c0f5),
    on_accent: rgb(0xffffff),
    violet: rgb(0x6d28d9),
    violet_pill_bg: rgb(0xf3efff),
    violet_pill_line: rgb(0xd6c9fb),
    violet_text: rgb(0x5b21b6),
    drop_line: rgb(0x93a8d8),
    drop_bg: rgb(0xf4f7fe),
    drop_glow: rgba(0x2563eb, 0),
    secondary_line: rgb(0xb9c4d8),
    secondary_bg: rgb(0xffffff),
    secondary_fg: rgb(0x0f172a),
    toggle_off: rgb(0xcbd3e1),
    knob_off: rgb(0xffffff),
    tooltip_bg: rgb(0xffffff),
    tooltip_line: rgb(0xd5dce8),
    halo: 0.0,
    ok: StateColors {
        line: rgb(0x22a55a),
        bg: rgb(0xffffff),
        icon: rgb(0x15803d),
        key: rgb(0x15803d),
        badge: rgb(0x16a34a),
        badge_fg: rgb(0xffffff),
    },
    warn: StateColors {
        line: rgb(0xd97706),
        bg: rgb(0xfffaf0),
        icon: rgb(0xb45309),
        key: rgb(0x92400e),
        badge: rgb(0xd97706),
        badge_fg: rgb(0xffffff),
    },
    bad: StateColors {
        line: rgb(0xdc2626),
        bg: rgb(0xfff5f5),
        icon: rgb(0xb91c1c),
        key: rgb(0xb91c1c),
        badge: rgb(0xdc2626),
        badge_fg: rgb(0xffffff),
    },
    unused: StateColors {
        line: rgb(0xd5dce8),
        bg: rgb(0xf7f9fc),
        icon: rgb(0x9aa4b5),
        key: rgb(0x5b6578),
        badge: rgb(0xd5dce8),
        badge_fg: rgb(0x5b6578),
    },
};
