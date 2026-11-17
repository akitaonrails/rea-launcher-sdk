//! Where the focus is and what each input does, without a window: the
//! pages, the top bar, the hub's buttons and tiles, the settings rows, the
//! setup page's path fields and the launcher's extra page. The window
//! draws this and acts on the [`Effect`]s it returns.

use crate::catalog::Support;
use crate::pad::{self, Nav, PadNav};

/// One menu input, from the pad, the keyboard or a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Up,
    Down,
    Left,
    Right,
    /// A, Enter, Space.
    Press,
    /// B, Backspace.
    Back,
    /// Esc: back, or quit on the hub.
    Escape,
    /// Start: quit on the hub, else back to the hub.
    Start,
    /// Y (the View button): the selected game's settings.
    Settings,
    /// LB, Page Up.
    PreviousTab,
    /// RB, Page Down.
    NextTab,
}

impl Input {
    /// The hub's input for a pad step ([`crate::pad::PadNav`]).
    pub fn from_nav(nav: Nav) -> Input {
        match nav {
            Nav::Up => Input::Up,
            Nav::Down => Input::Down,
            Nav::Left => Input::Left,
            Nav::Right => Input::Right,
            Nav::Press => Input::Press,
            Nav::Back => Input::Back,
            Nav::Play => Input::Start,
            Nav::PreviousTab => Input::PreviousTab,
            Nav::NextTab => Input::NextTab,
        }
    }

    /// The hub's inputs for the pad's menu buttons now (XInput bits, the
    /// feeder's `Snapshot::nav`): `nav`'s steps, and Y as
    /// [`Input::Settings`], each once per press. What a launcher's
    /// `HubBackend::pad` returns.
    pub fn from_pad(nav: &mut PadNav, bits: u16) -> Vec<Input> {
        let settings = bits & pad::Y != 0 && nav.held() & pad::Y == 0;
        let mut inputs: Vec<Input> = nav.update(bits).into_iter().map(Input::from_nav).collect();
        if settings {
            inputs.push(Input::Settings);
        }
        inputs
    }
}

impl From<Nav> for Input {
    fn from(nav: Nav) -> Input {
        Input::from_nav(nav)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// The library folder and the launcher's other paths: the first run,
    /// and again from Settings.
    Setup,
    Hub,
    Controls,
    Settings,
    /// The launcher's own page for what a game still needs (Wangan's board
    /// files): a path field and a button, opened from the hub's first button.
    Extra,
}

impl Page {
    /// Pages built around typed paths: no top bar, keys type into the field.
    pub fn has_fields(self) -> bool {
        matches!(self, Page::Setup | Page::Extra)
    }
}

/// The top bar's sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Games,
    Controls,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Games, Tab::Controls, Tab::Settings];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Games => "Games",
            Tab::Controls => "Controls",
            Tab::Settings => "Settings",
        }
    }

    pub fn page(self) -> Page {
        match self {
            Tab::Games => Page::Hub,
            Tab::Controls => Page::Controls,
            Tab::Settings => Page::Settings,
        }
    }

    /// The tab a page belongs to; `None` for the pages without a top bar.
    pub fn of(page: Page) -> Option<Tab> {
        match page {
            Page::Hub => Some(Tab::Games),
            Page::Controls => Some(Tab::Controls),
            Page::Settings => Some(Tab::Settings),
            Page::Setup | Page::Extra => None,
        }
    }

    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// The settings page's items, top to bottom: the game's options (by their
/// `launcher.cfg` key), then the library actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Option(&'static str),
    Rescan,
    ChangeFolder,
}

