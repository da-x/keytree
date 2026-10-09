use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use calloop::timer::{TimeoutAction, Timer};
use calloop::{EventLoop, LoopHandle};
use calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputInfo, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keymap as SctkKeymap, Modifiers as SctkModifiers, RawModifiers,
};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_registry, dispatch2::Dispatch2, registry_handlers};
use wayland_client::backend::{ObjectData, ObjectId};
use wayland_client::globals::{registry_queue_init, BindError, GlobalList};
use wayland_client::protocol::{wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1, ext_workspace_handle_v1, ext_workspace_manager_v1,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1, zwlr_foreign_toplevel_manager_v1,
};
use xkbcommon::xkb::{self, Keysym};

use crate::cmdline::parse_position;
use crate::combination::{Combination, Modifiers};
use crate::draw::{self, Panel};
use crate::error::Error;
use crate::keysym;
use crate::session::{self, Session, Step};

pub(crate) fn run(session: Session, initial: Step) -> Result<(), Error> {
    match initial {
        Step::Close | Step::Ignore => return Ok(()),
        Step::Show(_) | Step::Error(_) => {}
    }

    let conn = Connection::connect_to_env().map_err(|err| Error::Wayland(err.to_string()))?;
    let (globals, mut queue) =
        registry_queue_init::<App>(&conn).map_err(|err| Error::Wayland(err.to_string()))?;
    let qh = queue.handle();

    let mut event_loop: EventLoop<'static, App> =
        EventLoop::try_new().map_err(|err| Error::Wayland(err.to_string()))?;
    let mut app = App::new(&globals, &qh, event_loop.handle(), session)?;

    // Outputs, xdg logical geometry, and the workspace snapshot each need a round trip.
    for _ in 0..3 {
        queue
            .roundtrip(&mut app)
            .map_err(|err| Error::Wayland(err.to_string()))?;
    }

    app.present(&qh, initial)?;
    if app.exit {
        return Ok(());
    }

    WaylandSource::new(conn, queue)
        .insert(event_loop.handle())
        .map_err(|err| Error::Wayland(err.error.to_string()))?;

    while !app.exit {
        event_loop
            .dispatch(None, &mut app)
            .map_err(|err| Error::Wayland(err.to_string()))?;
    }
    Ok(())
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    pool: SlotPool,
    loop_handle: LoopHandle<'static, App>,
    session: Session,

    exit: bool,
    closing: bool,
    error_mode: bool,
    configured: bool,
    needs_draw: bool,
    keyboard_focus: bool,

    keyboard: Option<wl_keyboard::WlKeyboard>,
    xkb_context: xkb::Context,
    xkb_keymap: Option<xkb::Keymap>,
    xkb_state: Option<xkb::State>,
    raw_mods: RawModifiers,
    layout: u32,

    layer: Option<LayerSurface>,
    output_id: Option<ObjectId>,
    panel: Option<Panel>,

    workspace_manager: Option<ext_workspace_manager_v1::ExtWorkspaceManagerV1>,
    toplevel_manager: Option<zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1>,
    groups: HashMap<ObjectId, Group>,
    workspaces: HashMap<ObjectId, WorkspaceInfo>,
    toplevels: HashMap<ObjectId, ToplevelInfo>,
}

#[derive(Default)]
struct Group {
    outputs: Vec<ObjectId>,
    workspaces: Vec<ObjectId>,
}

#[derive(Default)]
struct WorkspaceInfo {
    active: bool,
}

#[derive(Default)]
struct ToplevelInfo {
    outputs: Vec<ObjectId>,
    activated: bool,
}

#[derive(Clone)]
struct OutputGeom {
    id: ObjectId,
    output: wl_output::WlOutput,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scale: i32,
}

struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

struct WorkspaceArea {
    rect: Rect,
    output_ids: Vec<ObjectId>,
}

