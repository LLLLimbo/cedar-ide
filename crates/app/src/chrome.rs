//! Presentation tokens and original outline glyphs for Cedar's Islands-inspired shell.
//! These colors are calibrated starting points, not a claim of JetBrains pixel parity.
use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, Widget};

pub const FRAME: Color32 = Color32::from_rgb(38, 40, 44);
pub const SURFACE: Color32 = Color32::from_rgb(25, 26, 28);
pub const BORDER: Color32 = Color32::from_rgb(48, 50, 54);
pub const TEXT: Color32 = Color32::from_rgb(209, 211, 217);
pub const MUTED: Color32 = Color32::from_rgb(147, 150, 158);
pub const HOVER: Color32 = Color32::from_rgb(53, 55, 61);
pub const SELECTED: Color32 = Color32::from_rgb(35, 53, 88);
pub const FOCUS: Color32 = Color32::from_rgb(75, 131, 245);
pub const GAP: i8 = 4;
pub const RADIUS: u8 = 10;
pub const ROW_HEIGHT: f32 = 24.0;
pub const INDENT: f32 = 18.0;
pub const HEADER_HEIGHT: f32 = 30.0;

/// Preserve egui's actual menu ownership and keyboard behavior while making
/// idle chrome quiet. Hover, keyboard focus and open-menu feedback stay visible.
pub fn menu_button<R>(
    ui: &mut Ui,
    title: impl Into<egui::WidgetText>,
    contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let style = ui.style().clone();
    ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
    let open = egui::menu::BarState::load(ui.ctx(), ui.id())
        .as_ref()
        .is_some_and(|root| root.id == ui.next_auto_id());
    let mut button = egui::Button::new(title).min_size(egui::vec2(26.0, 26.0));
    if open {
        button = button.fill(HOVER);
    }
    let response = egui::menu::menu_custom_button(ui, button, contents);
    ui.set_style(style);
    response
}

pub fn island() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(RADIUS)
        .inner_margin(8)
        .outer_margin(egui::Margin {
            left: 0,
            right: GAP,
            top: 0,
            bottom: GAP,
        })
}

#[derive(Clone, Copy)]
pub enum Icon {
    Project,
    Search,
    SearchText,
    Git,
    Back,
    Forward,
    Add,
    Refresh,
    Target,
    Close,
    Folder,
    File,
    Code,
    Java,
    ChevronRight,
    ChevronDown,
    More,
}

pub fn file_icon(path: &str, directory: bool) -> Icon {
    if directory {
        Icon::Folder
    } else if path.ends_with(".java") {
        Icon::Java
    } else if crate::syntax::supports(path) {
        Icon::Code
    } else {
        Icon::File
    }
}

