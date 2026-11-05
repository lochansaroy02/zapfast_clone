//! The glass navigation bar: the rail's destinations in a floating capsule
//! of liquid glass at the foot of the chat list, as a phone keeps its tabs at
//! the bottom of the screen. Settings > Appearance > Glass navigation bar
//! turns it on; the rail then steps aside wherever the full list shows, and
//! stays where the list is collapsed to its avatars, which only it holds.
//!
//! The reader picks which destinations it holds (Settings > Appearance), and
//! Settings is always among them; our avatar, the account switcher, moves to
//! the list header's corner.
//!
//! The list scrolls on beneath the bar: what passes under it is blurred and
//! bent at the rim by `glass::lens`, then tinted, edged with light,
//! and the lit destination sits on a lighter capsule that slides from tab to
//! tab.

use egui::{Align2, Color32, Margin, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};

use crate::app::App;
use crate::settings::NavItem;
use crate::theme::{self, Palette};

use super::focus::TabStop;
use super::rail::{self, Destination, Look};
use super::widgets;

/// The bar's height.
const HEIGHT: f32 = 62.0;
/// Space between the bar and the list's sides and foot.
const MARGIN: f32 = 12.0;
/// The width an item takes when the list has room for it; the bar is as
/// wide as its items need, centred, and narrows them in a narrow list.
const SLOT: f32 = 76.0;
/// Space inside the bar around its items.
const PAD: f32 = 5.0;
/// A destination's icon.
const ICON: f32 = 21.0;
/// Our avatar, the account switcher, in the list header's corner.
pub const AVATAR: f32 = 30.0;
/// The least width an item needs to carry its name under its icon.
const NAMED: f32 = 58.0;
/// How long the lit capsule takes to slide to another tab.
const SLIDE: f32 = 0.28;

/// Whether the glass bar takes the rail's place beside the full list.
pub fn active(app: &App) -> bool {
    app.settings.glass_navigation
}

/// Whether the bar is on and holds `item`.
pub fn holds(app: &App, item: NavItem) -> bool {
    active(app) && app.settings.glass_nav_items.contains(&item)
}

/// The destinations the bar shows now: the reader's picks that are offered,
/// in the bar's order, then Settings.
pub fn destinations(app: &App) -> Vec<Destination> {
    NavItem::ALL
        .into_iter()
        .filter(|item| app.settings.glass_nav_items.contains(item))
        .map(|item| match item {
            NavItem::Chats => Destination::Chats,
            NavItem::Calls => Destination::Calls,
            NavItem::Favorites => Destination::Favorites,
            NavItem::Starred => Destination::Starred,
            NavItem::Archived => Destination::Archived,
            NavItem::Locked => Destination::Locked,
        })
        .chain([Destination::Settings])
        .filter(|destination| destination.offered(app))
        .collect()
}

/// Our avatar at the start of the list header while the bar is on, as a
/// phone puts the profile in the top corner, with a gap after it.
pub fn profile(app: &mut App, ui: &mut egui::Ui) {
    if !active(app) {
        return;
    }
    super::accounts::avatar_button(app, ui, AVATAR).tab_stop(super::focus::Stop::Profile);
    ui.add_space(10.0);
}

/// Where the bar was laid out, for layout tests.
pub(crate) fn rect_id() -> egui::Id {
    egui::Id::new("glass-navigation-bar")
}

/// The room a list leaves under its last row so it can scroll clear of the
/// bar, or none while the rail stands in for it.
pub fn bottom_margin(app: &App) -> Margin {
    if active(app) {
        Margin {
            bottom: (HEIGHT + 2.0 * MARGIN) as i8,
            ..Margin::ZERO
        }
    } else {
        Margin::ZERO
    }
}

/// How much of a list's height the bar covers, for scrolling a row into
/// view above it.
pub fn covered(app: &App) -> f32 {
    f32::from(bottom_margin(app).bottom)
}