struct Placed {
    output_id: ObjectId,
    output: wl_output::WlOutput,
    margin_top: i32,
    margin_left: i32,
    panel: Panel,
}

impl App {
    fn new(
        globals: &GlobalList,
        qh: &QueueHandle<App>,
        loop_handle: LoopHandle<'static, App>,
        session: Session,
    ) -> Result<Self, Error> {
        let compositor =
            CompositorState::bind(globals, qh).map_err(|err| Error::Wayland(err.to_string()))?;
        let layer_shell = LayerShell::bind(globals, qh).map_err(|_| Error::LayerShellMissing)?;
        let shm = Shm::bind(globals, qh).map_err(|err| Error::Wayland(err.to_string()))?;
        let pool =
            SlotPool::new(256 * 256 * 4, &shm).map_err(|err| Error::Wayland(err.to_string()))?;

        // Both protocols refer to monitors with output_enter on wl_output objects
        // this client has already bound. Hyprland's foreign-toplevel manager sends
        // that event only while creating each toplevel, and does not repeat it when
        // an output is bound later. Queue the output binds first.
        let output_state = OutputState::new(globals, qh);
        let workspace_manager = match globals.bind(qh, 1..=1, WorkspaceManagerData) {
            Ok(manager) => Some(manager),
            Err(BindError::NotPresent) | Err(BindError::UnsupportedVersion) => None,
        };
        let toplevel_manager = match globals.bind(qh, 1..=3, ToplevelManagerData) {
            Ok(manager) => Some(manager),
            Err(BindError::NotPresent) | Err(BindError::UnsupportedVersion) => None,
        };

        Ok(Self {
            registry_state: RegistryState::new(globals),
            seat_state: SeatState::new(globals, qh),
            output_state,
            shm,
            compositor,
            layer_shell,
            pool,
            loop_handle,
            session,
            exit: false,
            closing: false,
            error_mode: false,
            configured: false,
            needs_draw: false,
            keyboard_focus: false,
            keyboard: None,
            xkb_context: xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
            xkb_keymap: None,
            xkb_state: None,
            raw_mods: RawModifiers::default(),
            layout: 0,
            layer: None,
            output_id: None,
            panel: None,
            workspace_manager,
            toplevel_manager,
            groups: HashMap::new(),
            workspaces: HashMap::new(),
            toplevels: HashMap::new(),
        })
    }

    fn present(&mut self, qh: &QueueHandle<App>, step: Step) -> Result<(), Error> {
        match step {
            Step::Ignore => Ok(()),
            Step::Close => {
                self.request_close();
                Ok(())
            }
            Step::Show(markup) => self.show_markup(qh, &markup),
            Step::Error(text) => self.show_error(qh, &text),
        }
    }