pub fn paint_icon(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    let p = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + x * rect.width() / 20.0,
            rect.top() + y * rect.height() / 20.0,
        )
    };
    let stroke = Stroke::new(1.35_f32, color);
    let line = |points: &[(f32, f32)]| {
        ui.painter().add(egui::Shape::line(
            points.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ))
    };
    match icon {
        Icon::Project | Icon::Folder => {
            line(&[
                (2., 5.),
                (7., 5.),
                (9., 7.),
                (18., 7.),
                (18., 16.),
                (2., 16.),
                (2., 5.),
            ]);
        }
        Icon::Search | Icon::SearchText => {
            ui.painter()
                .circle_stroke(p(8.5, 8.5), rect.width() * 0.28, stroke);
            line(&[(12.5, 12.5), (18., 18.)]);
            if matches!(icon, Icon::SearchText) {
                line(&[(6., 7.), (11., 7.)]);
                line(&[(8.5, 7.), (8.5, 11.)]);
            }
        }
        Icon::Git => {
            for &(x, y) in &[(5., 4.), (15., 5.), (5., 16.)] {
                ui.painter()
                    .circle_stroke(p(x, y), rect.width() * 0.105, stroke);
            }
            line(&[(5., 6.), (5., 14.)]);
            line(&[(15., 7.), (15., 10.), (5., 13.)]);
        }
        Icon::Back => {
            line(&[(17., 10.), (3., 10.), (8., 5.)]);
            line(&[(3., 10.), (8., 15.)]);
        }
        Icon::Forward => {
            line(&[(3., 10.), (17., 10.), (12., 5.)]);
            line(&[(17., 10.), (12., 15.)]);
        }
        Icon::Add => {
            line(&[(10., 4.), (10., 16.)]);
            line(&[(4., 10.), (16., 10.)]);
        }
        Icon::Refresh => {
            line(&[
                (16., 8.),
                (15., 5.),
                (11., 3.),
                (7., 4.),
                (4., 7.),
                (4., 12.),
                (7., 16.),
                (12., 17.),
                (16., 14.),
            ]);
            line(&[(16., 3.), (16., 8.), (11., 8.)]);
        }
        Icon::Target => {
            ui.painter()
                .circle_stroke(p(10., 10.), rect.width() * 0.26, stroke);
            for points in [
                &[(10., 1.), (10., 5.)][..],
                &[(10., 15.), (10., 19.)],
                &[(1., 10.), (5., 10.)],
                &[(15., 10.), (19., 10.)],
            ] {
                line(points);
            }
        }
        Icon::Close => {
            line(&[(5., 5.), (15., 15.)]);
            line(&[(15., 5.), (5., 15.)]);
        }
        Icon::File => {
            line(&[
                (5., 2.),
                (12., 2.),
                (16., 6.),
                (16., 18.),
                (5., 18.),
                (5., 2.),
            ]);
            line(&[(12., 2.), (12., 6.), (16., 6.)]);
        }
        Icon::Code => {
            line(&[(7., 5.), (2., 10.), (7., 15.)]);
            line(&[(13., 5.), (18., 10.), (13., 15.)]);
        }
        Icon::Java => {
            ui.painter().circle_stroke(
                p(10., 10.),
                rect.width() * 0.4,
                Stroke::new(1.2_f32, egui::Color32::from_rgb(84, 142, 242)),
            );
            line(&[
                (13., 6.),
                (8., 6.),
                (6., 9.),
                (6., 12.),
                (8., 14.),
                (13., 14.),
            ]);
        }
        Icon::ChevronRight => {
            line(&[(8., 5.), (13., 10.), (8., 15.)]);
        }
        Icon::ChevronDown => {
            line(&[(5., 8.), (10., 13.), (15., 8.)]);
        }
        Icon::More => {
            for x in [4.0, 10.0, 16.0] {
                ui.painter()
                    .circle_filled(p(x, 10.0), rect.width() * 0.075, color);
            }
        }
    }
}

pub struct IconButton {
    pub icon: Icon,
    pub label: &'static str,
    pub selected: bool,
    pub rail: bool,
}
impl IconButton {
    pub fn new(icon: Icon, label: &'static str) -> Self {
        Self {
            icon,
            label,
            selected: false,
            rail: false,
        }
    }
    pub fn rail(mut self, selected: bool) -> Self {
        self.rail = true;
        self.selected = selected;
        self
    }
}
impl Widget for IconButton {
    fn ui(self, ui: &mut Ui) -> Response {
        let side = if self.rail { 36.0 } else { 26.0 };
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                ui.is_enabled(),
                self.selected,
                self.label,
            )
        });
        if ui.is_rect_visible(rect) {
            if self.selected || response.hovered() {
                ui.painter()
                    .rect_filled(rect, 6, if self.selected { SELECTED } else { HOVER });
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect,
                    6,
                    Stroke::new(1.0_f32, FOCUS),
                    egui::StrokeKind::Inside,
                );
            }
            let size = if self.rail { 20.0 } else { 16.0 };
            paint_icon(
                ui,
                Rect::from_center_size(rect.center(), Vec2::splat(size)),
                self.icon,
                if ui.is_enabled() {
                    TEXT
                } else {
                    Color32::from_rgb(85, 88, 94)
                },
            );
        }
        response
    }
}
