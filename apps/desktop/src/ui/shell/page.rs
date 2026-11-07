use gpui_kit::assets::IconName;

/// A destination in the main content area. Feature issues render their page in the slot
/// [`super::Shell`] reserves for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Albums,
    Songs,
    Artists,
    Queue,
    Playlists,
    ReplayGain,
    Settings(SettingsSection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    SourcesAndOutput,
    ListeningDevice,
    Playback,
    ReplayGain,
    Library,
    Appearance,
    Shortcuts,
    About,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Self::Albums => "Albums",
            Self::Songs => "Songs",
            Self::Artists => "Artists",
            Self::Queue => "Queue",
            Self::Playlists => "Playlists",
            Self::ReplayGain => "ReplayGain",
            Self::Settings(section) => section.title(),
        }
    }
}

impl SettingsSection {
    pub const ALL: [Self; 8] = [
        Self::SourcesAndOutput,
        Self::ListeningDevice,
        Self::Playback,
        Self::ReplayGain,
        Self::Library,
        Self::Appearance,
        Self::Shortcuts,
        Self::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::SourcesAndOutput => "Sources & output",
            Self::ListeningDevice => "Listening device",
            Self::Playback => "Playback",
            Self::ReplayGain => "ReplayGain",
            Self::Library => "Library",
            Self::Appearance => "Appearance",
            Self::Shortcuts => "Shortcuts",
            Self::About => "About",
        }
    }
}

/// A sidebar entry.
#[derive(Debug, Clone, Copy)]
pub struct NavEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: IconName,
    pub page: Page,
}

pub struct NavGroup {
    pub label: &'static str,
    pub entries: &'static [NavEntry],
}

pub const NAV_GROUPS: [NavGroup; 2] = [
    NavGroup {
        label: "Your library",
        entries: &[
            NavEntry {
                id: "nav-albums",
                label: "Albums",
                icon: IconName::Disc3,
                page: Page::Albums,
            },
            NavEntry {
                id: "nav-songs",
                label: "Songs",
                icon: IconName::Music,
                page: Page::Songs,
            },
            NavEntry {
                id: "nav-artists",
                label: "Artists",
                icon: IconName::User,
                page: Page::Artists,
            },
            NavEntry {
                id: "nav-queue",
                label: "Queue",
                icon: IconName::ListMusic,
                page: Page::Queue,
            },
            NavEntry {
                id: "nav-playlists",
                label: "Playlists",
                icon: IconName::Heart,
                page: Page::Playlists,
            },
        ],
    },
    NavGroup {
        label: "Collection tools",
        entries: &[
            NavEntry {
                id: "nav-replaygain",
                label: "ReplayGain",
                icon: IconName::ScanLine,
                page: Page::ReplayGain,
            },
            NavEntry {
                id: "nav-sources",
                label: "Sources & output",
                icon: IconName::SlidersHorizontal,
                page: Page::Settings(SettingsSection::SourcesAndOutput),
            },
        ],
    },
];

impl NavEntry {
    /// Whether this entry is the current location. Every settings section belongs to the
    /// settings entry.
    pub fn is_current(&self, page: Page) -> bool {
        match (self.page, page) {
            (Page::Settings(_), Page::Settings(_)) => true,
            (entry, page) => entry == page,
        }
    }
}

/// Back/forward navigation. Visiting a page drops the forward entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    pages: Vec<Page>,
    index: usize,
}

impl History {
    pub fn new(start: Page) -> Self {
        Self {
            pages: vec![start],
            index: 0,
        }
    }

    pub fn current(&self) -> Page {
        self.pages[self.index]
    }

    pub fn visit(&mut self, page: Page) {
        if page == self.current() {
            return;
        }
        self.pages.truncate(self.index + 1);
        self.pages.push(page);
        self.index += 1;
    }

    pub fn can_go_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.index + 1 < self.pages.len()
    }

    pub fn back(&mut self) {
        if self.can_go_back() {
            self.index -= 1;
        }
    }

    pub fn forward(&mut self) {
        if self.can_go_forward() {
            self.index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visiting_after_going_back_drops_forward_pages() {
        let mut history = History::new(Page::Albums);
        history.visit(Page::Songs);
        history.visit(Page::Artists);
        history.back();
        assert_eq!(history.current(), Page::Songs);
        assert!(history.can_go_forward());

        history.visit(Page::ReplayGain);
        assert!(!history.can_go_forward());
        history.back();
        assert_eq!(history.current(), Page::Songs);
        history.back();
        assert_eq!(history.current(), Page::Albums);
        assert!(!history.can_go_back());
    }

    #[test]
    fn revisiting_the_current_page_adds_no_entry() {
        let mut history = History::new(Page::Albums);
        history.visit(Page::Albums);
        assert!(!history.can_go_back());
    }

    #[test]
    fn settings_sections_select_the_settings_entry() {
        let sources = NAV_GROUPS[1].entries[1];
        assert!(sources.is_current(Page::Settings(SettingsSection::Appearance)));
        assert!(!sources.is_current(Page::ReplayGain));
    }
}