    fn show_error(&mut self, qh: &QueueHandle<App>, text: &str) -> Result<(), Error> {
        self.error_mode = true;
        self.show_markup(qh, &session::error_markup(text))?;
        self.loop_handle
            .insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, app| {
                app.exit = true;
                TimeoutAction::Drop
            })
            .map_err(|err| Error::Wayland(err.error.to_string()))?;
        Ok(())
    }

    fn show_markup(&mut self, qh: &QueueHandle<App>, markup: &str) -> Result<(), Error> {
        let placed = self.place(markup)?;
        let output_id = placed.output_id.clone();
        let output = placed.output.clone();
        let margin_top = placed.margin_top;
        let margin_left = placed.margin_left;
        let logical_w = placed.panel.logical_width as u32;
        let logical_h = placed.panel.logical_height as u32;
        let scale = placed.panel.scale as u32;
        self.panel = Some(placed.panel);
        self.needs_draw = true;

        let same_output = self.output_id.as_ref() == Some(&output_id) && self.layer.is_some();
        if same_output {
            if let Some(layer) = &self.layer {
                layer.set_margin(margin_top, 0, 0, margin_left);
                layer.set_size(logical_w, logical_h);
                let _ = layer.set_buffer_scale(scale);
            }
            if self.configured {
                self.paint()?;
            } else if let Some(layer) = &self.layer {
                layer.commit();
            }
        } else {
            self.configured = false;
            self.keyboard_focus = false;
            self.layer = None;
            self.output_id = Some(output_id);
            self.create_layer(
                qh,
                &output,
                margin_top,
                margin_left,
                logical_w,
                logical_h,
                scale,
            );
        }
        Ok(())
    }

    fn create_layer(
        &mut self,
        qh: &QueueHandle<App>,
        output: &wl_output::WlOutput,
        margin_top: i32,
        margin_left: i32,
        logical_w: u32,
        logical_h: u32,
        scale: u32,
    ) {
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("keytree"),
            Some(output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_exclusive_zone(0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_margin(margin_top, 0, 0, margin_left);
        layer.set_size(logical_w, logical_h);
        let _ = layer.set_buffer_scale(scale);
        // The first commit must not attach a buffer. The compositor replies with configure.
        layer.commit();
        self.layer = Some(layer);
    }

    fn paint(&mut self) -> Result<(), Error> {
        let (buf_w, buf_h, logical_w, logical_h, pixels) = match &self.panel {
            Some(panel) => (
                panel.buf_width,
                panel.buf_height,
                panel.logical_width,
                panel.logical_height,
                panel.pixels.clone(),
            ),
            None => return Ok(()),
        };
        let stride = buf_w
            .checked_mul(4)
            .ok_or_else(|| Error::Draw("frame is too large".to_owned()))?;
        let (buffer, canvas) = self
            .pool
            .create_buffer(buf_w, buf_h, stride, wl_shm::Format::Argb8888)
            .map_err(|err| Error::Wayland(err.to_string()))?;
        if canvas.len() < pixels.len() {
            return Err(Error::Wayland(
                "shared memory buffer is shorter than the frame".to_owned(),
            ));
        }
        canvas[..pixels.len()].copy_from_slice(&pixels);

        {
            let Some(layer) = &self.layer else {
                return Ok(());
            };
            let surface = layer.wl_surface();
            if surface.version() >= 4 {
                surface.damage_buffer(0, 0, buf_w, buf_h);
            } else {
                surface.damage(0, 0, logical_w, logical_h);
            }
            buffer
                .attach_to(surface)
                .map_err(|err| Error::Wayland(err.to_string()))?;
            layer.commit();
        }
        self.needs_draw = false;
        Ok(())
    }

    fn request_close(&mut self) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.layer = None;
        self.exit = true;
    }

    fn on_key(&mut self, qh: &QueueHandle<App>, event: KeyEvent) {
        if self.error_mode || self.closing {
            return;
        }
        let keysym = self.level0_keysym(event.raw_code, event.keysym.raw());
        if keysym::is_modifier(keysym) {
            return;
        }
        let combination = Combination {
            key: keysym,
            modifiers: self.current_modifiers(),
        };
        match self.session.on_key(combination) {
            Ok(step) => {
                if let Err(err) = self.present(qh, step) {
                    eprintln!("{}", err);
                    self.exit = true;
                }
            }
            Err(err) => {
                eprintln!("{}", err);
                self.exit = true;
            }
        }
    }

    /// evdev keycodes are 8 below the X keycodes xkbcommon expects.
    fn level0_keysym(&self, raw_code: u32, fallback: u32) -> u32 {
        let Some(keymap) = &self.xkb_keymap else {
            return fallback;
        };
        let Some(xkb_state) = &self.xkb_state else {
            return fallback;
        };
        let code = xkb::Keycode::new(raw_code.saturating_add(8));
        let layout = xkb_state.key_get_layout(code);
        keymap
            .key_get_syms_by_level(code, layout, 0)
            .first()
            .map(|sym| sym.raw())
            .unwrap_or(fallback)
    }

    fn current_modifiers(&self) -> Modifiers {
        let (Some(xkb_state), Some(keymap)) = (&self.xkb_state, &self.xkb_keymap) else {
            return Modifiers::default();
        };
        let effective = xkb::STATE_MODS_EFFECTIVE;
        let meta_idx = keymap.mod_get_index("Meta");
        let alt_idx = keymap.mod_get_index(xkb::MOD_NAME_ALT);
        Modifiers {
            control: xkb_state.mod_name_is_active(xkb::MOD_NAME_CTRL, effective),
            alt: xkb_state.mod_name_is_active(xkb::MOD_NAME_ALT, effective),
            superr: xkb_state.mod_name_is_active(xkb::MOD_NAME_LOGO, effective),
            meta: meta_idx != xkb::MOD_INVALID
                && meta_idx != alt_idx
                && xkb_state.mod_name_is_active("Meta", effective),
            hyper: keymap.mod_get_index("Hyper") != xkb::MOD_INVALID
                && xkb_state.mod_name_is_active("Hyper", effective),
        }
    }

    fn sync_xkb_mask(&mut self) {
        if let Some(xkb_state) = &mut self.xkb_state {
            xkb_state.update_mask(
                self.raw_mods.depressed,
                self.raw_mods.latched,
                self.raw_mods.locked,
                0,
                0,
                self.layout,
            );
        }
    }

    fn our_surface(&self, surface: &wl_surface::WlSurface) -> bool {
        self.layer
            .as_ref()
            .map(|layer| layer.wl_surface() == surface)
            .unwrap_or(false)
    }

    fn output_geoms(&self) -> Vec<OutputGeom> {
        self.output_state
            .outputs()
            .filter_map(|output| {
                let info = self.output_state.info(&output)?;
                geom_from_info(&output, &info)
            })
            .collect()
    }

    fn workspace_area(&self, outputs: &[OutputGeom]) -> Result<WorkspaceArea, Error> {
        if outputs.is_empty() {
            return Err(Error::NoScreenFound);
        }
        if self.workspace_manager.is_none() && self.toplevel_manager.is_none() {
            return Ok(fallback_output(outputs));
        }
        if let Some(area) = self.area_from_active_groups(outputs) {
            return Ok(area);
        }
        let focus = self.activated_output_ids();
        if let Some(area) = area_for_ids(&focus, outputs) {
            return Ok(area);
        }
        if outputs.len() == 1 {
            return Ok(area_of(&outputs[0]));
        }
        Ok(fallback_output(outputs))
    }

    fn area_from_active_groups(&self, outputs: &[OutputGeom]) -> Option<WorkspaceArea> {
        let active: Vec<&Group> = self
            .groups
            .values()
            .filter(|group| {
                group.workspaces.iter().any(|id| {
                    self.workspaces
                        .get(id)
                        .map(|workspace| workspace.active)
                        .unwrap_or(false)
                })
            })
            .collect();

        if active.len() == 1 {
            return area_for_ids(&active[0].outputs, outputs);
        }
        if active.len() > 1 {
            let focus = self.activated_output_ids();
            let mut best: Option<&Group> = None;
            let mut best_count = 0usize;
            for group in &active {
                let count = group
                    .outputs
                    .iter()
                    .filter(|id| focus.iter().any(|focused| focused == *id))
                    .count();
                if count > best_count {
                    best_count = count;
                    best = Some(*group);
                }
            }
            if let Some(group) = best {
                return area_for_ids(&group.outputs, outputs);
            }
        }
        None
    }

    fn activated_output_ids(&self) -> Vec<ObjectId> {
        let mut ids = Vec::new();
        for toplevel in self.toplevels.values() {
            if !toplevel.activated {
                continue;
            }
            for id in &toplevel.outputs {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }
        ids
    }

    fn place(&self, markup: &str) -> Result<Placed, Error> {
        let outputs = self.output_geoms();
        let area = self.workspace_area(&outputs)?;
        let workspace_cx = area.rect.x + area.rect.w / 2;
        let workspace_cy = area.rect.y + area.rect.h / 2;
        let mut scale = outputs
            .iter()
            .find(|output| contains(output, workspace_cx, workspace_cy))
            .map(|output| output.scale)
            .unwrap_or(1)
            .max(1);

        let max_logical_height = draw::max_window_height(area.rect.h);
        let mut panel = draw::render(
            markup,
            &self.session.opt.font,
            scale,
            Some(max_logical_height),
        )?;
        let mut origin = card_origin(&self.session.opt.position, &panel, &area.rect)?;
        let mut output = pick_output(&outputs, &area.output_ids, origin, &panel)
            .cloned()
            .ok_or(Error::NoScreenFound)?;

        for _ in 0..3 {
            if output.scale.max(1) == scale {
                break;
            }
            scale = output.scale.max(1);
            panel = draw::render(
                markup,
                &self.session.opt.font,
                scale,
                Some(max_logical_height),
            )?;
            origin = card_origin(&self.session.opt.position, &panel, &area.rect)?;
            output = pick_output(&outputs, &area.output_ids, origin, &panel)
                .cloned()
                .ok_or(Error::NoScreenFound)?;
        }
        if output.scale.max(1) != panel.scale {
            panel = draw::render(
                markup,
                &self.session.opt.font,
                output.scale.max(1),
                Some(max_logical_height),
            )?;
            origin = card_origin(&self.session.opt.position, &panel, &area.rect)?;
        }

        Ok(Placed {
            output_id: output.id.clone(),
            output: output.output.clone(),
            margin_top: origin.1 - draw::SHADOW - output.y,
            margin_left: origin.0 - draw::SHADOW - output.x,
            panel,
        })
    }
}

fn geom_from_info(output: &wl_output::WlOutput, info: &OutputInfo) -> Option<OutputGeom> {
    let scale = info.scale_factor.max(1);
    let (x, y, w, h) =
        if let (Some((x, y)), Some((w, h))) = (info.logical_position, info.logical_size) {
            if w <= 0 || h <= 0 {
                return None;
            }
            (x, y, w, h)
        } else {
            let mode = info
                .modes
                .iter()
                .find(|mode| mode.current)
                .or_else(|| info.modes.first())?;
            let w = mode.dimensions.0 / scale;
            let h = mode.dimensions.1 / scale;
            if w <= 0 || h <= 0 {
                return None;
            }
            (info.location.0, info.location.1, w, h)
        };
    Some(OutputGeom {
        id: output.id(),
        output: output.clone(),
        x,
        y,
        w,
        h,
        scale,
    })
}

fn fallback_output(outputs: &[OutputGeom]) -> WorkspaceArea {
    log::warn!("compositor did not identify the active workspace; using the largest output");
    eprintln!("compositor did not identify the active workspace; using the largest output");
    let output = if outputs.len() == 1 {
        &outputs[0]
    } else {
        largest(outputs)
    };
    area_of(output)
}

fn largest(outputs: &[OutputGeom]) -> &OutputGeom {
    outputs
        .iter()
        .max_by_key(|output| output.w as i64 * output.h as i64)
        .expect("largest output is only used when one exists")
}

fn area_of(output: &OutputGeom) -> WorkspaceArea {
    WorkspaceArea {
        rect: Rect {
            x: output.x,
            y: output.y,
            w: output.w,
            h: output.h,
        },
        output_ids: vec![output.id.clone()],
    }
}

fn area_for_ids(ids: &[ObjectId], outputs: &[OutputGeom]) -> Option<WorkspaceArea> {
    if ids.is_empty() {
        return None;
    }
    let matched: Vec<&OutputGeom> = outputs
        .iter()
        .filter(|output| ids.iter().any(|id| id == &output.id))
        .collect();
    let rect = bbox(&matched)?;
    Some(WorkspaceArea {
        rect,
        output_ids: matched
            .into_iter()
            .map(|output| output.id.clone())
            .collect(),
    })
}

fn bbox(outputs: &[&OutputGeom]) -> Option<Rect> {
    let first = *outputs.first()?;
    let mut x0 = first.x;
    let mut y0 = first.y;
    let mut x1 = first.x.saturating_add(first.w);
    let mut y1 = first.y.saturating_add(first.h);
    for output in outputs.iter().skip(1) {
        x0 = x0.min(output.x);
        y0 = y0.min(output.y);
        x1 = x1.max(output.x.saturating_add(output.w));
        y1 = y1.max(output.y.saturating_add(output.h));
    }
    let w = x1.saturating_sub(x0);
    let h = y1.saturating_sub(y0);
    if w <= 0 || h <= 0 {
        None
    } else {
        Some(Rect { x: x0, y: y0, w, h })
    }
}

fn contains(output: &OutputGeom, x: i32, y: i32) -> bool {
    x >= output.x && y >= output.y && x < output.x + output.w && y < output.y + output.h
}

fn center_dist2(output: &OutputGeom, x: i32, y: i32) -> i64 {
    let dx = (output.x + output.w / 2) as i64 - x as i64;
    let dy = (output.y + output.h / 2) as i64 - y as i64;
    dx * dx + dy * dy
}

fn nearest(outputs: &[OutputGeom], x: i32, y: i32) -> Option<&OutputGeom> {
    outputs
        .iter()
        .min_by_key(|output| center_dist2(output, x, y))
}

fn pick_output<'a>(
    outputs: &'a [OutputGeom],
    group_ids: &[ObjectId],
    origin: (i32, i32),
    panel: &Panel,
) -> Option<&'a OutputGeom> {
    let x = origin.0 + panel.card_width / 2;
    let y = origin.1 + panel.card_height / 2;
    if let Some(hit) = outputs.iter().find(|output| contains(output, x, y)) {
        return Some(hit);
    }
    let group: Vec<&OutputGeom> = outputs
        .iter()
        .filter(|output| group_ids.iter().any(|id| id == &output.id))
        .collect();
    if group.is_empty() {
        nearest(outputs, x, y)
    } else {
        group
            .into_iter()
            .min_by_key(|output| center_dist2(output, x, y))
    }
}

