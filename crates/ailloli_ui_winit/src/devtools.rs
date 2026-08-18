use ailloli_ui_core::event::keyboard::{Key, KeyState, NamedKey};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::{Color, Constraints, ElementId, Event, Rect};
use ailloli_ui_devtools_core::{
    collect_debug_snapshot_with_state, debug_draw_cmds, pick_element_at, DebugDrawCmd,
    DebugSnapshot, DevToolsClientMessage, DevToolsMode, DevToolsServerMessage,
};
use ailloli_ui_devtools_ui::{
    build_devtools_overlay, DevToolsAction, DevToolsState, DEVTOOLS_PANEL_KEY,
};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::element::ElementTree;
use ailloli_ui_runtime::input::{absolute_paint_bounds, InputRouter};
use ailloli_ui_runtime::scene::{ClipStackSnapshot, DrawCmd, DrawRect, Layer, Scene};
use ailloli_ui_text::TextSystem;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

const MAX_DEVTOOLS_CLIENTS: usize = 8;
const MAX_DEVTOOLS_MESSAGE_BYTES: usize = 64 * 1024;

pub struct DevToolsWindowState {
    pub enabled: bool,
    pub mode: DevToolsMode,
    pub picker_active: bool,
    pub selected: Option<u64>,
    pub hovered: Option<u64>,
    runtime: Runtime<DevToolsAction>,
    input: InputRouter,
    frame_index: u64,
    last_snapshot: Option<DebugSnapshot>,
    last_panel_bounds: Option<Rect>,
    remote: Option<DevToolsRemote>,
}

impl DevToolsWindowState {
    pub fn new() -> Self {
        let remote = DevToolsRemote::from_env();
        Self {
            enabled: remote.is_some(),
            mode: DevToolsMode::Overlay,
            picker_active: false,
            selected: None,
            hovered: None,
            runtime: Runtime::new(RuntimeHandle::new()),
            input: InputRouter::default(),
            frame_index: 0,
            last_snapshot: None,
            last_panel_bounds: None,
            remote,
        }
    }

    pub fn set_remote_addr(&mut self, addr: Option<SocketAddr>) {
        self.remote = addr.and_then(DevToolsRemote::start);
        if self.remote.is_some() {
            self.enabled = true;
        }
    }

    pub fn handle_event(&mut self, event: &Event) -> bool {
        self.apply_remote_commands();
        match event {
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                if key.modifiers.ctrl
                    && key.modifiers.shift
                    && matches!(&key.key, Key::Character(c) if c.eq_ignore_ascii_case("i"))
                {
                    self.enabled = !self.enabled;
                    if self.enabled && matches!(self.mode, DevToolsMode::Hidden) {
                        self.mode = DevToolsMode::Overlay;
                    }
                    return true;
                }
                if key.modifiers.ctrl
                    && key.modifiers.shift
                    && matches!(&key.key, Key::Character(c) if c.eq_ignore_ascii_case("c"))
                {
                    self.enabled = true;
                    self.picker_active = !self.picker_active;
                    return true;
                }
                if matches!(&key.key, Key::Named(NamedKey::Escape)) {
                    if self.picker_active {
                        self.picker_active = false;
                    } else if self.enabled {
                        self.enabled = false;
                    } else {
                        return false;
                    }
                    return true;
                }
            }
            Event::Pointer(PointerEvent::Moved { pos, .. })
            | Event::Pointer(PointerEvent::Button { pos, .. })
            | Event::Pointer(PointerEvent::Wheel { pos, .. })
                if self.enabled && self.panel_contains(*pos) =>
            {
                self.input
                    .route_event(&self.runtime.tree, self.runtime.runtime.clone(), event);
                self.apply_runtime_actions();
                return true;
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) if self.picker_active => {
                self.hovered = self
                    .last_snapshot
                    .as_ref()
                    .and_then(|snapshot| pick_element_at(snapshot, *pos));
                return true;
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.picker_active => {
                self.selected = self
                    .last_snapshot
                    .as_ref()
                    .and_then(|snapshot| pick_element_at(snapshot, *pos));
                self.picker_active = false;
                return true;
            }
            _ => {}
        }
        false
    }