/// The bar, floating over the foot of `area`, the list pane. Call it after
/// the list is drawn: it blurs what is already there, and its buttons take
/// clicks from the rows beneath.
pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let palette = app.palette;
    let destinations = destinations(app);
    let slots = destinations.len();
    let width = (area.width() - 2.0 * MARGIN).min(slots as f32 * SLOT + 2.0 * PAD);
    if width < HEIGHT {
        return;
    }
    let bar = Rect::from_center_size(
        pos2(area.center().x, area.bottom() - MARGIN - HEIGHT / 2.0),
        vec2(width, HEIGHT),
    );
    ui.ctx().data_mut(|data| data.insert_temp(rect_id(), bar));
    let radius = HEIGHT / 2.0;

    // The whole capsule takes clicks, so one between two items never lands
    // on the row beneath it.
    ui.interact(bar, rect_id(), Sense::click());

    let inner = bar.shrink(PAD);
    let slot = inner.width() / slots as f32;
    let named = slot >= NAMED;
    let cell = |index: usize| {
        Rect::from_min_size(
            pos2(inner.left() + slot * index as f32, inner.top()),
            vec2(slot, inner.height()),
        )
    };

    paint_glass(ui, &palette, bar, radius);

    // The lit destination's capsule slides to it, stretching as it goes.
    if let Some(lit) = destinations.iter().position(|d| d.lit(app)) {
        let target = cell(lit).center().x;
        let x = ui
            .ctx()
            .animate_value_with_time(rect_id().with("lit"), target, SLIDE);
        let stretch = ((x - target).abs() * 0.5).min(slot * 0.6);
        // A round lens where the items are tight, a wide capsule where
        // they carry names.
        let capsule = Rect::from_center_size(
            pos2(x, inner.center().y),
            vec2(slot - 4.0 + stretch, inner.height().min(slot - 4.0)),
        );
        let fill = if palette.dark {
            Color32::from_white_alpha(30)
        } else {
            Color32::from_black_alpha(14)
        };
        ui.painter()
            .rect_filled(capsule, capsule.height() / 2.0, fill);
        ui.painter().rect_stroke(
            capsule,
            capsule.height() / 2.0,
            Stroke::new(
                0.75,
                Color32::from_white_alpha(if palette.dark { 26 } else { 90 }),
            ),
            StrokeKind::Inside,
        );
    }

    for (index, destination) in destinations.into_iter().enumerate() {
        item(app, ui, &palette, destination, cell(index), named);
    }
}

/// The glass itself: a soft shadow, the blurred and bent content beneath,
/// a tint that keeps the icons legible where nothing can blur, and a rim
/// lit from above.
fn paint_glass(ui: &egui::Ui, palette: &Palette, bar: Rect, radius: f32) {
    let painter = ui.painter();
    painter.add(
        egui::epaint::Shadow {
            offset: [0, 6],
            blur: 22,
            spread: 0,
            color: Color32::from_black_alpha(if palette.dark { 70 } else { 26 }),
        }
        .as_shape(bar, radius),
    );
    painter.add(super::glass::lens(bar, radius));
    let tint = if palette.dark { 0.42 } else { 0.5 };
    painter.rect_filled(bar, radius, palette.panel.gamma_multiply(tint));
    // Light catches the upper rim more than the lower.
    let top = Rect::from_min_max(bar.min, pos2(bar.right(), bar.center().y));
    let bottom = Rect::from_min_max(pos2(bar.left(), bar.center().y), bar.max);
    let (upper, lower) = if palette.dark { (60, 22) } else { (190, 70) };
    for (half, alpha) in [(top, upper), (bottom, lower)] {
        painter.with_clip_rect(half).rect_stroke(
            bar,
            radius,
            Stroke::new(1.0, Color32::from_white_alpha(alpha)),
            StrokeKind::Inside,
        );
    }
    // A hairline outside keeps the edge on a light list.
    painter.rect_stroke(
        bar,
        radius,
        Stroke::new(
            0.5,
            Color32::from_black_alpha(if palette.dark { 90 } else { 22 }),
        ),
        StrokeKind::Outside,
    );
}