fn card_origin(position: &str, panel: &Panel, workspace: &Rect) -> Result<(i32, i32), Error> {
    let mut parts = position.split(',');
    let x_spec = parts
        .next()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .ok_or(Error::InvalidPosition)?;
    let y_spec = parts
        .next()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .ok_or(Error::InvalidPosition)?;
    if parts.next().is_some() {
        return Err(Error::InvalidPosition);
    }
    let x = parse_position(x_spec, panel.card_width, workspace.w)?;
    let y = parse_position(y_spec, panel.card_height, workspace.h)?;
    Ok((workspace.x + x, workspace.y + y))
}

fn workspace_is_active(state: WEnum<ext_workspace_handle_v1::State>) -> bool {
    match state {
        WEnum::Value(bits) => bits.contains(ext_workspace_handle_v1::State::Active),
        WEnum::Unknown(bits) => bits & 1 != 0,
    }
}

fn toplevel_is_activated(bytes: &[u8]) -> bool {
    bytes.chunks_exact(4).any(|chunk| {
        let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        value == 2
    })
}

fn push_unique(ids: &mut Vec<ObjectId>, id: ObjectId) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.output_id.as_ref() == Some(&output.id()) && !self.closing && !self.error_mode {
            self.request_close();
        }
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        _: LayerSurfaceConfigure,
        _: u32,
    ) {
        self.configured = true;
        if self.needs_draw {
            if let Err(err) = self.paint() {
                eprintln!("{}", err);
                self.exit = true;
            }
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(keyboard) => self.keyboard = Some(keyboard),
                Err(err) => log::warn!("keyboard: {}", err),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if self.our_surface(surface) {
            self.keyboard_focus = true;
        }
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.error_mode || self.closing {
            return;
        }
        if self.keyboard_focus && self.our_surface(surface) {
            self.keyboard_focus = false;
            self.request_close();
        }
    }

    fn press_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.on_key(qh, event);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: SctkModifiers,
        raw_modifiers: RawModifiers,
        layout: u32,
    ) {
        self.raw_mods = raw_modifiers;
        self.layout = layout;
        self.sync_xkb_mask();
    }

    fn update_keymap(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        keymap: SctkKeymap<'_>,
    ) {
        let Some(compiled) = xkb::Keymap::new_from_string(
            &self.xkb_context,
            keymap.as_string(),
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        ) else {
            return;
        };
        self.xkb_state = Some(xkb::State::new(&compiled));
        self.xkb_keymap = Some(compiled);
        self.sync_xkb_mask();
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
smithay_client_toolkit::delegate_dispatch2!(App);

struct WorkspaceManagerData;

impl Dispatch2<ext_workspace_manager_v1::ExtWorkspaceManagerV1, App> for WorkspaceManagerData {
    fn event(
        &self,
        state: &mut App,
        _proxy: &ext_workspace_manager_v1::ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                state.groups.entry(workspace_group.id()).or_default();
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                state.workspaces.entry(workspace.id()).or_default();
            }
            ext_workspace_manager_v1::Event::Done => {}
            ext_workspace_manager_v1::Event::Finished => {}
            _ => {}
        }
    }

    fn event_created_child(opcode: u16, qh: &QueueHandle<App>) -> Arc<dyn ObjectData> {
        match opcode {
            0 => qh.make_data::<ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1, WorkspaceGroupData>(
                WorkspaceGroupData,
            ),
            1 => qh.make_data::<ext_workspace_handle_v1::ExtWorkspaceHandleV1, WorkspaceData>(
                WorkspaceData,
            ),
            _ => panic!("unexpected ext_workspace_manager_v1 child opcode {}", opcode),
        }
    }
}

