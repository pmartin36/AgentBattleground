use std::cell::RefCell;

use crossterm::event::MouseEvent;
use engine_core::color::Rgba;
use engine_render::dots::Dot;
use engine_render::{draw_dots, ui_primitives, Button, ButtonColors, ButtonCore, DotRect, StateColors};
use ratatui::layout::Rect;
use ratatui::Frame;

pub struct BattleMenu {
    showing: bool,
    finish_button: RefCell<Button>,
    /// Top-right close (X) button — shares the `close_button` glyph with the
    /// prompt-editor popup for consistency.
    close_button: RefCell<ButtonCore>,
}

impl BattleMenu {
    pub(super) fn toggle(&mut self) { self.showing = !self.showing; }
    pub(super) fn is_open(&self) -> bool { self.showing }

    /// Route a mouse event to the menu while it's open. Returns `true` if the
    /// "Finish Battle" button was clicked (a completed left-click inside it);
    /// a no-op returning `false` when the menu is closed. The button hit-tests
    /// against the rect it was given in the previous `render`.
    pub(super) fn handle_mouse(&mut self, ev: &MouseEvent) -> bool {
        if !self.showing {
            return false;
        }
        // A completed click on the close (X) dismisses the menu.
        if self.close_button.get_mut().handle_mouse(ev) {
            self.showing = false;
            return false;
        }
        self.finish_button.get_mut().handle_mouse(ev)
    }

    pub(super) fn new() -> Self {
        Self {
            showing: false,
            finish_button: RefCell::new(
                Button::new(Rect::default(), crate::assets::FRAME_PANEL)
                    .label("Finish Battle")
                    .colors(Self::FINISH_SCHEME),
            ),
            close_button: RefCell::new(ButtonCore::new(Rect::default())),
        }
    }

    const CLOSE_W: u16 = 3; // cells — 3×3 hit-area; draw_close_button centers a
    const CLOSE_H: u16 = 3; // round 6×6-dot circle + "X" inside it
    /// Inset of the close button from the panel's top and right. 1 cell top /
    /// 2 cells right — equal in dots (4 vs 4), so the gap reads evenly.
    const CLOSE_TOP_INSET: u16 = 1;
    const CLOSE_RIGHT_INSET: u16 = 2;
    /// Rows reserved at the panel top for the close button, so the centered
    /// "Finish Battle" button below it never collides with the corner X.
    const TOP_BAND: u16 = Self::CLOSE_TOP_INSET + Self::CLOSE_H;

    const BORDER_COLOR: Rgba = Rgba::rgb(0xff, 0xbf, 0x00);
    const BORDER_THICKNESS: usize = 1; // dots
    const CORNER_RADIUS: usize = 2; // dots chamfered off each corner
    const BUTTON_W: u16 = 20; // cells, capped to the panel width
    const BUTTON_H: u16 = 3; // cells, capped to the panel height

    /// Coordinated per-state scheme: the whole "Finish Battle" button — ring
    /// (background tint) AND label — brightens together on hover, the label
    /// lighting to the menu's amber `BORDER_COLOR`. Idle-vs-Hover label color
    /// differs by construction (spec 45:100).
    const FINISH_SCHEME: ButtonColors = ButtonColors {
        idle: StateColors {
            background: Rgba::rgb(0xc8, 0xc8, 0xc8),
            icon: Rgba::rgb(0xc8, 0xc8, 0xc8),
            label: Rgba::rgb(0xc8, 0x96, 0x28),
        },
        hover: StateColors {
            background: Rgba::rgb(0xff, 0xff, 0xff),
            icon: Rgba::rgb(0xff, 0xff, 0xff),
            label: Rgba::rgb(0xff, 0xbf, 0x00),
        },
        pressed: StateColors {
            background: Rgba::rgb(0x8c, 0x8c, 0x8c),
            icon: Rgba::rgb(0x8c, 0x8c, 0x8c),
            label: Rgba::rgb(0x9a, 0x72, 0x18),
        },
    };

