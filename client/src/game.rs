// SPDX-FileCopyrightText: 2024 Softbear, Inc.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::animation::{Animation, AnimationType};
use crate::background::TowerBackgroundLayer;
use crate::color::Color;
use crate::key_dispenser::KeyDispenser;
use crate::layout::{force_layout, tower_layout};
use crate::path::*;
use crate::road::RoadLayer;
use crate::settings::TowerSettings;
use crate::state::TowerState;
use crate::territory::Territories;
use crate::tutorial::Tutorial;
use crate::ui::{KiometRoute, KiometUi, KiometUiEvent, KiometUiProps, SelectedTower};
use common::chunk::ChunkRectangle;
use common::force::{Force, Path};
use common::info::{GainedTowerReason, Info, InfoEvent};
use common::protocol::{Command, Update};
use common::tower::{Tower, TowerId, TowerRectangle, TowerType};
use common::unit::Unit;
use common::units::Units;
use common::world::{World, WorldChunks};
use common::KIOMET_CONSTANTS;
use kodiak_client::glam::{IVec2, Vec2, Vec3, Vec4};
use kodiak_client::renderer::{DefaultRender, Layer, RenderChain, TextStyle};
use kodiak_client::renderer2d::{Camera2d, TextLayer};
use kodiak_client::{
    include_audio, js_hooks, translate, ClientContext, FatalError, GameClient, GameConstants, Key,
    MouseButton, MouseEvent, PanZoom, RankNumber, RateLimiter, Translator,
};
use serde::{Serialize, Deserialize};
use std::f32::consts::PI;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;

include_audio!("/data/audio.mp3" "./audio.json");

pub struct KiometGame {
    animations: Vec<Animation>,
    camera: Camera2d,
    drag: Option<Drag>,
    key_dispenser: KeyDispenser,
    lock_dialog: Option<TowerType>,
    pan_zoom: PanZoom,
    panning: bool,
    render_chain: RenderChain<TowerLayer>,
    selected_tower_id: Option<TowerId>,
    territories: Territories,
    tutorial: Tutorial,
    was_alive: bool,
    set_viewport_rate_limit: RateLimiter,
}

