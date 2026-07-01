use bevy::prelude::*;
use bevy::state::condition::in_state;
use bevy::text::LineBreak;

use crate::app_state::AppState;
use crate::game::{HudText, LogText};

pub mod hud;
pub mod inventory;
pub mod log;
pub mod targeting;

#[derive(Component)]
pub struct UiRoot;

const PANEL_BG: Color = Color::srgb(0.02, 0.02, 0.03);
const PANEL_BORDER: Color = Color::srgb(0.30, 0.30, 0.34);
const TOP_BAR_HEIGHT: f32 = 44.0;
const SIDEBAR_WIDTH: f32 = 340.0;

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_game_ui);
        app.add_systems(
            Update,
            (
                hud::update_hud,
                inventory::update_inventory_ui,
                targeting::update_targeting,
                log::flush_combat_log,
            )
                .run_if(in_state(AppState::Playing).or_else(in_state(AppState::GameOver))),
        );
    }
}

fn setup_game_ui(mut commands: Commands<'_, '_>, existing: Query<'_, '_, Entity, With<UiRoot>>) {
    if existing.iter().next().is_some() {
        return;
    }

    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::NONE),
            UiRoot,
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    Node {
                        width: percent(100),
                        height: px(TOP_BAR_HEIGHT),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(16.0)),
                        border: UiRect::bottom(px(2.0)),
                        ..default()
                    },
                    BackgroundColor(PANEL_BG),
                    BorderColor {
                        top: Color::NONE,
                        right: Color::NONE,
                        bottom: PANEL_BORDER,
                        left: Color::NONE,
                    },
                ))
                .with_child((
                    Text::new(""),
                    TextFont::from_font_size(16.0),
                    TextColor(Color::WHITE),
                    HudText,
                ));

            parent
                .spawn(Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    body.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });

                    body.spawn((
                        Node {
                            width: px(SIDEBAR_WIDTH),
                            height: percent(100),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(12.0)),
                            row_gap: px(8.0),
                            border: UiRect::left(px(2.0)),
                            ..default()
                        },
                        BackgroundColor(PANEL_BG),
                        BorderColor {
                            top: Color::NONE,
                            right: Color::NONE,
                            bottom: Color::NONE,
                            left: PANEL_BORDER,
                        },
                    ))
                    .with_children(|sidebar| {
                        sidebar.spawn((
                            Text::new("Action Log"),
                            TextFont::from_font_size(15.0),
                            TextColor(Color::srgb(0.80, 0.80, 0.84)),
                        ));

                        sidebar
                            .spawn(Node {
                                flex_grow: 1.0,
                                width: percent(100),
                                overflow: Overflow::scroll_y(),
                                scrollbar_width: 10.0,
                                padding: UiRect::right(px(8.0)),
                                ..default()
                            })
                            .with_child((
                                Text::new(""),
                                TextFont::from_font_size(14.0),
                                TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                                TextColor(Color::srgb(0.85, 0.85, 0.85)),
                                LogText,
                            ));
                    });
                });
        });
}