    pub(super) fn render(&self, frame: &mut Frame, area: Rect) {
        if !self.showing {
            return;
        }

        let panel = Self::get_dot_rect(area);

        // Rounded panel: a lit border ring around an Occlude interior — the
        // hollow that covers the battle underneath.
        let buf = ui_primitives::rounded_rect(
            panel.w as usize,
            panel.h as usize,
            Self::BORDER_THICKNESS,
            Self::CORNER_RADIUS,
            Self::BORDER_COLOR,
            Dot::Occlude,
        );
        draw_dots(frame.buffer_mut(), panel.to_cell_rect(), &buf);

        // "Finish Battle" button: a fixed size (capped to the panel), centered
        // in the area below the close band.
        let mut button = self.finish_button.borrow_mut();
        button.set_rect(Self::button_rect(panel));
        button.render(frame.buffer_mut());
        drop(button);

        // Close (X): top-right corner, shared red glyph keyed to hover/press.
        let close_rect = Self::close_rect(panel);
        self.close_button.borrow_mut().set_rect(close_rect);
        let close_state = self.close_button.borrow().state();
        let close_dots = DotRect {
            x: close_rect.x as i32 * 2,
            y: close_rect.y as i32 * 4,
            w: close_rect.width as i32 * 2,
            h: close_rect.height as i32 * 4,
        };
        crate::scenes::close_button::draw_close_button(frame.buffer_mut(), close_dots, close_state);
    }

    /// The close (X) button rect: a `CLOSE_W × CLOSE_H` cell box in the panel's
    /// top-right corner, inset `CLOSE_TOP_INSET`/`CLOSE_RIGHT_INSET` from the
    /// top/right borders. Single source of truth for the close hit-test + render.
    fn close_rect(panel: DotRect) -> Rect {
        let p = panel.to_cell_rect();
        let w = Self::CLOSE_W.min(p.width);
        let h = Self::CLOSE_H.min(p.height);
        let x = p.x + p.width.saturating_sub(w).saturating_sub(Self::CLOSE_RIGHT_INSET);
        let y = p.y + Self::CLOSE_TOP_INSET;
        Rect::new(x, y, w, h)
    }

    fn get_dot_rect(area: Rect) -> DotRect {
        let screen = DotRect {
            x: area.x as i32 * 2,
            y: area.y as i32 * 4,
            w: area.width as i32 * 2,
            h: area.height as i32 * 4,
        };

        let w = screen.w * 40 / 100;
        let h = screen.h * 30 / 100;

        // Round to whole cells (2 dots wide, 4 tall) so the dot buffer maps 1:1
        // onto braille cells — otherwise `dots_to_grid` yields a trailing
        // partial cell that `to_cell_rect` doesn't reserve, clipping the edge.
        let w = w - w % 2;
        let h = h - h % 4;

        let x = screen.x + (screen.w - w) / 2;
        let y = screen.y + (screen.h - h) / 2;

        DotRect { x, y, w, h }
    }