/// One destination: its icon, with its name under it when the bar is wide
/// enough, tinted with the accent while the window shows it.
fn item(
    app: &mut App,
    ui: &egui::Ui,
    palette: &Palette,
    destination: Destination,
    cell: Rect,
    named: bool,
) {
    let lit = destination.lit(app);
    let Look {
        icon,
        stop,
        name,
        tooltip,
        badge,
    } = rail::look(app, destination);
    let response = ui
        .interact(cell, rail::destination_id(destination), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    theme::reveal_focus(&response);
    let spoken = if badge > 0 {
        format!("{tooltip}, {badge}")
    } else {
        tooltip.clone()
    };
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            lit,
            spoken.as_str(),
        )
    });
    let hovered = response.hovered() || response.has_focus();
    if hovered && !lit {
        let wash = Rect::from_center_size(cell.center(), cell.size() - vec2(4.0, 0.0));
        let color = if palette.dark {
            Color32::from_white_alpha(12)
        } else {
            Color32::from_black_alpha(8)
        };
        ui.painter().rect_filled(wash, wash.height() / 2.0, color);
    }
    let tint = if lit { palette.accent } else { palette.text };
    let scale = if response.is_pointer_button_down_on() {
        0.9
    } else {
        1.0
    };
    let icon_center = if named {
        pos2(cell.center().x, cell.center().y - 7.0)
    } else {
        cell.center()
    };
    let plate = Rect::from_center_size(icon_center, Vec2::splat(ICON + 8.0));
    theme::paint_icon(ui, icon, plate, ICON * scale, tint);
    if named {
        ui.painter().text(
            pos2(cell.center().x, cell.center().y + 14.0),
            Align2::CENTER_CENTER,
            name,
            theme::medium(10.5),
            tint,
        );
    }
    if badge > 0 {
        widgets::unread_indicator(
            ui,
            palette,
            plate.right_top() + vec2(-2.0, 4.0),
            badge,
            false,
            false,
        );
    }
    let response = response.on_hover_text(tooltip).tab_stop(stop);
    if response.clicked() {
        let actions = destination.actions(app);
        app.actions.extend(actions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, ChatFilter};

    fn app(items: &[NavItem]) -> (tempfile::TempDir, App) {
        let root = tempfile::tempdir().unwrap();
        let settings = crate::settings::Settings {
            glass_navigation: true,
            glass_nav_items: items.to_vec(),
            ..Default::default()
        };
        let app = App::headless(crate::paths::AppDirs::under(root.path()), settings).0;
        (root, app)
    }

    #[test]
    fn the_bar_holds_the_picks_in_its_order_then_settings() {
        let (_root, mut app) = app(&[NavItem::Starred, NavItem::Chats]);
        assert_eq!(
            destinations(&app),
            [
                Destination::Chats,
                Destination::Starred,
                Destination::Settings
            ]
        );
        // Settings stays when nothing else is picked.
        app.settings.glass_nav_items.clear();
        assert_eq!(destinations(&app), [Destination::Settings]);
    }

    #[test]
    fn archived_shows_only_while_chats_are_archived() {
        let (_root, mut app) = app(&NavItem::DEFAULT);
        assert_eq!(
            destinations(&app),
            [
                Destination::Chats,
                Destination::Calls,
                Destination::Favorites,
                Destination::Settings
            ]
        );
        let mut chat = crate::model::Chat::new("1@s.whatsapp.net".into(), "Ada".into());
        chat.archived = true;
        chat.last_activity = 1;
        app.chats.push(chat);
        assert!(destinations(&app).contains(&Destination::Archived));
    }

    /// The Favorites tab is lit under the Favorites filter instead of Chats,
    /// and Chats takes the filter off again.
    #[test]
    fn favorites_and_chats_share_the_list() {
        let (_root, mut app) = app(&NavItem::DEFAULT);
        assert!(Destination::Chats.lit(&app));
        assert!(!Destination::Favorites.lit(&app));
        assert!(matches!(
            Destination::Favorites.actions(&app).as_slice(),
            [Action::SetChatFilter(ChatFilter::Favorites)]
        ));
        app.chat_filter = ChatFilter::Favorites;
        assert!(Destination::Favorites.lit(&app));
        assert!(!Destination::Chats.lit(&app));
        assert!(Destination::Favorites.actions(&app).is_empty());
        assert!(matches!(
            Destination::Chats.actions(&app).as_slice(),
            [Action::SetChatFilter(ChatFilter::All)]
        ));
        // Without the tab, Chats keeps its light under any filter.
        app.settings
            .glass_nav_items
            .retain(|item| *item != NavItem::Favorites);
        assert!(Destination::Chats.lit(&app));
        assert!(Destination::Chats.actions(&app).is_empty());
    }
}
