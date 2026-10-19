//! winit + softbuffer window (feature `window`): a top-down view that follows the car, a 10 m
//! grid, the car box with steered wheels, and a rack-torque bar. Arrows/WASD drive, Space
//! handbrake, R reset, Esc quit. Wall-clock time is used only to feed `Session::step`.

use std::collections::BTreeSet;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};
use wp_contracts::VehicleInput;
use wp_wasm::session::Session;

use crate::{new_session, FULL_SCALE_TORQUE_NM, PLAYER};

/// Pixels per metre.
const SCALE: f32 = 12.0;

struct App {
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    session: Session,
    keys: BTreeSet<KeyCode>,
    last: Instant,
    #[cfg(feature = "ffb")]
    wheel: Option<crate::ffb::Wheel>,
}

/// Open the window and run until closed.
pub fn run() {
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App {
        window: None,
        surface: None,
        session: new_session(),
        keys: BTreeSet::new(),
        last: Instant::now(),
        #[cfg(feature = "ffb")]
        wheel: crate::ffb::Wheel::open(),
    };
    event_loop.run_app(&mut app).expect("run app");
}

struct Canvas<'a> {
    buf: &'a mut [u32],
    w: i32,
    h: i32,
}

impl Canvas<'_> {
    fn put(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.buf[(y * self.w + x) as usize] = c;
        }
    }

    fn line(&mut self, a: (f32, f32), b: (f32, f32), c: u32) {
        let (mut x0, mut y0, x1, y1) = (a.0 as i32, a.1 as i32, b.0 as i32, b.1 as i32);
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let mut err = dx + dy;
        for _ in 0..4096 {
            self.put(x0, y0, c);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.put(xx, yy, c);
            }
        }
    }
}

impl App {
    fn input(&self) -> VehicleInput {
        let k = |c: &[KeyCode]| c.iter().any(|c| self.keys.contains(c));
        let mut steer = k(&[KeyCode::ArrowRight, KeyCode::KeyD]) as i32 as f32
            - k(&[KeyCode::ArrowLeft, KeyCode::KeyA]) as i32 as f32;
        #[cfg(feature = "ffb")]
        if let Some(axis) = self.wheel.as_ref().and_then(|w| w.steer_axis()) {
            steer = axis;
        }
        steer = steer.clamp(-1.0, 1.0);
        VehicleInput {
            steer,
            throttle: k(&[KeyCode::ArrowUp, KeyCode::KeyW]) as i32 as f32,
            brake: k(&[KeyCode::ArrowDown, KeyCode::KeyS]) as i32 as f32,
            handbrake: k(&[KeyCode::Space]) as i32 as f32,
            ..Default::default()
        }
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        let input = self.input();
        let _ = self.session.set_input(PLAYER, input);
        self.session.step(dt);
        let ffb = self.session.ffb(PLAYER).unwrap_or_default();
        #[cfg(feature = "ffb")]
        if let Some(w) = self.wheel.as_mut() {
            w.set_torque(ffb.rack_torque_nm / FULL_SCALE_TORQUE_NM);
        }

        let (Some(window), Some(surface)) = (self.window.as_ref(), self.surface.as_mut()) else {
            return;
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        surface.resize(w, h).expect("resize");
        let mut buffer = surface.buffer_mut().expect("buffer");
        let mut cv = Canvas {
            buf: &mut buffer,
            w: w.get() as i32,
            h: h.get() as i32,
        };
        cv.buf.fill(0x0020_2428);

        let (_, st) = self.session.sim().vehicle_states()[0].clone();
        let def = &self.session.extra(PLAYER).expect("player").car.def;
        let (cx, cy) = (cv.w as f32 / 2.0, cv.h as f32 / 2.0);
        // World → screen, camera centred on the car, +Y up on screen.
        let to_screen =
            |x: f32, y: f32| (cx + (x - st.pos[0]) * SCALE, cy - (y - st.pos[1]) * SCALE);
        let g0x = ((st.pos[0] - cx / SCALE) / 10.0).floor() as i32;
        let g0y = ((st.pos[1] - cy / SCALE) / 10.0).floor() as i32;
        let (fw, fh) = (cv.w as f32, cv.h as f32);
        for i in 0..=(fw / SCALE / 10.0) as i32 + 1 {
            let (sx, _) = to_screen((g0x + i) as f32 * 10.0, 0.0);
            cv.line((sx, 0.0), (sx, fh - 1.0), 0x0040_464c);
        }
        for i in 0..=(fh / SCALE / 10.0) as i32 + 1 {
            let (_, sy) = to_screen(0.0, (g0y + i) as f32 * 10.0);
            cv.line((0.0, sy), (fw - 1.0, sy), 0x0040_464c);
        }

        // Yaw from the quaternion (Z rotation only, enough for a top-down stub).
        let q = st.rot;
        let yaw =
            (2.0 * (q[3] * q[2] + q[0] * q[1])).atan2(1.0 - 2.0 * (q[1] * q[1] + q[2] * q[2]));
        let (s, c) = yaw.sin_cos();
        let local =
            |x: f32, y: f32| to_screen(st.pos[0] + x * c - y * s, st.pos[1] + x * s + y * c);
        let [bw, bl, _] = def.body_size_m;
        let corners = [
            (-bw / 2.0, -bl / 2.0),
            (bw / 2.0, -bl / 2.0),
            (bw / 2.0, bl / 2.0),
            (-bw / 2.0, bl / 2.0),
        ];
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            cv.line(local(a.0, a.1), local(b.0, b.1), 0x00d0_4a2a);
        }
        cv.line(
            local(-bw * 0.3, bl / 2.0 - 0.3),
            local(bw * 0.3, bl / 2.0 - 0.3),
            0x00ff_ee88,
        );
        for (i, wd) in def.wheels.iter().enumerate() {
            let steer = st.wheel_states[i].steer_rad;
            let (ss, sc) = steer.sin_cos();
            let r = wd.radius_m;
            let [px, py, _] = wd.position_m;
            cv.line(
                local(px - ss * -r, py + sc * -r),
                local(px - ss * r, py + sc * r),
                0x00dd_dddd,
            );
        }

        // Rack torque bar (centre = 0) and speed bar.
        let bar_w = 300;
        let x0 = cv.w / 2 - bar_w / 2;
        cv.rect(x0, cv.h - 30, bar_w, 10, 0x0022_2222);
        let frac = (ffb.rack_torque_nm / FULL_SCALE_TORQUE_NM).clamp(-1.0, 1.0);
        let len = (frac.abs() * bar_w as f32 / 2.0) as i32;
        let start = if frac < 0.0 { cv.w / 2 - len } else { cv.w / 2 };
        cv.rect(start, cv.h - 30, len.max(1), 10, 0x00f0_12be);
        cv.rect(
            x0,
            cv.h - 16,
            ((st.speed_mps.abs() / 60.0).min(1.0) * bar_w as f32) as i32,
            6,
            0x007f_dbff,
        );

        window.pre_present_notify();
        buffer.present().expect("present");
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Rc::new(
            el.create_window(
                Window::default_attributes().with_title("WreckPursuit native testbed (stub)"),
            )
            .expect("window"),
        );
        let ctx = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&ctx, window.clone()).expect("surface"));
        self.window = Some(window);
        self.last = Instant::now();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            if code == KeyCode::Escape {
                                el.exit();
                            }
                            if code == KeyCode::KeyR && !event.repeat {
                                let _ = self.session.reset_vehicle(PLAYER);
                            }
                            self.keys.insert(code);
                        }
                        ElementState::Released => {
                            self.keys.remove(&code);
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