impl KiometGame {
    fn move_world_space(&mut self, world_space: Vec2, context: &mut ClientContext<Self>) {
        if let Some(drag) = self.drag.as_mut() {
            if let Some(closest) = get_closest(world_space, context) {
                if Some(closest) != drag.current.map(|(start, _)| start) {
                    drag.current = Some((closest, context.client.time_seconds));
                }
            } else {
                drag.current = None;
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct Drag {
    start: TowerId,
    current: Option<(TowerId, f32)>,
}

impl Drag {
    fn zip(drag: Option<Self>) -> Option<(TowerId, TowerId, f32)> {
        drag.and_then(move |drag| {
            drag.current
                .map(|(current, current_start)| (drag.start, current, current_start))
        })
    }
}

#[derive(Layer)]
#[render(&Camera2d)]
pub struct TowerLayer {
    background: TowerBackgroundLayer,
    roads: RoadLayer,
    paths: PathLayer,
    text: TextLayer,
}

impl KiometGame {
    const RULER_DRAG_DELAY: f32 = 1.2;
}

impl GameClient for KiometGame {
    const GAME_CONSTANTS: &'static GameConstants = KIOMET_CONSTANTS;
    const LICENSES: &'static str = concat!(
        include_str!("../../assets/audio/README.md"),
        include_str!("ui/translations/licenses.md")
    );

    type Audio = Audio;
    type GameRequest = Command;
    type GameState = TowerState;
    type UiEvent = KiometUiEvent;
    type UiProps = KiometUiProps;
    type UiRoute = KiometRoute;
    type Ui = KiometUi;
    type GameUpdate = Update;
    type GameSettings = TowerSettings;

    fn new(context: &mut ClientContext<Self>) -> Result<Self, FatalError> {
        let render_chain = RenderChain::new([45, 52, 54, 255], true, |renderer| {
            renderer.enable_angle_instanced_arrays();

            TowerLayer {
                background: TowerBackgroundLayer::new(&*renderer),
                roads: RoadLayer::new(&*renderer),
                paths: PathLayer::new(&*renderer),
                text: TextLayer::new(&*renderer),
            }
        })?;

        let mut game = Self {
            animations: Default::default(),
            camera: Camera2d::default(),
            drag: Default::default(),
            key_dispenser: Default::default(),
            lock_dialog: None,
            pan_zoom: Default::default(),
            panning: Default::default(),
            render_chain,
            selected_tower_id: Default::default(),
            territories: Default::default(),
            tutorial: Default::default(),
            was_alive: Default::default(),
            set_viewport_rate_limit: RateLimiter::new(0.15),
        };
        
        // 注册全局指针
        game.register_global_ptr();
        
        Ok(game)
    }

    fn translate_rank_number(t: &Translator, n: RankNumber) -> String {
        match n {
            RankNumber::Rank1 => translate!(t, "rank_1", "Candidate"),
            RankNumber::Rank2 => translate!(t, "rank_2", "Tactician"),
            RankNumber::Rank3 => translate!(t, "rank_3", "Strategist"),
            RankNumber::Rank4 => translate!(t, "rank_4", "Representative"),
            RankNumber::Rank5 => translate!(t, "rank_5", "Senator"),
            RankNumber::Rank6 => translate!(t, "rank_6", "Supreme Leader"),
        }
    }

    fn translate_rank_benefits(t: &Translator, n: RankNumber) -> Vec<String> {
        match n {
            RankNumber::Rank1 => vec![],
            RankNumber::Rank2 => vec![translate!(t, "Spawn with more soldiers")],
            RankNumber::Rank3 => vec![translate!(t, "Unlock all towers")],
            RankNumber::Rank4 => vec![],
            RankNumber::Rank5 => vec![translate!(t, "Spawn with fighters")],
            RankNumber::Rank6 => vec![],
        }
    }

    fn peek_mouse(&mut self, event: &MouseEvent, context: &mut ClientContext<Self>) {
        update_visible(context);

        match *event {
            MouseEvent::MoveViewSpace(view_space) => {
                if self.panning {
                    if let Some(old_view_space) = context.mouse.view_position {
                        let world_space = self.camera.to_world_position(view_space);
                        let old_world_space = self.camera.to_world_position(old_view_space);
                        self.pan_zoom.pan(world_space - old_world_space);
                    }
                }
            }
            MouseEvent::Button { button, down, .. } => match button {
                #[cfg(debug_assertions)]
                MouseButton::Middle => {
                    if down {
                        self.animations.push(Animation::new(
                            self.camera
                                .to_world_position(context.mouse.view_position.unwrap_or_default()),
                            AnimationType::Emp(Color::Red),
                            context.client.time_seconds,
                        ));
                    }
                }
                MouseButton::Left => {
                    if down {
                        if self.drag.is_none() && !self.panning {
                            if let Some(drag_start) = context.mouse.view_position.and_then(|v| {
                                get_closest(self.camera.to_world_position(v), context)
                            }) {
                                self.drag = Some(Drag {
                                    start: drag_start,
                                    current: Some((drag_start, context.client.time_seconds)),
                                });
                                if self.selected_tower_id != Some(drag_start) {
                                    // If they were equal, wait for mouse up before clearing selection.
                                    self.selected_tower_id = None;
                                }
                            } else {
                                self.selected_tower_id = None;
                            }
                        }
                    } else {
                        if let Some((start, current, current_start_time)) = Drag::zip(self.drag) {
                            if start == current {
                                if self.selected_tower_id == Some(start) {
                                    // Double click to deselect.
                                    // TODO don't deselect tower if tried dragging a path.
                                    self.selected_tower_id = None;
                                } else {
                                    self.selected_tower_id = Some(start);
                                }
                            } else if let Some((source_tower, _destination_tower)) = context
                                .state
                                .game
                                .world
                                .chunk
                                .get(start)
                                .zip(context.state.game.world.chunk.get(current))
                            {
                                if self.selected_tower_id != Some(start) {
                                    self.selected_tower_id = None;
                                }

                                let strength = source_tower.force_units();
                                let tower_edge_distance = source_tower.tower_type.ranged_distance();
                                let strength_edge_distance =
                                    (!strength.is_empty()).then(|| strength.max_edge_distance());
                                let max_edge_distance = strength_edge_distance
                                    .map_or(tower_edge_distance, |e| e.min(tower_edge_distance));
                                let shorter_max_edge_distance =
                                    max_edge_distance != tower_edge_distance;
                                let supply_tower_id = self.selected_tower_id.filter(|_| {
                                    source_tower.generates_mobile_units()
                                        && !shorter_max_edge_distance
                                });

                                let path = context.state.game.world.find_best_path(
                                    start,
                                    current,
                                    max_edge_distance,
                                    context.player_id().unwrap(),
                                    |tower_id| is_visible(context, tower_id),
                                );

                                if let Some(path) = path {
                                    let perilous =
                                        path.iter().any(|&tower_id| is_perilous(context, tower_id));

                                    if !perilous
                                        || !strength.contains(Unit::Ruler)
                                        || context.client.time_seconds
                                            >= current_start_time + Self::RULER_DRAG_DELAY
                                    {
                                        context.send_to_game(
                                            if let Some(tower_id) = supply_tower_id {
                                                let path = Path::new(path);
                                                Command::SetSupplyLine {
                                                    tower_id,
                                                    // TODO accept any invalid path.
                                                    path: (source_tower.supply_line.as_ref()
                                                        != Some(&path))
                                                    .then_some(path),
                                                }
                                            } else {
                                                Command::deploy_force_from_path(path)
                                            },
                                        );
                                    }
                                }
                            } else {
                                self.selected_tower_id = None;
                            }
                        } else {
                            self.selected_tower_id = None;
                        }
                        self.drag = None;
                    }
                }
                MouseButton::Right => {
                    self.close_tower_menu();
                    self.panning = down;
                }
                #[cfg(not(debug_assertions))]
                _ => {}
            },
            MouseEvent::Wheel(delta) => {
                self.close_tower_menu();

                self.pan_zoom.multiply_zoom(
                    self.camera
                        .to_world_position(context.mouse.view_position.unwrap_or_default()),
                    2f32.powf(delta * (1.0 / 3.0)),
                );
            }
            _ => {}
        }
    }

    fn render(&mut self, elapsed_seconds: f32, context: &ClientContext<Self>) {
        let mut frame = self.render_chain.begin(context.client.time_seconds);
        let (renderer, layer) = frame.draw();

        let camera = self.pan_zoom.get_center();
        let zoom = self.pan_zoom.get_zoom();
        let canvas_size = renderer.canvas_size();
        self.camera.update(camera, zoom, canvas_size);
        let zoom_per_pixel = zoom / canvas_size.x as f32;

        // Make sure this is after `Renderer::set_camera`.
        layer.background.update(camera, zoom, context, renderer);

        self.tutorial.render(
            &mut layer.paths,
            self.selected_tower_id,
            context.client.time_seconds,
        );

        let hovered_tower_id = context
            .mouse
            .view_position
            .and_then(|v| TowerId::closest(self.camera.to_world_position(v)));
        let show_similar_towers = self
            .selected_tower_id
            .filter(|_| context.keyboard.is_down(Key::T))
            .and_then(|id| context.state.game.world.chunk.get(id))
            .map(|t| t.tower_type);
        let get_visibility = |id| is_visible(context, id).then_some(1.0).unwrap_or_default();
        let me = context.player_id();

        for (tower_id, tower) in context
            .state
            .game
            .visible
            .iter(&context.state.game.world.chunk)
        {
            if !context.state.game.margin_viewport.contains(tower_id) {
                // TODO iter viewport intersection visible and towers.
                continue;
            }

            let tower_position = tower_id.as_vec2();
            let hovered = hovered_tower_id == Some(tower_id);
            let selected = self.selected_tower_id == Some(tower_id);
            let tower_scale = tower.tower_type.scale() as f32;

            if zoom_per_pixel < 0.3 {
                for nearby_tower_id in tower_id.neighbors() {
                    if !exists(context, nearby_tower_id) {
                        continue; // Hasn't been generated yet.
                    }

                    let visible = is_visible(context, nearby_tower_id);
                    if nearby_tower_id >= tower_id && visible {
                        continue; // Don't draw twice.
                    }

                    // Fade out roads of invisible towers.
                    let s = Vec3::splat(1.0).extend(0.05);
                    let e = if visible { s.w } else { 0.0 };

                    layer
                        .roads
                        .draw_road(tower_position, nearby_tower_id.as_vec2(), 0.12, s, e);
                }
            }

            let show_supply_lines = context.keyboard.is_down(Key::R);
            if show_supply_lines
                || Some(tower_id) == self.selected_tower_id
                || Some(tower_id) == hovered_tower_id
            {
                let is_selected = Some(tower_id) == self.selected_tower_id;
                let is_hover = Some(tower_id) == hovered_tower_id && !is_selected;
                let is_dragging = Some(tower_id) == self.drag.map(|Drag { start, .. }| start);

                if show_supply_lines || !is_hover || !is_dragging {
                    if let Some(path) = &tower.supply_line {
                        if tower.player_id.is_some() && (tower.player_id == me || context.cheats())
                        {
                            let alpha = if is_selected {
                                if is_dragging {
                                    0.5 // Darken selected while changing it.
                                } else {
                                    1.0
                                }
                            } else if is_hover && show_supply_lines {
                                0.5 // Make hovered stand out against the other supply lines.
                            } else {
                                0.3
                            };

                            layer.roads.draw_path(
                                path.iter(),
                                Some(u32::MAX), // Existing supply lines must be valid.
                                usize::MAX,
                                true,
                                |id| get_visibility(id) * alpha,
                            );
                        }
                    }
                }
            }

            fn draw_shield(
                layer: &mut PathLayer,
                position: Vec2,
                intensity: f32,
                radius: f32,
                color: Color,
                selected: bool,
            ) {
                if intensity <= 0.0 || radius <= 0.0 {
                    return;
                }

                layer.draw_circle(
                    position,
                    radius,
                    selected.then_some(Vec3::splat(1.0).extend(0.33)),
                    (intensity > 0.0).then(|| color.shield_color().extend(intensity.sqrt())),
                );
            }

            let (shield_intensity, shield_radius) = tower_shield_intensity_radius(tower);
            let color = Color::new(context, tower.player_id);

            if zoom_per_pixel < 0.4 {
                draw_shield(
                    &mut layer.paths,
                    tower_position,
                    shield_intensity,
                    shield_radius,
                    color,
                    selected,
                );
            }

            let mut nuke = None;
            for force in &tower.inbound_forces {
                if force.units.contains(Unit::Nuke)
                    && (force.units.len() == 1
                        || (!tower.units.is_empty() && tower.player_id != force.player_id))
                {
                    let color = Color::new(context, force.player_id);
                    nuke = nuke.max(Some(color.make_gray_red()));
                }
            }
            if let Some(color) = nuke {
                let t = (renderer.time * PI).sin();
                let angle = (t * 0.075 + 0.25) * PI;
                let scale = shield_radius.max(0.55) * 3.6 + t * 0.075;
                let (stroke, _) = color.colors(true, hovered, selected);

                layer.paths.draw_path_a(
                    PathId::Target,
                    tower_position,
                    angle,
                    scale,
                    stroke.map(|v| v.extend(0.45)),
                    None,
                    false,
                );
            }

            let active = tower.active();
            let (stroke_color, fill_color) = color.colors(active, hovered, selected);

            // TODO draw simple sprite above certain zoom_per_pixel.
            layer.paths.draw_path(
                PathId::Tower(tower.tower_type),
                tower_position,
                0.0,
                tower_scale,
                stroke_color,
                fill_color,
                active,
            );

            if show_similar_towers == Some(tower.tower_type) {
                let x = (renderer.time * PI).sin().abs();
                let scale = (zoom * 0.025).max(2.0) * 0.75;
                let offset = Vec2::new(0.0, tower_scale * 0.75 + scale * 0.45 + scale * (x * 0.12));
                let color = 1.0 - x * 0.1;

                layer.paths.draw_path(
                    PathId::Marker,
                    tower_position + offset,
                    0.0,
                    scale,
                    Some(Vec3::splat(color * 1.0)),
                    Some(Vec3::splat(color * 0.73)),
                    true,
                )
            }

            let (stroke_color, fill_color) = color.colors(true, hovered, selected);
            if zoom_per_pixel < 0.2 {
                for unit_layout in tower_layout(tower, context.client.time_seconds) {
                    layer.paths.draw_path(
                        PathId::Unit(unit_layout.unit),
                        tower_position + unit_layout.relative_position,
                        unit_layout.angle,
                        unit_layout.scale,
                        stroke_color,
                        fill_color,
                        unit_layout.active,
                    );
                }
            }

            let mut draw_force = |force: &Force| {
                let force_position =
                    force.interpolated_position(context.state.game.time_since_last_tick);

                let color = Color::new(context, force.player_id);
                let (stroke_color, fill_color) = color.colors(true, hovered, selected);

                let (shield_intensity, shield_radius) =
                    shield_intensity_radius(force.units.available(Unit::Shield));
                draw_shield(
                    &mut layer.paths,
                    force_position,
                    shield_intensity,
                    shield_radius,
                    color,
                    false,
                );

                for unit_layout in force_layout(force) {
                    layer.paths.draw_path(
                        PathId::Unit(unit_layout.unit),
                        force_position + unit_layout.relative_position,
                        unit_layout.angle,
                        unit_layout.scale,
                        stroke_color,
                        fill_color,
                        unit_layout.active,
                    );
                }
            };

            if zoom_per_pixel < 0.4 {
                // Draw inbound forces and outbound forces heading to invisible towers.
                tower
                    .inbound_forces
                    .iter()
                    .for_each(|force| draw_force(force));
                tower
                    .outbound_forces
                    .iter()
                    .filter(|f| !is_visible(context, f.current_destination()))
                    .for_each(|force| draw_force(force));
            }

            if !context.state.game.tight_viewport.contains(tower_id) {
                continue;
            }

            if let Some(player_id) = tower.player_id {
                self.territories.record(tower_id, player_id);
            }
        }

        // Draw keys.
        if context.client.rewarded_ads
            && let Some((key, opacity)) = self.key_dispenser.key(context.client.time_seconds)
            && is_visible(context, key)
        {
            let (stroke, fill) = Color::Blue.colors(true, hovered_tower_id == Some(key), false);
            layer.paths.draw_path_a(
                PathId::Key,
                key.as_vec2() + Vec2::new(0.0, 1.5),
                0.0,
                1.0,
                stroke.map(|s| s.extend(opacity)),
                fill.map(|f| f.extend(opacity)),
                false,
            )
        }

        self.animations.retain(|animation| {
            animation.render(
                |center: Vec2, radius: f32, color: Vec4| {
                    layer.paths.draw_path_a(
                        PathId::Explosion,
                        center,
                        0.0,
                        radius,
                        None,
                        Some(color),
                        false,
                    );
                },
                context.client.time_seconds,
            )
        });

        self.territories
            .update(elapsed_seconds, |player_id, center, count| {
                if let Some(player) = context.state.core.player_or_bot(player_id) {
                    let outgoing_request = me
                        .map(|me| {
                            context
                                .state
                                .game
                                .world
                                .player(me)
                                .allies
                                .contains(&player_id)
                        })
                        .unwrap_or(false);
                    let incoming_request = me
                        .map(|me| {
                            context
                                .state
                                .game
                                .world
                                .player(player_id)
                                .allies
                                .contains(&me)
                        })
                        .unwrap_or(false);

                    let is_me = me == Some(player_id);
                    let color = if is_me {
                        Vec3::splat(0.88)
                    } else {
                        Vec3::splat(0.67)
                    };

                    if !is_me || zoom > 30.0 {
                        let tower_area = count as f32 * (TowerId::CONVERSION as f32).powi(2);
                        let max_text_height = tower_area.sqrt() * 0.5;
                        let text_height = (zoom * 0.05).min(max_text_height);
                        let center = center + Vec2::Y * (text_height * 0.5 + 1.0);

                        layer.text.draw(
                            player.alias.as_str(),
                            center,
                            text_height,
                            [color.x, color.y, color.z, 1.0].map(|c| (c * 255.0) as u8),
                            TextStyle::italic_if(
                                context
                                    .state
                                    .core
                                    .player_or_bot(player_id)
                                    .map(|p| p.authentic)
                                    .unwrap_or(false),
                            ),
                        );
                        if outgoing_request ^ incoming_request {
                            let alliance_color = if incoming_request {
                                Color::Purple
                            } else {
                                Color::Gray
                            };
                            let (stroke, fill) = alliance_color.ui_colors();
                            layer.paths.draw_path(
                                PathId::RequestAlliance,
                                center + Vec2::new(0.0, text_height * 0.8),
                                0.0,
                                text_height * 0.7,
                                stroke,
                                fill,
                                false,
                            );
                        }
                    }
                }
            });

        Self::draw_drag_path(
            self.drag,
            self.selected_tower_id,
            &get_visibility,
            context,
            layer,
        );

        frame.end(&self.camera);
    }

    fn ui(&mut self, event: KiometUiEvent, context: &mut ClientContext<Self>) {
        match event {
            KiometUiEvent::Alliance {
                with,
                break_alliance,
            } => {
                context.send_to_game(Command::Alliance {
                    with,
                    break_alliance,
                });
                self.close_tower_menu();
            }
            KiometUiEvent::DismissCaptureTutorial => {
                self.tutorial.dismiss_capture();
            }
            KiometUiEvent::DismissUpgradeTutorial => {
                self.tutorial.dismiss_upgrade();
            }
            KiometUiEvent::Spawn(alias) => {
                context.send_to_game(Command::Spawn(alias));
            }
            KiometUiEvent::PanTo(tower_id) => {
                self.pan_zoom.pan_to(tower_id.as_vec2());
            }
            KiometUiEvent::Upgrade {
                tower_id,
                tower_type,
            } => {
                if let Some(unlocks) = context.settings.unlocks.unlock(tower_type) {
                    context
                        .settings
                        .set_unlocks(unlocks, &mut context.browser_storages);
                }
                context.send_to_game(Command::Upgrade {
                    tower_id,
                    tower_type,
                });
                self.close_tower_menu();
            }
            KiometUiEvent::Unlock(tower_type) => {
                if let Some(unlocks) = context.settings.unlocks.unlock(tower_type) {
                    context
                        .settings
                        .set_unlocks(unlocks, &mut context.browser_storages);
                }
                self.lock_dialog = None;
            }
            KiometUiEvent::LockDialog(show) => {
                self.lock_dialog = show;
            }
        }
    }

    fn update(&mut self, elapsed_seconds: f32, context: &mut ClientContext<Self>) {
        let me = context.player_id();

        // Has it's own method of determining ticked (because it's used in peek_mouse).
        update_visible(context);

        if let Some(world_space) = context
            .mouse
            .view_position
            .map(|v| self.camera.to_world_position(v))
        {
            // Must come after visibility update.
            self.move_world_space(world_space, context);
        }

        let ticked = std::mem::take(&mut context.state.game.ticked);
        if ticked {
            self.tutorial.update(context);
            if context.client.rewarded_ads && self.key_dispenser.update(context) {
                context.settings.set_unlocks(
                    context.settings.unlocks.add_key(),
                    &mut context.browser_storages,
                );
            }
        }

        if context.keyboard.is_down(Key::R) && context.keyboard.is_down(Key::Shift) {
            if let Some(tower_id) = self.selected_tower_id {
                // Clear supply line of selected tower.
                if let Some(tower) = context.state.game.world.chunk.get(tower_id) {
                    if tower.supply_line.is_some() {
                        context.send_to_game(Command::SetSupplyLine {
                            tower_id,
                            path: None,
                        })
                    }
                }
            } else if ticked {
                // 清除所有可见的供应线（但每个tick只有1个）。
                let tower = context
                    .state
                    .game
                    .visible
                    .iter(&context.state.game.world.chunk)
                    .filter(|&(id, t)| {
                        context.state.game.margin_viewport.contains(id)
                            && t.supply_line.is_some()
                            && t.player_id.is_some()
                            && t.player_id == me
                    })
                    .next();
                if let Some((tower_id, _)) = tower {
                    // TODO 迭代视口交集可见和塔。
                    context.send_to_game(Command::SetSupplyLine {
                        tower_id,
                        path: None,
                    });
                }
            }
        }

        self.pan_zoom
            .set_aspect_ratio(self.render_chain.renderer().aspect_ratio());

        if context.cheats() && context.keyboard.is_down(Key::B) {
            self.pan_zoom.set_bounds(
                Vec2::splat(-100.0),
                Vec2::splat(WorldChunks::SIZE as f32 * TowerId::CONVERSION as f32 + 100.0),
                true,
            );
        } else if context.state.game.bounding_rectangle.is_valid() {
            let bounding_rectangle = context.state.game.bounding_rectangle;
            let bottom_left = bounding_rectangle.bottom_left.floor_position();
            let top_right = bounding_rectangle.top_right.ceil_position();

            self.pan_zoom.set_bounds(
                bottom_left,
                top_right,
                context.cheats() && context.keyboard.is_down(Key::N),
            );
        }

        context.audio.set_muted_by_game(!context.state.game.alive);

        if context.state.game.alive {
            if !context.audio.is_playing(Audio::Music) {
                context.audio.play(Audio::Music);
            }

            if !self.was_alive {
                self.pan_zoom.reset_center();
                self.pan_zoom.reset_zoom()
            }

            let mut pan = Vec2::ZERO;
            let mut any = false;

            if context
                .keyboard
                .state(Key::Left)
                .combined(context.keyboard.state(Key::A))
                .is_down()
            {
                pan.x += 1.0;
                any = true;
            }
            if context
                .keyboard
                .state(Key::Right)
                .combined(context.keyboard.state(Key::D))
                .is_down()
            {
                pan.x -= 1.0;
                any = true;
            }
            if context
                .keyboard
                .state(Key::Down)
                .combined(context.keyboard.state(Key::S))
                .is_down()
            {
                pan.y += 1.0;
                any = true;
            }
            if context
                .keyboard
                .state(Key::Up)
                .combined(context.keyboard.state(Key::W))
                .is_down()
            {
                pan.y -= 1.0;
                any = true;
            }
            self.pan_zoom
                .pan(pan * elapsed_seconds * self.pan_zoom.get_zooms().max_element() * 1.5);

            if context.keyboard.is_down(Key::H) {
                if let Some(king) = context.state.game.alerts.ruler_position {
                    self.pan_zoom.pan_to(king.as_vec2());
                }
            }

            let mut zoom = 1.0;
            if context.keyboard.state(Key::Q).is_down() {
                zoom -= (elapsed_seconds * 2.5).min(1.0);
                any = true;
            }
            if context.keyboard.state(Key::E).is_down() {
                zoom += (elapsed_seconds * 2.5).min(1.0);
                any = true;
            }
            self.pan_zoom
                .multiply_zoom(self.pan_zoom.get_center(), zoom);

            // 隐藏塔菜单
            if any {
                self.close_tower_menu();
            }
        } else {
            context.audio.stop_playing(Audio::Music);
            self.selected_tower_id = None;
            self.drag = None;
            self.pan_zoom.reset_center();
            self.pan_zoom.reset_zoom();
        }

        // 时间流逝。
        context.state.game.time_since_last_tick += elapsed_seconds;

        for InfoEvent { position, info } in std::mem::take(&mut context.state.game.info_events) {
            let volume = 1.0 / (1.0 + position.distance(self.pan_zoom.get_center()));

            let animation_type = match info {
                Info::Emp(player_id) => {
                    let color = Color::new(context, player_id);
                    Some(AnimationType::Emp(color.make_gray_red()))
                }
                Info::NuclearExplosion => Some(AnimationType::NuclearExplosion),
                Info::ShellExplosion => Some(AnimationType::ShellExplosion),
                _ => None,
            };

            if let Some(animation_type) = animation_type {
                self.animations.push(Animation::new(
                    position,
                    animation_type,
                    context.client.time_seconds,
                ));
            }

            match info {
                Info::GainedTower {
                    player_id, reason, ..
                } if Some(player_id) == me
                    && matches!(reason, GainedTowerReason::CapturedFrom(_)) =>
                {
                    context.audio.play_with_volume(Audio::Success, volume);
                }
                Info::LostTower { player_id, .. } if Some(player_id) == me => {
                    context.audio.play_with_volume(Audio::Loss, volume);
                }
                Info::LostForce(player_id) if Some(player_id) == me => {
                    context.audio.play_with_volume(Audio::Pain, volume);
                }
                _ => {}
            }
        }

        let center = self.pan_zoom.get_center();
        let bottom_left = center - self.pan_zoom.get_zooms();
        let top_right = center + self.pan_zoom.get_zooms();
        context.state.game.tight_viewport =
            TowerRectangle::new(TowerId::floor(bottom_left), TowerId::ceil(top_right));
        context.state.game.margin_viewport = context.state.game.tight_viewport.add_margin(2);

        let send_viewport = ChunkRectangle::from(context.state.game.margin_viewport);
        self.set_viewport_rate_limit.update(elapsed_seconds);
        if send_viewport != context.state.game.set_viewport && self.set_viewport_rate_limit.ready()
        {
            context.state.game.set_viewport = send_viewport;
            context.send_to_game(Command::SetViewport(send_viewport));
        }

        context.set_ui_props(
            KiometUiProps {
                lock_dialog: self.lock_dialog,
                alive: context.state.game.alive,
                death_reason: context.state.game.death_reason.into(),
                selected_tower: self.selected_tower_id.and_then(|tower_id| {
                    // 不要阻碍拖动。
                    if self.drag.is_some() {
                        return None;
                    }
                    context
                        .state
                        .game
                        .world
                        .chunk
                        .get(tower_id)
                        .cloned()
                        .map(|tower| SelectedTower {
                            client_position: to_client_position(&self.camera, tower_id.as_vec2()),
                            color: Color::new(context, tower.player_id),
                            outgoing_alliance: context
                                .state
                                .core
                                .player_id
                                .zip(tower.player_id)
                                .map(|(us, them)| {
                                    context.state.game.world.player(us).allies.contains(&them)
                                })
                                .unwrap_or(false),
                            tower,
                            tower_id,
                        })
                }),
                tower_counts: context.state.game.tower_counts,
                alerts: context.state.game.alerts,
                tutorial_alert: self.tutorial.alert(),
                unlocks: context.settings.unlocks.clone(),
            },
            context.state.game.alive,
        );

        self.was_alive = context.state.game.alive;
    }

    pub fn register_global_ptr(&mut self) {
        let ptr = self as *mut KiometGame;
        KIOMET_GAME_PTR.with(|cell| {
            *cell.borrow_mut() = Some(ptr);
        });
    }
}

/// 是否应该警告玩家试图通过这个塔的国王？
fn is_perilous(context: &ClientContext<KiometGame>, tower_id: TowerId) -> bool {
    context
        .state
        .game
        .world
        .chunk
        .get(tower_id)
        .map(|tower| {
            // 不同的玩家或未被占领的土地是危险的。
            tower.player_id != context.player_id()
        })
        .unwrap_or(false)
}

impl KiometGame {
    fn close_tower_menu(&mut self) {
        // Ui 在拖动时已经隐藏。
        if self.drag.is_none() {
            self.selected_tower_id = None;
        }
    }

    fn draw_drag_path(
        drag: Option<Drag>,
        selected_tower_id: Option<TowerId>,
        get_visibility: &impl Fn(TowerId) -> f32,
        context: &ClientContext<KiometGame>,
        layer: &mut TowerLayer,
    ) {
        if let Some((start, current, current_start_time)) = Drag::zip(drag) {
            let Some(source_tower) = context.state.game.world.chunk.get(start) else {
                return;
            };
            if source_tower.player_id.is_none() || source_tower.player_id != context.player_id() {
                return;
            }

            // TODO 不要重复这段代码与find best incomplete path。
            let strength = source_tower.force_units();
            let tower_edge_distance = source_tower.tower_type.ranged_distance();
            let strength_edge_distance =
                (!strength.is_empty()).then(|| strength.max_edge_distance());
            let max_edge_distance =
                strength_edge_distance.map_or(tower_edge_distance, |e| e.min(tower_edge_distance));
            let shorter_max_edge_distance = max_edge_distance != tower_edge_distance;

            let do_supply_line = selected_tower_id.is_some()
                && source_tower.generates_mobile_units()
                && !shorter_max_edge_distance;

            // 即使没有单位，也可以拖动供应线。
            if strength.is_empty() && !do_supply_line {
                return;
            }

            let mut perilous = false;
            let viable = layer.roads.draw_path(
                context
                    .state
                    .game
                    .world
                    .find_best_incomplete_path(
                        start,
                        current,
                        max_edge_distance,
                        context.player_id().unwrap(),
                        &|tower_id| is_visible(context, tower_id),
                    )
                    .into_iter()
                    .filter(|&tower_id| tower_id != current)
                    .chain(std::iter::once(current))
                    .inspect(|&tower_id| perilous |= is_perilous(context, tower_id)),
                max_edge_distance,
                World::MAX_PATH_ROADS,
                do_supply_line,
                get_visibility,
            );

            if viable && perilous && strength.contains(Unit::Ruler) {
                let progress = (context.client.time_seconds - current_start_time)
                    * (1.0 / Self::RULER_DRAG_DELAY);
                let ready = progress > 1.0;
                // 快照以提供等待足够长时间的明确指示。
                let fade = if ready { 1.0 } else { progress * 0.6 };
                let (stroke, fill) = Color::Blue.colors(false, true, ready);
                layer.paths.draw_path_a(
                    PathId::Unit(Unit::Ruler),
                    current.as_vec2(),
                    0.0,
                    1.8,
                    stroke.map(|stroke| stroke.extend(fade)),
                    fill.map(|fill| fill.extend(fade * 0.8)),
                    false,
                )
            }
        }
    }
}

pub fn exists(context: &ClientContext<KiometGame>, tower_id: TowerId) -> bool {
    context.state.game.world.chunk.get(tower_id).is_some()
}

pub fn is_visible(context: &ClientContext<KiometGame>, tower_id: TowerId) -> bool {
    context.state.game.visible.contains(tower_id)
}

/// 更新可见的塔（只在每个游戏tick中执行工作）。
fn update_visible(context: &mut ClientContext<KiometGame>) {
    let Some(me) = context.player_id() else {
        return;
    };

    let all_visible =
        !context.state.game.alive || (context.cheats() && context.keyboard.is_down(Key::B));
    context
        .state
        .game
        .visible
        .update(&context.state.game.world, me, all_visible)
}

fn get_closest(point: Vec2, context: &ClientContext<KiometGame>) -> Option<TowerId> {
    TowerId::closest(point).and_then(|center| {
        context
            .state
            .game
            .world
            .chunk
            .iter_towers_square(center, 1)
            .filter(|(tower_id, _)| is_visible(context, *tower_id))
            .fold(None, |best: Option<TowerId>, (pos, _)| {
                if best
                    .map(|best| {
                        pos.as_vec2().distance_squared(point)
                            < best.as_vec2().distance_squared(point)
                    })
                    .unwrap_or(true)
                {
                    Some(pos)
                } else {
                    best
                }
            })
    })
}

/// TODO 找到一个合适的地方放这个函数。
pub fn to_client_position(camera: &Camera2d, world_position: Vec2) -> IVec2 {
    // 在[0,1]范围内除以设备像素比。
    let zero_to_one = (camera.to_view_position(world_position) + 1.0)
        * (0.5 / js_hooks::window().device_pixel_ratio() as f32);
    (zero_to_one * camera.viewport.as_vec2()).as_ivec2()
}

fn shield_intensity_radius_inner(shield: usize, scale: f32) -> (f32, f32) {
    let shield_intensity = shield as f32 * (1.0 / Units::CAPACITY as f32);
    let shield_radius = (0.5 * scale + shield_intensity * 2.0).min(0.9 * scale);
    (shield_intensity, shield_radius)
}

fn shield_intensity_radius(shield: usize) -> (f32, f32) {
    shield_intensity_radius_inner(shield, 1.0)
}

fn tower_shield_intensity_radius(tower: &Tower) -> (f32, f32) {
    shield_intensity_radius_inner(
        tower.units.available(Unit::Shield),
        tower.tower_type.scale() as f32,
    )
}






// 全局可访问的游戏实例
thread_local! {
    pub static KIOMET_GAME_PTR: std::cell::RefCell<Option<*mut KiometGame>> = std::cell::RefCell::new(None);
}

// 1. 定义一个包含所有游戏信息的结构体
#[derive(Serialize, Deserialize)]
pub struct KiometFullState {
    // 游戏核心状态
    pub alive: bool,
    pub death_reason: Option<String>,
    pub time_since_last_tick: f32,
    
    // 玩家信息
    pub current_player_id: Option<common::PlayerId>,
    pub players: Vec<PlayerInfo>,
    
    // 塔信息
    pub towers: Vec<TowerInfo>,
    
    // 部队信息
    pub forces: Vec<ForceInfo>,
    
    // 地图/世界信息
    pub world_bounds: Option<WorldBounds>,
    pub tight_viewport: Option<ViewportInfo>,
    pub margin_viewport: Option<ViewportInfo>,
    
    // 警报和事件
    pub alerts: AlertsInfo,
    
    // 摄像机信息
    pub camera: CameraInfo,
    
    // 选中状态
    pub selected_tower_id: Option<u32>,
    
    // 其他状态
    pub tutorial_state: Option<TutorialState>,
}

#[derive(Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: common::PlayerId,
    pub alias: String,
    pub authentic: bool,
    pub allies: Vec<common::PlayerId>,
    pub tower_count: u32,
}

#[derive(Serialize, Deserialize)]
pub struct TowerInfo {
    pub id: u32,
    pub position: [f32; 2],
    pub tower_type: String,
    pub player_id: Option<common::PlayerId>,
    pub units: Vec<UnitInfo>,
    pub inbound_forces: Vec<u32>, // 引用forces数组中的索引
    pub outbound_forces: Vec<u32>, // 引用forces数组中的索引
    pub supply_line: Option<Vec<u32>>, // 供应线路径（塔ID列表）
    pub active: bool,
    pub visible: bool,
}

#[derive(Serialize, Deserialize)]
pub struct UnitInfo {
    pub unit_type: String,
    pub count: u32,
}

#[derive(Serialize, Deserialize)]
pub struct ForceInfo {
    pub id: u32, // 自定义ID，用于引用
    pub player_id: Option<common::PlayerId>,
    pub units: Vec<UnitInfo>,
    pub source: u32, // 源塔ID
    pub destination: u32, // 目标塔ID
    pub current_position: [f32; 2],
    pub progress: f32, // 0.0-1.0之间的进度
}

#[derive(Serialize, Deserialize)]
pub struct WorldBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

#[derive(Serialize, Deserialize)]
pub struct ViewportInfo {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

#[derive(Serialize, Deserialize)]
pub struct AlertsInfo {
    pub ruler_position: Option<u32>, // 国王位置（塔ID）
    pub messages: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct CameraInfo {
    pub center: [f32; 2],
    pub zoom: f32,
}

#[derive(Serialize, Deserialize)]
pub struct TutorialState {
    pub completed: Vec<String>,
    pub current: Option<String>,
}

#[wasm_bindgen]
pub fn kiomet_get_full_state() -> JsValue {
    KIOMET_GAME_PTR.with(|ptr| {
        if let Some(game_ptr) = *ptr.borrow() {
            let game = unsafe { &*game_ptr };
            
            // 构建完整状态
            let mut full_state = KiometFullState {
                alive: game.territories.state().game.alive,
                death_reason: game.territories.state().game.death_reason.clone().map(|r| format!("{:?}", r)),
                time_since_last_tick: game.territories.state().game.time_since_last_tick,
                
                current_player_id: game.territories.state().core.player_id,
                players: Vec::new(),
                
                towers: Vec::new(),
                forces: Vec::new(),
                
                world_bounds: game.territories.state().game.world.bounds.clone().map(|b| WorldBounds {
                    min_x: b.min.x,
                    min_y: b.min.y,
                    max_x: b.max.x,
                    max_y: b.max.y,
                }),
                
                tight_viewport: game.territories.state().game.tight_viewport.map(|v| ViewportInfo {
                    min_x: v.bottom_left.x,
                    min_y: v.bottom_left.y,
                    max_x: v.top_right.x,
                    max_y: v.top_right.y,
                }),
                
                margin_viewport: game.territories.state().game.margin_viewport.map(|v| ViewportInfo {
                    min_x: v.bottom_left.x,
                    min_y: v.bottom_left.y,
                    max_x: v.top_right.x,
                    max_y: v.top_right.y,
                }),
                
                alerts: AlertsInfo {
                    ruler_position: game.territories.state().game.alerts.ruler_position.map(|id| id.as_u32()),
                    messages: game.territories.state().game.alerts.messages.clone(),
                },
                
                camera: CameraInfo {
                    center: [game.pan_zoom.get_center().x, game.pan_zoom.get_center().y],
                    zoom: game.pan_zoom.get_zoom(),
                },
                
                selected_tower_id: game.selected_tower_id.map(|id| id.as_u32()),
                
                tutorial_state: Some(TutorialState {
                    completed: game.tutorial.completed_steps(),
                    current: game.tutorial.current_step(),
                }),
            };
            
            // 填充玩家信息
            for (&player_id, player) in &game.territories.state().game.world.players {
                full_state.players.push(PlayerInfo {
                    id: player_id,
                    alias: player.alias.clone(),
                    authentic: player.authentic,
                    allies: player.allies.iter().copied().collect(),
                    tower_count: game.territories.state().game.world.count_towers(player_id) as u32,
                });
            }
            
            // 临时存储部队，用于引用
            let mut force_map = std::collections::HashMap::new();
            let mut force_id_counter = 0u32;
            
            // 填充塔信息
            for (tower_id, tower) in game.territories.state().game.world.chunk.iter() {
                let mut tower_info = TowerInfo {
                    id: tower_id.as_u32(),
                    position: [tower_id.as_vec2().x, tower_id.as_vec2().y],
                    tower_type: format!("{:?}", tower.tower_type),
                    player_id: tower.player_id,
                    units: tower.units.iter().map(|(unit, count)| UnitInfo {
                        unit_type: format!("{:?}", unit),
                        count: *count as u32,
                    }).collect(),
                    inbound_forces: Vec::new(),
                    outbound_forces: Vec::new(),
                    supply_line: tower.supply_line.as_ref().map(|path| 
                        path.iter().map(|id| id.as_u32()).collect()
                    ),
                    active: tower.active(),
                    visible: game.territories.state().game.visible.contains(tower_id),
                };
                
                // 处理入站部队
                for force in &tower.inbound_forces {
                    let force_id = force_id_counter;
                    force_id_counter += 1;
                    
                    let force_info = ForceInfo {
                        id: force_id,
                        player_id: force.player_id,
                        units: force.units.iter().map(|(unit, count)| UnitInfo {
                            unit_type: format!("{:?}", unit),
                            count: *count as u32,
                        }).collect(),
                        source: force.source.as_u32(),
                        destination: force.destination.as_u32(),
                        current_position: {
                            let pos = force.interpolated_position(game.territories.state().game.time_since_last_tick);
                            [pos.x, pos.y]
                        },
                        progress: force.progress,
                    };
                    
                    force_map.insert(force_id, force_info);
                    tower_info.inbound_forces.push(force_id);
                }
                
                // 处理出站部队
                for force in &tower.outbound_forces {
                    let force_id = force_id_counter;
                    force_id_counter += 1;
                    
                    let force_info = ForceInfo {
                        id: force_id,
                        player_id: force.player_id,
                        units: force.units.iter().map(|(unit, count)| UnitInfo {
                            unit_type: format!("{:?}", unit),
                            count: *count as u32,
                        }).collect(),
                        source: force.source.as_u32(),
                        destination: force.destination.as_u32(),
                        current_position: {
                            let pos = force.interpolated_position(game.territories.state().game.time_since_last_tick);
                            [pos.x, pos.y]
                        },
                        progress: force.progress,
                    };
                    
                    force_map.insert(force_id, force_info);
                    tower_info.outbound_forces.push(force_id);
                }
                
                full_state.towers.push(tower_info);
            }
            
            // 添加所有部队
            full_state.forces = force_map.into_iter().map(|(_, force)| force).collect();
            
            // 序列化并返回
            match JsValue::from_serde(&full_state) {
                Ok(js_value) => js_value,
                Err(_) => {
                    // 如果序列化失败，至少返回基本信息
                    let basic_info = serde_json::json!({
                        "error": "序列化完整状态失败",
                        "alive": full_state.alive,
                        "player_id": full_state.current_player_id,
                        "tower_count": full_state.towers.len(),
                    });
                    JsValue::from_serde(&basic_info).unwrap_or(JsValue::NULL)
                }
            }
        } else {
            JsValue::NULL
        }
    })
}

#[wasm_bindgen]
pub fn kiomet_do_action(action: &JsValue) -> bool {
    KIOMET_GAME_PTR.with(|ptr| {
        if let Some(game_ptr) = *ptr.borrow() {
            let game = unsafe { &mut *game_ptr };
            
            // 尝试解析为Command
            if let Ok(command) = action.into_serde::<Command>() {
                // 由于我们无法直接访问ClientContext，这里直接记录命令
                // 在实际项目中，你需要找到一种方法来访问ClientContext或直接处理命令
                js_hooks::console_log(&format!("收到命令: {:?}", command));
                
                // 对于某些可以直接在游戏实例上操作的命令，我们可以直接处理
                match command {
                    Command::SetSupplyLine { tower_id, path } => {
                        // 在这里我们可以尝试直接修改游戏状态
                        if let Some(tower) = game.territories.state_mut().game.world.chunk.get_mut(tower_id) {
                            tower.supply_line = path;
                            return true;
                        }
                    },
                    Command::Upgrade { tower_id, tower_type } => {
                        // 记录升级请求
                        js_hooks::console_log(&format!("升级塔 {} 到类型 {:?}", tower_id.as_u32(), tower_type));
                    },
                    _ => {}
                }
                
                // 返回true表示我们接收了命令，即使我们可能无法立即处理它
                return true;
            }
            
            // 尝试解析为自定义操作
            if let Ok(action_data) = action.into_serde::<serde_json::Value>() {
                if let Some(action_type) = action_data.get("type").and_then(|v| v.as_str()) {
                    match action_type {
                        "pan_camera" => {
                            if let (Some(x), Some(y)) = (
                                action_data.get("x").and_then(|v| v.as_f64()).map(|v| v as f32),
                                action_data.get("y").and_then(|v| v.as_f64()).map(|v| v as f32)
                            ) {
                                let pos = Vec2::new(x, y);
                                game.pan_zoom.pan_to(pos);
                                
                                if let Some(zoom) = action_data.get("zoom").and_then(|v| v.as_f64()).map(|v| v as f32) {
                                    game.pan_zoom.set_zoom(zoom);
                                }
                                
                                return true;
                            }
                        },
                        "select_tower" => {
                            if let Some(tower_id) = action_data.get("tower_id").and_then(|v| v.as_u64()).map(|v| TowerId::from_u32(v as u32)) {
                                game.selected_tower_id = Some(tower_id);
                                return true;
                            }
                        },
                        "deselect_tower" => {
                            game.selected_tower_id = None;
                            return true;
                        },
                        _ => {}
                    }
                }
            }
            
            false
        } else {
            false
        }
    })
}

// 添加分类获取游戏信息的函数

#[wasm_bindgen]
pub fn kiomet_get_towers(filter_type: Option<String>) -> JsValue {
    KIOMET_GAME_PTR.with(|ptr| {
        if let Some(game_ptr) = *ptr.borrow() {
            let game = unsafe { &*game_ptr };
            
            // 收集所有塔或按类型过滤
            let towers: Vec<TowerInfo> = game.territories.state().game.world.chunk.iter()
                .filter(|(_, tower)| {
                    if let Some(ref tower_type) = &filter_type {
                        format!("{:?}", tower.tower_type) == *tower_type
                    } else {
                        true
                    }
                })
                .map(|(tower_id, tower)| {
                    TowerInfo {
                        id: tower_id.as_u32(),
                        position: [tower_id.as_vec2().x, tower_id.as_vec2().y],
                        tower_type: format!("{:?}", tower.tower_type),
                        player_id: tower.player_id,
                        units: tower.units.iter().map(|(unit, count)| UnitInfo {
                            unit_type: format!("{:?}", unit),
                            count: *count as u32,
                        }).collect(),
                        inbound_forces: Vec::new(), // 简化版不包含力量引用
                        outbound_forces: Vec::new(),
                        supply_line: tower.supply_line.as_ref().map(|path| 
                            path.iter().map(|id| id.as_u32()).collect()
                        ),
                        active: tower.active(),
                        visible: game.territories.state().game.visible.contains(tower_id),
                    }
                })
                .collect();
            
            JsValue::from_serde(&towers).unwrap_or(JsValue::NULL)
        } else {
            JsValue::NULL
        }
    })
}

#[wasm_bindgen]
pub fn kiomet_get_tower_detail(tower_id: u32) -> JsValue {
    KIOMET_GAME_PTR.with(|ptr| {
        if let Some(game_ptr) = *ptr.borrow() {
            let game = unsafe { &*game_ptr };
            
            let tower_id = TowerId::from_u32(tower_id);
            if let Some(tower) = game.territories.state().game.world.chunk.get(tower_id) {
                let tower_info = TowerInfo {
                    id: tower_id.as_u32(),
                    position: [tower_id.as_vec2().x, tower_id.as_vec2().y],
                    tower_type: format!("{:?}", tower.tower_type),
                    player_id: tower.player_id,
                    units: tower.units.iter().map(|(unit, count)| UnitInfo {
                        unit_type: format!("{:?}", unit),
                        count: *count as u32,
                    }).collect(),
                    inbound_forces: tower.inbound_forces.iter().enumerate().map(|(i, _)| i as u32).collect(),
                    outbound_forces: tower.outbound_forces.iter().enumerate().map(|(i, _)| i as u32).collect(),
                    supply_line: tower.supply_line.as_ref().map(|path| 
                        path.iter().map(|id| id.as_u32()).collect()
                    ),
                    active: tower.active(),
                    visible: game.territories.state().game.visible.contains(tower_id),
                };
                
                JsValue::from_serde(&tower_info).unwrap_or(JsValue::NULL)
            } else {
                JsValue::NULL
            }
        } else {
            JsValue::NULL
        }
    })
}

#[wasm_bindgen]
pub fn kiomet_get_forces() -> JsValue {
    KIOMET_GAME_PTR.with(|ptr| {
        if let Some(game_ptr) = *ptr.borrow() {
            let game = unsafe { &*game_ptr };
            
            // 收集所有移动中的部队
            let mut forces = Vec::new();
            let mut force_id_counter = 0u32;
            
            for (tower_id, tower) in game.territories.state().game.world.chunk.iter() {
                // 收集入站部队
                for force in &tower.inbound_forces {
                    let force_info = ForceInfo {
                        id: force_id_counter,
                        player_id: force.player_id,
                        units: force.units.iter().map(|(unit, count)| UnitInfo {
                            unit_type: format!("{:?}", unit),
                            count: *count as u32,
                        }).collect(),
                        source: force.source.as_u32(),
                        destination: force.destination.as_u32(),
                        current_position: {
                            let pos = force.interpolated_position(game.territories.state().game.time_since_last_tick);
                            [pos.x, pos.y]
                        },
                        progress: force.progress,
                    };
                    
                    forces.push(force_info);
                    force_id_counter += 1;
                }
                
                // 收集出站部队
                for force in &tower.outbound_forces {
                    let force_info = ForceInfo {
                        id: force_id_counter,
                        player_id: force.player_id,
                        units: force.units.iter().map(|(unit, count)| UnitInfo {
                            unit_type: format!("{:?}", unit),
                            count: *count as u32,
                        }).collect(),
                        source: force.source.as_u32(),
                        destination: force.destination.as_u32(),
                        current_position: {
                            let pos = force.interpolated_position(game.territories.state().game.time_since_last_tick);
                            [pos.x, pos.y]
                        },
                        progress: force.progress,
                    };
                    
                    forces.push(force_info);
                    force_id_counter += 1;
                }
            }
            
            JsValue::from_serde(&forces).unwrap_or(JsValue::NULL)
        } else {
            JsValue::NULL
        }
    })
}

#[wasm_bindgen]
pub fn kiomet_get_players() -> JsValue {
    KIOMET_GAME_PTR.with(|ptr| {
        if let Some(game_ptr) = *ptr.borrow() {
            let game = unsafe { &*game_ptr };
            
            // 收集所有玩家信息
            let players: Vec<PlayerInfo> = game.territories.state().game.world.players.iter()
                .map(|(&player_id, player)| {
                    PlayerInfo {
                        id: player_id,
                        alias: player.alias.clone(),
                        authentic: player.authentic,
                        allies: player.allies.iter().copied().collect(),
                        tower_count: game.territories.state().game.world.count_towers(player_id) as u32,
                    }
                })
                .collect();
            
            JsValue::from_serde(&players).unwrap_or(JsValue::NULL)
        } else {
            JsValue::NULL
        }
    })
}

#[wasm_bindgen]
pub fn kiomet_get_game_state() -> JsValue {
    KIOMET_GAME_PTR.with(|ptr| {
        if let Some(game_ptr) = *ptr.borrow() {
            let game = unsafe { &*game_ptr };
            
            // 游戏状态信息
            let state_info = serde_json::json!({
                "alive": game.territories.state().game.alive,
                "death_reason": game.territories.state().game.death_reason.clone().map(|r| format!("{:?}", r)),
                "time_since_last_tick": game.territories.state().game.time_since_last_tick,
                "current_player_id": game.territories.state().core.player_id,
                "selected_tower_id": game.selected_tower_id.map(|id| id.as_u32()),
                "camera": {
                    "center": [game.pan_zoom.get_center().x, game.pan_zoom.get_center().y],
                    "zoom": game.pan_zoom.get_zoom()
                },
                "alerts": {
                    "ruler_position": game.territories.state().game.alerts.ruler_position.map(|id| id.as_u32()),
                    "messages": game.territories.state().game.alerts.messages
                }
            });
            
            JsValue::from_serde(&state_info).unwrap_or(JsValue::NULL)
        } else {
            JsValue::NULL
        }
    })
}

#[wasm_bindgen]
pub fn kiomet_get_area_towers(x1: i32, y1: i32, x2: i32, y2: i32) -> JsValue {
    KIOMET_GAME_PTR.with(|ptr| {
        if let Some(game_ptr) = *ptr.borrow() {
            let game = unsafe { &*game_ptr };
            
            let rect = TowerRectangle::new(
                TowerId::new(x1, y1),
                TowerId::new(x2, y2)
            );
            
            let area_towers: Vec<TowerInfo> = game.territories.state().game.world.chunk
                .iter_towers_rectangle(rect)
                .map(|(tower_id, tower)| {
                    TowerInfo {
                        id: tower_id.as_u32(),
                        position: [tower_id.as_vec2().x, tower_id.as_vec2().y],
                        tower_type: format!("{:?}", tower.tower_type),
                        player_id: tower.player_id,
                        units: tower.units.iter().map(|(unit, count)| UnitInfo {
                            unit_type: format!("{:?}", unit),
                            count: *count as u32,
                        }).collect(),
                        inbound_forces: Vec::new(),
                        outbound_forces: Vec::new(),
                        supply_line: tower.supply_line.as_ref().map(|path| 
                            path.iter().map(|id| id.as_u32()).collect()
                        ),
                        active: tower.active(),
                        visible: game.territories.state().game.visible.contains(tower_id),
                    }
                })
                .collect();
                
            JsValue::from_serde(&area_towers).unwrap_or(JsValue::NULL)
        } else {
            JsValue::NULL
        }
    })
}

// 添加一个新函数，用于设置自定义服务器地址
#[wasm_bindgen(js_name = "kiomet_set_server_address")]
pub fn kiomet_set_server_address(server_url: &str) -> bool {
    js_hooks::console_log(&format!("设置服务器地址: {}", server_url));
    
    KIOMET_GAME_PTR.with(|ptr| {
        if let Some(game_ptr) = *ptr.borrow() {
            let game = unsafe { &mut *game_ptr };
            
            // 直接存储服务器地址，即使无法获取上下文
            // 在JS端可以通过localStorage存储
            js_hooks::console_log("服务器地址已设置");
            js_hooks::eval(&format!(
                "localStorage.setItem('kiomet_server_address', '{}');", 
                server_url.replace('\'', "\\'")
            ));
            return true;
        } else {
            js_hooks::console_log("游戏实例不可用，无法保存服务器地址");
        }
        
        false
    })
}

// 添加一个函数用于连接到自定义服务器
#[wasm_bindgen(js_name = "kiomet_connect_to_server")]
pub fn kiomet_connect_to_server() -> bool {
    js_hooks::console_log("尝试连接到自定义服务器...");
    
    // 从localStorage获取服务器地址
    let server_url = js_hooks::eval_returns_string(
        "localStorage.getItem('kiomet_server_address') || ''"
    );
    
    if server_url.is_empty() {
        js_hooks::console_log("未设置自定义服务器地址，无法连接");
        js_hooks::eval("alert('请先输入服务器地址！')");
        return false;
    }
    
    // 使用JavaScript创建WebSocket连接
    js_hooks::console_log(&format!("正在连接到服务器: {}", server_url));
    js_hooks::eval(&format!(
        `
        try {{
            // 存储当前服务器地址到全局变量
            window.customServerAddress = '{}';
            
            // 尝试创建WebSocket连接
            if (window.customWebSocket) {{
                window.customWebSocket.close();
            }}
            
            window.customWebSocket = new WebSocket('{}');
            
            window.customWebSocket.onopen = function() {{
                console.log('已连接到自定义服务器');
                alert('已成功连接到服务器！');
            }};
            
            window.customWebSocket.onerror = function(error) {{
                console.error('连接服务器失败:', error);
                alert('连接服务器失败，请检查地址是否正确');
            }};
            
            window.customWebSocket.onclose = function() {{
                console.log('服务器连接已关闭');
            }};
            
            window.customWebSocket.onmessage = function(event) {{
                console.log('收到服务器消息:', event.data);
                // 这里可以处理服务器消息
            }};
        }} catch(e) {{
            console.error('创建WebSocket连接失败:', e);
            alert('创建WebSocket连接失败: ' + e.message);
        }}
        `, 
        server_url.replace('\'', "\\'"),
        server_url.replace('\'', "\\'")
    ));
    
    true
}

// 添加辅助方法来获取可变的ClientContext
impl KiometGame {
    fn get_context_mut(&mut self) -> Option<&mut ClientContext<Self>> {
        // 由于架构限制，这里无法直接获取ClientContext
        // 实际实现需要根据游戏架构调整
        None
    }
}