    pub fn build_scene<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        root: ElementId,
        viewport: Rect,
        scale: ailloli_ui_core::math::Scale,
        text_system: &mut TextSystem,
    ) -> Option<Scene> {
        self.apply_remote_commands();
        if !self.enabled && self.remote.is_none() {
            return None;
        }

        self.frame_index = self.frame_index.saturating_add(1);
        let snapshot = collect_debug_snapshot_with_state(
            tree,
            root,
            viewport,
            self.selected.map(ElementId),
            self.hovered.map(ElementId),
            self.frame_index,
        );
        if let Some(remote) = &self.remote {
            remote.publish_snapshot(snapshot.clone());
        }
        self.last_snapshot = Some(snapshot.clone());

        if !self.enabled || matches!(self.mode, DevToolsMode::Hidden) {
            return None;
        }

        let ui_state = DevToolsState {
            enabled: self.enabled,
            mode: self.mode,
            picker_active: self.picker_active,
            selected: self.selected,
            hovered: self.hovered,
            filter: String::new(),
        };
        let overlay = build_devtools_overlay(&snapshot, &ui_state);
        self.runtime.reconcile(overlay);
        self.runtime.layout(
            Constraints::tight(viewport.w, viewport.h),
            scale,
            text_system,
        );
        self.last_panel_bounds = self.resolve_panel_bounds();
        self.apply_runtime_actions();
        let mut scene = self.runtime.paint(text_system);
        let draw_cmds = debug_draw_cmds(&snapshot)
            .into_iter()
            .flat_map(debug_draw_to_runtime)
            .collect::<Vec<_>>();
        if !draw_cmds.is_empty() {
            let mut layer = Layer::overlay(ClipStackSnapshot::empty());
            layer.cmds = draw_cmds;
            scene.layers.insert(0, layer);
        }
        Some(scene)
    }

    fn panel_contains(&self, pos: ailloli_ui_core::Point) -> bool {
        self.last_panel_bounds
            .is_some_and(|bounds| bounds.contains(pos.x, pos.y))
    }

    fn resolve_panel_bounds(&self) -> Option<Rect> {
        let id = self
            .runtime
            .tree
            .resolve_element_by_view_key(DEVTOOLS_PANEL_KEY)
            .ok()?;
        absolute_paint_bounds(&self.runtime.tree, id)
    }

    fn apply_runtime_actions(&mut self) {
        for action in self.runtime.runtime.take_actions() {
            self.apply_action(action);
        }
    }

    fn apply_action(&mut self, action: DevToolsAction) {
        match action {
            DevToolsAction::Select(id) => {
                self.selected = id;
                self.enabled = true;
            }
            DevToolsAction::Hover(id) => {
                self.hovered = id;
            }
            DevToolsAction::SetMode(mode) => {
                self.mode = mode;
                self.enabled = !matches!(mode, DevToolsMode::Hidden);
            }
            DevToolsAction::TogglePicker => {
                self.enabled = true;
                self.picker_active = !self.picker_active;
            }
            DevToolsAction::SetFilter(filter) => {
                let _ = filter;
            }
        }
    }

    fn apply_remote_commands(&mut self) {
        let Some(remote) = &self.remote else {
            return;
        };
        for cmd in remote.drain_commands() {
            match cmd {
                DevToolsClientMessage::Select { id } => {
                    self.apply_action(DevToolsAction::Select(id));
                }
                DevToolsClientMessage::Hover { id } => {
                    self.apply_action(DevToolsAction::Hover(id));
                }
                DevToolsClientMessage::SetMode { mode } => {
                    self.apply_action(DevToolsAction::SetMode(mode));
                }
                DevToolsClientMessage::Ping => {}
            }
        }
    }
}

impl Default for DevToolsWindowState {
    fn default() -> Self {
        Self::new()
    }
}

fn debug_draw_to_runtime(cmd: DebugDrawCmd) -> Vec<DrawCmd> {
    match cmd {
        DebugDrawCmd::RectOutline {
            rect,
            color,
            thickness,
        } => outline_rect(
            Rect::new(rect.x, rect.y, rect.w, rect.h),
            Color::f32(color.r, color.g, color.b, color.a),
            thickness,
        ),
        DebugDrawCmd::RectFill { rect, color } => vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y, rect.w, rect.h),
            color: Color::f32(color.r, color.g, color.b, color.a),
        })],
        DebugDrawCmd::TextLabel { .. } => Vec::new(),
    }
}

