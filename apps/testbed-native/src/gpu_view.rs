//! 3D view through `wp_render` (feature `gpu`): a winit + wgpu window with the chase camera,
//! and `--shot` headless screenshots for checking the renderer without a display.
//!
//! Window keys: arrows/WASD drive, Space handbrake, R reset, C cycle camera orbit, Esc quit.
//! Wall-clock time is used only to feed `Session::step` and the camera smoothing.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};
use wp_contracts::VehicleInput;
use wp_render::gpu::{wgpu, Offscreen, Renderer};
use wp_render::{CarMeshLibrary, ChaseCamera, Scene, StaticBox};
use wp_wasm::session::Session;

use crate::{new_session, PLAYER};

/// Colour of scenario barriers (linear RGB).
const BARRIER_COLOR: [f32; 3] = [0.55, 0.52, 0.45];

/// A scene for `session`'s cars plus its scenario barriers.
fn scene_for(session: &Session) -> Scene {
    let mut scene = Scene::new(CarMeshLibrary::repo_default());
    let boxes: Vec<StaticBox> = session
        .scenario()
        .barriers
        .iter()
        .map(|b| StaticBox {
            pos: b.pos,
            size: b.size,
            yaw: b.yaw,
            color: BARRIER_COLOR,
        })
        .collect();
    scene.set_static_boxes(&boxes);
    scene
}

/// Options of `--shot`.
pub struct ShotArgs {
    /// Output PNG.
    pub out: PathBuf,
    /// simcli scenario (`.ron`); free drive when absent.
    pub scenario: Option<PathBuf>,
    /// Simulated time before the picture, s.
    pub seconds: f32,
    /// Image size.
    pub size: (u32, u32),
    /// Camera orbit about the followed car, degrees.
    pub orbit_deg: f32,
}

impl ShotArgs {
    /// Parse `--shot OUT [--scenario PATH] [--seconds S] [--size WxH] [--orbit DEG]`.
    pub fn parse(args: &[String]) -> Option<Result<Self, String>> {
        let at = args.iter().position(|a| a == "--shot")?;
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
        };
        Some((|| {
            let out = args.get(at + 1).ok_or("--shot needs an output path")?;
            let size = match value("--size") {
                Some(s) => {
                    let (w, h) = s.split_once('x').ok_or("--size is WxH")?;
                    (
                        w.parse().map_err(|_| "--size width")?,
                        h.parse().map_err(|_| "--size height")?,
                    )
                }
                None => (1280, 720),
            };
            Ok(ShotArgs {
                out: out.into(),
                scenario: value("--scenario").map(PathBuf::from),
                seconds: match value("--seconds") {
                    Some(s) => s.parse().map_err(|_| "--seconds")?,
                    None => 2.0,
                },
                size,
                orbit_deg: match value("--orbit") {
                    Some(s) => s.parse().map_err(|_| "--orbit")?,
                    None => 0.0,
                },
            })
        })())
    }
}

/// Run a scenario for `seconds` at 60 fps and save one rendered frame.
pub fn shot(args: &ShotArgs) -> Result<(), String> {
    let mut session = match &args.scenario {
        Some(path) => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            Session::new(wp_wasm::scenario::from_ron(&text)?, true)?
        }
        None => new_session(),
    };
    let mut scene = scene_for(&session);
    let mut camera = ChaseCamera::default();
    camera.orbit = args.orbit_deg.to_radians();
    let mut offscreen = Offscreen::new(args.size.0, args.size.1)?;
    let dt = 1.0 / 60.0;
    let frames = (args.seconds / dt).round().max(1.0) as u32;
    let mut view = None;
    let mut sync_s = 0.0;
    for _ in 0..frames {
        if args.scenario.is_none() {
            let _ = session.set_input(
                PLAYER,
                VehicleInput {
                    throttle: 0.6,
                    ..Default::default()
                },
            );
        }
        session.step(dt);
        let t = Instant::now();
        scene.sync(session.sim(), dt);
        sync_s += t.elapsed().as_secs_f64();
        if let Some(car) = scene.cars().next() {
            view = Some(camera.update(car.pos, car.rot, car.speed_mps, dt));
        }
    }
    let view = view.ok_or("scenario has no vehicles")?;
    let started = Instant::now();
    let rgba = offscreen.render(&scene, &view)?;
    let ms = started.elapsed().as_secs_f64() * 1e3;
    wp_render::gpu::write_png(&args.out, args.size.0, args.size.1, &rgba)?;
    let cars = scene.cars().count().max(1);
    println!(
        "wrote {} ({}x{}, tick {}, first frame {ms:.1} ms incl. upload, scene sync {:.3} ms/car/frame)",
        args.out.display(),
        args.size.0,
        args.size.1,
        session.tick(),
        sync_s * 1e3 / frames as f64 / cars as f64,
    );
    Ok(())
}