struct WorkspaceGroupData;

impl Dispatch2<ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1, App>
    for WorkspaceGroupData
{
    fn event(
        &self,
        state: &mut App,
        proxy: &ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
        use ext_workspace_group_handle_v1::Event;
        match event {
            Event::Capabilities { .. } => {}
            Event::OutputEnter { output } => {
                push_unique(
                    &mut state.groups.entry(proxy.id()).or_default().outputs,
                    output.id(),
                );
            }
            Event::OutputLeave { output } => {
                let id = output.id();
                if let Some(group) = state.groups.get_mut(&proxy.id()) {
                    group.outputs.retain(|output_id| output_id != &id);
                }
            }
            Event::WorkspaceEnter { workspace } => {
                let workspace_id = workspace.id();
                state.workspaces.entry(workspace_id.clone()).or_default();
                push_unique(
                    &mut state.groups.entry(proxy.id()).or_default().workspaces,
                    workspace_id,
                );
            }
            Event::WorkspaceLeave { workspace } => {
                let id = workspace.id();
                if let Some(group) = state.groups.get_mut(&proxy.id()) {
                    group.workspaces.retain(|workspace_id| workspace_id != &id);
                }
            }
            Event::Removed => {
                state.groups.remove(&proxy.id());
                proxy.destroy();
            }
            _ => {}
        }
    }
}