fn outline_rect(rect: Rect, color: Color, thickness: f32) -> Vec<DrawCmd> {
    let t = thickness.max(1.0);
    vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y, rect.w, t),
            color,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.bottom() - t, rect.w, t),
            color,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y, t, rect.h),
            color,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.right() - t, rect.y, t, rect.h),
            color,
        }),
    ]
}

pub struct DevToolsRemote {
    events: Sender<RemoteEvent>,
    commands: Receiver<DevToolsClientMessage>,
}

impl DevToolsRemote {
    pub fn from_env() -> Option<Self> {
        let addr =
            crate::framework_env_var_os("AILLOLI_UI_DEVTOOLS_REMOTE", "OCTAVUI_DEVTOOLS_REMOTE")?;
        let addr = addr.to_string_lossy().parse::<SocketAddr>().ok()?;
        Self::start(addr)
    }

    pub fn start(addr: SocketAddr) -> Option<Self> {
        if !addr.ip().is_loopback() {
            eprintln!("ailloli_ui devtools remote refused non-loopback addr {addr}");
            return None;
        }
        let listener = TcpListener::bind(addr).ok()?;
        listener.set_nonblocking(true).ok()?;
        let (events_tx, events_rx) = mpsc::channel();
        let (commands_tx, commands_rx) = mpsc::channel();
        thread::spawn(move || run_remote_server(listener, events_rx, commands_tx));
        Some(Self {
            events: events_tx,
            commands: commands_rx,
        })
    }

    pub fn publish_snapshot(&self, snapshot: DebugSnapshot) {
        let _ = self.events.send(RemoteEvent::Snapshot(snapshot));
    }

    pub fn drain_commands(&self) -> Vec<DevToolsClientMessage> {
        let mut out = Vec::new();
        while let Ok(cmd) = self.commands.try_recv() {
            out.push(cmd);
        }
        out
    }
}

enum RemoteEvent {
    Snapshot(DebugSnapshot),
}

struct Client {
    stream: TcpStream,
    read_buf: String,
}