/// Window state once the OS window exists.
struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

struct App {
    gpu: Option<Gpu>,
    session: Session,
    scene: Scene,
    camera: ChaseCamera,
    keys: BTreeSet<KeyCode>,
    last: Instant,
    #[cfg(feature = "ffb")]
    wheel: Option<crate::ffb::Wheel>,
}

/// Open the 3D window and run until closed.
pub fn run_window() {
    let session = new_session();
    let scene = scene_for(&session);
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App {
        gpu: None,
        session,
        scene,
        camera: ChaseCamera::default(),
        keys: BTreeSet::new(),
        last: Instant::now(),
        #[cfg(feature = "ffb")]
        wheel: crate::ffb::Wheel::open(),
    };
    event_loop.run_app(&mut app).expect("run app");
}

impl App {
    fn input(&self) -> VehicleInput {
        let k = |c: &[KeyCode]| c.iter().any(|c| self.keys.contains(c));
        #[allow(unused_mut)]
        let mut steer = k(&[KeyCode::ArrowRight, KeyCode::KeyD]) as i32 as f32
            - k(&[KeyCode::ArrowLeft, KeyCode::KeyA]) as i32 as f32;
        #[cfg(feature = "ffb")]
        if let Some(axis) = self.wheel.as_ref().and_then(|w| w.steer_axis()) {
            steer = axis;
        }
        VehicleInput {
            steer: steer.clamp(-1.0, 1.0),
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
        let _ = self.session.set_input(PLAYER, self.input());
        self.session.step(dt);
        #[cfg(feature = "ffb")]
        if let Some(w) = self.wheel.as_mut() {
            let f = self.session.ffb(PLAYER).unwrap_or_default();
            w.set_torque(f.rack_torque_nm / crate::FULL_SCALE_TORQUE_NM);
        }
        self.scene.sync(self.session.sim(), dt);
        let Some(car) = self.scene.car(PLAYER) else {
            return;
        };
        let view = self.camera.update(car.pos, car.rot, car.speed_mps, dt);

        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let frame = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            // Minimised or a slow frame: skip it.
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            _ => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return;
            }
        };
        let target = frame.texture.create_view(&Default::default());
        gpu.renderer.render(
            &self.scene,
            &view,
            &target,
            (gpu.config.width, gpu.config.height),
        );
        gpu.window.pre_present_notify();
        gpu.queue.present(frame);
    }

    fn resize(&mut self, width: u32, height: u32) {
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.config.width = width.max(1);
            gpu.config.height = height.max(1);
            gpu.surface.configure(&gpu.device, &gpu.config);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let window = Arc::new(
            el.create_window(Window::default_attributes().with_title("WreckPursuit — wp_render"))
                .expect("window"),
        );
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("GPU adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("GPU device");
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface not supported by adapter");
        // Prefer an sRGB format so the shader can write linear colour.
        let caps = surface.get_capabilities(&adapter);
        if let Some(f) = caps.formats.iter().find(|f| f.is_srgb()) {
            config.format = *f;
        }
        surface.configure(&device, &config);
        let renderer = Renderer::new(&device, &queue, config.format, 4);
        self.gpu = Some(Gpu {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
        });
        self.last = Instant::now();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                match event.state {
                    ElementState::Pressed => {
                        match code {
                            KeyCode::Escape => el.exit(),
                            KeyCode::KeyR if !event.repeat => {
                                let _ = self.session.reset_vehicle(PLAYER);
                                self.camera.snap();
                            }
                            KeyCode::KeyC if !event.repeat => {
                                self.camera.orbit = (self.camera.orbit
                                    + std::f32::consts::FRAC_PI_2)
                                    % std::f32::consts::TAU;
                            }
                            _ => {}
                        }
                        self.keys.insert(code);
                    }
                    ElementState::Released => {
                        self.keys.remove(&code);
                    }
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(gpu) = &self.gpu {
            gpu.window.request_redraw();
        }
    }
}