struct WorkspaceData;

impl Dispatch2<ext_workspace_handle_v1::ExtWorkspaceHandleV1, App> for WorkspaceData {
    fn event(
        &self,
        state: &mut App,
        proxy: &ext_workspace_handle_v1::ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
        use ext_workspace_handle_v1::Event;
        match event {
            Event::Id { .. }
            | Event::Name { .. }
            | Event::Coordinates { .. }
            | Event::Capabilities { .. } => {}
            Event::State { state: ws_state } => {
                state.workspaces.entry(proxy.id()).or_default().active =
                    workspace_is_active(ws_state);
            }
            Event::Removed => {
                let id = proxy.id();
                for group in state.groups.values_mut() {
                    group.workspaces.retain(|workspace_id| workspace_id != &id);
                }
                state.workspaces.remove(&id);
                proxy.destroy();
            }
            _ => {}
        }
    }
}

struct ToplevelManagerData;

impl Dispatch2<zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1, App>
    for ToplevelManagerData
{
    fn event(
        &self,
        state: &mut App,
        _proxy: &zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
        use zwlr_foreign_toplevel_manager_v1::Event;
        match event {
            Event::Toplevel { toplevel } => {
                state.toplevels.entry(toplevel.id()).or_default();
            }
            Event::Finished => {}
            _ => {}
        }
    }

    fn event_created_child(opcode: u16, qh: &QueueHandle<App>) -> Arc<dyn ObjectData> {
        match opcode {
            0 => qh.make_data::<zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1, ToplevelData>(
                ToplevelData,
            ),
            _ => panic!(
                "unexpected zwlr_foreign_toplevel_manager_v1 child opcode {}",
                opcode
            ),
        }
    }
}

