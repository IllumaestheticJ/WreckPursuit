//! Constant-force FFB to the first SDL2 haptic device (feature `ffb`). Uses the raw `sdl2-sys`
//! haptic API because the safe wrapper only exposes rumble.

use sdl2::sys;

/// An open haptic wheel with one running constant-force effect.
pub struct Wheel {
    haptic: *mut sys::SDL_Haptic,
    effect: i32,
    data: sys::SDL_HapticEffect,
    joystick: *mut sys::SDL_Joystick,
}

impl Wheel {
    /// Open haptic device 0 and start a zero-force constant effect. `None` (with a message)
    /// when SDL has no haptic device or it lacks constant force.
    pub fn open() -> Option<Self> {
        // SAFETY: plain SDL C calls; every pointer is checked before use and released in Drop.
        unsafe {
            if sys::SDL_Init(sys::SDL_INIT_HAPTIC | sys::SDL_INIT_JOYSTICK) != 0 {
                eprintln!("ffb: SDL_Init failed");
                return None;
            }
            if sys::SDL_NumHaptics() < 1 {
                eprintln!("ffb: no haptic device found");
                return None;
            }
            let haptic = sys::SDL_HapticOpen(0);
            if haptic.is_null() {
                eprintln!("ffb: SDL_HapticOpen failed");
                return None;
            }
            if sys::SDL_HapticQuery(haptic) & sys::SDL_HAPTIC_CONSTANT == 0 {
                eprintln!("ffb: device has no constant-force support");
                sys::SDL_HapticClose(haptic);
                return None;
            }
            let joystick = if sys::SDL_NumJoysticks() > 0 {
                sys::SDL_JoystickOpen(0)
            } else {
                std::ptr::null_mut()
            };
            sys::SDL_HapticSetGain(haptic, 100);
            sys::SDL_HapticSetAutocenter(haptic, 0);
            let mut data: sys::SDL_HapticEffect = std::mem::zeroed();
            data.constant.type_ = sys::SDL_HAPTIC_CONSTANT as u16;
            data.constant.direction.type_ = sys::SDL_HAPTIC_STEERING_AXIS as u8;
            data.constant.length = sys::SDL_HAPTIC_INFINITY;
            data.constant.level = 0;
            let effect = sys::SDL_HapticNewEffect(haptic, &mut data);
            if effect < 0 {
                eprintln!("ffb: SDL_HapticNewEffect failed");
                sys::SDL_HapticClose(haptic);
                return None;
            }
            sys::SDL_HapticRunEffect(haptic, effect, 1);
            eprintln!("ffb: constant force running on haptic device 0");
            Some(Self {
                haptic,
                effect,
                data,
                joystick,
            })
        }
    }

    /// Set the constant force, -1..=1 of full scale (positive = toward +steer).
    pub fn set_torque(&mut self, frac: f32) {
        let level = (frac.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        // SAFETY: haptic and effect are valid for the lifetime of self.
        unsafe {
            if self.data.constant.level != level {
                self.data.constant.level = level;
                sys::SDL_HapticUpdateEffect(self.haptic, self.effect, &mut self.data);
            }
        }
    }

    /// Steering axis -1..=1 of the wheel's first axis, if a joystick is open.
    pub fn steer_axis(&self) -> Option<f32> {
        if self.joystick.is_null() {
            return None;
        }
        // SAFETY: joystick is a valid open handle.
        unsafe {
            sys::SDL_JoystickUpdate();
            Some(sys::SDL_JoystickGetAxis(self.joystick, 0) as f32 / 32767.0)
        }
    }
}

impl Drop for Wheel {
    fn drop(&mut self) {
        // SAFETY: handles were opened in `open` and are closed exactly once here.
        unsafe {
            sys::SDL_HapticStopEffect(self.haptic, self.effect);
            sys::SDL_HapticClose(self.haptic);
            if !self.joystick.is_null() {
                sys::SDL_JoystickClose(self.joystick);
            }
        }
    }
}