    /// The "Finish Battle" button rect: a fixed `BUTTON_W × BUTTON_H` (capped
    /// to the panel so it never overflows a narrow one), centered in `panel`.
    /// Single source of truth for button placement, shared by `render` and the
    /// mouse hit-test.
    fn button_rect(panel: DotRect) -> Rect {
        let p = panel.to_cell_rect();
        let bw = Self::BUTTON_W.min(p.width);
        let bh = Self::BUTTON_H.min(p.height);
        // Center within the area BELOW the close band so the two never collide.
        let band = Self::TOP_BAND.min(p.height);
        let region_y = p.y + band;
        let region_h = p.height.saturating_sub(band);
        let bx = p.x + p.width.saturating_sub(bw) / 2;
        let by = region_y + region_h.saturating_sub(bh) / 2;
        Rect::new(bx, by, bw, bh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::Terminal;

    /// Render `menu` into a fresh `w×h` TestBackend and return the buffer.
    fn render(menu: &BattleMenu, w: u16, h: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                menu.render(f, area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent { kind, column: col, row, modifiers: KeyModifiers::empty() }
    }

    /// Count blank-occluder cells (braille U+2800) and lit-braille cells
    /// (U+2801..=U+28FF) across the whole buffer.
    fn tally(buf: &ratatui::buffer::Buffer, w: u16, h: u16) -> (usize, usize) {
        let (mut blanks, mut lit) = (0, 0);
        for y in 0..h {
            for x in 0..w {
                let s = buf.cell((x, y)).unwrap().symbol().to_string();
                if s == "\u{2800}" {
                    blanks += 1;
                } else if s.chars().next().is_some_and(|c| ('\u{2801}'..='\u{28FF}').contains(&c)) {
                    lit += 1;
                }
            }
        }
        (blanks, lit)
    }

    /// A closed menu must draw absolutely nothing — every cell stays blank space.
    #[test]
    fn closed_menu_draws_nothing() {
        let menu = BattleMenu::new(); // starts closed
        let buf = render(&menu, 80, 24);
        for y in 0..24 {
            for x in 0..80 {
                assert_eq!(
                    buf.cell((x, y)).unwrap().symbol(),
                    " ",
                    "closed menu must leave cell ({x},{y}) untouched"
                );
            }
        }
    }

    /// An open menu must occlude (braille-blank cells appear) AND draw a lit
    /// border/button (lit-braille cells appear) — proving the Occlude→Blank
    /// path reaches the screen and the frame renders.
    #[test]
    fn open_menu_occludes_and_draws_border() {
        let mut menu = BattleMenu::new();
        menu.toggle();
        let buf = render(&menu, 80, 24);
        let (blanks, lit) = tally(&buf, 80, 24);
        assert!(blanks > 0, "open menu must cover its interior with braille-blank occluders");
        assert!(lit > 0, "open menu must draw a lit border/button");
    }

    /// A completed left-click on the centered button reports the finish action.
    /// The button hit-tests against the rect set during `render`, so we render
    /// first.
    #[test]
    fn click_on_finish_button_reports_true() {
        let mut menu = BattleMenu::new();
        menu.toggle();
        let _ = render(&menu, 80, 24); // sets the button's rect

        // Click the button's ACTUAL centre — via the same helper `render`
        // uses — so this stays correct if the layout constants change.
        let panel = BattleMenu::get_dot_rect(Rect::new(0, 0, 80, 24));
        let button = BattleMenu::button_rect(panel);
        let cx = button.x + button.width / 2;
        let cy = button.y + button.height / 2;

        menu.handle_mouse(&mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));
        assert!(
            menu.handle_mouse(&mouse(MouseEventKind::Up(MouseButton::Left), cx, cy)),
            "down+up on the button centre ({cx},{cy}) must report a completed click"
        );
    }

    /// A completed left-click on the close (X) dismisses the menu.
    #[test]
    fn click_on_close_dismisses_menu() {
        let mut menu = BattleMenu::new();
        menu.toggle();
        assert!(menu.is_open());
        let _ = render(&menu, 80, 24); // sets the close button's rect

        let panel = BattleMenu::get_dot_rect(Rect::new(0, 0, 80, 24));
        let close = BattleMenu::close_rect(panel);
        let cx = close.x + close.width / 2;
        let cy = close.y + close.height / 2;

        menu.handle_mouse(&mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));
        menu.handle_mouse(&mouse(MouseEventKind::Up(MouseButton::Left), cx, cy));
        assert!(!menu.is_open(), "clicking the close (X) must dismiss the menu");
    }

    /// A click while the menu is closed is a no-op (never reports a finish).
    #[test]
    fn click_while_closed_is_noop() {
        let mut menu = BattleMenu::new(); // closed
        menu.handle_mouse(&mouse(MouseEventKind::Down(MouseButton::Left), 40, 12));
        assert!(
            !menu.handle_mouse(&mouse(MouseEventKind::Up(MouseButton::Left), 40, 12)),
            "a closed menu must not report clicks"
        );
    }

    /// Find the first cell in `buf` on row `y` within `[x0, x0+w)` painted
    /// with an ASCII-letter glyph — i.e. the "Finish Battle" label text
    /// (plain terminal chars, not braille dots — CLAUDE.md rule 4).
    fn find_label_cell(buf: &ratatui::buffer::Buffer, x0: u16, w: u16, y: u16) -> (u16, u16) {
        for x in x0..(x0 + w) {
            let s = buf.cell((x, y)).unwrap().symbol().to_string();
            if s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                return (x, y);
            }
        }
        panic!("no label glyph found in row {y}, cols [{x0}, {})", x0 + w);
    }

    /// The whole "Finish Battle" button — ring AND label — must recolor
    /// together on hover: the label's rendered foreground color at Hover
    /// must differ from its Idle color (spec 45's coordinated per-state
    /// scheme; the reason this widget-unification exists).
    #[test]
    fn finish_label_recolors_on_hover() {
        let mut menu = BattleMenu::new();
        menu.toggle();
        let _ = render(&menu, 80, 24); // sets the button's rect

        let panel = BattleMenu::get_dot_rect(Rect::new(0, 0, 80, 24));
        let button = BattleMenu::button_rect(panel);
        let label_row = button.y + button.height / 2;

        let buf_idle = render(&menu, 80, 24);
        let (lx, ly) = find_label_cell(&buf_idle, button.x, button.width, label_row);
        let idle_fg = buf_idle.cell((lx, ly)).unwrap().fg;

        let cx = button.x + button.width / 2;
        let cy = button.y + button.height / 2;
        menu.handle_mouse(&mouse(MouseEventKind::Moved, cx, cy));
        let buf_hover = render(&menu, 80, 24);
        let hover_fg = buf_hover.cell((lx, ly)).unwrap().fg;

        assert_ne!(
            idle_fg, hover_fg,
            "the \"Finish Battle\" label must recolor between Idle and Hover"
        );
    }
}