/// The settings items for option rows `keys`.
pub fn settings_items(keys: &[&'static str]) -> Vec<Item> {
    keys.iter()
        .map(|k| Item::Option(k))
        .chain([Item::Rescan, Item::ChangeFolder])
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tab(Tab),
    Play,
    GameSettings,
    Tile,
    Item(usize),
    /// A path field: on the setup page 0 is the library folder and 1… the
    /// launcher's path fields; on the extra page there is only 0.
    Field(usize),
    /// The button beside [`Focus::Field`] with the same index.
    Submit(usize),
    /// The setup page's last stop.
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Start the selected game.
    Play,
    Quit,
    /// Change a setting by -1/+1; `wrap` when it came from Press.
    Adjust(&'static str, i32, bool),
    Rescan,
    /// Use what is typed in setup field `n` (0: add a library folder).
    SubmitPath(usize),
    /// Leave the setup page for the hub.
    Finish,
    /// The settings page was opened (load the selected game's form).
    OpenedSettings,
    /// The setup page was opened from settings.
    OpenedSetup,
    /// The extra page was opened for the selected game.
    OpenedExtra,
    /// Use what is typed on the extra page.
    SubmitExtra,
}

/// What the model needs to know about the world to move the focus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct World {
    /// The selected game can start now.
    pub playable: bool,
    /// A library folder is set (the setup page can be left).
    pub has_library: bool,
    /// The selected game waits for something the extra page adds (the
    /// hub's first button opens it).
    pub needs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub page: Page,
    pub focus: Focus,
    /// The selected game, an index into the launcher's games.
    pub selected: usize,
    pub games: usize,
    pub items: Vec<Item>,
    /// Setup fields: the library folder plus the launcher's path fields.
    pub fields: usize,
}

impl Model {
    /// A hub over `games` with settings rows `keys` and `paths` path fields
    /// besides the library folder; the setup page first when there is no
    /// library yet.
    pub fn new(games: usize, keys: &[&'static str], paths: usize, has_library: bool) -> Model {
        let mut m = Model {
            page: Page::Hub,
            focus: Focus::Tile,
            selected: 0,
            games,
            items: settings_items(keys),
            fields: paths + 1,
        };
        if !has_library {
            m.open(Page::Setup);
        }
        m
    }

    /// Go to a page with its natural first focus.
    pub fn open(&mut self, page: Page) -> Effect {
        self.page = page;
        self.focus = match page {
            Page::Hub => Focus::Tile,
            Page::Controls => Focus::Tab(Tab::Controls),
            Page::Settings => Focus::Item(0),
            Page::Setup | Page::Extra => Focus::Field(0),
        };
        match page {
            Page::Settings => Effect::OpenedSettings,
            Page::Extra => Effect::OpenedExtra,
            _ => Effect::None,
        }
    }

    fn hub_button(w: World) -> Focus {
        if w.playable || w.needs {
            Focus::Play
        } else {
            Focus::GameSettings
        }
    }

    pub fn select(&mut self, i: usize) {
        self.selected = i.min(self.games.saturating_sub(1));
    }

    pub fn input(&mut self, input: Input, w: World) -> Effect {
        if self.page != Page::Setup {
            if let Some(e) = self.on_any_page(input) {
                return e;
            }
            if let Focus::Tab(t) = self.focus {
                return self.on_tab(t, input, w);
            }
        }
        match self.page {
            Page::Hub => self.on_hub(input, w),
            Page::Settings => self.on_settings(input),
            Page::Controls => Effect::None,
            Page::Setup => self.on_setup(input, w),
            Page::Extra => self.on_extra(input),
        }
    }

    /// The inputs every page but setup shares: page keys, quit, back.
    fn on_any_page(&mut self, input: Input) -> Option<Effect> {
        match input {
            Input::PreviousTab | Input::NextTab => {
                let cur = Tab::of(self.page).unwrap_or(Tab::Games).index() as i32;
                let d = if input == Input::NextTab { 1 } else { -1 };
                let n = (cur + d).rem_euclid(Tab::ALL.len() as i32) as usize;
                Some(self.open(Tab::ALL[n].page()))
            }
            Input::Settings if self.page == Page::Hub => Some(self.open(Page::Settings)),
            Input::Start | Input::Escape if self.page == Page::Hub => Some(Effect::Quit),
            Input::Start | Input::Escape | Input::Back if self.page != Page::Hub => {
                let from = self.page;
                let e = self.open(Page::Hub);
                if from == Page::Extra {
                    self.focus = Focus::Play;
                }
                Some(e)
            }
            _ => None,
        }
    }

    fn on_tab(&mut self, t: Tab, input: Input, w: World) -> Effect {
        let i = t.index();
        match input {
            Input::Left if i > 0 => self.focus = Focus::Tab(Tab::ALL[i - 1]),
            Input::Right if i + 1 < Tab::ALL.len() => self.focus = Focus::Tab(Tab::ALL[i + 1]),
            Input::Press => {
                if self.page == t.page() {
                    return self.input(Input::Down, w);
                }
                let e = self.open(t.page());
                if t == Tab::Controls {
                    self.focus = Focus::Tab(Tab::Controls);
                }
                return e;
            }
            Input::Down => {
                self.focus = match self.page {
                    Page::Hub => Self::hub_button(w),
                    Page::Settings => Focus::Item(0),
                    _ => self.focus,
                }
            }
            Input::Back => self.focus = Focus::Tile,
            _ => {}
        }
        Effect::None
    }

    fn on_hub(&mut self, input: Input, w: World) -> Effect {
        match (self.focus, input) {
            (Focus::Tile, Input::Left) => self.select(self.selected.saturating_sub(1)),
            (Focus::Tile, Input::Right) => self.select(self.selected + 1),
            (Focus::Tile, Input::Up) => self.focus = Self::hub_button(w),
            (Focus::Tile | Focus::Play, Input::Press) if w.playable => return Effect::Play,
            (Focus::Tile | Focus::Play, Input::Press) if w.needs => {
                return self.open(Page::Extra);
            }
            (Focus::Tile, Input::Press) => self.focus = Focus::GameSettings,
            (Focus::Play | Focus::GameSettings, Input::Down | Input::Back) => {
                self.focus = Focus::Tile
            }
            (Focus::Play | Focus::GameSettings, Input::Up) => self.focus = Focus::Tab(Tab::Games),
            (Focus::Play, Input::Right) => self.focus = Focus::GameSettings,
            (Focus::GameSettings, Input::Left) if w.playable || w.needs => self.focus = Focus::Play,
            (Focus::GameSettings, Input::Press) => return self.open(Page::Settings),
            _ => {}
        }
        Effect::None
    }

    /// After the selection or the library changed: keep focus off a Play
    /// that can't be pressed.
    pub fn settle(&mut self, w: World) {
        if self.focus == Focus::Play && !w.playable && !w.needs {
            self.focus = Focus::GameSettings;
        }
    }

    fn on_settings(&mut self, input: Input) -> Effect {
        let Focus::Item(i) = self.focus else {
            self.focus = Focus::Item(0);
            return Effect::None;
        };
        let i = i.min(self.items.len() - 1);
        match (self.items[i], input) {
            (_, Input::Up) if i == 0 => self.focus = Focus::Tab(Tab::Settings),
            (_, Input::Up) => self.focus = Focus::Item(i - 1),
            (_, Input::Down) if i + 1 < self.items.len() => self.focus = Focus::Item(i + 1),
            (Item::Option(k), Input::Left) => return Effect::Adjust(k, -1, false),
            (Item::Option(k), Input::Right) => return Effect::Adjust(k, 1, false),
            (Item::Option(k), Input::Press) => return Effect::Adjust(k, 1, true),
            (Item::Rescan, Input::Press) => return Effect::Rescan,
            (Item::ChangeFolder, Input::Press) => {
                self.open(Page::Setup);
                return Effect::OpenedSetup;
            }
            _ => {}
        }
        Effect::None
    }

    /// The setup page's stops, top to bottom: each field and its button,
    /// then Done.
    fn setup_stops(&self) -> Vec<Focus> {
        (0..self.fields)
            .flat_map(|n| [Focus::Field(n), Focus::Submit(n)])
            .chain([Focus::Done])
            .collect()
    }

    fn on_setup(&mut self, input: Input, w: World) -> Effect {
        let stops = self.setup_stops();
        let at = stops.iter().position(|f| *f == self.focus).unwrap_or(0);
        match (self.focus, input) {
            (Focus::Field(n) | Focus::Submit(n), Input::Press) => return Effect::SubmitPath(n),
            (Focus::Done, Input::Press) if w.has_library => return Effect::Finish,
            (_, Input::Down) if at + 1 < stops.len() => self.focus = stops[at + 1],
            (_, Input::Up) if at > 0 => self.focus = stops[at - 1],
            (Focus::Field(n), Input::Right) => self.focus = Focus::Submit(n),
            (Focus::Submit(n), Input::Left) => self.focus = Focus::Field(n),
            (_, Input::Back | Input::Escape) if w.has_library => {
                self.open(Page::Settings);
                self.focus = Focus::Item(self.items.len() - 1);
                return Effect::OpenedSettings;
            }
            (_, Input::Escape | Input::Start) if !w.has_library => return Effect::Quit,
            _ => {}
        }
        Effect::None
    }

    fn on_extra(&mut self, input: Input) -> Effect {
        match (self.focus, input) {
            (Focus::Field(_) | Focus::Submit(_), Input::Press) => return Effect::SubmitExtra,
            (Focus::Field(_), Input::Down | Input::Right) => self.focus = Focus::Submit(0),
            (Focus::Submit(_), Input::Up | Input::Left) => self.focus = Focus::Field(0),
            _ => {}
        }
        Effect::None
    }
}

/// Something a game found in the library still needs before it can be
/// played (Wangan's board files), which the launcher's extra page adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    /// The hero's chip: "Needs board files".
    pub chip: &'static str,
    /// The tile's badge: "Add files".
    pub badge: &'static str,
    /// The hub's first button, which opens the extra page: "Add files".
    pub action: &'static str,
    /// The sentence under the chip: "Also needs the Wangan Maxi 3 files.".
    pub reason: String,
}

/// What a tile and the hero say about a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileState {
    /// The library is still being read.
    Looking,
    Ready,
    Unverified(&'static str),
    NotInLibrary,
    Needs(Need),
    Blocked(&'static str),
}

impl TileState {
    /// A game's state from its support here, the library scan and what it
    /// still needs. A blocked game stays blocked whatever the library says;
    /// a need only shows once the game is found.
    pub fn of(support: Support, scanned: bool, in_library: bool, need: Option<Need>) -> TileState {
        match (support, scanned, in_library, need) {
            (Support::Blocked(why), ..) => TileState::Blocked(why),
            (_, false, _, _) => TileState::Looking,
            (_, true, false, _) => TileState::NotInLibrary,
            (_, true, true, Some(need)) => TileState::Needs(need),
            (Support::Ready, true, true, None) => TileState::Ready,
            (Support::Unverified(note), true, true, None) => TileState::Unverified(note),
        }
    }

    /// Whether Play can be pressed.
    pub fn playable(&self) -> bool {
        matches!(self, TileState::Ready | TileState::Unverified(_))
    }

    pub fn chip(&self) -> &'static str {
        match self {
            TileState::Looking => "Looking for your games…",
            TileState::Ready => "Ready",
            TileState::Unverified(_) => "Starts, not verified",
            TileState::NotInLibrary => "Not in your library",
            TileState::Needs(need) => need.chip,
            TileState::Blocked(_) => "Not supported yet",
        }
    }

    /// The tile's small badge (shape and word, never colour alone).
    pub fn badge(&self) -> Option<&'static str> {
        match self {
            TileState::NotInLibrary => Some("Not found"),
            TileState::Blocked(_) => Some("Locked"),
            TileState::Unverified(_) => Some("Unverified"),
            TileState::Needs(need) => Some(need.badge),
            TileState::Ready | TileState::Looking => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pads_buttons_become_hub_inputs_once_per_press() {
        use crate::pad::{self, PadNav};
        let mut nav = PadNav::default();
        assert_eq!(
            Input::from_pad(&mut nav, pad::DPAD_DOWN | pad::Y),
            [Input::Down, Input::Settings]
        );
        assert!(
            Input::from_pad(&mut nav, pad::DPAD_DOWN | pad::Y).is_empty(),
            "held"
        );
        assert_eq!(Input::from_pad(&mut nav, pad::A), [Input::Press]);
        assert_eq!(
            Input::from_pad(&mut nav, pad::START | pad::Y),
            [Input::Start, Input::Settings]
        );
        assert!(Input::from_pad(&mut nav, 0).is_empty());
    }

    const GAMES: usize = 11;
    const KEYS: [&str; 3] = ["render", "output", "deadzone"];

    const READY: World = World {
        playable: true,
        has_library: true,
        needs: false,
    };
    const NOT_READY: World = World {
        playable: false,
        has_library: true,
        needs: false,
    };
    const NEEDS: World = World {
        playable: false,
        has_library: true,
        needs: true,
    };
    const NONE: World = World {
        playable: false,
        has_library: false,
        needs: false,
    };

    fn hub() -> Model {
        Model::new(GAMES, &KEYS, 1, true)
    }

    #[test]
    fn every_pad_step_has_a_hub_input() {
        for (nav, input) in [
            (Nav::Up, Input::Up),
            (Nav::Down, Input::Down),
            (Nav::Left, Input::Left),
            (Nav::Right, Input::Right),
            (Nav::Press, Input::Press),
            (Nav::Back, Input::Back),
            (Nav::Play, Input::Start),
            (Nav::PreviousTab, Input::PreviousTab),
            (Nav::NextTab, Input::NextTab),
        ] {
            assert_eq!(Input::from(nav), input);
        }
    }

    #[test]
    fn tiles_move_left_right_and_clamp() {
        let mut m = hub();
        assert_eq!((m.page, m.focus), (Page::Hub, Focus::Tile));
        m.input(Input::Left, READY);
        assert_eq!(m.selected, 0);
        for _ in 0..20 {
            m.input(Input::Right, READY);
        }
        assert_eq!(m.selected, GAMES - 1);
    }

    #[test]
    fn up_goes_tiles_to_buttons_to_top_bar_and_back_down() {
        let mut m = hub();
        m.input(Input::Up, READY);
        assert_eq!(m.focus, Focus::Play);
        m.input(Input::Right, READY);
        assert_eq!(m.focus, Focus::GameSettings);
        m.input(Input::Up, READY);
        assert_eq!(m.focus, Focus::Tab(Tab::Games));
        m.input(Input::Right, READY);
        assert_eq!(m.focus, Focus::Tab(Tab::Controls));
        m.input(Input::Back, READY);
        assert_eq!(m.focus, Focus::Tile);
        m.input(Input::Up, READY);
        m.input(Input::Down, READY);
        assert_eq!(m.focus, Focus::Tile);
    }

    #[test]
    fn play_only_when_playable() {
        let mut m = hub();
        assert_eq!(m.input(Input::Press, READY), Effect::Play);
        assert_eq!(m.input(Input::Press, NOT_READY), Effect::None);
        assert_eq!(
            m.focus,
            Focus::GameSettings,
            "Press on a locked game points at Settings"
        );
        m.input(Input::Down, NOT_READY);
        m.input(Input::Up, NOT_READY);
        assert_eq!(m.focus, Focus::GameSettings, "Play is skipped");
        m.input(Input::Left, NOT_READY);
        assert_eq!(m.focus, Focus::GameSettings);
        m.focus = Focus::Play;
        m.settle(NOT_READY);
        assert_eq!(m.focus, Focus::GameSettings);
    }

    #[test]
    fn quit_only_from_the_hub_and_back_returns_there() {
        let mut m = hub();
        assert_eq!(m.open(Page::Settings), Effect::OpenedSettings);
        assert_eq!(m.input(Input::Escape, READY), Effect::None);
        assert_eq!(m.page, Page::Hub);
        assert_eq!(
            m.input(Input::Back, READY),
            Effect::None,
            "Back never quits"
        );
        assert_eq!(m.input(Input::Escape, READY), Effect::Quit);
        m.open(Page::Controls);
        assert_eq!(m.input(Input::Start, READY), Effect::None);
        assert_eq!(m.page, Page::Hub, "Start goes back to the hub");
        assert_eq!(
            m.input(Input::Start, READY),
            Effect::Quit,
            "and quits there"
        );
    }

    #[test]
    fn y_opens_the_selected_games_settings_from_the_hub_only() {
        let mut m = hub();
        assert_eq!(m.input(Input::Settings, READY), Effect::OpenedSettings);
        assert_eq!((m.page, m.focus), (Page::Settings, Focus::Item(0)));
        m.open(Page::Controls);
        assert_eq!(m.input(Input::Settings, READY), Effect::None);
        assert_eq!(m.page, Page::Controls);
    }

    #[test]
    fn page_keys_cycle_the_top_bar_pages() {
        let mut m = hub();
        m.input(Input::NextTab, READY);
        assert_eq!(m.page, Page::Controls);
        assert_eq!(m.input(Input::NextTab, READY), Effect::OpenedSettings);
        m.input(Input::NextTab, READY);
        assert_eq!(m.page, Page::Hub);
        m.input(Input::PreviousTab, READY);
        assert_eq!(m.page, Page::Settings);
    }

    #[test]
    fn top_bar_opens_pages() {
        let mut m = hub();
        m.focus = Focus::Tab(Tab::Controls);
        m.input(Input::Press, READY);
        assert_eq!(
            (m.page, m.focus),
            (Page::Controls, Focus::Tab(Tab::Controls))
        );
        m.input(Input::Right, READY);
        assert_eq!(m.input(Input::Press, READY), Effect::OpenedSettings);
        assert_eq!((m.page, m.focus), (Page::Settings, Focus::Item(0)));
        m.focus = Focus::Tab(Tab::Settings);
        m.input(Input::Press, READY);
        assert_eq!(m.focus, Focus::Item(0), "the open tab moves into its page");
    }

    #[test]
    fn settings_rows_adjust_and_end_in_library_actions() {
        let mut m = hub();
        m.open(Page::Settings);
        assert_eq!(
            m.input(Input::Right, READY),
            Effect::Adjust("render", 1, false)
        );
        assert_eq!(
            m.input(Input::Left, READY),
            Effect::Adjust("render", -1, false)
        );
        assert_eq!(
            m.input(Input::Press, READY),
            Effect::Adjust("render", 1, true)
        );
        m.input(Input::Up, READY);
        assert_eq!(m.focus, Focus::Tab(Tab::Settings));
        for _ in 0..30 {
            m.input(Input::Down, READY);
        }
        assert_eq!(m.focus, Focus::Item(m.items.len() - 1));
        assert_eq!(m.items[m.items.len() - 2], Item::Rescan);
        m.input(Input::Up, READY);
        assert_eq!(m.input(Input::Press, READY), Effect::Rescan);
        m.input(Input::Down, READY);
        assert_eq!(m.input(Input::Press, READY), Effect::OpenedSetup);
        assert_eq!((m.page, m.focus), (Page::Setup, Focus::Field(0)));
        assert_eq!(m.input(Input::Back, READY), Effect::OpenedSettings);
        assert_eq!(
            (m.page, m.focus),
            (Page::Settings, Focus::Item(m.items.len() - 1))
        );
    }

    #[test]
    fn setup_walks_every_field_and_needs_a_library_to_finish() {
        let mut m = Model::new(GAMES, &KEYS, 2, false);
        assert_eq!((m.page, m.focus), (Page::Setup, Focus::Field(0)));
        assert_eq!(m.input(Input::NextTab, NONE), Effect::None, "no top bar");
        assert_eq!(m.page, Page::Setup);
        assert_eq!(m.input(Input::Back, NONE), Effect::None);
        assert_eq!(m.input(Input::Press, NONE), Effect::SubmitPath(0));
        m.input(Input::Right, NONE);
        assert_eq!(m.focus, Focus::Submit(0));
        assert_eq!(m.input(Input::Press, NONE), Effect::SubmitPath(0));
        m.input(Input::Left, NONE);
        assert_eq!(m.focus, Focus::Field(0));
        let mut seen = vec![m.focus];
        for _ in 0..10 {
            m.input(Input::Down, NONE);
            seen.dedup();
            seen.push(m.focus);
        }
        seen.dedup();
        assert_eq!(
            seen,
            [
                Focus::Field(0),
                Focus::Submit(0),
                Focus::Field(1),
                Focus::Submit(1),
                Focus::Field(2),
                Focus::Submit(2),
                Focus::Done
            ]
        );
        assert_eq!(m.input(Input::Press, NONE), Effect::None, "no folder yet");
        assert_eq!(m.input(Input::Press, READY), Effect::Finish);
        m.input(Input::Up, NONE);
        assert_eq!(m.input(Input::Press, NONE), Effect::SubmitPath(2));
        assert_eq!(m.input(Input::Escape, NONE), Effect::Quit);
        assert_eq!(m.input(Input::Start, NONE), Effect::Quit);
    }

    #[test]
    fn a_game_that_needs_files_opens_the_extra_page_from_play() {
        let mut m = hub();
        assert_eq!(m.input(Input::Press, NEEDS), Effect::OpenedExtra);
        assert_eq!((m.page, m.focus), (Page::Extra, Focus::Field(0)));
        assert_eq!(m.input(Input::Press, NEEDS), Effect::SubmitExtra);
        m.input(Input::Down, NEEDS);
        assert_eq!(m.focus, Focus::Submit(0));
        assert_eq!(m.input(Input::Press, NEEDS), Effect::SubmitExtra);
        m.input(Input::Up, NEEDS);
        assert_eq!(m.focus, Focus::Field(0));
        assert_eq!(m.input(Input::Back, NEEDS), Effect::None);
        assert_eq!(
            (m.page, m.focus),
            (Page::Hub, Focus::Play),
            "back on the button that opened it"
        );
        m.focus = Focus::Tile;
        m.input(Input::Up, NEEDS);
        assert_eq!(
            m.focus,
            Focus::Play,
            "Play is a stop while files are needed"
        );
        m.settle(NEEDS);
        assert_eq!(m.focus, Focus::Play);
        assert_eq!(m.input(Input::Press, NEEDS), Effect::OpenedExtra);
    }

    #[test]
    fn tile_state_from_support_library_and_needs() {
        let need = Need {
            chip: "Needs board files",
            badge: "Add files",
            action: "Add files",
            reason: "Also needs the board files.".into(),
        };
        let ready = Support::Ready;
        let unverified = Support::Unverified("Not verified in a race yet.");
        let blocked = Support::Blocked("Not yet.");
        assert_eq!(TileState::of(ready, false, false, None), TileState::Looking);
        assert_eq!(TileState::of(ready, true, true, None), TileState::Ready);
        assert_eq!(
            TileState::of(ready, true, false, None),
            TileState::NotInLibrary
        );
        assert_eq!(
            TileState::of(unverified, true, true, None),
            TileState::Unverified("Not verified in a race yet.")
        );
        assert_eq!(
            TileState::of(unverified, true, false, None),
            TileState::NotInLibrary
        );
        assert_eq!(
            TileState::of(ready, true, true, Some(need.clone())),
            TileState::Needs(need.clone())
        );
        assert_eq!(
            TileState::of(ready, true, false, Some(need.clone())),
            TileState::NotInLibrary,
            "a need shows once the game is found"
        );
        for (scanned, found) in [(false, false), (true, false), (true, true)] {
            assert_eq!(
                TileState::of(blocked, scanned, found, Some(need.clone())),
                TileState::Blocked("Not yet."),
                "blocked wins over the library"
            );
        }
        assert!(TileState::Ready.playable());
        assert!(TileState::Unverified("x").playable());
        for s in [
            TileState::Looking,
            TileState::NotInLibrary,
            TileState::Blocked("x"),
            TileState::Needs(need.clone()),
        ] {
            assert!(!s.playable(), "{s:?}");
        }
        assert_eq!(TileState::NotInLibrary.badge(), Some("Not found"));
        assert_eq!(TileState::Blocked("x").badge(), Some("Locked"));
        assert_eq!(TileState::Unverified("x").badge(), Some("Unverified"));
        assert_eq!(TileState::Needs(need.clone()).badge(), Some("Add files"));
        assert_eq!(TileState::Needs(need).chip(), "Needs board files");
        assert_eq!(TileState::Ready.badge(), None);
    }
}
