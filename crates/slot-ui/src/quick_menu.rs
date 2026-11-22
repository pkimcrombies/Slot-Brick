use crate::draw::{pixel_aligned_texture, Draw, TexId, OUT_H, OUT_W};
use crate::menu_style::{draw_menu_cursor, draw_menu_hints, menu_hint_face_scaled, MENU_BACKGROUND,
    MENU_MUTED_INK, MENU_ROSE_INK, MENU_TEXT_INK};
use crate::plate::{UndoFace, LEGEND_Y};
use crate::power_menu::{MENU_H, MENU_INK, MENU_PAD, MENU_PX};
use crate::text;

/// The Brick Pro: set once at startup, it adds the Brick Pro's own rows (the stick lights).
static BRICK_PRO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_brick_pro(pro: bool) {
    BRICK_PRO.store(pro, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum QuickRow {
    FastForward,
    FastForwardSound,
    Screen,
    Rumble,
    DateTime,
    About,
    GbaShader,
    GbShader,
    GbaColourCorrection,
    Game,
    EjectSave,
    Turbo,
    Rewind,
    SwapAb,
    GbPalettes,
    Lights,
    LedMode,
    LedTop,
    LedButtons,
    LedTriggers,
    LedColor,
    LedEffect,
    Wifi,
    CartArt,
    Achievements,
    ScreenGb,
    ScreenGba,
    ScreenNes,
    ScreenSnes,
    GbColourCorrection,
    NesShader,
    SnesShader,
    GbOverlay,
    GbcOverlay,
    GbaOverlay,
    NesOverlay,
    SnesOverlay,
    AutoSave,
    BatteryShutdown,
    Update,
    System,
    AutoClock,
    LedSticks,
    Library,
    LibrarySort,
    LibraryShow,
    LibraryPlayTime,
    LibraryTitleCard,
    ScreenPs1,
    Ps1Shader,
    Ps1Dithering,
    Ps1Overlay,
    Font,
    Online,
    Clock,
    Saving,
    LibrarySystems,
    ShowGba,
    ShowGb,
    ShowGbc,
    ShowNes,
    ShowSnes,
    ShowPs1,
    ShowPsp,
    ResetSettings,
    AboutVersion,
    AboutBuild,
    AboutDevice,
    AboutFirmware,
    AboutCard,
    AboutLabel,
    SoundMenu,
    SystemSounds,
    SoundStar,
    SoundCart,
    SoundCartOpen,
    SoundLabel,
    SoundVolume,
    UpdateCheck,
    Music,
}

impl QuickRow {
    pub const ALL: [QuickRow; 80] = [
        QuickRow::FastForward,
        QuickRow::FastForwardSound,
        QuickRow::Screen,
        QuickRow::Rumble,
        QuickRow::DateTime,
        QuickRow::About,
        QuickRow::GbaShader,
        QuickRow::GbShader,
        QuickRow::GbaColourCorrection,
        QuickRow::Game,
        QuickRow::EjectSave,
        QuickRow::Turbo,
        QuickRow::Rewind,
        QuickRow::SwapAb,
        QuickRow::GbPalettes,
        QuickRow::Lights,
        QuickRow::LedMode,
        QuickRow::LedTop,
        QuickRow::LedButtons,
        QuickRow::LedTriggers,
        QuickRow::LedColor,
        QuickRow::LedEffect,
        QuickRow::Wifi,
        QuickRow::CartArt,
        QuickRow::Achievements,
        QuickRow::ScreenGb,
        QuickRow::ScreenGba,
        QuickRow::ScreenNes,
        QuickRow::ScreenSnes,
        QuickRow::GbColourCorrection,
        QuickRow::NesShader,
        QuickRow::SnesShader,
        QuickRow::GbOverlay,
        QuickRow::GbcOverlay,
        QuickRow::GbaOverlay,
        QuickRow::NesOverlay,
        QuickRow::SnesOverlay,
        QuickRow::AutoSave,
        QuickRow::BatteryShutdown,
        QuickRow::Update,
        QuickRow::System,
        QuickRow::AutoClock,
        QuickRow::LedSticks,
        QuickRow::Library,
        QuickRow::LibrarySort,
        QuickRow::LibraryShow,
        QuickRow::LibraryPlayTime,
        QuickRow::LibraryTitleCard,
        QuickRow::ScreenPs1,
        QuickRow::Ps1Shader,
        QuickRow::Ps1Dithering,
        QuickRow::Ps1Overlay,
        QuickRow::Font,
        QuickRow::Online,
        QuickRow::Clock,
        QuickRow::Saving,
        QuickRow::LibrarySystems,
        QuickRow::ShowGba,
        QuickRow::ShowGb,
        QuickRow::ShowGbc,
        QuickRow::ShowNes,
        QuickRow::ShowSnes,
        QuickRow::ShowPs1,
        QuickRow::ShowPsp,
        QuickRow::ResetSettings,
        QuickRow::AboutVersion,
        QuickRow::AboutBuild,
        QuickRow::AboutDevice,
        QuickRow::AboutFirmware,
        QuickRow::AboutCard,
        QuickRow::AboutLabel,
        QuickRow::SoundMenu,
        QuickRow::SystemSounds,
        QuickRow::SoundStar,
        QuickRow::SoundCart,
        QuickRow::SoundCartOpen,
        QuickRow::SoundLabel,
        QuickRow::SoundVolume,
        QuickRow::UpdateCheck,
        QuickRow::Music,
    ];

    #[cfg(not(feature = "brick"))]
    pub const MAIN: [QuickRow; 4] = [QuickRow::Screen, QuickRow::Game, QuickRow::Library, QuickRow::System];
    #[cfg(not(feature = "brick"))]
    pub const SYSTEM: [QuickRow; 7] = [QuickRow::DateTime, QuickRow::Font, QuickRow::Rumble, QuickRow::SystemSounds, QuickRow::BatteryShutdown, QuickRow::ResetSettings, QuickRow::About];

    #[cfg(feature = "brick")]
    pub const MAIN: [QuickRow; 6] = [
        QuickRow::Screen, QuickRow::Game, QuickRow::Library, QuickRow::Online, QuickRow::Music,
        QuickRow::System,
    ];
    /// System: the Brick's lights, its clock, the font, rumble, Slot's sounds, the battery,
    /// asking GitHub for a newer Slot, every setting back to how it came, and what Slot is.
    #[cfg(feature = "brick")]
    pub const SYSTEM: [QuickRow; 9] = [QuickRow::Lights,QuickRow::Clock,QuickRow::Font,QuickRow::Rumble,QuickRow::SystemSounds,QuickRow::BatteryShutdown,QuickRow::UpdateCheck,QuickRow::ResetSettings,QuickRow::About];
    /// System → System Sounds: how loud they all are, and each kind on or off.
    pub const SOUNDS: [QuickRow; 6] = [
        QuickRow::SoundVolume,
        QuickRow::SoundMenu,
        QuickRow::SoundStar,
        QuickRow::SoundCart,
        QuickRow::SoundCartOpen,
        QuickRow::SoundLabel,
    ];
    /// Clock: setting it by hand, and from the internet.
    pub const CLOCK: [QuickRow; 2] = [QuickRow::DateTime,QuickRow::AutoClock];
    /// Online: what needs Wi-Fi, and Wi-Fi itself.
    pub const ONLINE: [QuickRow; 4] = [QuickRow::Wifi,QuickRow::CartArt,QuickRow::Achievements,QuickRow::Update];
    pub const LIGHTS: [QuickRow; 6] = [QuickRow::LedMode,QuickRow::LedTop,QuickRow::LedButtons,QuickRow::LedTriggers,QuickRow::LedColor,QuickRow::LedEffect];
    /// The Brick Pro's: its sticks have lights of their own.
    pub const LIGHTS_PRO: [QuickRow; 7] = [QuickRow::LedMode,QuickRow::LedTop,QuickRow::LedButtons,QuickRow::LedTriggers,QuickRow::LedSticks,QuickRow::LedColor,QuickRow::LedEffect];
    /// The LEDs page of the Brick this is (see `set_brick_pro`).
    pub fn lights() -> &'static [QuickRow] {
        match BRICK_PRO.load(std::sync::atomic::Ordering::Relaxed) {
            true => &QuickRow::LIGHTS_PRO,
            false => &QuickRow::LIGHTS,
        }
    }
    pub fn led_setting(self) -> bool { Self::LIGHTS_PRO.contains(&self) }

    /// Library: how the shelves are ordered, whether they hold only the favorites, which
    /// systems have a shelf, and what is printed around them.
    pub const LIBRARY: [QuickRow; 5] = [QuickRow::LibrarySort, QuickRow::LibraryShow, QuickRow::LibrarySystems, QuickRow::LibraryTitleCard, QuickRow::LibraryPlayTime];
    /// Library → Systems: each system's shelf shown or hidden, in the shelves' order.
    pub const SYSTEMS: [QuickRow; 7] =
        [QuickRow::ShowGba, QuickRow::ShowGb, QuickRow::ShowGbc, QuickRow::ShowNes, QuickRow::ShowSnes, QuickRow::ShowPs1, QuickRow::ShowPsp];
    /// Gameplay → Saving: when a game is saved without being asked.
    pub const SAVING: [QuickRow; 2] = [QuickRow::AutoSave, QuickRow::EjectSave];

    /// Screen: a page for each system, holding that system's own picture settings.
    pub const SCREEN: [QuickRow; 5] =
        [QuickRow::ScreenGb, QuickRow::ScreenGba, QuickRow::ScreenNes, QuickRow::ScreenSnes, QuickRow::ScreenPs1];
    pub const SCREEN_GB: [QuickRow; 5] =
        [QuickRow::GbShader, QuickRow::GbColourCorrection, QuickRow::GbPalettes, QuickRow::GbOverlay, QuickRow::GbcOverlay];
    pub const SCREEN_GBA: [QuickRow; 3] = [QuickRow::GbaShader, QuickRow::GbaColourCorrection, QuickRow::GbaOverlay];
    pub const SCREEN_NES: [QuickRow; 2] = [QuickRow::NesShader, QuickRow::NesOverlay];
    pub const SCREEN_SNES: [QuickRow; 2] = [QuickRow::SnesShader, QuickRow::SnesOverlay];
    pub const SCREEN_PS1: [QuickRow; 3] = [QuickRow::Ps1Shader, QuickRow::Ps1Dithering, QuickRow::Ps1Overlay];

    /// System → About: what this Slot is and what it runs on, then its label.
    pub const ABOUT: [QuickRow; 6] = [
        QuickRow::AboutVersion,
        QuickRow::AboutBuild,
        QuickRow::AboutDevice,
        QuickRow::AboutFirmware,
        QuickRow::AboutCard,
        QuickRow::AboutLabel,
    ];
    /// The About rows that only say something: nothing to change or open.
    pub const ABOUT_INFO: [QuickRow; 5] =
        [QuickRow::AboutVersion, QuickRow::AboutBuild, QuickRow::AboutDevice, QuickRow::AboutFirmware, QuickRow::AboutCard];
    pub fn info(self) -> bool {
        Self::ABOUT_INFO.contains(&self)
    }

    /// The system an Overlay row picks the overlay for.
    /// Rows that read a name rather than a fixed value (an overlay's, the font's, About's, the
    /// sounds' volume), numbered: the overlay rows by their platform, then Font, then About's,
    /// then Volume.
    pub const NAMED: usize = slot_store::Platform::ALL.len() + 1 + Self::ABOUT_INFO.len() + 1;
    pub fn named(self) -> Option<usize> {
        let about = slot_store::Platform::ALL.len() + 1;
        match self {
            QuickRow::Font => Some(slot_store::Platform::ALL.len()),
            QuickRow::SoundVolume => Some(Self::NAMED - 1),
            row if row.info() => Self::ABOUT_INFO.iter().position(|r| *r == row).map(|at| about + at),
            _ => self.overlay_of().map(|p| p.index()),
        }
    }

    /// The system a Library → Systems row shows or hides.
    pub fn shelf_of(self) -> Option<slot_store::Platform> {
        use slot_store::Platform;
        Some(match self {
            QuickRow::ShowGba => Platform::Gba,
            QuickRow::ShowGb => Platform::Gb,
            QuickRow::ShowGbc => Platform::Gbc,
            QuickRow::ShowNes => Platform::Nes,
            QuickRow::ShowSnes => Platform::Snes,
            QuickRow::ShowPs1 => Platform::Ps1,
            QuickRow::ShowPsp => Platform::Psp,
            _ => return None,
        })
    }

    pub fn overlay_of(self) -> Option<slot_store::Platform> {
        use slot_store::Platform;
        Some(match self {
            QuickRow::GbOverlay => Platform::Gb,
            QuickRow::GbcOverlay => Platform::Gbc,
            QuickRow::GbaOverlay => Platform::Gba,
            QuickRow::NesOverlay => Platform::Nes,
            QuickRow::SnesOverlay => Platform::Snes,
            QuickRow::Ps1Overlay => Platform::Ps1,
            _ => return None,
        })
    }

    pub const GAME: [QuickRow; 6] = [
        QuickRow::FastForward,
        QuickRow::FastForwardSound,
        QuickRow::Rewind,
        QuickRow::Turbo,
        QuickRow::SwapAb,
        QuickRow::Saving,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn page(self) -> &'static [QuickRow] {
        match self {
            QuickRow::LedMode | QuickRow::LedTop | QuickRow::LedButtons | QuickRow::LedTriggers | QuickRow::LedSticks | QuickRow::LedColor | QuickRow::LedEffect => QuickRow::lights(),
            QuickRow::ScreenGb | QuickRow::ScreenGba | QuickRow::ScreenNes | QuickRow::ScreenSnes | QuickRow::ScreenPs1 => &QuickRow::SCREEN,
            QuickRow::GbShader
            | QuickRow::GbColourCorrection
            | QuickRow::GbPalettes
            | QuickRow::GbOverlay
            | QuickRow::GbcOverlay => &QuickRow::SCREEN_GB,
            QuickRow::GbaShader | QuickRow::GbaColourCorrection | QuickRow::GbaOverlay => &QuickRow::SCREEN_GBA,
            QuickRow::NesShader | QuickRow::NesOverlay => &QuickRow::SCREEN_NES,
            QuickRow::SnesShader | QuickRow::SnesOverlay => &QuickRow::SCREEN_SNES,
            QuickRow::Ps1Shader | QuickRow::Ps1Dithering | QuickRow::Ps1Overlay => &QuickRow::SCREEN_PS1,
            QuickRow::FastForward | QuickRow::FastForwardSound | QuickRow::Rewind | QuickRow::Turbo | QuickRow::SwapAb | QuickRow::Saving => &QuickRow::GAME,
            QuickRow::AutoSave | QuickRow::EjectSave => &QuickRow::SAVING,
            #[cfg(feature = "brick")]
            QuickRow::DateTime | QuickRow::AutoClock => &QuickRow::CLOCK,
            #[cfg(not(feature = "brick"))]
            QuickRow::DateTime | QuickRow::AutoClock => &QuickRow::SYSTEM,
            QuickRow::Lights
            | QuickRow::Clock
            | QuickRow::Font
            | QuickRow::Rumble
            | QuickRow::SystemSounds
            | QuickRow::BatteryShutdown
            | QuickRow::ResetSettings
            | QuickRow::About => &QuickRow::SYSTEM,
            #[cfg(feature = "brick")]
            QuickRow::UpdateCheck => &QuickRow::SYSTEM,
            QuickRow::ShowGba
            | QuickRow::ShowGb
            | QuickRow::ShowGbc
            | QuickRow::ShowNes
            | QuickRow::ShowSnes
            | QuickRow::ShowPs1
            | QuickRow::ShowPsp => &QuickRow::SYSTEMS,
            QuickRow::SoundVolume
            | QuickRow::SoundMenu
            | QuickRow::SoundStar
            | QuickRow::SoundCart
            | QuickRow::SoundCartOpen
            | QuickRow::SoundLabel => &QuickRow::SOUNDS,
            QuickRow::AboutVersion
            | QuickRow::AboutBuild
            | QuickRow::AboutDevice
            | QuickRow::AboutFirmware
            | QuickRow::AboutCard
            | QuickRow::AboutLabel => &QuickRow::ABOUT,
            QuickRow::LibrarySort
            | QuickRow::LibraryShow
            | QuickRow::LibrarySystems
            | QuickRow::LibraryPlayTime
            | QuickRow::LibraryTitleCard => &QuickRow::LIBRARY,
            QuickRow::Wifi | QuickRow::CartArt | QuickRow::Achievements | QuickRow::Update => &QuickRow::ONLINE,
            _ => &QuickRow::MAIN,
        }
    }

    pub fn parent(self) -> Option<QuickRow> {
        match self.page()[0] {
            QuickRow::LedMode => Some(QuickRow::Lights),
            QuickRow::ScreenGb => Some(QuickRow::Screen),
            QuickRow::GbShader => Some(QuickRow::ScreenGb),
            QuickRow::GbaShader => Some(QuickRow::ScreenGba),
            QuickRow::NesShader => Some(QuickRow::ScreenNes),
            QuickRow::SnesShader => Some(QuickRow::ScreenSnes),
            QuickRow::Ps1Shader => Some(QuickRow::ScreenPs1),
            QuickRow::FastForward => Some(QuickRow::Game),
            QuickRow::LibrarySort => Some(QuickRow::Library),
            QuickRow::ShowGba => Some(QuickRow::LibrarySystems),
            QuickRow::AboutVersion => Some(QuickRow::About),
            QuickRow::SoundVolume => Some(QuickRow::SystemSounds),
            QuickRow::Wifi => Some(QuickRow::Online),
            QuickRow::AutoSave => Some(QuickRow::Saving),
            #[cfg(feature = "brick")]
            QuickRow::DateTime => Some(QuickRow::Clock),
            first if first == QuickRow::SYSTEM[0] => Some(QuickRow::System),
            _ => None,
        }
    }

    pub fn child(self) -> Option<QuickRow> {
        match self {
            QuickRow::Lights => Some(QuickRow::lights()[0]),
            QuickRow::Screen => Some(QuickRow::SCREEN[0]),
            QuickRow::ScreenGb => Some(QuickRow::SCREEN_GB[0]),
            QuickRow::ScreenGba => Some(QuickRow::SCREEN_GBA[0]),
            QuickRow::ScreenNes => Some(QuickRow::SCREEN_NES[0]),
            QuickRow::ScreenSnes => Some(QuickRow::SCREEN_SNES[0]),
            QuickRow::ScreenPs1 => Some(QuickRow::SCREEN_PS1[0]),
            QuickRow::Game => Some(QuickRow::GAME[0]),
            QuickRow::System => Some(QuickRow::SYSTEM[0]),
            QuickRow::Library => Some(QuickRow::LIBRARY[0]),
            QuickRow::LibrarySystems => Some(QuickRow::SYSTEMS[0]),
            QuickRow::About => Some(QuickRow::ABOUT[0]),
            QuickRow::SystemSounds => Some(QuickRow::SOUNDS[0]),
            QuickRow::Online => Some(QuickRow::ONLINE[0]),
            QuickRow::Clock => Some(QuickRow::CLOCK[0]),
            QuickRow::Saving => Some(QuickRow::SAVING[0]),
            _ => None,
        }
    }

    pub fn position(self) -> usize {
        self.page().iter().position(|r| *r == self).unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            QuickRow::Wifi => "Wi-Fi",
            QuickRow::CartArt => "Cart Art",
            QuickRow::Achievements => "RetroAchievements",
            QuickRow::Update => "Update",
            QuickRow::System => "System",
            QuickRow::Online => "Online",
            QuickRow::Clock => "Clock",
            QuickRow::Saving => "Saving",
            QuickRow::AutoClock => "Auto Clock",
            QuickRow::UpdateCheck => "Check for Updates",
            QuickRow::Music => "Music",
            QuickRow::Font => "Font",
            QuickRow::Lights => "LEDs",
            QuickRow::LedMode => "All LEDs",
            QuickRow::LedTop => "Top Bar",
            QuickRow::LedButtons => "Buttons",
            QuickRow::LedTriggers => "Triggers",
            QuickRow::LedColor => "Color",
            QuickRow::LedEffect => "Effect",
            QuickRow::LedSticks => "Sticks",
            QuickRow::Library => "Library",
            QuickRow::LibrarySort => "Sort By",
            QuickRow::LibraryShow => "Show",
            QuickRow::LibraryPlayTime => "Play Time",
            QuickRow::LibraryTitleCard => "Title Card",
            QuickRow::LibrarySystems => "Systems",
            QuickRow::ShowGba => "Game Boy Advance",
            QuickRow::ShowGb => "Game Boy",
            QuickRow::ShowGbc => "Game Boy Color",
            QuickRow::ShowNes => "NES",
            QuickRow::ShowSnes => "SNES",
            QuickRow::ShowPs1 => "PlayStation",
            QuickRow::ShowPsp => "PSP",
            QuickRow::ResetSettings => "Reset Settings",
            QuickRow::AboutVersion => "Version",
            QuickRow::AboutBuild => "Build",
            QuickRow::AboutDevice => "Device",
            QuickRow::AboutFirmware => "Firmware",
            QuickRow::AboutCard => "SD Card",
            QuickRow::AboutLabel => "Label",
            QuickRow::SoundMenu => "Menu",
            QuickRow::SystemSounds => "System Sounds",
            QuickRow::SoundStar => "Favorite Star",
            QuickRow::SoundCart => "Cart In & Out",
            QuickRow::SoundCartOpen => "Cart Opening",
            QuickRow::SoundLabel => "Label On",
            QuickRow::SoundVolume => "Volume",
            QuickRow::FastForward => "Fast Forward",
            QuickRow::FastForwardSound => "Fast Forward Sound",
            QuickRow::Screen => "Emulation",
            QuickRow::Rumble => "Rumble",
            QuickRow::DateTime => "Date & Time",
            QuickRow::About => "About",
            QuickRow::GbaShader | QuickRow::GbShader | QuickRow::NesShader | QuickRow::SnesShader | QuickRow::Ps1Shader => "Shader",
            QuickRow::GbColourCorrection | QuickRow::GbaColourCorrection => "Color Correction",
            QuickRow::GbOverlay => "GB Overlay",
            QuickRow::GbcOverlay => "GBC Overlay",
            QuickRow::GbaOverlay | QuickRow::NesOverlay | QuickRow::SnesOverlay | QuickRow::Ps1Overlay => "Overlay",
            QuickRow::Ps1Dithering => "Dithering",
            QuickRow::ScreenGb => "Game Boy / GBC",
            QuickRow::ScreenGba => "Game Boy Advance",
            QuickRow::ScreenNes => "NES",
            QuickRow::ScreenSnes => "SNES",
            QuickRow::ScreenPs1 => "PlayStation",
            QuickRow::Game => "Gameplay",
            QuickRow::EjectSave => "Save on Eject",
            QuickRow::AutoSave => "Auto Save",
            QuickRow::BatteryShutdown => "Low Battery Shutdown",
            QuickRow::Turbo => "Turbo Buttons",
            QuickRow::Rewind => "Rewind",
            QuickRow::SwapAb => "Swap A/B",
            QuickRow::GbPalettes => "GB Palettes",
        }
    }

    pub fn opens(self) -> bool {
        matches!(
            self,
            QuickRow::DateTime | QuickRow::About | QuickRow::Screen | QuickRow::Game | QuickRow::Lights | QuickRow::Wifi | QuickRow::CartArt | QuickRow::Achievements | QuickRow::Update | QuickRow::System | QuickRow::Library
                | QuickRow::LibrarySystems | QuickRow::ResetSettings | QuickRow::AboutLabel | QuickRow::SystemSounds
                | QuickRow::Online | QuickRow::Clock | QuickRow::Saving | QuickRow::Music
                | QuickRow::ScreenGb | QuickRow::ScreenGba | QuickRow::ScreenNes | QuickRow::ScreenSnes | QuickRow::ScreenPs1
        )
    }

    pub fn up(self) -> QuickRow {
        let page = self.page();
        page[(self.position() + page.len() - 1) % page.len()]
    }

    pub fn down(self) -> QuickRow {
        let page = self.page();
        page[(self.position() + 1) % page.len()]
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum QuickValue {
    Speed2,
    Speed3,
    Speed4,
    Speed6,
    On,
    Off,
    Lcd3x,
    Grid,
    Dot,
    Simpletex,
    Stock,
    Low,
    Medium,
    High,
    White,
    Blue,
    Green,
    Amber,
    Red,
    Purple,
    Steady,
    Breathe,
    Unavailable,
    Min1,
    Min5,
    Min10,
    Percent3,
    Percent5,
    Percent10,
    SortAz,
    SortConsole,
    AllGames,
    Favorites,
    Minutes,
    Hours,
    /// Reset Settings, asked once: A again to go ahead.
    ResetAsk,
    /// Reset Settings, done.
    ResetDone,
}

impl QuickValue {
    pub const ALL: [QuickValue; 37] = [
        QuickValue::Speed2,
        QuickValue::Speed3,
        QuickValue::Speed4,
        QuickValue::Speed6,
        QuickValue::On,
        QuickValue::Off,
        QuickValue::Lcd3x,
        QuickValue::Grid,
        QuickValue::Dot,
        QuickValue::Simpletex,
        QuickValue::Stock,
        QuickValue::Low,
        QuickValue::Medium,
        QuickValue::High,
        QuickValue::White,
        QuickValue::Blue,
        QuickValue::Green,
        QuickValue::Amber,
        QuickValue::Red,
        QuickValue::Purple,
        QuickValue::Steady,
        QuickValue::Breathe,
        QuickValue::Unavailable,
        QuickValue::Min1,
        QuickValue::Min5,
        QuickValue::Min10,
        QuickValue::Percent3,
        QuickValue::Percent5,
        QuickValue::Percent10,
        QuickValue::SortAz,
        QuickValue::SortConsole,
        QuickValue::AllGames,
        QuickValue::Favorites,
        QuickValue::Minutes,
        QuickValue::Hours,
        QuickValue::ResetAsk,
        QuickValue::ResetDone,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn text(self) -> &'static str {
        match self {
            QuickValue::Speed2 => "2×",
            QuickValue::Speed3 => "3×",
            QuickValue::Speed4 => "4×",
            QuickValue::Speed6 => "6×",
            QuickValue::On => "On",
            QuickValue::Off => "Off",
            QuickValue::Lcd3x => "LCD3x",
            QuickValue::Grid => "Grid",
            QuickValue::Dot => "Dot",
            QuickValue::Simpletex => "Simpletex",
            QuickValue::Stock => "Stock",
            QuickValue::Low => "Low",
            QuickValue::Medium => "Medium",
            QuickValue::High => "High",
            QuickValue::White => "White",
            QuickValue::Blue => "Blue",
            QuickValue::Green => "Green",
            QuickValue::Amber => "Amber",
            QuickValue::Red => "Red",
            QuickValue::Purple => "Purple",
            QuickValue::Steady => "Steady",
            QuickValue::Breathe => "Breathe",
            QuickValue::Unavailable => "Unavailable",
            QuickValue::Min1 => "1 min",
            QuickValue::Min5 => "5 min",
            QuickValue::Min10 => "10 min",
            QuickValue::Percent3 => "3%",
            QuickValue::Percent5 => "5%",
            QuickValue::Percent10 => "10%",
            QuickValue::SortAz => "A–Z",
            QuickValue::SortConsole => "By Console",
            QuickValue::AllGames => "All Games",
            QuickValue::Favorites => "Favorites",
            QuickValue::Minutes => "Minutes",
            QuickValue::Hours => "Hours",
            QuickValue::ResetAsk => "Sure?",
            QuickValue::ResetDone => "Done",
        }
    }

    pub fn speed(frames: u8) -> Option<QuickValue> {
        match frames {
            2 => Some(QuickValue::Speed2),
            3 => Some(QuickValue::Speed3),
            4 => Some(QuickValue::Speed4),
            6 => Some(QuickValue::Speed6),
            _ => None,
        }
    }

    /// An autosave interval in minutes, 0 for off.
    pub fn minutes(min: u8) -> Option<QuickValue> {
        match min {
            0 => Some(QuickValue::Off),
            1 => Some(QuickValue::Min1),
            5 => Some(QuickValue::Min5),
            10 => Some(QuickValue::Min10),
            _ => None,
        }
    }

    /// A battery percent, 0 for off.
    pub fn percent(pct: u8) -> Option<QuickValue> {
        match pct {
            0 => Some(QuickValue::Off),
            3 => Some(QuickValue::Percent3),
            5 => Some(QuickValue::Percent5),
            10 => Some(QuickValue::Percent10),
            _ => None,
        }
    }

    pub fn flag(on: bool) -> QuickValue {
        if on {
            QuickValue::On
        } else {
            QuickValue::Off
        }
    }
}

pub const QUICK_PITCH: f32 = 52.0;
pub const QUICK_TOP: f32 = 108.0;

/// Row spacing for a page of `rows`: QUICK_PITCH, or tighter for a page that would
/// otherwise run into the legend (the Brick's main menu).
pub const fn quick_pitch(rows: usize) -> f32 {
    let fit = ((QUICK_SPLIT - QUICK_TOP - 6.0) / rows as f32) as u32 as f32;
    if fit < QUICK_PITCH { fit } else { QUICK_PITCH }
}
pub const QUICK_EDGE: f32 = 70.0;
const HEADER_EDGE: f32 = QUICK_EDGE - 20.0;
const HEADER_TOP: f32 = 42.0;
const QUICK_PX: f32 = 26.0;
const CARET_PX: f32 = 24.0;
pub const QUICK_SPLIT: f32 = LEGEND_Y - 2.0;
pub const QUICK_TEXT_H: u32 = MENU_H;

pub fn quick_label_face(row: QuickRow) -> UndoFace {
    quick_label_face_scaled(row, false, [1.0, 1.0]).0
}

pub fn quick_value_face(text: &str, lit: bool) -> UndoFace {
    quick_value_face_scaled(text, lit, [1.0, 1.0]).0
}

pub fn quick_label_face_scaled(row: QuickRow, active: bool, scale: [f32; 2]) -> (UndoFace, u32) {
    quick_text_face(row.label(), if active { MENU_ROSE_INK } else { MENU_TEXT_INK }, scale)
}

pub fn quick_value_face_scaled(text: &str, lit: bool, scale: [f32; 2]) -> (UndoFace, u32) {
    quick_text_face(text, if lit { MENU_ROSE_INK } else { MENU_MUTED_INK }, scale)
}

fn quick_text_face(label: &str, colour: [u8; 3], [sx, sy]: [f32; 2]) -> (UndoFace, u32) {
    let Some(font) = text::menu_font() else {
        return (UndoFace {
            rgba: Vec::new(),
            w: 0,
            h: 0,
        }, 0);
    };
    let layout = text::menu_line(font, &label.to_lowercase(), OUT_W as f32, QUICK_PX);
    let set = layout
        .lines
        .iter()
        .map(|l| text::line_width(font, l, layout.px, layout.tracking))
        .fold(0.0, f32::max);
    let width = set.ceil() as u32 + 2 * MENU_PAD;
    let w = (width as f32 * sx).round() as u32;
    let h = (MENU_H as f32 * sy).round() as u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    text::in_font(font, || text::draw_centred(&mut rgba, w, h, &layout.scaled(sy), colour));
    (UndoFace { rgba, w, h }, width)
}

/// The least space kept between a row's label and its value.
const VALUE_GAP: f32 = 28.0;

/// The widest value face that fits beside this label.
pub fn quick_value_room(label_w: u32) -> u32 {
    let pad = MENU_PAD as f32;
    let right = OUT_W as f32 - QUICK_EDGE;
    let label_end = QUICK_EDGE + label_w as f32 - 2.0 * pad;
    let room = right + 2.0 * pad - label_end - VALUE_GAP;
    room.max(0.0) as u32
}

pub fn quick_caret_face(right: bool) -> UndoFace {
    quick_caret_face_scaled(right, [1.0, 1.0]).0
}

pub fn quick_caret_face_scaled(right: bool, [sx, sy]: [f32; 2]) -> (UndoFace, u32) {
    let glyph = if right { '\u{f0da}' } else { '\u{f0d9}' };
    let (Some(symbols), Some(label)) = (crate::icon::symbols_font(), text::label_font()) else {
        return (UndoFace {
            rgba: Vec::new(),
            w: 0,
            h: 0,
        }, 0);
    };
    let width = symbols.metrics(glyph, CARET_PX).width as u32;
    let (m, cov) = symbols.rasterize(glyph, CARET_PX * sy);
    let w = (width as f32 * sx).round() as u32;
    let h = (MENU_H as f32 * sy).round() as u32;
    let px = MENU_PX * sy;
    let centre = match label.horizontal_line_metrics(px) {
        Some(v) => {
            (h as f32 - v.new_line_size) / 2.0 + v.ascent
                - label.metrics('H', px).height as f32 / 2.0
        }
        None => h as f32 / 2.0,
    };
    let top = (centre - m.height as f32 / 2.0).round() as i32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for gy in 0..m.height {
        let y = top + gy as i32;
        if y < 0 || y >= h as i32 {
            continue;
        }
        for gx in 0..m.width.min(w as usize) {
            let at = ((y as u32 * w + gx as u32) * 4) as usize;
            let a = cov[gy * m.width + gx];
            rgba[at..at + 4].copy_from_slice(&[MENU_INK[0], MENU_INK[1], MENU_INK[2], a]);
        }
    }
    (UndoFace { rgba, w, h }, width)
}

pub fn quick_legend_faces() -> [UndoFace; 4] {
    quick_legend_faces_scaled([1.0, 1.0]).map(|(face, _)| face)
}

pub fn quick_legend_faces_scaled(scale: [f32; 2]) -> [(UndoFace, u32); 4] {
    [
        menu_hint_face_scaled("B", "back", scale),
        menu_hint_face_scaled("‹ ›", "change", scale),
        menu_hint_face_scaled("A", "open", scale),
        menu_hint_face_scaled("A", "reset", scale),
    ]
}

pub struct QuickMenuFaces {
    pub scale: [f32; 2],
    pub labels: Vec<[(TexId, u32, u32); 2]>,
    pub values: Vec<[(TexId, u32, u32); 2]>,
    pub open: (TexId, u32, u32),
    pub root: (TexId, u32, u32),
    pub headings: Vec<Option<QuickHeading>>,
    /// Back, Change, Open, Reset.
    pub legend: [(TexId, u32); 4],
}

#[derive(Clone, Copy)]
pub struct QuickHeading {
    pub crumb: (TexId, u32, u32),
    pub title: (TexId, u32, u32),
}

pub struct QuickMenu<'a> {
    pub row: QuickRow,
    pub values: [Option<QuickValue>; QuickRow::ALL.len()],
    pub clock: Option<[(TexId, u32, u32); 2]>,
    /// Rows whose value is free text rather than one of the QuickValues (an overlay's name):
    /// its faces, dim and lit.
    pub texts: [Option<[(TexId, u32, u32); 2]>; QuickRow::ALL.len()],
    pub faces: Option<&'a QuickMenuFaces>,
    /// Rows shown greyed out that do nothing, such as Cart Art and RetroAchievements with Wi-Fi off.
    pub disabled: [bool; QuickRow::ALL.len()],
}

impl QuickMenu<'_> {
    pub fn draw(&self, out: &mut Vec<Draw>) {
        out.push(Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: OUT_W as f32,
            h: OUT_H as f32,
            colour: MENU_BACKGROUND,
        });
        let pitch = quick_pitch(self.row.page().len());
        draw_menu_cursor(out, HEADER_EDGE, row_top(self.row) + pitch / 2.0);
        let Some(faces) = self.faces else {
            return;
        };
        let mut draw = |(tex, w, h), x, y, alpha| {
            if w > 0 && h > 0 {
                out.push(pixel_aligned_texture(tex, [x, y], [w as f32, h as f32], faces.scale, alpha));
            }
        };
        if let Some(heading) = self.row.parent().and_then(|row| faces.headings.get(row.index())).copied().flatten() {
            let (_, crumb_w, crumb_h) = heading.crumb;
            let (_, _, title_h) = heading.title;
            draw(heading.crumb, HEADER_EDGE, HEADER_TOP + (title_h as f32 - crumb_h as f32) / 2.0 + 3.0, 1.0);
            draw(heading.title, HEADER_EDGE + crumb_w as f32 + 9.0, HEADER_TOP, 1.0);
        } else {
            draw(faces.root, HEADER_EDGE, HEADER_TOP, 1.0);
        }
        let (right, pad) = (OUT_W as f32 - QUICK_EDGE, MENU_PAD as f32);
        for &row in self.row.page() {
            let y = row_top(row) + (pitch - MENU_H as f32) / 2.0;
            let lit = row == self.row;
            let alpha = if self.disabled[row.index()] { DISABLED_ALPHA } else { 1.0 };
            if let Some(labels) = faces.labels.get(row.index()) {
                draw(labels[lit as usize], QUICK_EDGE - pad, y, alpha);
            }
            let value = match row {
                QuickRow::DateTime => self.clock.map(|c| c[lit as usize]),
                _ if self.texts[row.index()].is_some() => self.texts[row.index()].map(|t| t[lit as usize]),
                _ => self.values[row.index()]
                    .and_then(|v| faces.values.get(v.index()))
                    .map(|v| v[lit as usize]),
            };
            if let Some((tex, w, h)) = value {
                draw((tex, w, h), right + pad - w as f32, y, alpha);
            } else if row.opens() {
                draw(faces.open, right + pad - faces.open.1 as f32, y, alpha);
            }
        }
        let [back, change, open, reset] = faces.legend;
        let other = match self.row {
            QuickRow::ResetSettings => reset,
            row if row.opens() => open,
            _ => change,
        };
        let done = self.values[self.row.index()] == Some(QuickValue::ResetDone);
        let only_back = self.disabled[self.row.index()] || done || self.row.info();
        let hints: &[(TexId, u32)] = if only_back { &[back] } else { &[back, other] };
        draw_menu_hints(out, hints, HEADER_EDGE, faces.scale);
    }
}

fn row_top(row: QuickRow) -> f32 {
    let pitch = quick_pitch(row.page().len());
    QUICK_TOP + pitch * row.position() as f32
}

const DISABLED_ALPHA: f32 = 0.35;