fn run_remote_server(
    listener: TcpListener,
    events: Receiver<RemoteEvent>,
    commands: Sender<DevToolsClientMessage>,
) {
    let mut clients = Vec::<Client>::new();
    loop {
        loop {
            match listener.accept() {
                Ok((mut stream, _)) if clients.len() < MAX_DEVTOOLS_CLIENTS => {
                    let _ = stream.set_nonblocking(true);
                    let hello = DevToolsServerMessage::Hello { protocol: 1 };
                    let _ = write_jsonl(&mut stream, &hello);
                    clients.push(Client {
                        stream,
                        read_buf: String::new(),
                    });
                }
                Ok((_stream, _)) => {}
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        while let Ok(event) = events.try_recv() {
            match event {
                RemoteEvent::Snapshot(snapshot) => {
                    let message = DevToolsServerMessage::Snapshot { snapshot };
                    clients.retain_mut(|client| write_jsonl(&mut client.stream, &message).is_ok());
                }
            }
        }

        clients.retain_mut(|client| read_client_commands(client, &commands));
        thread::sleep(Duration::from_millis(16));
    }
}

fn write_jsonl(stream: &mut TcpStream, message: &DevToolsServerMessage) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(message)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    line.push(b'\n');
    stream.write_all(&line)
}

fn read_client_commands(client: &mut Client, commands: &Sender<DevToolsClientMessage>) -> bool {
    let mut buf = [0u8; 4096];
    loop {
        match client.stream.read(&mut buf) {
            Ok(0) => return false,
            Ok(n) => {
                client
                    .read_buf
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
                if client.read_buf.len() > MAX_DEVTOOLS_MESSAGE_BYTES
                    && !client.read_buf[..MAX_DEVTOOLS_MESSAGE_BYTES].contains('\n')
                {
                    let message = DevToolsServerMessage::Error {
                        message: "devtools message exceeds the 65536-byte limit".to_string(),
                    };
                    let _ = write_jsonl(&mut client.stream, &message);
                    return false;
                }
                while let Some(pos) = client.read_buf.find('\n') {
                    if pos > MAX_DEVTOOLS_MESSAGE_BYTES {
                        let message = DevToolsServerMessage::Error {
                            message: "devtools message exceeds the 65536-byte limit".to_string(),
                        };
                        let _ = write_jsonl(&mut client.stream, &message);
                        return false;
                    }
                    let line = client.read_buf[..pos].trim().to_string();
                    client.read_buf = client.read_buf[pos + 1..].to_string();
                    if line.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<DevToolsClientMessage>(&line) {
                        Ok(DevToolsClientMessage::Ping) => {
                            if write_jsonl(&mut client.stream, &DevToolsServerMessage::Pong)
                                .is_err()
                            {
                                return false;
                            }
                        }
                        Ok(cmd) => {
                            let _ = commands.send(cmd);
                        }
                        Err(err) => {
                            let message = DevToolsServerMessage::Error {
                                message: format!("invalid devtools message: {err}"),
                            };
                            if write_jsonl(&mut client.stream, &message).is_err() {
                                return false;
                            }
                        }
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return true,
            Err(_) => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    use ailloli_ui_core::event::keyboard::Modifiers;
    use ailloli_ui_core::math::Scale;
    use ailloli_ui_core::Point;
    use ailloli_ui_devtools_ui::DEVTOOLS_MODE_BOTTOM_KEY;
    use ailloli_ui_runtime::component::IntoView;
    use ailloli_ui_runtime::scene::DrawCmd;
    use ailloli_ui_widgets::layout::Container;
    use ailloli_ui_widgets::text::Text;

    fn app_runtime() -> (Runtime<()>, TextSystem) {
        let mut runtime = Runtime::new(RuntimeHandle::new());
        runtime.reconcile(
            Container::new()
                .fill()
                .child(Text::new("application text"))
                .into_view(),
        );
        let mut text_system = TextSystem::new();
        runtime.layout(
            Constraints::tight(800.0, 600.0),
            Scale::new(1.0),
            &mut text_system,
        );
        (runtime, text_system)
    }

    fn text_cmd_count(scene: &Scene) -> usize {
        scene
            .layers
            .iter()
            .flat_map(|layer| layer.cmds.iter())
            .filter(|cmd| matches!(cmd, DrawCmd::Text(_)))
            .count()
    }

    fn pointer_button(pos: Point, pressed: bool) -> Event {
        Event::Pointer(PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed,
            modifiers: Modifiers::default(),
        })
    }

    fn center_of_key(state: &DevToolsWindowState, key: &str) -> Point {
        let id = state
            .runtime
            .tree
            .resolve_element_by_view_key(key)
            .expect("keyed devtools element");
        let bounds = absolute_paint_bounds(&state.runtime.tree, id).expect("element bounds");
        Point::new(bounds.x + bounds.w / 2.0, bounds.y + bounds.h / 2.0)
    }

    fn connected_client() -> (Client, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let addr = listener.local_addr().expect("read listener addr");
        let client_stream = TcpStream::connect(addr).expect("connect client");
        let (server_stream, _) = listener.accept().expect("accept client");
        server_stream
            .set_nonblocking(true)
            .expect("server stream nonblocking");
        client_stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("client read timeout");
        (
            Client {
                stream: server_stream,
                read_buf: String::new(),
            },
            client_stream,
        )
    }

    #[test]
    fn remote_reader_accepts_select_command() {
        let (mut server_client, mut client_stream) = connected_client();
        let (commands_tx, commands_rx) = mpsc::channel();

        client_stream
            .write_all(br#"{"type":"select","id":42}"#)
            .expect("write command");
        client_stream.write_all(b"\n").expect("write newline");

        assert!(read_client_commands(&mut server_client, &commands_tx));
        assert_eq!(
            commands_rx.try_recv().expect("command queued"),
            DevToolsClientMessage::Select { id: Some(42) }
        );
    }

    #[test]
    fn remote_reader_replies_to_ping() {
        let (mut server_client, mut client_stream) = connected_client();
        let (commands_tx, commands_rx) = mpsc::channel();

        client_stream
            .write_all(br#"{"type":"ping"}"#)
            .expect("write ping");
        client_stream.write_all(b"\n").expect("write newline");

        assert!(read_client_commands(&mut server_client, &commands_tx));

        let mut line = String::new();
        let mut reader = BufReader::new(client_stream);
        reader.read_line(&mut line).expect("read pong");
        let message: DevToolsServerMessage = serde_json::from_str(line.trim()).expect("parse pong");
        assert_eq!(message, DevToolsServerMessage::Pong);
        assert!(commands_rx.try_recv().is_err());
    }

    #[test]
    fn remote_reader_reports_unknown_message() {
        let (mut server_client, mut client_stream) = connected_client();
        let (commands_tx, commands_rx) = mpsc::channel();

        client_stream
            .write_all(br#"{"type":"unknown"}"#)
            .expect("write unknown command");
        client_stream.write_all(b"\n").expect("write newline");

        assert!(read_client_commands(&mut server_client, &commands_tx));

        let mut line = String::new();
        let mut reader = BufReader::new(client_stream);
        reader.read_line(&mut line).expect("read error");
        let message: DevToolsServerMessage =
            serde_json::from_str(line.trim()).expect("parse error");
        assert!(matches!(message, DevToolsServerMessage::Error { .. }));
        assert!(commands_rx.try_recv().is_err());
    }

    #[test]
    fn remote_reader_disconnects_when_input_exceeds_limit() {
        let (mut server_client, mut client_stream) = connected_client();
        let (commands_tx, commands_rx) = mpsc::channel();
        server_client.read_buf = "x".repeat(MAX_DEVTOOLS_MESSAGE_BYTES + 1);
        client_stream.write_all(b"x").expect("write trigger byte");

        assert!(!read_client_commands(&mut server_client, &commands_tx));
        assert!(commands_rx.try_recv().is_err());

        let mut line = String::new();
        let mut reader = BufReader::new(client_stream);
        reader.read_line(&mut line).expect("read size error");
        let message: DevToolsServerMessage =
            serde_json::from_str(line.trim()).expect("parse size error");
        assert!(matches!(message, DevToolsServerMessage::Error { .. }));
    }

    #[test]
    fn remote_server_refuses_non_loopback_address() {
        let addr = "192.0.2.1:0"
            .parse()
            .expect("reserved documentation address");
        assert!(DevToolsRemote::start(addr).is_none());
    }

    #[test]
    fn devtools_scene_uses_shared_text_system_and_emits_text() {
        let (app, mut text_system) = app_runtime();
        let mut devtools = DevToolsWindowState::new();
        devtools.enabled = true;

        let scene = devtools
            .build_scene(
                &app.tree,
                app.root.expect("app root"),
                Rect::new(0.0, 0.0, 800.0, 600.0),
                Scale::new(1.0),
                &mut text_system,
            )
            .expect("devtools scene");

        assert!(
            text_cmd_count(&scene) > 0,
            "devtools scene should include text draw commands"
        );
        assert!(
            !text_system.face_blobs_snapshot().is_empty(),
            "shared text system should contain font blobs for renderer upload"
        );
    }

    #[test]
    fn devtools_panel_click_changes_mode() {
        let (app, mut text_system) = app_runtime();
        let mut devtools = DevToolsWindowState::new();
        devtools.enabled = true;
        devtools
            .build_scene(
                &app.tree,
                app.root.expect("app root"),
                Rect::new(0.0, 0.0, 800.0, 600.0),
                Scale::new(1.0),
                &mut text_system,
            )
            .expect("devtools scene");

        let pos = center_of_key(&devtools, DEVTOOLS_MODE_BOTTOM_KEY);
        assert!(devtools.handle_event(&pointer_button(pos, true)));
        assert!(devtools.handle_event(&pointer_button(pos, false)));

        assert_eq!(devtools.mode, DevToolsMode::DockBottom);
    }

    #[test]
    fn devtools_click_outside_panel_is_not_consumed() {
        let (app, mut text_system) = app_runtime();
        let mut devtools = DevToolsWindowState::new();
        devtools.enabled = true;
        devtools
            .build_scene(
                &app.tree,
                app.root.expect("app root"),
                Rect::new(0.0, 0.0, 800.0, 600.0),
                Scale::new(1.0),
                &mut text_system,
            )
            .expect("devtools scene");

        assert!(!devtools.handle_event(&pointer_button(Point::new(10.0, 580.0), true)));
    }
}