struct ToplevelData;

impl Dispatch2<zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1, App> for ToplevelData {
    fn event(
        &self,
        state: &mut App,
        proxy: &zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        match event {
            Event::Title { .. } | Event::AppId { .. } | Event::Done => {}
            Event::OutputEnter { output } => {
                push_unique(
                    &mut state.toplevels.entry(proxy.id()).or_default().outputs,
                    output.id(),
                );
            }
            Event::OutputLeave { output } => {
                let id = output.id();
                if let Some(toplevel) = state.toplevels.get_mut(&proxy.id()) {
                    toplevel.outputs.retain(|output_id| output_id != &id);
                }
            }
            Event::State { state: raw_state } => {
                state.toplevels.entry(proxy.id()).or_default().activated =
                    toplevel_is_activated(&raw_state);
            }
            Event::Closed => {
                state.toplevels.remove(&proxy.id());
                proxy.destroy();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hyprland drops foreign-toplevel output_enter unless wl_output was bound first.
    #[test]
    fn activated_toplevel_names_a_bound_output() {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        let conn = match Connection::connect_to_env() {
            Ok(conn) => conn,
            Err(_) => return,
        };
        let (globals, mut queue) = registry_queue_init::<App>(&conn).expect("registry");
        let qh = queue.handle();
        let event_loop: EventLoop<'static, App> = EventLoop::try_new().expect("event loop");
        let mut app = App::new(&globals, &qh, event_loop.handle(), Session::blank())
            .expect("wayland globals");
        for _ in 0..3 {
            queue.roundtrip(&mut app).expect("roundtrip");
        }
        if app.toplevel_manager.is_none() {
            return;
        }
        let outputs = app.output_geoms();
        let activated: Vec<_> = app
            .toplevels
            .values()
            .filter(|toplevel| toplevel.activated)
            .collect();
        if activated.is_empty() || outputs.is_empty() {
            return;
        }
        assert!(
            activated.iter().any(|toplevel| {
                toplevel
                    .outputs
                    .iter()
                    .any(|id| outputs.iter().any(|output| output.id == *id))
            }),
            "an activated toplevel did not name a wl_output bound by this client"
        );

        if app.workspace_manager.is_some() && outputs.len() > 1 {
            let focus = app.activated_output_ids();
            let area = app.workspace_area(&outputs).expect("workspace area");
            assert!(
                area.output_ids.iter().any(|id| focus.contains(id)),
                "active workspace did not cover the focused toplevel's output"
            );
        }
    }
}
